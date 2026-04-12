use crate::matcher::order_book::BookSnapshot;

#[derive(Debug, Clone)]
pub enum WalEntry {
    OrderAdd { order_id: u64 },
    OrderCancel { order_id: u64 },
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct WalAck {
    pub order_id: u64,
    pub entry_type: WalEntryType,
    pub seq: u64,
    pub snapshot: Option<BookSnapshot>,
}

#[derive(Debug, Clone, Copy)]
pub enum WalEntryType {
    Add,
    Cancel,
    // Dummy, for debug
    Query,
}
