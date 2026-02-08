use std::sync::Arc;

use crate::core::capacity::{Capacity, ReserveResult};
use crate::core::messages::{IngressCmd, OrderBookResponse};
use crate::core::order::Side;
use crate::core::order_book::BookSnapshot;
use crate::error::Error;
use crate::wal::WalAck;
use rtrb::{Consumer, Producer};

#[derive(Debug, Clone)]
pub struct IngressConfig {
    pub min_order_qty: u64,
    pub max_order_qty: u64,
}

pub struct Ingress {
    cmd_tx: Producer<IngressCmd>,
    resp_rx: Consumer<WalAck>,
    config: IngressConfig,
    capacity: Arc<Capacity>,
}

#[derive(Debug)]
pub enum CancelOrderResult {
    Cancelled,
    NotFound,
}

impl Ingress {
    pub fn new(cmd_tx: Producer<IngressCmd>, resp_rx: Consumer<WalAck>, config: IngressConfig, capacity: Arc<Capacity>) -> Self {
        Self { cmd_tx, resp_rx, config, capacity }
    }

    pub fn add_limit_order(&mut self, order_id: u64, trader_id: u64, side: Side, px: u64, qty: u64) -> Result<OrderBookResponse, Error> {
        // Validate order quantity before sending to ring buffer
        if qty < self.config.min_order_qty || qty > self.config.max_order_qty {
            return Ok(OrderBookResponse::Rejected { order_id });
        }

        // Try to reserve a slot before committing to WAL
        let slot_id = match self.capacity.reserve() {
            ReserveResult::Reserved(id) => id,
            ReserveResult::CapacityExceeded => {
                return Ok(OrderBookResponse::Rejected { order_id });
            }
        };

        self.cmd_tx.push(IngressCmd::AddLimitOrder { order_id, trader_id, side, px, qty, slot_id }).map_err(|_| Error::BufferFull)?;

        loop {
            if let Ok(resp) = self.resp_rx.pop() {
                match resp {
                    WalAck {
                        order_id: resp_order_id,
                        entry_type: _,
                        seq: _,
                        snapshot: _,
                    } if resp_order_id == order_id => {
                        return Ok(OrderBookResponse::Added { order_id });
                    }
                    _ => {}
                }
            } else {
                std::hint::spin_loop();
            }
        }
    }

    pub fn cancel_order(&mut self, order_id: u64) -> Result<CancelOrderResult, Error> {
        self.cmd_tx.push(IngressCmd::CancelOrder { order_id }).map_err(|_| Error::BufferFull)?;

        loop {
            if let Ok(resp) = self.resp_rx.pop() {
                match resp {
                    WalAck {
                        order_id: resp_order_id,
                        entry_type: _,
                        seq: _,
                        snapshot: _,
                    } if resp_order_id == order_id => {
                        return Ok(CancelOrderResult::Cancelled);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn snapshot(&mut self) -> Result<BookSnapshot, Error> {
        self.cmd_tx.push(IngressCmd::GetSnapshot).map_err(|_| Error::BufferFull)?;

        loop {
            if let Ok(resp) = self.resp_rx.pop() {
                match resp {
                    WalAck {
                        order_id: _,
                        entry_type: _,
                        seq: _,
                        snapshot: Some(snapshot),
                    } => {
                        return Ok(snapshot);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn add_market_order(&mut self, order_id: u64, trader_id: u64, side: Side, qty: u64) -> Result<OrderBookResponse, Error> {
        // Validate order quantity before sending to ring buffer
        if qty < self.config.min_order_qty || qty > self.config.max_order_qty {
            return Ok(OrderBookResponse::Rejected { order_id });
        }

        self.cmd_tx.push(IngressCmd::AddMarketOrder { order_id, trader_id, side, qty }).map_err(|_| Error::BufferFull)?;

        loop {
            if let Ok(resp) = self.resp_rx.pop() {
                match resp {
                    WalAck {
                        order_id: resp_order_id,
                        entry_type: _,
                        seq: _,
                        snapshot: _,
                    } if resp_order_id == order_id => {
                        return Ok(OrderBookResponse::Added { order_id });
                    }
                    _ => {}
                }
            } else {
                std::hint::spin_loop();
            }
        }
    }

    pub fn shutdown(&mut self) -> Result<(), Error> {
        self.cmd_tx.push(IngressCmd::Shutdown).map_err(|_| Error::BufferFull)?;
        Ok(())
    }
}
