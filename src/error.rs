#[derive(Debug)]
pub enum Error {
    ThreadSpawn(std::io::Error),
    BufferFull,
    EngineStopped,
    OrderNotFound(u64),
    CapacityCollision,
}
