use std::sync::Arc;

use crate::matcher::capacity::Capacity;
use crate::matcher::messages::{MatchCmd, OrderBookResponse};
use crate::matcher::order::Order;
use crate::matcher::order_book::OrderBook;
use rtrb::{Consumer, Producer, PushError};

pub struct OrderBookThread {
    symbol: u32,
    base_px: u64,
    tick_size: u64,
    tick_capacity: u64,
    pool_capacity: u64,
    min_order_qty: u64,
    max_order_qty: u64,
    cmd_rx: Consumer<MatchCmd>,
    cmd_tx: Producer<OrderBookResponse>,
}

impl OrderBookThread {
    pub fn new(symbol: u32, base_px: u64, tick_size: u64, tick_capacity: u64, pool_capacity: u64, min_order_qty: u64, max_order_qty: u64, cmd_rx: Consumer<MatchCmd>, cmd_tx: Producer<OrderBookResponse>) -> Self {
        Self {
            symbol,
            base_px,
            tick_size,
            tick_capacity,
            pool_capacity,
            min_order_qty,
            max_order_qty,
            cmd_rx,
            cmd_tx,
        }
    }

    pub fn run(mut self, capacity: Arc<Capacity>) {
        // Config is validated before spawn; if not, exit so the WAL sees an abandoned queue.
        let Ok(mut book) = OrderBook::try_new(self.tick_capacity as usize, self.pool_capacity as usize, self.symbol, self.tick_size, self.base_px, self.min_order_qty, self.max_order_qty, capacity) else {
            return;
        };

        loop {
            if let Ok(cmd) = self.cmd_rx.pop() {
                match cmd {
                    MatchCmd::AddLimitOrder {
                        order_id,
                        trader_id,
                        side,
                        px,
                        qty,
                        seq,
                        slot_id,
                    } => {
                        #[cfg(debug_assertions)]
                        println!("[BOOK] limit order_id {} seq {} received", order_id, seq);

                        let order = Order::new_limit(order_id, trader_id, side, px, qty, order_id);
                        book.add_order(order, seq, slot_id);

                        #[cfg(debug_assertions)]
                        println!("[BOOK] limit order_id {} seq {} processed", order_id, seq);
                    }
                    MatchCmd::AddMarketOrder { order_id, trader_id, side, qty, seq } => {
                        #[cfg(debug_assertions)]
                        println!("[BOOK] market order_id {} seq {} received", order_id, seq);

                        let order = Order::new_market(order_id, trader_id, side, qty, order_id);
                        book.add_order(order, seq, 0); // Market orders don't use slots

                        #[cfg(debug_assertions)]
                        println!("[BOOK] market order_id {} seq {} processed", order_id, seq);
                    }
                    MatchCmd::CancelOrder { order_id, seq: _seq } => {
                        #[cfg(debug_assertions)]
                        println!("[BOOK] cancel order_id {} seq {} received", order_id, _seq);

                        book.cancel_order(order_id);
                    }
                    MatchCmd::GetSnapshot => {
                        let snapshot = book.snapshot();
                        let mut resp = OrderBookResponse::Snapshot { snapshot };
                        // Spin until delivered; give up only if the consumer is gone.
                        while let Err(PushError::Full(back)) = self.cmd_tx.push(resp) {
                            if self.cmd_tx.is_abandoned() {
                                break;
                            }
                            resp = back;
                            std::hint::spin_loop();
                        }
                    }
                    MatchCmd::Shutdown => {
                        break;
                    }
                }
            } else if self.cmd_rx.is_abandoned() && self.cmd_rx.is_empty() {
                break;
            } else {
                std::hint::spin_loop();
            }
        }
    }
}
