#![allow(dead_code)]

use std::hint::black_box;
use std::time::{Duration, Instant};

use mtch::matcher::{BookSnapshot, Capacity, ReserveResult, Side};

pub const SYMBOL: u32 = 100;
pub const TICK_SIZE: u64 = 1;
// Bids rest at px <= BASE_PX, asks at px >= BASE_PX.
pub const BASE_PX: u64 = 10_000;
pub const TICK_CAPACITY: u64 = 64;
// Resting order IDs directly index the preallocated array: all are < POOL_CAPACITY.
pub const POOL_CAPACITY: u64 = 128;
pub const MIN_QTY: u64 = 50;
pub const MAX_QTY: u64 = 5000;
pub const QTY: u64 = 100;
pub const LEVELS: u64 = 8;
pub const ORDERS_PER_LEVEL: u64 = 3;
pub const MAKER: u64 = 1;
pub const TAKER: u64 = 2;
pub const PROBE_ID: u64 = 100;
pub const TAKER_ID: u64 = 120;
// Fully fills the best 2 opposite levels: 6 maker fills.
pub const SWEEP_LEVELS: u64 = 2;
pub const SWEEP_QTY: u64 = SWEEP_LEVELS * ORDERS_PER_LEVEL * QTY;
pub const BATCH: usize = 32;

/// Seeded level `level` (0 = best). Best bid 9999, best ask 10001.
pub const fn level_px(side: Side, level: u64) -> u64 {
    match side {
        Side::Bid => BASE_PX - 1 - level,
        Side::Ask => BASE_PX + 1 + level,
    }
}

/// Bids use ids 0..24, asks 24..48.
pub const fn seed_id(side: Side, level: u64, pos: u64) -> u64 {
    let offset = if matches!(side, Side::Ask) { LEVELS * ORDERS_PER_LEVEL } else { 0 };
    offset + level * ORDERS_PER_LEVEL + pos
}

pub const fn opposite(side: Side) -> Side {
    match side {
        Side::Bid => Side::Ask,
        Side::Ask => Side::Bid,
    }
}

pub const fn side_name(side: Side) -> &'static str {
    match side {
        Side::Bid => "Bid",
        Side::Ask => "Ask",
    }
}

/// `(id, side, px)` for `levels` of `side`, in FIFO insertion order.
pub fn side_orders(side: Side, levels: std::ops::Range<u64>) -> impl Iterator<Item = (u64, Side, u64)> {
    levels.flat_map(move |level| (0..ORDERS_PER_LEVEL).map(move |pos| (seed_id(side, level, pos), side, level_px(side, level))))
}

pub fn seed_orders() -> impl Iterator<Item = (u64, Side, u64)> {
    side_orders(Side::Bid, 0..LEVELS).chain(side_orders(Side::Ask, 0..LEVELS))
}

/// Makers consumed by a `SWEEP_QTY` market order on `taker_side`.
pub fn swept_makers(taker_side: Side) -> impl Iterator<Item = (u64, Side, u64)> {
    side_orders(opposite(taker_side), 0..SWEEP_LEVELS)
}

/// Slots are reserved the way ingress does before an order reaches the matcher.
pub fn reserve(capacity: &Capacity) -> u64 {
    let slot = match capacity.reserve() {
        ReserveResult::Reserved(slot) => Some(slot),
        ReserveResult::CapacityExceeded => None,
    };
    slot.expect("fixture never exhausts capacity")
}

/// `(px, qty, count)` per non-empty level, best first.
#[derive(Debug, PartialEq, Eq)]
pub struct BookShape {
    pub bids: Vec<(u64, u64, u64)>,
    pub asks: Vec<(u64, u64, u64)>,
}

impl BookShape {
    pub fn from_snapshot(snapshot: &BookSnapshot) -> Self {
        let levels = |levels: &[mtch::matcher::PriceLevelSnapshot]| levels.iter().map(|l| (l.px, l.qty, l.count)).collect();
        Self {
            bids: levels(&snapshot.bids),
            asks: levels(&snapshot.asks),
        }
    }

    pub fn seeded() -> Self {
        let side = |side| (0..LEVELS).map(|level| (level_px(side, level), ORDERS_PER_LEVEL * QTY, ORDERS_PER_LEVEL)).collect();
        Self { bids: side(Side::Bid), asks: side(Side::Ask) }
    }

    fn side_mut(&mut self, side: Side) -> &mut Vec<(u64, u64, u64)> {
        match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        }
    }

    /// Seeded book whose best level on `side` holds `count` orders.
    pub fn seeded_best_count(side: Side, count: u64) -> Self {
        let mut shape = Self::seeded();
        shape.side_mut(side)[0] = (level_px(side, 0), count * QTY, count);
        shape
    }

    pub fn seeded_swept_by(taker_side: Side) -> Self {
        let mut shape = Self::seeded();
        let levels = shape.side_mut(opposite(taker_side));
        *levels = levels.split_off(SWEEP_LEVELS as usize);
        shape
    }
}

/// Times `iters` runs of `op`, one per fixture, `fixtures.len()` per `Instant` pair. `settle`
/// runs untimed after each batch to verify each result and restore its fixture. `results` must be
/// pre-reserved so the timed loop does not allocate. Fixed overhead per op (loop, result write,
/// amortised clock reads) is measured by `harness_overhead/batched_noop`.
pub fn time_batched<F, R>(iters: u64, fixtures: &mut [F], results: &mut Vec<R>, mut op: impl FnMut(&mut F) -> R, mut settle: impl FnMut(&mut F, R)) -> Duration {
    assert!(!fixtures.is_empty() && results.capacity() >= fixtures.len());
    let mut total = Duration::ZERO;
    let mut remaining = iters;
    while remaining > 0 {
        let n = fixtures.len().min(remaining as usize);
        results.clear();

        let start = Instant::now();
        for fixture in &mut fixtures[..n] {
            results.push(op(black_box(fixture)));
        }
        total += start.elapsed();

        for (fixture, result) in fixtures[..n].iter_mut().zip(results.drain(..)) {
            settle(fixture, black_box(result));
        }
        remaining -= n as u64;
    }
    total
}
