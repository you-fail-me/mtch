use crate::core::fill_buffer::FillBuffer;
use crate::core::order::{Handle, Order, OrderLocation, OrderType, Side};
use crate::core::order_index::OrderIndex;
use crate::core::order_pool::OrderPool;
use crate::core::price_level::PriceLevel;
use crate::core::tick_array::TickArray;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
pub struct Fill {
    pub taker_order_id: u64,
    pub maker_order_id: u64,
    pub px: u64,
    pub qty: u64,
    pub seq: u64,
}

#[derive(Debug)]
pub enum AddOrderResult {
    Filled { remaining_qty: u64 },
    Added,
    Rejected,
}

#[derive(Debug)]
pub enum CancelResult {
    Cancelled,
    NotFound,
}

#[derive(Debug, Clone)]
pub struct PriceLevelSnapshot {
    pub px: u64,
    pub qty: u64,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct BookSnapshot {
    pub symbol: u32,
    pub bids: Vec<PriceLevelSnapshot>,
    pub asks: Vec<PriceLevelSnapshot>,
}

pub struct OrderBook {
    symbol: u32,
    tick_size: u64,
    pool_capacity: usize,
    min_order_qty: u64,
    max_order_qty: u64,

    pool: OrderPool,
    bids: TickArray,
    asks: TickArray,
    order_index: OrderIndex,
    fill_buffer: FillBuffer,
}

impl OrderBook {
    pub fn new(tick_capacity: usize, pool_capacity: usize, symbol: u32, tick_size: u64, base_px: u64, min_order_qty: u64, max_order_qty: u64) -> Self {
        let max_fills = (max_order_qty / min_order_qty) as usize;
        Self {
            symbol,
            tick_size,
            pool_capacity,
            min_order_qty,
            max_order_qty,
            pool: OrderPool::new(pool_capacity),
            bids: TickArray::new(tick_capacity, base_px, tick_size, Ordering::Less),
            asks: TickArray::new(tick_capacity, base_px, tick_size, Ordering::Greater),
            order_index: OrderIndex::new(pool_capacity),
            fill_buffer: FillBuffer::new(max_fills),
        }
    }

    pub fn snapshot(&self) -> BookSnapshot {
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        for i in 0..self.bids.levels.len() {
            let level = &self.bids.levels[i];
            if !level.is_empty() {
                let px = level.px;

                bids.push(PriceLevelSnapshot {
                    px,
                    qty: level.qty,
                    count: level.order_count(),
                });
            }
        }

        for i in 0..self.asks.levels.len() {
            let level = &self.asks.levels[i];
            if !level.is_empty() {
                let px = level.px;

                asks.push(PriceLevelSnapshot {
                    px,
                    qty: level.qty,
                    count: level.order_count(),
                });
            }
        }

        bids.sort_by(|a, b| b.px.cmp(&a.px));
        asks.sort_by(|a, b| a.px.cmp(&b.px));

        BookSnapshot { symbol: self.symbol, bids, asks }
    }

    pub fn add_order(&mut self, mut order: Order, seq: u64) -> AddOrderResult {
        // Validate order quantity
        if order.qty < self.min_order_qty || order.qty > self.max_order_qty {
            return AddOrderResult::Rejected;
        }

        if order.order_type == OrderType::Market {
            order.px = match order.side {
                Side::Bid => u64::MAX,
                Side::Ask => 0,
            };
        }

        if order.order_type == OrderType::Limit {
            if self.pool.live_count >= self.pool_capacity {
                return AddOrderResult::Rejected;
            }
        }

        self.fill_buffer.drain();
        self.try_match(&mut order, seq);

        if order.order_type == OrderType::Market {
            if order.qty == 0 {
                return AddOrderResult::Filled { remaining_qty: 0 };
            }
            return AddOrderResult::Rejected;
        }

        if order.qty == 0 {
            AddOrderResult::Filled { remaining_qty: 0 }
        } else {
            if self.add_to_level(order).is_none() {
                return AddOrderResult::Rejected;
            }
            AddOrderResult::Added
        }
    }

    pub fn cancel_order(&mut self, order_id: u64) -> CancelResult {
        let location = match self.order_index.remove(order_id) {
            Some(loc) => loc,
            None => return CancelResult::NotFound,
        };

        match location.side {
            Side::Bid => {
                if let Some(idx) = self.bids.index(location.px, self.tick_size, Side::Bid) {
                    let level = &mut self.bids.levels[idx];
                    if let Some(became_empty) = level.remove_order(location.handle, &mut self.pool) {
                        if became_empty {
                            self.bids.decrement_non_empty();
                        }
                        self.pool.dealloc(location.handle);
                        return CancelResult::Cancelled;
                    }
                }
            }
            Side::Ask => {
                if let Some(idx) = self.asks.index(location.px, self.tick_size, Side::Ask) {
                    let level = &mut self.asks.levels[idx];
                    if let Some(became_empty) = level.remove_order(location.handle, &mut self.pool) {
                        if became_empty {
                            self.asks.decrement_non_empty();
                        }
                        self.pool.dealloc(location.handle);
                        return CancelResult::Cancelled;
                    }
                }
            }
        }
        CancelResult::NotFound
    }

    fn try_match(&mut self, order: &mut Order, seq: u64) {
        match order.side {
            Side::Bid => self.match_bid(order, seq),
            Side::Ask => self.match_ask(order, seq),
        }
    }

    fn match_bid(&mut self, bid: &mut Order, seq: u64) {
        loop {
            if bid.qty == 0 {
                break;
            }

            let best_hot = self.asks.best_level_mut(self.tick_size, Side::Ask);
            let (ask_px, level) = match best_hot {
                Some((px, level)) if !level.is_empty() => (px, level),
                _ => break,
            };

            if bid.px < ask_px {
                break;
            }

            let qty_before = bid.qty;

            let level_empty = Self::match_orders_at_level(bid, level, ask_px, &mut self.fill_buffer, &mut self.pool, &mut self.order_index, seq);

            if level_empty {
                self.asks.decrement_non_empty();
                self.asks.update_best();
            }

            if bid.qty == qty_before {
                break;
            }
        }
    }

    fn match_ask(&mut self, ask: &mut Order, seq: u64) {
        loop {
            if ask.qty == 0 {
                break;
            }

            let best_hot = self.bids.best_level_mut(self.tick_size, Side::Bid);
            let (bid_px, level) = match best_hot {
                Some((px, level)) if !level.is_empty() => (px, level),
                _ => break,
            };

            if ask.px > bid_px {
                break;
            }

            let qty_before = ask.qty;

            let level_empty = Self::match_orders_at_level(ask, level, bid_px, &mut self.fill_buffer, &mut self.pool, &mut self.order_index, seq);

            if level_empty {
                self.bids.decrement_non_empty();
                self.bids.update_best();
            }

            if ask.qty == qty_before {
                break;
            }
        }
    }

    fn match_orders_at_level(taker_order: &mut Order, level: &mut PriceLevel, match_px: u64, fills: &mut FillBuffer, pool: &mut OrderPool, order_index: &mut OrderIndex, seq: u64) -> bool {
        let mut current = level.head;

        while taker_order.qty > 0 {
            let handle = match current {
                Some(h) => h,
                None => break,
            };

            let (maker_id, maker_qty, maker_trader_id, next) = {
                let maker = match pool.get(handle) {
                    Some(o) => o,
                    None => break,
                };
                (maker.id, maker.qty, maker.trader_id, maker.next)
            };

            if taker_order.trader_id == maker_trader_id {
                current = next;
                continue;
            }

            let match_qty = taker_order.qty.min(maker_qty);

            fills.push(Fill {
                taker_order_id: taker_order.id,
                maker_order_id: maker_id,
                px: match_px,
                qty: match_qty,
                seq,
            });

            taker_order.qty -= match_qty;

            if match_qty == maker_qty {
                current = next;
                level.remove_order(handle, pool);
                pool.dealloc(handle);
                order_index.remove(maker_id);
            } else {
                pool.get_mut(handle).unwrap().qty -= match_qty;
                level.qty -= match_qty;
            }
        }

        level.is_empty()
    }

    fn add_to_level(&mut self, order: Order) -> Option<Handle> {
        let order_id = order.id;
        let side = order.side;
        let px = order.px;

        let (handle, became_non_empty) = match side {
            Side::Bid => {
                let level = self.bids.level_mut(px, self.tick_size, Side::Bid)?;
                let (h, became) = level.add_order(order, &mut self.pool)?;
                (h, became)
            }
            Side::Ask => {
                let level = self.asks.level_mut(px, self.tick_size, Side::Ask)?;
                let (h, became) = level.add_order(order, &mut self.pool)?;
                (h, became)
            }
        };

        if became_non_empty {
            match side {
                Side::Bid => self.bids.increment_non_empty(),
                Side::Ask => self.asks.increment_non_empty(),
            }
        }

        if let Err(_) = self.order_index.insert(order_id, OrderLocation { side, px, handle }) {
            return None;
        }

        Some(handle)
    }
}
