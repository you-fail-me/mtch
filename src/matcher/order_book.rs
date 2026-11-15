use std::sync::Arc;

use crate::matcher::capacity::Capacity;
use crate::matcher::fill_buffer::FillBuffer;
use crate::matcher::order::{Order, OrderLocation, OrderType, Side};
use crate::matcher::order_index::OrderIndex;
use crate::matcher::order_pool::OrderPool;
use crate::matcher::price_level::PriceLevel;
use crate::matcher::tick_array::TickArray;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub taker_order_id: u64,
    pub maker_order_id: u64,
    pub px: u64,
    pub qty: u64,
    pub seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOrderResult {
    /// Executed and nothing rests. `remaining_qty > 0` means partially filled with the remainder cancelled:
    /// market orders never rest, and a limit remainder is cancelled when it cannot rest (price outside its
    /// side's window, or resting insertion failed).
    Filled {
        remaining_qty: u64,
    },
    Added,
    /// Nothing executed and nothing rests.
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelResult {
    Cancelled,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookConfigError {
    ZeroTickSize,
    ZeroTickCapacity,
    ZeroPoolCapacity,
    ZeroMinOrderQty,
    MinAboveMax,
    AskWindowOverflow,
    BidWindowUnderflow,
}

#[derive(Debug, Clone)]
pub struct PriceLevelSnapshot {
    pub px: u64,
    pub qty: u64,
    pub count: u64,
}

#[derive(Debug, Clone)]
pub struct BookSnapshot {
    pub symbol: u32,
    pub bids: Vec<PriceLevelSnapshot>,
    pub asks: Vec<PriceLevelSnapshot>,
}

/// Price window: bids rest at `base_px - i * tick_size`, asks at `base_px + i * tick_size`, `i < tick_capacity`.
/// Limit prices must be on that grid or are rejected before any side effect. An aligned price outside its own
/// side's window may still execute against crossing liquidity; its remainder is cancelled, never re-priced.
///
/// Self-trade: the taker's own resting orders are skipped and keep their priority; matching continues with the
/// next eligible maker in price-time order, including at worse crossing levels.
///
/// Slots: a limit order's `slot_id` is a `Capacity` reservation owned by the book from this call on. It moves
/// to the resting order, or is released on any other exit. Market orders pass a sentinel and release nothing.
pub struct OrderBook {
    symbol: u32,
    min_order_qty: u64,
    max_order_qty: u64,

    pool: OrderPool,
    bids: TickArray,
    asks: TickArray,
    order_index: OrderIndex,
    fill_buffer: FillBuffer,
}

impl OrderBook {
    /// # Errors
    /// Returns the first invalid parameter.
    pub fn validate_config(tick_capacity: u64, pool_capacity: u64, tick_size: u64, base_px: u64, min_order_qty: u64, max_order_qty: u64) -> Result<(), BookConfigError> {
        if pool_capacity == 0 {
            return Err(BookConfigError::ZeroPoolCapacity);
        }
        if min_order_qty == 0 {
            return Err(BookConfigError::ZeroMinOrderQty);
        }
        if min_order_qty > max_order_qty {
            return Err(BookConfigError::MinAboveMax);
        }
        TickArray::validate(tick_capacity, base_px, tick_size, Side::Ask)?;
        TickArray::validate(tick_capacity, base_px, tick_size, Side::Bid)
    }

    /// # Errors
    /// See [`OrderBook::validate_config`].
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(tick_capacity: usize, pool_capacity: usize, symbol: u32, tick_size: u64, base_px: u64, min_order_qty: u64, max_order_qty: u64, capacity: Arc<Capacity>) -> Result<Self, BookConfigError> {
        let (tick_capacity, pool_capacity) = (tick_capacity as u64, pool_capacity as u64);
        Self::validate_config(tick_capacity, pool_capacity, tick_size, base_px, min_order_qty, max_order_qty)?;
        // Each fill of one taker either consumes a distinct maker (<= pool_capacity) or is its last fill, and
        // each fill is >= 1 of a qty <= max_order_qty. max/min is not a bound: residual makers can be < min.
        let max_fills = pool_capacity.min(max_order_qty);
        Ok(Self {
            symbol,
            min_order_qty,
            max_order_qty,
            pool: OrderPool::new(pool_capacity, capacity),
            bids: TickArray::try_new(tick_capacity, base_px, tick_size, Side::Bid)?,
            asks: TickArray::try_new(tick_capacity, base_px, tick_size, Side::Ask)?,
            order_index: OrderIndex::new(pool_capacity),
            fill_buffer: FillBuffer::new(max_fills),
        })
    }

    /// # Panics
    /// On invalid configuration; see [`OrderBook::try_new`].
    #[allow(clippy::too_many_arguments, clippy::panic)]
    pub fn new(tick_capacity: usize, pool_capacity: usize, symbol: u32, tick_size: u64, base_px: u64, min_order_qty: u64, max_order_qty: u64, capacity: Arc<Capacity>) -> Self {
        match Self::try_new(tick_capacity, pool_capacity, symbol, tick_size, base_px, min_order_qty, max_order_qty, capacity) {
            Ok(book) => book,
            Err(e) => panic!("invalid order book configuration: {e:?}"),
        }
    }

    pub fn snapshot(&self) -> BookSnapshot {
        // Levels are already ordered best-first on both sides.
        let collect = |side: &TickArray| {
            side.levels
                .iter()
                .filter(|l| !l.is_empty())
                .map(|l| PriceLevelSnapshot {
                    px: l.px,
                    qty: l.qty,
                    count: l.order_count() as u64,
                })
                .collect()
        };
        BookSnapshot {
            symbol: self.symbol,
            bids: collect(&self.bids),
            asks: collect(&self.asks),
        }
    }

    /// Fills of the most recent `add_order` call; cleared at the start of every call.
    #[must_use]
    pub fn last_fills(&self) -> &[Fill] {
        self.fill_buffer.as_slice()
    }

    pub fn add_order(&mut self, mut order: Order, seq: u64, slot_id: u64) -> AddOrderResult {
        self.fill_buffer.drain();
        let is_limit = order.order_type == OrderType::Limit;

        if !self.is_valid_new_order(&order, slot_id) {
            if is_limit {
                self.pool.release_unused_slot(slot_id);
            }
            return AddOrderResult::Rejected;
        }

        if !is_limit {
            order.px = match order.side {
                Side::Bid => u64::MAX,
                Side::Ask => 0,
            };
        }

        self.match_order(&mut order, seq);
        let executed = !self.fill_buffer.as_slice().is_empty();
        let remaining_qty = order.qty;

        if remaining_qty == 0 {
            if is_limit {
                self.pool.release_unused_slot(slot_id);
            }
            return AddOrderResult::Filled { remaining_qty: 0 };
        }
        if is_limit && self.rest_order(order, slot_id) {
            return AddOrderResult::Added;
        }
        if executed { AddOrderResult::Filled { remaining_qty } } else { AddOrderResult::Rejected }
    }

    pub fn cancel_order(&mut self, order_id: u64) -> CancelResult {
        let loc = match self.order_index.get(order_id) {
            Some(loc) => *loc,
            None => return CancelResult::NotFound,
        };
        // Verify the entry really is this order before mutating anything.
        match self.pool.get(loc.handle) {
            Some(o) if o.id == order_id && o.side == loc.side && o.px == loc.px => {}
            _ => return CancelResult::NotFound,
        }

        let side = match loc.side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        let Some(idx) = side.index(loc.px) else {
            return CancelResult::NotFound;
        };
        let Some(became_empty) = side.levels[idx].remove_order(loc.handle, &mut self.pool) else {
            return CancelResult::NotFound;
        };
        if became_empty {
            side.on_level_emptied(idx);
        }
        self.pool.dealloc(loc.handle);
        self.order_index.remove(order_id);
        CancelResult::Cancelled
    }

    /// O(n) consistency check of levels, links, totals, index and best pointers, for tests.
    ///
    /// # Errors
    /// Describes the first violated invariant.
    pub fn check_invariants(&self) -> Result<(), &'static str> {
        let mut total = 0;
        for (side, arr) in [(Side::Bid, &self.bids), (Side::Ask, &self.asks)] {
            arr.check_invariants()?;
            for level in &arr.levels {
                total += self.check_level(side, level)?;
            }
        }
        if total != self.pool.live_count() {
            return Err("linked orders do not match live pool slots");
        }
        Ok(())
    }

    fn check_level(&self, side: Side, level: &PriceLevel) -> Result<usize, &'static str> {
        let (mut n, mut qty, mut prev, mut cur) = (0, 0u64, None, level.head);
        while let Some(h) = cur {
            if n >= level.count {
                return Err("level list longer than its count");
            }
            let o = self.pool.get(h).ok_or("linked handle is not live")?;
            if o.prev != prev || o.qty == 0 || o.px != level.px || o.side != side {
                return Err("resting order inconsistent with its level");
            }
            match self.order_index.get(o.id) {
                Some(loc) if loc.handle == h && loc.px == level.px && loc.side == side => {}
                _ => return Err("order index does not point at resting order"),
            }
            qty += o.qty;
            n += 1;
            prev = Some(h);
            cur = o.next;
        }
        if n != level.count || qty != level.qty || prev != level.tail() {
            return Err("level count/qty/tail mismatch");
        }
        Ok(n)
    }

    /// Everything that must hold before any side effect.
    fn is_valid_new_order(&self, order: &Order, slot_id: u64) -> bool {
        if order.qty < self.min_order_qty || order.qty > self.max_order_qty {
            return false;
        }
        // A live id is rejected for both order types so a duplicate never trades.
        if self.order_index.get(order.id).is_some() {
            return false;
        }
        // A limit may rest, so its id must be a direct index key; a market id is only a fill tag.
        let slots = self.pool.slot_count() as u64;
        order.order_type == OrderType::Market || (self.bids.is_aligned(order.px) && order.id < slots && slot_id < slots)
    }

    fn match_order(&mut self, taker: &mut Order, seq: u64) {
        let side = match taker.side {
            Side::Bid => &mut self.asks,
            Side::Ask => &mut self.bids,
        };
        let Some(mut from) = side.best_idx() else {
            return;
        };
        // Levels are visited once each, best first, so this terminates even if all makers are skipped.
        while taker.qty > 0 {
            let Some(idx) = side.next_non_empty(from) else {
                break;
            };
            let level = &mut side.levels[idx];
            let crosses = match taker.side {
                Side::Bid => taker.px >= level.px,
                Side::Ask => taker.px <= level.px,
            };
            if !crosses {
                break;
            }
            let ok = Self::match_level(taker, level, &mut self.fill_buffer, &mut self.pool, &mut self.order_index, seq);
            if level.is_empty() {
                side.on_level_emptied(idx);
            }
            if !ok {
                break;
            }
            // If this emptied the best level, `on_level_emptied` already found the next one.
            from = match side.best_idx() {
                Some(best) if best > idx => best,
                _ => idx + 1,
            };
        }
    }

    /// FIFO at one level, skipping the taker's own makers. Returns false if matching must stop: the fill
    /// buffer is full (unreachable given its size) or the level is corrupt. No trade happens unrecorded.
    fn match_level(taker: &mut Order, level: &mut PriceLevel, fills: &mut FillBuffer, pool: &mut OrderPool, index: &mut OrderIndex, seq: u64) -> bool {
        let mut current = level.head;
        while taker.qty > 0 {
            let Some(handle) = current else {
                break;
            };
            let Some(maker) = pool.get(handle) else {
                debug_assert!(false, "dangling handle in price level");
                return false;
            };
            let (maker_id, maker_qty, next) = (maker.id, maker.qty, maker.next);
            current = next;
            if maker.trader_id == taker.trader_id {
                continue;
            }
            if fills.is_full() {
                debug_assert!(false, "fill buffer bound violated");
                return false;
            }

            let qty = taker.qty.min(maker_qty);
            let applied = if qty == maker_qty {
                level.remove_order(handle, pool).is_some_and(|_| {
                    pool.dealloc(handle);
                    index.remove(maker_id);
                    true
                })
            } else {
                level.reduce_order_qty(handle, qty, pool).is_some()
            };
            if !applied {
                debug_assert!(false, "price level corruption while matching");
                return false;
            }
            taker.qty -= qty;
            let fill = Fill {
                taker_order_id: taker.id,
                maker_order_id: maker_id,
                px: level.px,
                qty,
                seq,
            };
            fills.push(fill);
        }
        true
    }

    /// Rests a validated limit remainder. On failure all partial steps are undone and the slot is released
    /// exactly once (by `dealloc` if the pool took it).
    fn rest_order(&mut self, order: Order, slot_id: u64) -> bool {
        let (order_id, side, px) = (order.id, order.side, order.px);
        let arr = match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        let Some(idx) = arr.index(px) else {
            self.pool.release_unused_slot(slot_id);
            return false;
        };
        let level = &mut arr.levels[idx];
        let Some((handle, became_non_empty)) = level.add_order(order, &mut self.pool, slot_id) else {
            self.pool.release_unused_slot(slot_id);
            return false;
        };
        if self.order_index.insert(order_id, OrderLocation { side, px, handle }).is_err() {
            let unlinked = level.remove_order(handle, &mut self.pool).is_some();
            debug_assert!(unlinked, "rollback of fresh tail failed");
            self.pool.dealloc(handle);
            return false;
        }
        arr.on_order_added(idx, became_non_empty);
        true
    }
}
