use mtch::core::BookSnapshot;
use mtch::core::messages::OrderBookResponse;
use mtch::ingress::CancelOrderResult;
use mtch::{Engine, EngineConfig};

pub fn setup_engine(base_px: u64, tick_size: u64) -> Engine {
    let config = EngineConfig::new(1100, tick_size, base_px, 1024, 64, 131072, 50, 5000);
    Engine::new(config).expect("failed to create engine")
}

pub fn assert_book(snapshot: &BookSnapshot, expected_bids: &[(u64, u64)], expected_asks: &[(u64, u64)]) {
    assert_eq!(
        snapshot.bids.len(),
        expected_bids.len(),
        "Bid level count mismatch. Expected {} levels, got {}. Bids: {:?}",
        expected_bids.len(),
        snapshot.bids.len(),
        snapshot.bids
    );
    assert_eq!(
        snapshot.asks.len(),
        expected_asks.len(),
        "Ask level count mismatch. Expected {} levels, got {}. Asks: {:?}",
        expected_asks.len(),
        snapshot.asks.len(),
        snapshot.asks
    );

    for (i, (expected_px, expected_qty)) in expected_bids.iter().enumerate() {
        let bid = &snapshot.bids[i];
        assert_eq!(bid.px, *expected_px, "Bid level {} price mismatch. Expected {}, got {}", i, expected_px, bid.px);
        assert_eq!(bid.qty, *expected_qty, "Bid level {} qty mismatch. Expected {}, got {}", i, expected_qty, bid.qty);
    }

    for (i, (expected_px, expected_qty)) in expected_asks.iter().enumerate() {
        let ask = &snapshot.asks[i];
        assert_eq!(ask.px, *expected_px, "Ask level {} price mismatch. Expected {}, got {}", i, expected_px, ask.px);
        assert_eq!(ask.qty, *expected_qty, "Ask level {} qty mismatch. Expected {}, got {}", i, expected_qty, ask.qty);
    }
}

pub fn assert_added(resp: OrderBookResponse) {
    match resp {
        OrderBookResponse::Added { .. } => {}
        _ => panic!("Expected Added response, got {:?}", resp),
    }
}

pub fn assert_rejected(resp: OrderBookResponse) {
    match resp {
        OrderBookResponse::Rejected { .. } => {}
        _ => panic!("Expected Rejected response, got {:?}", resp),
    }
}

pub fn assert_cancelled(resp: CancelOrderResult) {
    match resp {
        CancelOrderResult::Cancelled => {}
        _ => panic!("Expected Cancelled response, got {:?}", resp),
    }
}
