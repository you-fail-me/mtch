use std::sync::Arc;

use crate::error::Error;
use crate::matcher::capacity::{Capacity, ReserveResult};
use crate::matcher::messages::{IngressCmd, OrderBookResponse};
use crate::matcher::order::Side;
use crate::matcher::order_book::BookSnapshot;
use crate::wal::WalAck;
use crate::wal::messages::WalEntryType;
use crate::wal::wal_thread::push_reliable;
use rtrb::{Consumer, Producer, PushError};

#[derive(Debug, Clone)]
pub struct IngressConfig {
    pub min_order_qty: u64,
    pub max_order_qty: u64,
}

/// One request outstanding at a time; each call returns the WAL's receipt ACK, not the execution
/// outcome. A full command queue returns `BufferFull` and a gone WAL returns `EngineStopped`;
/// neither leaks a slot reservation or spins forever.
#[derive(Debug)]
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

    /// Never waits: `BufferFull` on a full queue, `EngineStopped` if the WAL is gone.
    fn enqueue(&mut self, cmd: IngressCmd) -> Result<(), Error> {
        if self.cmd_tx.is_abandoned() {
            return Err(Error::EngineStopped);
        }
        self.cmd_tx.push(cmd).map_err(|PushError::Full(_rejected)| Error::BufferFull)
    }

    /// Discards non-matching ACKs; returns `EngineStopped` once the WAL is gone and the queue drained.
    fn wait_ack(&mut self, matches: impl Fn(&WalAck) -> bool) -> Result<WalAck, Error> {
        loop {
            match self.resp_rx.pop() {
                Ok(ack) if matches(&ack) => return Ok(ack),
                Ok(_) => {}
                Err(_) => {
                    if self.resp_rx.is_abandoned() && self.resp_rx.is_empty() {
                        return Err(Error::EngineStopped);
                    }
                    std::hint::spin_loop();
                }
            }
        }
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

        if let Err(err) = self.enqueue(IngressCmd::AddLimitOrder { order_id, trader_id, side, px, qty, slot_id }) {
            // Never enqueued, so the reservation is still ours.
            self.capacity.release(slot_id);
            return Err(err);
        }

        self.wait_ack(|ack| ack.order_id == order_id && matches!(ack.entry_type, WalEntryType::Add))?;
        Ok(OrderBookResponse::Added { order_id })
    }

    pub fn cancel_order(&mut self, order_id: u64) -> Result<CancelOrderResult, Error> {
        self.enqueue(IngressCmd::CancelOrder { order_id })?;
        self.wait_ack(|ack| ack.order_id == order_id && matches!(ack.entry_type, WalEntryType::Cancel))?;
        Ok(CancelOrderResult::Cancelled)
    }

    pub fn snapshot(&mut self) -> Result<BookSnapshot, Error> {
        self.enqueue(IngressCmd::GetSnapshot)?;
        let ack = self.wait_ack(|ack| matches!(ack.entry_type, WalEntryType::Query))?;
        ack.snapshot.ok_or(Error::SnapshotUnavailable)
    }

    pub fn add_market_order(&mut self, order_id: u64, trader_id: u64, side: Side, qty: u64) -> Result<OrderBookResponse, Error> {
        // Validate order quantity before sending to ring buffer
        if qty < self.config.min_order_qty || qty > self.config.max_order_qty {
            return Ok(OrderBookResponse::Rejected { order_id });
        }

        self.enqueue(IngressCmd::AddMarketOrder { order_id, trader_id, side, qty })?;
        self.wait_ack(|ack| ack.order_id == order_id && matches!(ack.entry_type, WalEntryType::Add))?;
        Ok(OrderBookResponse::Added { order_id })
    }

    /// Spins through backpressure; `EngineStopped` if the WAL is already gone.
    pub fn shutdown(&mut self) -> Result<(), Error> {
        if push_reliable(&mut self.cmd_tx, IngressCmd::Shutdown) { Ok(()) } else { Err(Error::EngineStopped) }
    }
}
