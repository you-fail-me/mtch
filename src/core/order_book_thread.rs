use crate::core::messages::{MatchCmd, OrderBookResponse};
use crate::core::order::Order;
use crate::core::order_book::OrderBook;
use rtrb::{Consumer, Producer};

pub struct OrderBookThread {
    symbol: u32,
    base_px: u64,
    tick_size: u64,
    tick_capacity: usize,
    pool_capacity: usize,
    cmd_rx: Consumer<MatchCmd>,
    cmd_tx: Producer<OrderBookResponse>,
}

impl OrderBookThread {
    pub fn new(symbol: u32, base_px: u64, tick_size: u64, tick_capacity: usize, pool_capacity: usize, cmd_rx: Consumer<MatchCmd>, cmd_tx: Producer<OrderBookResponse>) -> Self {
        Self {
            symbol,
            base_px,
            tick_size,
            tick_capacity,
            pool_capacity,
            cmd_rx,
            cmd_tx,
        }
    }

    pub fn run(mut self) {
        let mut book = OrderBook::new(self.tick_capacity, self.pool_capacity, self.symbol, self.tick_size, self.base_px);

        loop {
            if let Ok(cmd) = self.cmd_rx.pop() {
                match cmd {
                    MatchCmd::AddLimitOrder { order_id, trader_id, side, px, qty, seq } => {
                        #[cfg(debug_assertions)]
                        println!("[BOOK] limit order_id {} seq {} received", order_id, seq);

                        let order = Order::new_limit(order_id, trader_id, side, px, qty, order_id);
                        let res = book.add_order(order, seq);

                        #[cfg(debug_assertions)]
                        println!("[BOOK] limit order_id {} seq {} processed with result {:?}", order_id, seq, res);
                    }
                    MatchCmd::AddMarketOrder { order_id, trader_id, side, qty, seq } => {
                        #[cfg(debug_assertions)]
                        println!("[BOOK] market order_id {} seq {} received", order_id, seq);

                        let order = Order::new_market(order_id, trader_id, side, qty, order_id);
                        let res = book.add_order(order, seq);

                        #[cfg(debug_assertions)]
                        println!("[BOOK] market order_id {} seq {} processed with result {:?}", order_id, seq, res);
                    }
                    MatchCmd::CancelOrder { order_id, seq } => {
                        #[cfg(debug_assertions)]
                        println!("[BOOK] cancel order_id {} seq {} received", order_id, seq);

                        let res = book.cancel_order(order_id);

                        #[cfg(debug_assertions)]
                        println!("[BOOK] cancel order_id {} seq {} processed with result {:?}", order_id, seq, res);
                    }
                    MatchCmd::GetSnapshot => {
                        let snapshot = book.snapshot();
                        let _ = self.cmd_tx.push(OrderBookResponse::Snapshot { snapshot });
                    }
                    MatchCmd::Shutdown => {
                        break;
                    }
                }
            } else {
                std::hint::spin_loop();
            }
        }
    }
}
