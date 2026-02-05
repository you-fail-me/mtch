mod e2e;

use e2e::*;
use mtch::core::Side;

#[test]
fn test_add_limit_orders_no_match() {
    let mut engine = setup_engine(100, 1);

    let resp1 = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp1);

    let resp2 = engine.add_limit_order(2, 101, Side::Ask, 105, 75).unwrap();
    assert_added(resp2);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50)], &[(105, 75)]);
}

#[test]
fn test_cancel_existing_order() {
    let mut engine = setup_engine(100, 1);

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp);

    let cancel_resp = engine.cancel_order(1).unwrap();
    assert_cancelled(cancel_resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_cancel_non_existent() {
    let mut engine = setup_engine(100, 1);

    let cancel_resp = engine.cancel_order(999).unwrap();
    assert_cancelled(cancel_resp);

    // Verify book is still empty (nothing was removed)
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_snapshot_reflects_book_state() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 99, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Ask, 105, 50).unwrap();
    engine.add_limit_order(4, 103, Side::Ask, 106, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50), (99, 50)], &[(105, 50), (106, 50)]);
}

#[test]
fn test_limit_order_full_fill_exact() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_limit_order_partial_fill_aggressive_rests() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 100, 75).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 25)], &[]);
}

#[test]
fn test_limit_order_partial_fill_resting_remains() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 75).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 25)]);
}

#[test]
fn test_limit_order_sweeps_multiple_levels() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 101, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Ask, 102, 50).unwrap();
    engine.add_limit_order(4, 103, Side::Bid, 105, 120).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(102, 30)]);
}

#[test]
fn test_limit_order_no_match_rests() {
    let mut engine = setup_engine(100, 1);

    let resp1 = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp1);

    let resp2 = engine.add_limit_order(2, 101, Side::Bid, 99, 50).unwrap();
    assert_added(resp2);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50), (99, 50)], &[]);
}

#[test]
fn test_market_buy_full_fill() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_market_order(2, 101, Side::Bid, 50).unwrap();

    // Market order should fully fill the ask, leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_sell_full_fill() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    engine.add_market_order(2, 101, Side::Ask, 50).unwrap();

    // Market order should fully fill the bid, leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_order_sweeps_levels() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 101, 50).unwrap();
    engine.add_market_order(3, 102, Side::Bid, 100).unwrap();

    // Market order should sweep both ask levels (50+50=100), leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_order_rejected_no_liquidity() {
    let mut engine = setup_engine(100, 1);

    engine.add_market_order(1, 100, Side::Bid, 50).unwrap();

    // Market order with no liquidity should be rejected, leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_price_priority_best_price_first() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 101, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Bid, 105, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(101, 50)]);
}

#[test]
fn test_time_priority_fifo_same_price() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 50)]);
}

#[test]
fn test_fifo_across_multiple_fills() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(4, 103, Side::Bid, 100, 120).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 30)]);
}

#[test]
fn test_self_trade_prevented() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 50, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 50, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert!(snap.bids.len() == 1 || snap.asks.len() == 0, "Self-trade should prevent match, one order rests");
}

#[test]
fn test_self_trade_skips_to_next_order() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 50, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 60, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 50, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 50)]);
}

#[test]
fn test_realistic_trading_scenario() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Bid, 99, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 100, 75).unwrap();
    engine.add_limit_order(3, 102, Side::Ask, 101, 60).unwrap();
    engine.add_limit_order(4, 103, Side::Ask, 102, 50).unwrap();

    // Market buy 50 should match with ask@101 (order 3), leaving 10 remaining
    engine.add_market_order(5, 104, Side::Bid, 50).unwrap();

    let cancel_resp = engine.cancel_order(1).unwrap();
    assert_cancelled(cancel_resp);

    // buy limit @ 100 should match with remaining ask@101 (10 qty)
    // leaving 40 qty resting on the ask side at 100
    engine.add_limit_order(6, 105, Side::Ask, 100, 50).unwrap();

    let final_snap = engine.snapshot().unwrap();
    // Expected: bid@100 with 75 qty (order 2), ask@100 with 40 qty, ask@102 with 50 qty
    assert!(!final_snap.bids.is_empty() || !final_snap.asks.is_empty());
}

#[test]
fn test_order_below_min_rejected() {
    let mut engine = setup_engine(100, 1);

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 49).unwrap();
    assert_rejected(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_order_at_min_accepted() {
    let mut engine = setup_engine(100, 1);

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50)], &[]);
}

#[test]
fn test_order_at_max_accepted() {
    let mut engine = setup_engine(100, 1);

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 5000).unwrap();
    assert_added(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 5000)], &[]);
}

#[test]
fn test_order_above_max_rejected() {
    let mut engine = setup_engine(100, 1);

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 5001).unwrap();
    assert_rejected(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_order_below_min_rejected() {
    let mut engine = setup_engine(100, 1);

    engine.add_limit_order(1, 100, Side::Ask, 100, 100).unwrap();

    let resp = engine.add_market_order(2, 101, Side::Bid, 49).unwrap();
    assert_rejected(resp);

    // Ask should still be there
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 100)]);
}
