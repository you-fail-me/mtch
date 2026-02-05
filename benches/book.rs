#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::core::{Order, OrderBook, Side};
use std::hint::black_box;

fn bench_add_limit_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_add_limit");

    for scenario in ["empty", "thin", "deep"].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(scenario), scenario, |b, &scenario| {
            // Config tailored to each scenario's needs
            let (tick_capacity, pool_capacity) = match scenario {
                "empty" => (64, 128),
                "thin" => (64, 256),
                "deep" => (256, 20000),
                _ => unreachable!(),
            };

            let mut book = OrderBook::new(tick_capacity, pool_capacity, 100, 1, 10000, 50, 5000);

            // Pre-populate for non-empty scenarios
            match scenario {
                "thin" => {
                    for i in 0..10 {
                        for j in 0..5 {
                            let bid_id = i * 5 + j;
                            let ask_id = bid_id + 1000;
                            book.add_order(Order::new_limit(bid_id, 1, Side::Bid, 10000 - i * 10, 100, bid_id), bid_id);
                            book.add_order(Order::new_limit(ask_id, 2, Side::Ask, 10100 + i * 10, 100, ask_id), ask_id);
                        }
                    }
                }
                "deep" => {
                    for level in 0..100 {
                        for order_num in 0..50 {
                            let bid_id = level * 50 + order_num;
                            let ask_id = bid_id + 100000;
                            book.add_order(Order::new_limit(bid_id, bid_id % 10, Side::Bid, 10000 - level * 10, 100, bid_id), bid_id);
                            book.add_order(Order::new_limit(ask_id, ask_id % 10, Side::Ask, 11000 + level * 10, 100, ask_id), ask_id);
                        }
                    }
                }
                _ => {}
            }

            let mut id = 10000u64;
            b.iter(|| {
                id = (id % 50000) + 10001;
                let order = Order::new_limit(id, 1, Side::Bid, 9900, 100, id);
                let result = black_box(book.add_order(order, id));
                book.cancel_order(id);
                result
            });
        });
    }
    group.finish();
}

fn bench_add_market_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_add_market");

    for scenario in ["empty", "thin", "deep"].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(scenario), scenario, |b, &scenario| {
            let (tick_capacity, pool_capacity) = match scenario {
                "empty" => (64, 128),
                "thin" => (64, 256),
                "deep" => (256, 20000),
                _ => unreachable!(),
            };

            let mut book = OrderBook::new(tick_capacity, pool_capacity, 100, 1, 10000, 50, 5000);

            match scenario {
                "thin" => {
                    for i in 0..10 {
                        for j in 0..5 {
                            let bid_id = i * 5 + j;
                            let ask_id = bid_id + 1000;
                            book.add_order(Order::new_limit(bid_id, 1, Side::Bid, 10000 - i * 10, 100, bid_id), bid_id);
                            book.add_order(Order::new_limit(ask_id, 2, Side::Ask, 10100 + i * 10, 100, ask_id), ask_id);
                        }
                    }
                }
                "deep" => {
                    for level in 0..100 {
                        for order_num in 0..50 {
                            let bid_id = level * 50 + order_num;
                            let ask_id = bid_id + 100000;
                            book.add_order(Order::new_limit(bid_id, bid_id % 10, Side::Bid, 10000 - level * 10, 100, bid_id), bid_id);
                            book.add_order(Order::new_limit(ask_id, ask_id % 10, Side::Ask, 11000 + level * 10, 100, ask_id), ask_id);
                        }
                    }
                }
                _ => {}
            }

            let mut id = 10000u64;
            b.iter(|| {
                id = (id % 50000) + 10001;
                let order = Order::new_market(id, 1, Side::Bid, 100, id);
                black_box(book.add_order(order, id))
            });
        });
    }
    group.finish();
}

fn bench_cancel_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_cancel");

    for scenario in ["empty", "thin", "deep"].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(scenario), scenario, |b, &scenario| {
            let (tick_capacity, pool_capacity) = match scenario {
                "empty" => (64, 128),
                "thin" => (64, 256),
                "deep" => (256, 20000),
                _ => unreachable!(),
            };

            let mut book = OrderBook::new(tick_capacity, pool_capacity, 100, 1, 10000, 50, 5000);

            match scenario {
                "thin" => {
                    for i in 0..10 {
                        for j in 0..5 {
                            let bid_id = i * 5 + j;
                            let ask_id = bid_id + 1000;
                            book.add_order(Order::new_limit(bid_id, 1, Side::Bid, 10000 - i * 10, 100, bid_id), bid_id);
                            book.add_order(Order::new_limit(ask_id, 2, Side::Ask, 10100 + i * 10, 100, ask_id), ask_id);
                        }
                    }
                }
                "deep" => {
                    for level in 0..100 {
                        for order_num in 0..50 {
                            let bid_id = level * 50 + order_num;
                            let ask_id = bid_id + 100000;
                            book.add_order(Order::new_limit(bid_id, bid_id % 10, Side::Bid, 10000 - level * 10, 100, bid_id), bid_id);
                            book.add_order(Order::new_limit(ask_id, ask_id % 10, Side::Ask, 11000 + level * 10, 100, ask_id), ask_id);
                        }
                    }
                }
                _ => {}
            }

            let mut id = 10000u64;
            b.iter(|| {
                id = (id % 50000) + 10001;
                let order = Order::new_limit(id, 1, Side::Bid, 9900, 100, id);
                book.add_order(order, id);
                black_box(book.cancel_order(id))
            });
        });
    }
    group.finish();
}

fn bench_limit_order_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_limit_matching");

    group.bench_function("full_fill", |b| {
        // Minimal config: only 2 orders needed at a time (maker + taker)
        let mut book = OrderBook::new(64, 256, 100, 1, 10000, 50, 5000);
        let mut seq = 0u64;

        b.iter(|| {
            let maker_id = (seq * 2) % 10000;
            let taker_id = (seq * 2 + 1) % 10000;
            seq += 1;

            book.add_order(Order::new_limit(maker_id, 1, Side::Ask, 10000, 100, maker_id), maker_id);
            let result = black_box(book.add_order(Order::new_limit(taker_id, 2, Side::Bid, 10000, 100, taker_id), taker_id));

            result
        });
    });

    group.bench_function("partial_fill", |b| {
        // Minimal config: only 3 orders needed at a time (maker + taker + cancel)
        let mut book = OrderBook::new(64, 256, 100, 1, 10000, 50, 5000);
        let mut seq = 0u64;

        b.iter(|| {
            let maker_id = (seq * 3) % 10000;
            let taker_id = (seq * 3 + 1) % 10000;
            seq += 1;

            book.add_order(Order::new_limit(maker_id, 1, Side::Ask, 10000, 100, maker_id), maker_id);
            let result = black_box(book.add_order(Order::new_limit(taker_id, 2, Side::Bid, 10000, 150, taker_id), taker_id));
            book.cancel_order(taker_id);

            result
        });
    });

    group.finish();
}

fn bench_market_order_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_market_scenarios");

    group.bench_function("full_fill", |b| {
        let mut book = OrderBook::new(64, 256, 100, 1, 10000, 50, 5000);
        let mut seq = 0u64;

        b.iter(|| {
            let maker_id = (seq * 2) % 10000;
            let taker_id = (seq * 2 + 1) % 10000;
            seq += 1;

            book.add_order(Order::new_limit(maker_id, 1, Side::Ask, 10000, 100, maker_id), maker_id);
            black_box(book.add_order(Order::new_market(taker_id, 2, Side::Bid, 100, taker_id), taker_id))
        });
    });

    group.bench_function("sweep", |b| {
        // Need space for 10 levels + 1 sweep order
        let mut book = OrderBook::new(64, 512, 100, 1, 10000, 50, 5000);
        let mut seq = 0u64;

        b.iter(|| {
            let base_id = (seq * 11) % 10000;
            seq += 1;

            for i in 0..10 {
                book.add_order(Order::new_limit(base_id + i, 1, Side::Ask, 10000 + i * 10, 10, base_id + i), base_id + i);
            }
            black_box(book.add_order(Order::new_market(base_id + 10, 2, Side::Bid, 100, base_id + 10), base_id + 10))
        });
    });

    group.bench_function("rejected", |b| {
        // Minimal config for rejected orders (no orders stored)
        let mut book = OrderBook::new(64, 128, 100, 1, 10000, 50, 5000);
        let mut seq = 0u64;

        b.iter(|| {
            let id = seq % 10000;
            seq += 1;
            black_box(book.add_order(Order::new_market(id, 1, Side::Bid, 100, id), id))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_add_limit_order, bench_add_market_order, bench_cancel_order, bench_limit_order_matching, bench_market_order_scenarios);
criterion_main!(benches);
