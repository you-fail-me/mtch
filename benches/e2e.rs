#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::engine::{Engine, EngineConfig};
use mtch::ingress::CancelOrderResult;
use mtch::matcher::Side;
use mtch::matcher::messages::OrderBookResponse;

use common::*;

// Ingress returns once the WAL thread acknowledges RECEIPT; matching happens afterwards on the
// book thread. These benchmarks time only that receipt round trip (one Instant pair per op).
// Untimed around every op: the test-only snapshot acts as a barrier proving the book thread
// processed the op with the expected outcome, the fixture is restored, and a second snapshot
// confirms the seeded book, so each timed op starts with an idle pipeline and empty queues.
// One engine per benchmark, started lazily and stopped when the benchmark ends.
fn seeded_engine() -> Engine {
    let mut engine = Engine::new(EngineConfig {
        symbol: SYMBOL,
        tick_size: TICK_SIZE,
        base_px: BASE_PX,
        rb_cap: 256,
        tick_capacity: TICK_CAPACITY,
        pool_capacity: POOL_CAPACITY,
        min_order_qty: MIN_QTY,
        max_order_qty: MAX_QTY,
    })
    .expect("engine start");
    for (id, side, px) in seed_orders() {
        rest(&mut engine, id, side, px);
    }
    assert_eq!(shape(&mut engine), BookShape::seeded());
    engine
}

fn rest(engine: &mut Engine, id: u64, side: Side, px: u64) {
    let resp = engine.add_limit_order(id, MAKER, side, px, QTY).expect("add");
    assert!(matches!(resp, OrderBookResponse::Added { order_id } if order_id == id), "{resp:?}");
}

fn shape(engine: &mut Engine) -> BookShape {
    BookShape::from_snapshot(&engine.snapshot().expect("snapshot"))
}

/// Runs `iters` ops, timing only `op`; `check` verifies and restores afterwards.
fn time_each<R>(iters: u64, engine: &mut Engine, mut op: impl FnMut(&mut Engine) -> R, mut check: impl FnMut(&mut Engine, R)) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iters {
        let start = Instant::now();
        let result = op(engine);
        total += start.elapsed();
        check(engine, result);
        assert_eq!(shape(engine), BookShape::seeded(), "engine not restored");
    }
    total
}

fn bench_receipt_ack(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_receipt_ack");

    // Receipt of a non-crossing limit order that then rests at the tail of the best level.
    for side in [Side::Bid, Side::Ask] {
        let mut engine = None;
        group.bench_function(BenchmarkId::new("add_limit_resting", side_name(side)), |b| {
            let engine = engine.get_or_insert_with(seeded_engine);
            b.iter_custom(|iters| {
                time_each(
                    iters,
                    engine,
                    |e| e.add_limit_order(PROBE_ID, MAKER, side, level_px(side, 0), QTY),
                    |e, resp| {
                        assert!(matches!(resp, Ok(OrderBookResponse::Added { order_id: PROBE_ID })), "{resp:?}");
                        assert_eq!(shape(e), BookShape::seeded_best_count(side, ORDERS_PER_LEVEL + 1));
                        assert!(matches!(e.cancel_order(PROBE_ID), Ok(CancelOrderResult::Cancelled)));
                    },
                )
            });
        });
    }

    // Receipt of a cancel for the last of 3 orders at the best level (level stays non-empty).
    for side in [Side::Bid, Side::Ask] {
        let target = seed_id(side, 0, ORDERS_PER_LEVEL - 1);
        let mut engine = None;
        group.bench_function(BenchmarkId::new("cancel_resting", side_name(side)), |b| {
            let engine = engine.get_or_insert_with(seeded_engine);
            b.iter_custom(|iters| {
                time_each(
                    iters,
                    engine,
                    |e| e.cancel_order(target),
                    |e, resp| {
                        // Ingress reports Cancelled on receipt even for unknown ids; the snapshot is the real check.
                        assert!(matches!(resp, Ok(CancelOrderResult::Cancelled)), "{resp:?}");
                        assert_eq!(shape(e), BookShape::seeded_best_count(side, ORDERS_PER_LEVEL - 1));
                        rest(e, target, side, level_px(side, 0));
                    },
                )
            });
        });
    }

    // Receipt of a 600 market order that then fully fills 6 makers across 2 levels.
    for (side, label) in [(Side::Bid, "Buy"), (Side::Ask, "Sell")] {
        let mut engine = None;
        group.bench_function(BenchmarkId::new("market_sweep", label), |b| {
            let engine = engine.get_or_insert_with(seeded_engine);
            b.iter_custom(|iters| {
                time_each(
                    iters,
                    engine,
                    |e| e.add_market_order(TAKER_ID, TAKER, side, SWEEP_QTY),
                    |e, resp| {
                        assert!(matches!(resp, Ok(OrderBookResponse::Added { order_id: TAKER_ID })), "{resp:?}");
                        assert_eq!(shape(e), BookShape::seeded_swept_by(side));
                        for (id, maker_side, px) in swept_makers(side) {
                            rest(e, id, maker_side, px);
                        }
                    },
                )
            });
        });
    }

    // Fixed cost of the Instant pair included in every measurement above.
    group.bench_function("instant_pair_overhead", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                black_box(());
                total += start.elapsed();
            }
            total
        });
    });

    group.finish();
}

criterion_group!(benches, bench_receipt_ack);
criterion_main!(benches);
