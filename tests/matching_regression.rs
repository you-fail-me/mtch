#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

//! Matcher correctness regressions on the direct `OrderBook` API, plus `OrderBookThread` queue handling.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mtch::matcher::messages::{MatchCmd, OrderBookResponse};
use mtch::matcher::order_book::BookConfigError;
use mtch::matcher::{AddOrderResult, CancelResult, Capacity, Order, OrderBook, OrderBookThread, ReserveResult, Side};

const SYM: u32 = 7;

fn book(tick_capacity: usize, pool_capacity: usize, tick_size: u64, base_px: u64, min_qty: u64, max_qty: u64) -> (OrderBook, Arc<Capacity>) {
    let capacity = Arc::new(Capacity::new(pool_capacity as u64));
    let book = OrderBook::new(tick_capacity, pool_capacity, SYM, tick_size, base_px, min_qty, max_qty, Arc::clone(&capacity));
    (book, capacity)
}

fn reserve(capacity: &Capacity) -> u64 {
    match capacity.reserve() {
        ReserveResult::Reserved(slot) => slot,
        ReserveResult::CapacityExceeded => panic!("capacity unexpectedly exhausted"),
    }
}

/// Counts free capacity slots by reserving until exhausted, then releasing them again.
fn free_slots(capacity: &Capacity, pool_capacity: u64) -> u64 {
    let mut taken = [u64::MAX; 64];
    assert!(pool_capacity as usize <= taken.len());
    let mut n = 0usize;
    while let ReserveResult::Reserved(slot) = capacity.reserve() {
        taken[n] = slot;
        n += 1;
    }
    for &slot in &taken[..n] {
        capacity.release(slot);
    }
    n as u64
}

fn limit(book: &mut OrderBook, capacity: &Capacity, id: u64, trader: u64, side: Side, px: u64, qty: u64) -> AddOrderResult {
    let slot = reserve(capacity);
    book.add_order(Order::new_limit(id, trader, side, px, qty, id), id, slot)
}

fn market(book: &mut OrderBook, id: u64, trader: u64, side: Side, qty: u64) -> AddOrderResult {
    book.add_order(Order::new_market(id, trader, side, qty, id), id, 0)
}

type Levels = Vec<(u64, u64, u64)>;

fn levels(book: &OrderBook) -> (Levels, Levels) {
    let snap = book.snapshot();
    (snap.bids.iter().map(|l| (l.px, l.qty, l.count)).collect(), snap.asks.iter().map(|l| (l.px, l.qty, l.count)).collect())
}

fn is_filled(r: &AddOrderResult, remaining: u64) -> bool {
    matches!(r, AddOrderResult::Filled { remaining_qty } if *remaining_qty == remaining)
}

// ---------------------------------------------------------------- 1. stale best after cancel

#[test]
fn test_cancel_best_ask_market_buy_reaches_next_level() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Ask, 100, 50), AddOrderResult::Added));
    assert!(matches!(limit(&mut b, &cap, 2, 10, Side::Ask, 101, 50), AddOrderResult::Added));
    assert!(matches!(b.cancel_order(1), CancelResult::Cancelled));

    let r = market(&mut b, 3, 20, Side::Bid, 50);
    assert!(is_filled(&r, 0), "market buy must reach ask@101 after best ask@100 was cancelled, got {r:?}");
    assert_eq!(levels(&b), (vec![], vec![]));
}

#[test]
fn test_cancel_best_bid_market_sell_reaches_next_level() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Bid, 100, 50), AddOrderResult::Added));
    assert!(matches!(limit(&mut b, &cap, 2, 10, Side::Bid, 99, 50), AddOrderResult::Added));
    assert!(matches!(b.cancel_order(1), CancelResult::Cancelled));

    let r = market(&mut b, 3, 20, Side::Ask, 50);
    assert!(is_filled(&r, 0), "market sell must reach bid@99 after best bid@100 was cancelled, got {r:?}");
    assert_eq!(levels(&b), (vec![], vec![]));
}

#[test]
fn test_cancel_last_order_then_level_reuse_matches() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Ask, 103, 50), AddOrderResult::Added));
    assert!(matches!(b.cancel_order(1), CancelResult::Cancelled));
    assert!(matches!(market(&mut b, 2, 20, Side::Bid, 10), AddOrderResult::Rejected));

    // Reuse a worse level first, then a better one: best must track both.
    assert!(matches!(limit(&mut b, &cap, 3, 10, Side::Ask, 105, 50), AddOrderResult::Added));
    assert!(matches!(limit(&mut b, &cap, 4, 11, Side::Ask, 103, 50), AddOrderResult::Added));
    assert!(matches!(b.cancel_order(4), CancelResult::Cancelled));
    let r = market(&mut b, 5, 20, Side::Bid, 50);
    assert!(is_filled(&r, 0), "got {r:?}");
    assert_eq!(levels(&b), (vec![], vec![]));
}

// ---------------------------------------------------------------- 2. atomic insertion / duplicate ids

#[test]
fn test_duplicate_live_id_rejected_without_orphan_level() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Bid, 100, 50), AddOrderResult::Added));
    let r = limit(&mut b, &cap, 1, 10, Side::Bid, 99, 70);
    assert!(matches!(r, AddOrderResult::Rejected), "got {r:?}");
    assert_eq!(levels(&b), (vec![(100, 50, 1)], vec![]), "rejected duplicate must not leave pool/list state");
    assert_eq!(free_slots(&cap, 16), 15, "rejected duplicate must release its slot");
}

#[test]
fn test_duplicate_live_id_rejected_before_trading() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Bid, 99, 50), AddOrderResult::Added));
    assert!(matches!(limit(&mut b, &cap, 2, 11, Side::Ask, 100, 50), AddOrderResult::Added));
    // Incoming bid reuses live id 1 and would cross ask@100.
    let r = limit(&mut b, &cap, 1, 12, Side::Bid, 100, 50);
    assert!(matches!(r, AddOrderResult::Rejected), "got {r:?}");
    assert_eq!(levels(&b), (vec![(99, 50, 1)], vec![(100, 50, 1)]), "duplicate must be rejected before any execution");
}

#[test]
fn test_cancel_missing_or_out_of_range_id_does_not_remove_other_order() {
    // 17 shared order 1's bucket in the old `id % capacity` index and cancelled order 1.
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Bid, 100, 50), AddOrderResult::Added));
    assert!(matches!(b.cancel_order(17), CancelResult::NotFound));
    assert_eq!(levels(&b), (vec![(100, 50, 1)], vec![]));
    assert!(matches!(b.cancel_order(1), CancelResult::Cancelled));
    assert!(matches!(b.cancel_order(1), CancelResult::NotFound));
    assert_eq!(levels(&b), (vec![], vec![]));
}

#[test]
fn test_out_of_range_limit_id_rejected_before_trading_market_id_unbounded() {
    let (mut b, cap) = book(16, 4, 1, 100, 1, 1000);
    assert_eq!(limit(&mut b, &cap, 0, 10, Side::Ask, 100, 50), AddOrderResult::Added);
    // Limit ids are direct index keys (0..pool capacity); 4 would cross but must not trade.
    assert_eq!(limit(&mut b, &cap, 4, 11, Side::Bid, 100, 20), AddOrderResult::Rejected);
    assert!(b.last_fills().is_empty());
    assert_eq!(levels(&b), (vec![], vec![(100, 50, 1)]));
    assert_eq!(free_slots(&cap, 4), 3, "rejected limit must release its reservation");
    // Market ids are only fill tags.
    assert_eq!(market(&mut b, 1_000, 11, Side::Bid, 20), AddOrderResult::Filled { remaining_qty: 0 });
    assert_eq!(fills(&b), vec![(1_000, 0, 100, 20)]);
    b.check_invariants().unwrap();
}

#[test]
fn test_market_with_live_id_rejected_before_trading() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert_eq!(limit(&mut b, &cap, 1, 10, Side::Bid, 99, 50), AddOrderResult::Added); // holds slot 0
    assert_eq!(limit(&mut b, &cap, 2, 11, Side::Ask, 100, 50), AddOrderResult::Added);
    assert_eq!(market(&mut b, 1, 12, Side::Bid, 10), AddOrderResult::Rejected);
    assert!(b.last_fills().is_empty());
    assert_eq!(levels(&b), (vec![(99, 50, 1)], vec![(100, 50, 1)]));
    assert_eq!(free_slots(&cap, 16), 14, "market sentinel slot 0 must not be released");
    b.check_invariants().unwrap();
}

// ---------------------------------------------------------------- 3. capacity reservations

#[test]
fn test_fully_filled_limit_taker_releases_slot() {
    let (mut b, cap) = book(16, 2, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 0, 10, Side::Ask, 100, 50), AddOrderResult::Added));
    let r = limit(&mut b, &cap, 1, 11, Side::Bid, 100, 50);
    assert!(is_filled(&r, 0), "got {r:?}");
    assert_eq!(free_slots(&cap, 2), 2, "both maker and fully-filled taker slots must be free");
}

#[test]
fn test_invalid_qty_limit_releases_slot() {
    let (mut b, cap) = book(16, 2, 1, 100, 50, 100);
    assert!(matches!(limit(&mut b, &cap, 0, 10, Side::Ask, 100, 10), AddOrderResult::Rejected));
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Ask, 100, 101), AddOrderResult::Rejected));
    assert_eq!(free_slots(&cap, 2), 2);
}

#[test]
fn test_market_order_never_releases_slot_zero() {
    let (mut b, cap) = book(16, 2, 1, 100, 1, 1000);
    // Slot 0 is held by a resting ask.
    let slot = reserve(&cap);
    assert_eq!(slot, 0);
    assert!(matches!(b.add_order(Order::new_limit(1, 10, Side::Ask, 100, 50, 1), 1, slot), AddOrderResult::Added));
    // Market orders on an empty opposite side / partial: must not release slot 0.
    assert!(matches!(market(&mut b, 2, 11, Side::Ask, 10), AddOrderResult::Rejected));
    assert_eq!(free_slots(&cap, 2), 1, "slot 0 still belongs to the resting ask");
    let r = market(&mut b, 3, 11, Side::Bid, 20);
    assert!(is_filled(&r, 0), "got {r:?}");
    assert_eq!(free_slots(&cap, 2), 1);
}

// ---------------------------------------------------------------- 4. fill buffer sizing

#[test]
fn test_sweep_many_tiny_residuals_does_not_panic() {
    // max/min == 2, but residual makers far below min_order_qty can exist.
    let (mut b, cap) = book(16, 16, 1, 100, 50, 100);
    let mut id = 1;
    for px in [102, 101, 100] {
        assert!(matches!(limit(&mut b, &cap, id, 10, Side::Ask, px, 51), AddOrderResult::Added));
        id += 1;
        assert!(is_filled(&market(&mut b, id, 20, Side::Bid, 50), 0));
        id += 1;
    }
    assert_eq!(levels(&b).1, vec![(100, 1, 1), (101, 1, 1), (102, 1, 1)]);
    assert!(matches!(limit(&mut b, &cap, id, 10, Side::Ask, 103, 100), AddOrderResult::Added));
    id += 1;

    let r = market(&mut b, id, 20, Side::Bid, 100);
    assert!(is_filled(&r, 0), "got {r:?}");
    assert_eq!(levels(&b).1, vec![(103, 3, 1)]);
}

// ---------------------------------------------------------------- 5. tick validation

#[test]
fn test_off_tick_limit_rejected_not_floored() {
    let (mut b, cap) = book(16, 16, 10, 1000, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Ask, 1005, 50), AddOrderResult::Rejected));
    assert!(matches!(limit(&mut b, &cap, 2, 10, Side::Bid, 995, 50), AddOrderResult::Rejected));
    assert_eq!(levels(&b), (vec![], vec![]));
    assert_eq!(free_slots(&cap, 16), 16);
}

#[test]
fn test_off_tick_crossing_limit_rejected_before_matching() {
    let (mut b, cap) = book(16, 16, 10, 1000, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 10, Side::Ask, 1010, 50), AddOrderResult::Added));
    // 1015 is off-tick even though it would cross 1010.
    assert!(matches!(limit(&mut b, &cap, 2, 11, Side::Bid, 1015, 50), AddOrderResult::Rejected));
    assert_eq!(levels(&b), (vec![], vec![(1010, 50, 1)]));
}

// ---------------------------------------------------------------- 6. self-trade

#[test]
fn test_self_trade_skips_own_best_level_and_fills_later_level() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 7, Side::Ask, 100, 50), AddOrderResult::Added));
    assert!(matches!(limit(&mut b, &cap, 2, 8, Side::Ask, 101, 50), AddOrderResult::Added));
    let r = market(&mut b, 3, 7, Side::Bid, 30);
    assert!(is_filled(&r, 0), "trader 7 must skip own ask@100 and fill against trader 8 @101, got {r:?}");
    assert_eq!(levels(&b).1, vec![(100, 50, 1), (101, 20, 1)]);
}

#[test]
fn test_self_trade_skip_for_sell_side() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert!(matches!(limit(&mut b, &cap, 1, 7, Side::Bid, 100, 50), AddOrderResult::Added));
    assert!(matches!(limit(&mut b, &cap, 2, 8, Side::Bid, 99, 50), AddOrderResult::Added));
    let r = market(&mut b, 3, 7, Side::Ask, 30);
    assert!(is_filled(&r, 0), "got {r:?}");
    assert_eq!(levels(&b).0, vec![(100, 50, 1), (99, 20, 1)]);
}

// ---------------------------------------------------------------- precise fills

fn fills(book: &OrderBook) -> Vec<(u64, u64, u64, u64)> {
    book.last_fills().iter().map(|f| (f.taker_order_id, f.maker_order_id, f.px, f.qty)).collect()
}

#[test]
fn test_self_trade_fills_skip_own_makers_fifo_across_levels() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    // ask@100: own(1) other(2) own(3) other(4); ask@101: own(5) other(6)
    for (id, trader, px, qty) in [(1, 7, 100, 10), (2, 8, 100, 20), (3, 7, 100, 30), (4, 9, 100, 40), (5, 7, 101, 50), (6, 8, 101, 60)] {
        assert_eq!(limit(&mut b, &cap, id, trader, Side::Ask, px, qty), AddOrderResult::Added);
    }
    let r = limit(&mut b, &cap, 10, 7, Side::Bid, 101, 100);
    assert_eq!(r, AddOrderResult::Filled { remaining_qty: 0 });
    assert_eq!(fills(&b), vec![(10, 2, 100, 20), (10, 4, 100, 40), (10, 6, 101, 40)]);
    assert!(b.last_fills().iter().all(|f| f.seq == 10));
    assert_eq!(levels(&b).1, vec![(100, 40, 2), (101, 70, 2)]);
    b.check_invariants().unwrap();
    // Skipped own orders kept their priority: another trader hits them first.
    let r = market(&mut b, 11, 8, Side::Bid, 45);
    assert_eq!(r, AddOrderResult::Filled { remaining_qty: 0 });
    assert_eq!(fills(&b), vec![(11, 1, 100, 10), (11, 3, 100, 30), (11, 5, 101, 5)]);
    b.check_invariants().unwrap();
}

#[test]
fn test_self_trade_only_own_liquidity_rests_without_fills() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert_eq!(limit(&mut b, &cap, 1, 7, Side::Ask, 100, 10), AddOrderResult::Added);
    assert_eq!(limit(&mut b, &cap, 2, 7, Side::Bid, 100, 10), AddOrderResult::Added);
    assert!(b.last_fills().is_empty());
    assert_eq!(levels(&b), (vec![(100, 10, 1)], vec![(100, 10, 1)]));
    assert_eq!(market(&mut b, 3, 7, Side::Bid, 10), AddOrderResult::Rejected);
    b.check_invariants().unwrap();
}

#[test]
fn test_partial_fills_fifo_and_sweep_both_sides() {
    for side in [Side::Bid, Side::Ask] {
        let (maker_side, worse) = match side {
            Side::Bid => (Side::Ask, 101),
            Side::Ask => (Side::Bid, 99),
        };
        let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
        for (id, px) in [(1, 100), (2, 100), (3, worse)] {
            assert_eq!(limit(&mut b, &cap, id, 1, maker_side, px, 30), AddOrderResult::Added);
        }
        assert_eq!(market(&mut b, 4, 2, side, 40), AddOrderResult::Filled { remaining_qty: 0 });
        assert_eq!(fills(&b), vec![(4, 1, 100, 30), (4, 2, 100, 10)]);
        // Limit at base takes the partially-filled maker, stops before the worse level, and rests.
        assert_eq!(limit(&mut b, &cap, 5, 2, side, 100, 30), AddOrderResult::Added);
        assert_eq!(fills(&b), vec![(5, 2, 100, 20)]);
        let (bids, asks) = levels(&b);
        let (own, other) = if side == Side::Bid { (bids, asks) } else { (asks, bids) };
        assert_eq!(own, vec![(100, 10, 1)]);
        assert_eq!(other, vec![(worse, 30, 1)]);
        b.check_invariants().unwrap();
        assert_eq!(free_slots(&cap, 16), 14);
    }
}

#[test]
fn test_residual_sweep_records_every_fill() {
    let (mut b, cap) = book(16, 16, 1, 100, 50, 100);
    let mut id = 1;
    for px in [102, 101, 100] {
        assert_eq!(limit(&mut b, &cap, id, 10, Side::Ask, px, 51), AddOrderResult::Added);
        assert_eq!(market(&mut b, id + 1, 20, Side::Bid, 50), AddOrderResult::Filled { remaining_qty: 0 });
        id += 2;
    }
    assert_eq!(limit(&mut b, &cap, 7, 10, Side::Ask, 103, 100), AddOrderResult::Added);
    assert_eq!(market(&mut b, 51, 20, Side::Bid, 100), AddOrderResult::Filled { remaining_qty: 0 });
    assert_eq!(fills(&b), vec![(51, 5, 100, 1), (51, 3, 101, 1), (51, 1, 102, 1), (51, 7, 103, 97)]);
    b.check_invariants().unwrap();
}

#[test]
fn test_fill_buffer_bound_is_pool_capacity_for_tiny_makers() {
    // 16 makers of qty 1 (min == 1) swept by one taker: pool_capacity < max_order_qty bounds the fills.
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    for i in 0..15 {
        assert_eq!(limit(&mut b, &cap, i, 1, Side::Ask, 100 + i, 1), AddOrderResult::Added);
    }
    assert_eq!(market(&mut b, 99, 2, Side::Bid, 1000), AddOrderResult::Filled { remaining_qty: 985 });
    assert_eq!(b.last_fills().len(), 15);
    assert_eq!(levels(&b), (vec![], vec![]));
    assert_eq!(free_slots(&cap, 16), 16);
}

#[test]
fn test_last_fills_cleared_on_rejection() {
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert_eq!(limit(&mut b, &cap, 1, 1, Side::Ask, 100, 10), AddOrderResult::Added);
    assert_eq!(market(&mut b, 2, 2, Side::Bid, 5), AddOrderResult::Filled { remaining_qty: 0 });
    assert_eq!(b.last_fills().len(), 1);
    assert_eq!(market(&mut b, 3, 2, Side::Bid, 0), AddOrderResult::Rejected);
    assert!(b.last_fills().is_empty());
}

// ---------------------------------------------------------------- price window semantics

#[test]
fn test_crossing_limit_outside_own_window_fills_then_cancels_remainder() {
    // Bid window is 100..=85; a bid at 101 can never rest but can take asks.
    let (mut b, cap) = book(16, 16, 1, 100, 1, 1000);
    assert_eq!(limit(&mut b, &cap, 1, 1, Side::Ask, 100, 10), AddOrderResult::Added);
    assert_eq!(limit(&mut b, &cap, 2, 1, Side::Ask, 102, 10), AddOrderResult::Added);
    let r = limit(&mut b, &cap, 3, 2, Side::Bid, 101, 25);
    assert_eq!(r, AddOrderResult::Filled { remaining_qty: 15 });
    assert_eq!(fills(&b), vec![(3, 1, 100, 10)]);
    assert_eq!(levels(&b), (vec![], vec![(102, 10, 1)]));
    assert_eq!(free_slots(&cap, 16), 15, "cancelled remainder must release its slot");
    b.check_invariants().unwrap();

    // Ask side mirror: ask at 99 is outside the ask window.
    assert_eq!(limit(&mut b, &cap, 4, 1, Side::Bid, 99, 10), AddOrderResult::Added);
    assert_eq!(limit(&mut b, &cap, 5, 2, Side::Ask, 99, 30), AddOrderResult::Filled { remaining_qty: 20 });
    assert_eq!(fills(&b), vec![(5, 4, 99, 10)]);
    assert_eq!(free_slots(&cap, 16), 15);
}

#[test]
fn test_non_crossing_limit_outside_window_rejected_without_effect() {
    let (mut b, cap) = book(4, 16, 1, 100, 1, 1000);
    // Ask window 100..=103, bid window 100..=97.
    assert_eq!(limit(&mut b, &cap, 1, 1, Side::Ask, 104, 10), AddOrderResult::Rejected);
    assert_eq!(limit(&mut b, &cap, 2, 1, Side::Bid, 96, 10), AddOrderResult::Rejected);
    assert_eq!(limit(&mut b, &cap, 3, 1, Side::Ask, 103, 10), AddOrderResult::Added);
    assert_eq!(limit(&mut b, &cap, 4, 1, Side::Bid, 97, 10), AddOrderResult::Added);
    assert_eq!(levels(&b), (vec![(97, 10, 1)], vec![(103, 10, 1)]));
    assert_eq!(free_slots(&cap, 16), 14);
}

#[test]
fn test_slot_out_of_pool_range_rejected_before_matching() {
    let (mut b, cap) = book(16, 4, 1, 100, 1, 1000);
    assert_eq!(limit(&mut b, &cap, 1, 1, Side::Ask, 100, 10), AddOrderResult::Added);
    let r = b.add_order(Order::new_limit(2, 2, Side::Bid, 100, 20, 2), 2, 4);
    assert_eq!(r, AddOrderResult::Rejected);
    assert!(b.last_fills().is_empty());
    assert_eq!(levels(&b), (vec![], vec![(100, 10, 1)]));
}

#[test]
fn test_invalid_config_rejected() {
    let cap = || Arc::new(Capacity::new(4));
    let err = |tick_cap, pool, tick, base, min, max| OrderBook::try_new(tick_cap, pool, SYM, tick, base, min, max, cap()).err();
    assert_eq!(err(4, 4, 0, 100, 1, 10), Some(BookConfigError::ZeroTickSize));
    assert_eq!(err(0, 4, 1, 100, 1, 10), Some(BookConfigError::ZeroTickCapacity));
    assert_eq!(err(4, 0, 1, 100, 1, 10), Some(BookConfigError::ZeroPoolCapacity));
    assert_eq!(err(4, 4, 1, 100, 0, 10), Some(BookConfigError::ZeroMinOrderQty));
    assert_eq!(err(4, 4, 1, 100, 11, 10), Some(BookConfigError::MinAboveMax));
    assert_eq!(err(4, 4, 1, u64::MAX - 1, 1, 10), Some(BookConfigError::AskWindowOverflow));
    assert_eq!(err(4, 4, 40, 100, 1, 10), Some(BookConfigError::BidWindowUnderflow));
    assert_eq!(err(4, 4, 1, 3, 1, 10), None);
    assert_eq!(OrderBook::validate_config(4, 4, 1, 2, 1, 10), Err(BookConfigError::BidWindowUnderflow));
}

// ---------------------------------------------------------------- cancel / reuse

#[test]
fn test_cancel_partial_and_filled_orders_and_slot_reuse() {
    let (mut b, cap) = book(16, 2, 1, 100, 1, 1000);
    assert_eq!(limit(&mut b, &cap, 0, 1, Side::Ask, 100, 30), AddOrderResult::Added);
    assert_eq!(limit(&mut b, &cap, 1, 1, Side::Ask, 100, 30), AddOrderResult::Added);
    assert_eq!(market(&mut b, 3, 2, Side::Bid, 40), AddOrderResult::Filled { remaining_qty: 0 });
    assert_eq!(b.cancel_order(0), CancelResult::NotFound, "fully filled maker is gone");
    assert_eq!(levels(&b).1, vec![(100, 20, 1)]);
    assert_eq!(b.cancel_order(1), CancelResult::Cancelled);
    assert_eq!(b.cancel_order(1), CancelResult::NotFound);
    assert_eq!(levels(&b), (vec![], vec![]));
    // Both slots are free again and can be reused (new generations) by the same ids in reverse order.
    assert_eq!(free_slots(&cap, 2), 2);
    assert_eq!(limit(&mut b, &cap, 1, 1, Side::Bid, 99, 5), AddOrderResult::Added);
    assert_eq!(limit(&mut b, &cap, 0, 1, Side::Bid, 99, 6), AddOrderResult::Added);
    assert_eq!(levels(&b).0, vec![(99, 11, 2)]);
    assert_eq!(market(&mut b, 4, 2, Side::Ask, 7), AddOrderResult::Filled { remaining_qty: 0 });
    assert_eq!(fills(&b), vec![(4, 1, 99, 5), (4, 0, 99, 2)]);
    b.check_invariants().unwrap();
}

// ---------------------------------------------------------------- order book thread

type ThreadIo = (rtrb::Producer<MatchCmd>, rtrb::Consumer<OrderBookResponse>, std::thread::JoinHandle<()>);

fn spawn_thread(rb_cap: usize) -> ThreadIo {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(rb_cap);
    let (resp_tx, resp_rx) = rtrb::RingBuffer::new(rb_cap);
    let t = OrderBookThread::new(SYM, 100, 1, 16, 16, 1, 1000, cmd_rx, resp_tx);
    let capacity = Arc::new(Capacity::new(16));
    (cmd_tx, resp_rx, std::thread::spawn(move || t.run(capacity)))
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::hint::spin_loop();
    }
    false
}

#[test]
fn test_snapshot_replies_survive_response_queue_backpressure() {
    let (mut cmd_tx, mut resp_rx, handle) = spawn_thread(1);
    let mut received = 0;
    for _ in 0..3 {
        while cmd_tx.push(MatchCmd::GetSnapshot).is_err() {
            std::hint::spin_loop();
        }
    }
    // Let the thread hit a full response queue before draining it.
    std::thread::sleep(Duration::from_millis(50));
    assert!(wait_until(|| {
        if let Ok(OrderBookResponse::Snapshot { snapshot }) = resp_rx.pop() {
            assert_eq!(snapshot.symbol, SYM);
            received += 1;
        }
        received == 3
    }));
    assert_eq!(received, 3, "snapshot replies were dropped under backpressure");
    while cmd_tx.push(MatchCmd::Shutdown).is_err() {}
    handle.join().unwrap();
}

#[test]
fn test_thread_exits_when_command_producer_abandoned() {
    let (cmd_tx, _resp_rx, handle) = spawn_thread(4);
    drop(cmd_tx);
    assert!(wait_until(|| handle.is_finished()), "book thread must exit when its command queue is abandoned");
    handle.join().unwrap();
}

#[test]
fn test_snapshot_push_gives_up_when_consumer_abandoned() {
    let (mut cmd_tx, resp_rx, handle) = spawn_thread(1);
    drop(resp_rx);
    for _ in 0..2 {
        while cmd_tx.push(MatchCmd::GetSnapshot).is_err() {}
    }
    while cmd_tx.push(MatchCmd::Shutdown).is_err() {}
    assert!(wait_until(|| handle.is_finished()));
    handle.join().unwrap();
}

// ---------------------------------------------------------------- randomized reference model

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[derive(Clone, Copy)]
struct ModelOrder {
    id: u64,
    trader: u64,
    qty: u64,
}

/// Straightforward price-time book with the same contract, used as an oracle.
struct Model {
    base: u64,
    tick: u64,
    levels: u64,
    /// Limit ids must be `< ids` (direct index keys).
    ids: u64,
    min: u64,
    max: u64,
    bids: BTreeMap<u64, VecDeque<ModelOrder>>,
    asks: BTreeMap<u64, VecDeque<ModelOrder>>,
    live: BTreeMap<u64, (Side, u64)>,
}

impl Model {
    fn can_rest(&self, side: Side, px: u64) -> bool {
        let diff = match side {
            Side::Bid => self.base.checked_sub(px),
            Side::Ask => px.checked_sub(self.base),
        };
        diff.is_some_and(|d| d % self.tick == 0 && d / self.tick < self.levels)
    }

    fn add(&mut self, id: u64, trader: u64, side: Side, px: Option<u64>, mut qty: u64) -> (AddOrderResult, Vec<(u64, u64, u64, u64)>) {
        let mut out = Vec::new();
        if qty == 0 || qty < self.min || qty > self.max {
            return (AddOrderResult::Rejected, out);
        }
        if self.live.contains_key(&id) || px.is_some_and(|px| px % self.tick != self.base % self.tick || id >= self.ids) {
            return (AddOrderResult::Rejected, out);
        }
        let opposite = match side {
            Side::Bid => &mut self.asks,
            Side::Ask => &mut self.bids,
        };
        let prices: Vec<u64> = match side {
            Side::Bid => opposite.keys().copied().collect(),
            Side::Ask => opposite.keys().rev().copied().collect(),
        };
        for lpx in prices {
            let crosses = match (side, px) {
                (_, None) => true,
                (Side::Bid, Some(p)) => p >= lpx,
                (Side::Ask, Some(p)) => p <= lpx,
            };
            if qty == 0 || !crosses {
                break;
            }
            let q = opposite.get_mut(&lpx).unwrap();
            let mut i = 0;
            while i < q.len() && qty > 0 {
                if q[i].trader == trader {
                    i += 1;
                    continue;
                }
                let m = q[i].qty.min(qty);
                out.push((id, q[i].id, lpx, m));
                qty -= m;
                q[i].qty -= m;
                if q[i].qty == 0 {
                    self.live.remove(&q[i].id);
                    q.remove(i);
                } else {
                    i += 1;
                }
            }
            if q.is_empty() {
                opposite.remove(&lpx);
            }
        }
        if qty == 0 {
            return (AddOrderResult::Filled { remaining_qty: 0 }, out);
        }
        if let Some(px) = px.filter(|&p| self.can_rest(side, p)) {
            let own = match side {
                Side::Bid => &mut self.bids,
                Side::Ask => &mut self.asks,
            };
            own.entry(px).or_default().push_back(ModelOrder { id, trader, qty });
            self.live.insert(id, (side, px));
            return (AddOrderResult::Added, out);
        }
        let r = if out.is_empty() { AddOrderResult::Rejected } else { AddOrderResult::Filled { remaining_qty: qty } };
        (r, out)
    }

    fn cancel(&mut self, id: u64) -> CancelResult {
        let Some((side, px)) = self.live.remove(&id) else {
            return CancelResult::NotFound;
        };
        let book = match side {
            Side::Bid => &mut self.bids,
            Side::Ask => &mut self.asks,
        };
        let q = book.get_mut(&px).unwrap();
        q.retain(|o| o.id != id);
        if q.is_empty() {
            book.remove(&px);
        }
        CancelResult::Cancelled
    }

    fn levels_of(book: &BTreeMap<u64, VecDeque<ModelOrder>>, bids: bool) -> Vec<(u64, u64, u64)> {
        let v = book.iter().map(|(px, q)| (*px, q.iter().map(|o| o.qty).sum(), q.len() as u64));
        if bids { v.rev().collect() } else { v.collect() }
    }
}

fn run_model(seed: u64, ops: usize) {
    const POOL: u64 = 32;
    let (base, tick, tick_cap, min, max) = (1000, 5, 12, 3, 40);
    let (mut b, cap) = book(tick_cap as usize, POOL as usize, tick, base, min, max);
    let mut m = Model {
        base,
        tick,
        levels: tick_cap,
        ids: POOL,
        min,
        max,
        bids: BTreeMap::new(),
        asks: BTreeMap::new(),
        live: BTreeMap::new(),
    };
    let mut rng = Rng(seed);

    for step in 0..ops {
        let seq = step as u64;
        let roll = rng.below(100);
        let trader = 1 + rng.below(4);
        let side = if rng.below(2) == 0 { Side::Bid } else { Side::Ask };
        let qty = if rng.below(10) == 0 { rng.below(50) } else { min + rng.below(max - min + 1) };

        if roll < 60 {
            // Mostly a free in-range id; sometimes any in-range id (possibly live) or an out-of-range one.
            let id = match rng.below(20) {
                0 => POOL + rng.below(POOL),
                1 => rng.below(POOL),
                _ => {
                    let start = rng.below(POOL);
                    (0..POOL).map(|k| (start + k) % POOL).find(|k| !m.live.contains_key(k)).unwrap_or(start)
                }
            };
            let px = if rng.below(8) == 0 { base - 100 + rng.below(200) } else { base - 18 * tick + rng.below(37) * tick };
            let slot = match cap.reserve() {
                ReserveResult::Reserved(s) => s,
                ReserveResult::CapacityExceeded => continue, // ingress would reject before the book
            };
            let expected = m.add(id, trader, side, Some(px), qty);
            let got = b.add_order(Order::new_limit(id, trader, side, px, qty, seq), seq, slot);
            assert_eq!((got, fills(&b)), expected, "seed {seed} step {step}: limit {id} {side:?} {px} {qty} trader {trader}");
        } else if roll < 80 {
            // Market ids are unbounded fill tags; a live one is still a duplicate.
            let id = rng.below(4 * POOL);
            let expected = m.add(id, trader, side, None, qty);
            let got = b.add_order(Order::new_market(id, trader, side, qty, seq), seq, 0);
            assert_eq!((got, fills(&b)), expected, "seed {seed} step {step}: market {id} {side:?} {qty} trader {trader}");
        } else {
            let id = rng.below(POOL + 4);
            assert_eq!(b.cancel_order(id), m.cancel(id), "seed {seed} step {step}: cancel {id}");
        }

        let snap = b.snapshot();
        let bids: Vec<_> = snap.bids.iter().map(|l| (l.px, l.qty, l.count)).collect();
        let asks: Vec<_> = snap.asks.iter().map(|l| (l.px, l.qty, l.count)).collect();
        assert_eq!(bids, Model::levels_of(&m.bids, true), "seed {seed} step {step}: bids");
        assert_eq!(asks, Model::levels_of(&m.asks, false), "seed {seed} step {step}: asks");
        if let Err(e) = b.check_invariants() {
            panic!("seed {seed} step {step}: {e}");
        }
        if step % 50 == 0 {
            assert_eq!(free_slots(&cap, POOL), POOL - m.live.len() as u64, "seed {seed} step {step}: leaked slots");
        }
    }
}

#[test]
fn test_randomized_against_reference_model() {
    for seed in [0x9E37_79B9_7F4A_7C15, 1, 42, 0xDEAD_BEEF, 7_777_777] {
        run_model(seed, 3000);
    }
}
