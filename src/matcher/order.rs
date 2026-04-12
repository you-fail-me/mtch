#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum OrderType {
    Limit,
    Market,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Handle {
    pub idx: usize,
    pub generation: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct OrderLocation {
    pub side: Side,
    pub px: u64,
    pub handle: Handle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: u64,
    pub trader_id: u64,
    pub side: Side,
    pub px: u64,
    pub qty: u64,
    pub original_qty: u64,
    pub ts: u64,
    pub order_type: OrderType,
    pub(crate) prev: Option<Handle>,
    pub(crate) next: Option<Handle>,
}

impl Order {
    pub fn new_limit(id: u64, trader_id: u64, side: Side, px: u64, qty: u64, ts: u64) -> Self {
        Self {
            id,
            trader_id,
            side,
            px,
            qty,
            original_qty: qty,
            ts,
            order_type: OrderType::Limit,
            prev: None,
            next: None,
        }
    }

    pub fn new_market(id: u64, trader_id: u64, side: Side, qty: u64, ts: u64) -> Self {
        Self {
            id,
            trader_id,
            side,
            px: 0,
            qty,
            original_qty: qty,
            ts,
            order_type: OrderType::Market,
            prev: None,
            next: None,
        }
    }
}
