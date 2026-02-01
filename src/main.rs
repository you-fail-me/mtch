use mtch::core::Side;
use mtch::{Engine, EngineConfig};

fn main() {
    let config = EngineConfig::new(100, 1, 50000, 32768, 2048, 1048576);
    let mut engine = Engine::new(config).expect("failed to create engine");

    // Limit orders
    let lo1 = engine.add_limit_order(1, 100, Side::Bid, 50000, 10);
    println!("[ING] Add LO1: {:?}", lo1);
    let lo2 = engine.add_limit_order(2, 101, Side::Bid, 49900, 5);
    println!("[ING] Add LO2: {:?}", lo2);
    let lo3 = engine.add_limit_order(3, 102, Side::Ask, 50100, 8);
    println!("[ING] Add LO3: {:?}", lo3);
    let lo4 = engine.add_limit_order(4, 103, Side::Ask, 50200, 12);
    println!("[ING] Add LO4: {:?}", lo4);
    let lo5 = engine.add_limit_order(5, 103, Side::Ask, 50110, 12);
    println!("[ING] Add LO5: {:?}", lo5);
    let lo6 = engine.add_limit_order(6, 105, Side::Ask, 51000, 20);
    println!("[ING] Add LO6: {:?}", lo6);

    let snap = engine.snapshot().unwrap();
    println!("[ING] Book: {:?}", snap);

    let res = engine.add_limit_order(7, 104, Side::Bid, 50150, 15);
    println!("[ING] Fill: {:?}", res);

    let snap = engine.snapshot().unwrap();
    println!("[ING] Book: {:?}", snap);

    // Cancel
    let cancel_result = engine.cancel_order(6);
    println!("[ING] Cancel result: {:?}", cancel_result);

    let snap = engine.snapshot().unwrap();
    println!("[ING] Book after cancel: {:?}", snap);

    // Market orders
    let mo1 = engine.add_market_order(100, 200, Side::Bid, 15);
    println!("[ING] Market order 1: {:?}", mo1);

    let snap = engine.snapshot().unwrap();
    println!("[ING] Book after market buy: {:?}", snap);

    let mo2 = engine.add_market_order(101, 201, Side::Ask, 20);
    println!("[ING] Market order 2: {:?}", mo2);

    let snap = engine.snapshot().unwrap();
    println!("[ING] Book after market sell: {:?}", snap);

    engine.stop().unwrap();
}
