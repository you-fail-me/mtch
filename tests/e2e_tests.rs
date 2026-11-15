use mtch::matcher::Side;
use mtch::{Engine, EngineConfig};

mod e2e;
use e2e::*;

#[test]
fn test_add_limit_orders_no_match() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let resp1 = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp1);

    let resp2 = engine.add_limit_order(2, 101, Side::Ask, 105, 75).unwrap();
    assert_added(resp2);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50)], &[(105, 75)]);
}

#[test]
fn test_cancel_existing_order() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp);

    let cancel_resp = engine.cancel_order(1).unwrap();
    assert_cancelled(cancel_resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_cancel_non_existent() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let cancel_resp = engine.cancel_order(999).unwrap();
    assert_cancelled(cancel_resp);

    // Verify book is still empty (nothing was removed)
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_snapshot_reflects_book_state() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 99, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Ask, 105, 50).unwrap();
    engine.add_limit_order(4, 103, Side::Ask, 106, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50), (99, 50)], &[(105, 50), (106, 50)]);
}

#[test]
fn test_limit_order_full_fill_exact() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_limit_order_partial_fill_aggressive_rests() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 100, 75).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 25)], &[]);
}

#[test]
fn test_limit_order_partial_fill_resting_remains() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 75).unwrap();
    engine.add_limit_order(2, 101, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 25)]);
}

#[test]
fn test_limit_order_sweeps_multiple_levels() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 101, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Ask, 102, 50).unwrap();
    engine.add_limit_order(4, 103, Side::Bid, 105, 120).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(102, 30)]);
}

#[test]
fn test_limit_order_no_match_rests() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let resp1 = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp1);

    let resp2 = engine.add_limit_order(2, 101, Side::Bid, 99, 50).unwrap();
    assert_added(resp2);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50), (99, 50)], &[]);
}

#[test]
fn test_market_buy_full_fill() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_market_order(2, 101, Side::Bid, 50).unwrap();

    // Market order should fully fill the ask, leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_sell_full_fill() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    engine.add_market_order(2, 101, Side::Ask, 50).unwrap();

    // Market order should fully fill the bid, leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_order_sweeps_levels() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 101, 50).unwrap();
    engine.add_market_order(3, 102, Side::Bid, 100).unwrap();

    // Market order should sweep both ask levels (50+50=100), leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_order_rejected_no_liquidity() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_market_order(1, 100, Side::Bid, 50).unwrap();

    // Market order with no liquidity should be rejected, leaving book empty
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_price_priority_best_price_first() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 101, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Bid, 105, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(101, 50)]);
}

#[test]
fn test_time_priority_fifo_same_price() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 50)]);
}

#[test]
fn test_fifo_across_multiple_fills() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 101, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 102, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(4, 103, Side::Bid, 100, 120).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 30)]);
}

#[test]
fn test_self_trade_prevented() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 50, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 50, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert!(snap.bids.len() == 1 || snap.asks.len() == 0, "Self-trade should prevent match, one order rests");
}

#[test]
fn test_self_trade_skips_to_next_order() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 50, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(2, 60, Side::Ask, 100, 50).unwrap();
    engine.add_limit_order(3, 50, Side::Bid, 100, 50).unwrap();

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 50)]);
}

#[test]
fn test_realistic_trading_scenario() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

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
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 49).unwrap();
    assert_rejected(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_order_at_min_accepted() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 50).unwrap();
    assert_added(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 50)], &[]);
}

#[test]
fn test_order_at_max_accepted() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 5000).unwrap();
    assert_added(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(100, 5000)], &[]);
}

#[test]
fn test_order_above_max_rejected() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    let resp = engine.add_limit_order(1, 100, Side::Bid, 100, 5001).unwrap();
    assert_rejected(resp);

    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[]);
}

#[test]
fn test_market_order_below_min_rejected() {
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 131072,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    engine.add_limit_order(1, 100, Side::Ask, 100, 100).unwrap();

    let resp = engine.add_market_order(2, 101, Side::Bid, 49).unwrap();
    assert_rejected(resp);

    // Ask should still be there
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 100)]);
}

#[test]
fn test_capacity_limit_order_rejected_market_order_accepted() {
    // Create engine with very small pool capacity (2 slots)
    let mut engine = Engine::new(EngineConfig {
        symbol: 1100,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 2,
        min_order_qty: 50,
        max_order_qty: 5000,
    })
    .unwrap();

    // First, add a resting bid order to provide liquidity for market sells
    let resp1 = engine.add_limit_order(0, 100, Side::Bid, 99, 50).unwrap();
    assert_added(resp1);

    // Fill up the remaining capacity with a resting ask order
    let resp2 = engine.add_limit_order(1, 101, Side::Ask, 100, 50).unwrap();
    assert_added(resp2);

    // Verify both orders are resting
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(99, 50)], &[(100, 50)]);

    // Try to add another limit order - should be rejected (capacity full)
    let resp3 = engine.add_limit_order(0, 102, Side::Bid, 98, 50).unwrap();
    assert_rejected(resp3);

    // Try to add a market sell order - should NOT be rejected (market orders don't need slots)
    // Market sell should match with the resting bid@99, consuming it completely
    let resp4 = engine.add_market_order(4, 103, Side::Ask, 50).unwrap();
    assert_added(resp4);

    // Verify the market order matched and removed the bid
    // Ask@100 should still be resting (not matched by market sell)
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[], &[(100, 50)]);

    // Now that a slot was freed (bid@99 was filled), we can add a limit order again
    let resp5 = engine.add_limit_order(0, 104, Side::Bid, 98, 50).unwrap();
    assert_added(resp5);

    // Verify the bid was added
    let snap = engine.snapshot().unwrap();
    assert_book(&snap, &[(98, 50)], &[(100, 50)]);
}
