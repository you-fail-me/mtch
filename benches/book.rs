#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::missing_docs)]

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mtch::matcher::{Capacity, Order, OrderBook, Side};

fn bench_add_limit_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_add_limit");

    for (side, name) in [(Side::Bid, "Buy"), (Side::Ask, "Sell")] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &side, |b, side| {
            let mut id = 0u64;
            b.iter_with_setup(
                || {
                    let capacity = Arc::new(Capacity::new(256));
                    OrderBook::new(64, 64, 100, 1, 10000, 50, 5000, capacity)
                },
                |mut book| {
                    id += 1;
                    black_box(book.add_order(Order::new_limit(id, 1, *side, 10000, 100, id), id, id));
                },
            );
        });
    }
    group.finish();
}

fn bench_cancel_order(c: &mut Criterion) {
    c.bench_function("book_cancel", |b| {
        let mut id = 0u64;
        b.iter_with_setup(
            || {
                let capacity = Arc::new(Capacity::new(256));
                let mut book = OrderBook::new(64, 64, 100, 1, 10000, 50, 5000, capacity);
                for i in 0..10 {
                    book.add_order(Order::new_limit(i, 1, Side::Bid, 9000 + i, 100, i), i, i);
                }
                book
            },
            |mut book| {
                black_box(book.cancel_order(id));
                id = (id + 1) % 10;
            },
        );
    });
}

fn bench_add_market_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("book_add_market");
    let mut id = 200u64;

    for (side, name) in [(Side::Bid, "Buy"), (Side::Ask, "Sell")] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &side, |b, side| {
            b.iter_with_setup(
                || {
                    let capacity = Arc::new(Capacity::new(512));
                    let mut book = OrderBook::new(256, 256, 100, 1, 10000, 50, 5000, capacity);
                    if *side == Side::Bid {
                        // For buy market order: setup asks to match against
                        for i in 0..100 {
                            book.add_order(Order::new_limit(i, 1, Side::Ask, 10000 + i, 100, i), i, i);
                        }
                    } else {
                        // For sell market order: setup bids to match against
                        for i in 0..100 {
                            book.add_order(Order::new_limit(i, 1, Side::Bid, 9900 - i, 100, i), i, i);
                        }
                    }
                    book
                },
                |mut book| {
                    id += 1;
                    // Market order qty 600 matches 6 orders (6 partial fills)
                    black_box(book.add_order(Order::new_market(id, 2, *side, 600, id), id, 0));
                },
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_add_limit_order, bench_cancel_order, bench_add_market_order);
criterion_main!(benches);
