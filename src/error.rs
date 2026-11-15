#[derive(Debug)]
pub enum Error {
    ThreadSpawn(std::io::Error),
    BufferFull,
    EngineStopped,
    OrderNotFound(u64),
    CapacityCollision,
    DuplicateOrderId(u64),
    OrderIdOutOfRange(u64),
    InvalidConfig(&'static str),
    ThreadPanicked(&'static str),
    SnapshotUnavailable,
}
