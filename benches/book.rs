#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::matcher::{AddOrderResult, CancelResult, Capacity, Order, OrderBook, Side};

use common::*;

// Each benchmark times one op on each of BATCH identically seeded books, then (untimed) asserts
// the result, restores the seeded state, and every sample ends with a snapshot comparison.
struct Fixture {
    book: OrderBook,
    capacity: Arc<Capacity>,
    slot: u64,
}

impl Fixture {
    fn seeded() -> Self {
        let capacity = Arc::new(Capacity::new(POOL_CAPACITY));
        let book = OrderBook::new(TICK_CAPACITY as usize, POOL_CAPACITY as usize, SYMBOL, TICK_SIZE, BASE_PX, MIN_QTY, MAX_QTY, Arc::clone(&capacity));
        let mut f = Self { book, capacity, slot: 0 };
        for (id, side, px) in seed_orders() {
            f.rest(id, side, px);
        }
        assert_eq!(f.shape(), BookShape::seeded());
        f
    }

    fn rest(&mut self, id: u64, side: Side, px: u64) {
        let slot = reserve(&self.capacity);
        let result = self.book.add_order(Order::new_limit(id, MAKER, side, px, QTY, id), 0, slot);
        assert!(matches!(result, AddOrderResult::Added), "order {id} at {px}: {result:?}");
    }

    fn shape(&self) -> BookShape {
        BookShape::from_snapshot(&self.book.snapshot())
    }
}

fn fixtures() -> Vec<Fixture> {
    (0..BATCH).map(|_| Fixture::seeded()).collect()
}

fn assert_seeded(books: &[Fixture]) {
    for f in books {
        assert_eq!(f.shape(), BookShape::seeded(), "book drifted from seeded state");
    }
}

/// Non-crossing limit order joining the tail of the best level (3 orders ahead), slot pre-reserved.
/// Restore: cancel it.
fn bench_add_limit_resting(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_add_limit_resting");
    for side in [Side::Bid, Side::Ask] {
        group.bench_function(BenchmarkId::from_parameter(side_name(side)), |b| {
            let mut books = fixtures();
            for f in &mut books {
                f.slot = reserve(&f.capacity);
            }
            let mut results = Vec::with_capacity(BATCH);
            b.iter_custom(|iters| {
                let mut checked = 0;
                let elapsed = time_batched(
                    iters,
                    &mut books,
                    &mut results,
                    |f| f.book.add_order(Order::new_limit(PROBE_ID, MAKER, side, level_px(side, 0), QTY, PROBE_ID), 0, f.slot),
                    |f, result| {
                        assert!(matches!(result, AddOrderResult::Added), "{result:?}");
                        if checked < BATCH {
                            checked += 1;
                            assert_eq!(f.shape(), BookShape::seeded_best_count(side, ORDERS_PER_LEVEL + 1));
                        }
                        assert!(matches!(f.book.cancel_order(PROBE_ID), CancelResult::Cancelled));
                        f.slot = reserve(&f.capacity);
                    },
                );
                assert_seeded(&books);
                elapsed
            });
        });
    }
    group.finish();
}

/// Cancel the last of 3 orders at the best level (level stays non-empty, so the cached best index
/// stays valid). Restore: re-add it at the tail.
fn bench_cancel_resting(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_cancel_resting");
    for side in [Side::Bid, Side::Ask] {
        let target = seed_id(side, 0, ORDERS_PER_LEVEL - 1);
        group.bench_function(BenchmarkId::from_parameter(side_name(side)), |b| {
            let mut books = fixtures();
            let mut results = Vec::with_capacity(BATCH);
            b.iter_custom(|iters| {
                let mut checked = 0;
                let elapsed = time_batched(
                    iters,
                    &mut books,
                    &mut results,
                    |f| f.book.cancel_order(target),
                    |f, result| {
                        assert!(matches!(result, CancelResult::Cancelled), "{result:?}");
                        if checked < BATCH {
                            checked += 1;
                            assert_eq!(f.shape(), BookShape::seeded_best_count(side, ORDERS_PER_LEVEL - 1));
                        }
                        f.rest(target, side, level_px(side, 0));
                    },
                );
                assert_seeded(&books);
                elapsed
            });
        });
    }
    group.finish();
}

/// Market order of 600 fully filling 6 makers and emptying the best 2 opposite levels.
/// Restore: re-add the makers in FIFO order (`Added` also proves they left the index).
fn bench_market_sweep(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_market_sweep");
    for (side, label) in [(Side::Bid, "Buy"), (Side::Ask, "Sell")] {
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            let mut books = fixtures();
            let mut results = Vec::with_capacity(BATCH);
            b.iter_custom(|iters| {
                let mut checked = 0;
                let elapsed = time_batched(
                    iters,
                    &mut books,
                    &mut results,
                    // Market orders never rest; OrderBookThread passes slot 0 too.
                    |f| f.book.add_order(Order::new_market(TAKER_ID, TAKER, side, SWEEP_QTY, TAKER_ID), 0, 0),
                    |f, result| {
                        assert!(matches!(result, AddOrderResult::Filled { remaining_qty: 0 }), "{result:?}");
                        if checked < BATCH {
                            checked += 1;
                            assert_eq!(f.shape(), BookShape::seeded_swept_by(side));
                        }
                        for (id, maker_side, px) in swept_makers(side) {
                            f.rest(id, maker_side, px);
                        }
                    },
                );
                assert_seeded(&books);
                elapsed
            });
        });
    }
    group.finish();
}

/// `time_batched` with an empty op: the fixed per-op harness cost included in the numbers above.
fn bench_harness_overhead(c: &mut Criterion) {
    c.bench_function("harness_overhead/batched_noop", |b| {
        let mut books = fixtures();
        let mut results: Vec<()> = Vec::with_capacity(BATCH);
        b.iter_custom(|iters| time_batched(iters, &mut books, &mut results, |_| {}, |_, ()| {}));
    });
}

criterion_group!(benches, bench_add_limit_resting, bench_cancel_resting, bench_market_sweep, bench_harness_overhead);
criterion_main!(benches);
