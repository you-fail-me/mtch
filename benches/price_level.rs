#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::collections::VecDeque;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::matcher::order::Handle;
use mtch::matcher::{Capacity, Order, OrderPool, PriceLevel, Side};

use common::{BATCH, MAKER, QTY, reserve, time_batched};

const PX: u64 = 10_000;

// One level of n orders (ids 0..n, same px) in its own pool, plus an untimed FIFO model. Each
// benchmark times one op per fixture, then (untimed) asserts against the model and restores n
// orders. `verify_fifo` checks the real list order against the model at build and after every
// sample. None of these ops free pool slots; the matcher deallocates separately.
struct Fixture {
    pool: OrderPool,
    capacity: Arc<Capacity>,
    level: PriceLevel,
    fifo: VecDeque<(u64, Handle)>,
    slot: u64,
    target: Handle,
}

impl Fixture {
    fn new(n: usize) -> Self {
        let capacity = Arc::new(Capacity::new(n as u64 + 16));
        let pool = OrderPool::new(n as u64 + 16, Arc::clone(&capacity));
        let mut f = Self {
            pool,
            capacity,
            level: PriceLevel::new(PX),
            fifo: VecDeque::with_capacity(n + 1),
            slot: 0,
            target: Handle { idx: 0, generation: 0 },
        };
        for id in 0..n as u64 {
            f.push_tail(id);
        }
        f.verify_fifo();
        f
    }

    fn push_tail(&mut self, id: u64) {
        let slot = reserve(&self.capacity);
        let (handle, became_non_empty) = self.level.add_order(order(id), &mut self.pool, slot).expect("append failed");
        assert_eq!(became_non_empty, self.fifo.is_empty());
        self.fifo.push_back((id, handle));
    }

    fn pop_head(&mut self) -> u64 {
        let (id, handle) = self.fifo.pop_front().expect("model empty");
        assert_eq!(self.level.head, Some(handle));
        self.level.remove_head(&mut self.pool).expect("remove_head failed");
        self.pool.dealloc(handle);
        id
    }

    fn assert_metrics(&self) {
        assert_eq!(self.level.count, self.fifo.len());
        assert_eq!(self.level.qty, self.fifo.len() as u64 * QTY);
        assert_eq!(self.level.head, self.fifo.front().map(|&(_, h)| h));
    }

    /// Walks the list head to tail by unlinking and re-appending every order (restoring the
    /// same order) and asserts the visit order matches the model.
    fn verify_fifo(&mut self) {
        self.assert_metrics();
        for _ in 0..self.fifo.len() {
            let expected = self.fifo.front().map(|&(id, _)| id);
            assert_eq!(self.level.peek_head(&self.pool).map(|o| o.id), expected, "FIFO order differs from model");
            let id = self.pop_head();
            self.push_tail(id);
        }
        self.assert_metrics();
    }
}

fn order(id: u64) -> Order {
    Order::new_limit(id, MAKER, Side::Bid, PX, QTY, id)
}

fn fixtures(n: usize) -> Vec<Fixture> {
    (0..BATCH).map(|_| Fixture::new(n)).collect()
}

fn verify_all(levels: &mut [Fixture]) {
    for f in levels {
        f.verify_fifo();
    }
}

/// `add_order`: append at the tail of a level of n orders into a pre-reserved slot.
/// Restore: unlink it with `remove_order` and free the slot.
fn bench_append_tail(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_append_tail");
    for n in [0, 10, 100, 1000] {
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            let mut levels = fixtures(n);
            for f in &mut levels {
                f.slot = reserve(&f.capacity);
            }
            let mut results = Vec::with_capacity(BATCH);
            b.iter_custom(|iters| {
                let mut checked = 0;
                let elapsed = time_batched(
                    iters,
                    &mut levels,
                    &mut results,
                    |f| f.level.add_order(order(n as u64), &mut f.pool, f.slot),
                    |f, result| {
                        let (handle, became_non_empty) = result.expect("append failed");
                        assert_eq!(became_non_empty, n == 0);
                        f.fifo.push_back((n as u64, handle));
                        if checked < BATCH {
                            checked += 1;
                            f.verify_fifo(); // probe must be visited last
                        }
                        let (_, handle) = f.fifo.pop_back().expect("model empty");
                        assert_eq!(f.level.remove_order(handle, &mut f.pool), Some(n == 0));
                        f.pool.dealloc(handle);
                        f.assert_metrics();
                        f.slot = reserve(&f.capacity);
                    },
                );
                verify_all(&mut levels);
                elapsed
            });
        });
    }
    group.finish();
}

/// `remove_head`: list unlink and aggregate update only (no matching, no deallocation; unused by
/// `OrderBook`). Restore: free the slot and re-append the id at the tail.
fn bench_unlink_head(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_unlink_head");
    for n in [10, 100, 1000] {
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            let mut levels = fixtures(n);
            let mut results = Vec::with_capacity(BATCH);
            b.iter_custom(|iters| {
                let elapsed = time_batched(
                    iters,
                    &mut levels,
                    &mut results,
                    |f| f.level.remove_head(&mut f.pool),
                    |f, result| {
                        assert_eq!(result, Some(()));
                        let (id, handle) = f.fifo.pop_front().expect("model empty");
                        f.assert_metrics();
                        f.pool.dealloc(handle);
                        f.push_tail(id);
                    },
                );
                verify_all(&mut levels);
                elapsed
            });
        });
    }
    group.finish();
}

const REMOVE_N: usize = 100;
const POSITIONS: [(&str, usize); 3] = [("head", 0), ("middle", REMOVE_N / 2), ("tail", REMOVE_N - 1)];

/// `remove_order` (unlink by handle, as cancel and full fills use it) of the order at FIFO index
/// 0, 50 or 99 of one level of 100, all at the same px. Restore: free the slot, re-append the id
/// at the tail; the next target is whichever order now sits at the same index.
fn bench_remove_order(c: &mut Criterion) {
    // On a fresh level the targets are ids 0/50/99 and removal leaves the other 99 in order.
    for (label, idx) in POSITIONS {
        let mut f = Fixture::new(REMOVE_N);
        let (id, handle) = f.fifo.remove(idx).expect("index");
        assert_eq!(id, idx as u64, "{label}");
        assert_eq!(f.level.remove_order(handle, &mut f.pool), Some(false), "{label}");
        f.pool.dealloc(handle);
        f.verify_fifo();
        assert!(f.fifo.iter().map(|&(id, _)| id).eq((0..REMOVE_N as u64).filter(|&i| i != id)), "{label}");
    }

    let mut group = c.benchmark_group("price_level_remove_order");
    for (label, idx) in POSITIONS {
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            let mut levels = fixtures(REMOVE_N);
            let mut results = Vec::with_capacity(BATCH);
            b.iter_custom(|iters| {
                for f in &mut levels {
                    f.target = f.fifo[idx].1;
                }
                let elapsed = time_batched(
                    iters,
                    &mut levels,
                    &mut results,
                    |f| f.level.remove_order(f.target, &mut f.pool),
                    |f, result| {
                        assert_eq!(result, Some(false));
                        let (id, handle) = f.fifo.remove(idx).expect("index");
                        assert_eq!(handle, f.target);
                        f.assert_metrics();
                        f.pool.dealloc(handle);
                        f.push_tail(id);
                        f.target = f.fifo[idx].1;
                    },
                );
                verify_all(&mut levels);
                elapsed
            });
        });
    }
    group.finish();
}

/// `update_head_qty(50)`: partial-fill bookkeeping on the head (100 -> 50), no list change.
/// Restore: unlink the reduced head and append a full-quantity order with the same id.
fn bench_reduce_head_qty(c: &mut Criterion) {
    let mut group = c.benchmark_group("price_level_reduce_head_qty");
    for n in [10, 100, 1000] {
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            let mut levels = fixtures(n);
            let mut results = Vec::with_capacity(BATCH);
            b.iter_custom(|iters| {
                let elapsed = time_batched(
                    iters,
                    &mut levels,
                    &mut results,
                    |f| f.level.update_head_qty(50, &mut f.pool),
                    |f, result| {
                        assert_eq!(result, Some(()));
                        assert_eq!(f.level.peek_head(&f.pool).map(|o| o.qty), Some(50));
                        assert_eq!(f.level.qty, n as u64 * QTY - 50);
                        let id = f.pop_head();
                        f.push_tail(id);
                        f.assert_metrics();
                    },
                );
                verify_all(&mut levels);
                elapsed
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_append_tail, bench_unlink_head, bench_remove_order, bench_reduce_head_qty);
criterion_main!(benches);
