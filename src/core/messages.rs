use crate::core::order::{OrderType, Side};
use crate::core::order_book::{BookSnapshot, Fill};

#[derive(Debug, Clone)]
pub enum IngressCmd {
    AddLimitOrder { order_id: u64, trader_id: u64, side: Side, px: u64, qty: u64 },
    AddMarketOrder { order_id: u64, trader_id: u64, side: Side, qty: u64 },
    CancelOrder { order_id: u64 },
    GetSnapshot,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum OrderBookResponse {
    Filled { order_id: u64, order_type: OrderType, fills: Vec<Fill>, remaining_qty: u64 },
    Added { order_id: u64 },
    Rejected { order_id: u64 },
    Cancelled { order_id: u64 },
    NotFound { order_id: u64 },
    Snapshot { snapshot: BookSnapshot },
    Error { msg: String },
}

#[derive(Debug, Clone)]
pub enum MatchCmd {
    AddLimitOrder { order_id: u64, trader_id: u64, side: Side, px: u64, qty: u64, seq: u64 },
    AddMarketOrder { order_id: u64, trader_id: u64, side: Side, qty: u64, seq: u64 },
    CancelOrder { order_id: u64, seq: u64 },
    GetSnapshot,
    Shutdown,
}
