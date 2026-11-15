use std::sync::Arc;
use std::thread::{self, JoinHandle};

use core_affinity::{CoreId, get_core_ids, set_for_current};
use thread_priority::{ThreadBuilderExt, ThreadPriority};

use crate::error::Error;
use crate::ingress::{CancelOrderResult, Ingress, IngressConfig};
use crate::matcher::Capacity;
use crate::matcher::OrderBookThread;
use crate::matcher::messages::OrderBookResponse;
use crate::matcher::order::Side;
use crate::matcher::order_book::BookSnapshot;
use crate::wal::WalThread;

/// Three distinct logical CPUs for (main, wal, book), or `None` (run unpinned) if fewer than three
/// are reported. Stepping by 2 avoids SMT siblings only when the OS numbers them adjacently; the
/// physical topology is not known, so distinct physical cores are not guaranteed.
fn pick_cores() -> Option<(CoreId, CoreId, CoreId)> {
    let cores = get_core_ids()?;
    let step = if cores.len() >= 5 { 2 } else { 1 };
    Some((*cores.first()?, *cores.get(step)?, *cores.get(2 * step)?))
}

/// Joins a pipeline thread, surfacing a panic as `ThreadPanicked`.
fn join(handle: Option<JoinHandle<()>>, role: &'static str) -> Result<(), Error> {
    match handle {
        Some(handle) => handle.join().map_err(|_panic| Error::ThreadPanicked(role)),
        None => Ok(()),
    }
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub symbol: u32,
    pub tick_size: u64,
    pub base_px: u64,
    pub rb_cap: u64,
    pub tick_capacity: u64,
    pub pool_capacity: u64,
    pub min_order_qty: u64,
    pub max_order_qty: u64,
}

pub struct Engine {
    ingress: Ingress,
    wal_handle: Option<JoinHandle<()>>,
    book_handle: Option<JoinHandle<()>>,
    running: bool,
}

impl EngineConfig {
    /// # Errors
    ///
    /// `InvalidConfig` naming the first violated rule.
    pub fn validate(&self) -> Result<(), Error> {
        let check = |ok: bool, msg: &'static str| if ok { Ok(()) } else { Err(Error::InvalidConfig(msg)) };
        check(self.tick_size > 0, "tick_size must be > 0")?;
        check(self.min_order_qty > 0, "min_order_qty must be > 0")?;
        check(self.min_order_qty <= self.max_order_qty, "min_order_qty must be <= max_order_qty")?;
        check(self.rb_cap > 0 && self.tick_capacity > 0 && self.pool_capacity > 0, "rb_cap, tick_capacity and pool_capacity must be > 0")?;
        let span = (self.tick_capacity - 1).checked_mul(self.tick_size).ok_or(Error::InvalidConfig("(tick_capacity - 1) * tick_size overflows u64"))?;
        check(self.base_px.checked_add(span).is_some(), "base_px + (tick_capacity - 1) * tick_size overflows u64")?;
        check(span <= self.base_px, "bid range below zero: need base_px >= (tick_capacity - 1) * tick_size")
    }
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, Error> {
        config.validate()?;
        let cores = pick_cores();

        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(config.rb_cap as usize);
        let (resp_tx, resp_rx) = rtrb::RingBuffer::new(config.rb_cap as usize);
        let (wal_tx, wal_rx) = rtrb::RingBuffer::new(config.rb_cap as usize);
        let (wal_ack_tx, wal_ack_rx) = rtrb::RingBuffer::new(config.rb_cap as usize);

        // Pin Main Thread to Core 0
        if let Some((main_core, _, _)) = cores {
            set_for_current(main_core);
        }

        // Create shared Capacity for slot reservation
        let capacity = Arc::new(Capacity::new(config.pool_capacity));
        let capacity_for_ingress = Arc::clone(&capacity);
        let capacity_for_book = Arc::clone(&capacity);

        let wal_thread = WalThread::new(cmd_rx, resp_tx, wal_ack_rx, wal_tx);
        let wal_handle = thread::Builder::new()
            .name("mtch-wal".to_string())
            .spawn_with_priority(ThreadPriority::Max, move |_| {
                if let Some((_, wal_core, _)) = cores {
                    set_for_current(wal_core);
                }
                wal_thread.run()
            })
            .map_err(Error::ThreadSpawn)?;

        let ob_thread = OrderBookThread::new(
            config.symbol,
            config.base_px,
            config.tick_size,
            config.tick_capacity,
            config.pool_capacity,
            config.min_order_qty,
            config.max_order_qty,
            wal_rx,
            wal_ack_tx,
        );
        let book_handle = thread::Builder::new().name("mtch-book".to_string()).spawn_with_priority(ThreadPriority::Max, move |_| {
            if let Some((_, _, book_core)) = cores {
                set_for_current(book_core);
            }
            ob_thread.run(capacity_for_book)
        });

        let ingress_config = IngressConfig {
            min_order_qty: config.min_order_qty,
            max_order_qty: config.max_order_qty,
        };
        let ingress = Ingress::new(cmd_tx, resp_rx, ingress_config, capacity_for_ingress);

        let book_handle = match book_handle {
            Ok(handle) => handle,
            Err(err) => {
                // The failed closure dropped the book's queue ends; dropping ingress lets the WAL exit.
                drop(ingress);
                join(Some(wal_handle), "wal")?;
                return Err(Error::ThreadSpawn(err));
            }
        };

        Ok(Self {
            ingress,
            wal_handle: Some(wal_handle),
            book_handle: Some(book_handle),
            running: true,
        })
    }

    pub fn add_limit_order(&mut self, order_id: u64, trader_id: u64, side: Side, px: u64, qty: u64) -> Result<OrderBookResponse, Error> {
        if !self.running {
            return Err(Error::EngineStopped);
        }
        self.ingress.add_limit_order(order_id, trader_id, side, px, qty)
    }

    pub fn add_market_order(&mut self, order_id: u64, trader_id: u64, side: Side, qty: u64) -> Result<OrderBookResponse, Error> {
        if !self.running {
            return Err(Error::EngineStopped);
        }
        self.ingress.add_market_order(order_id, trader_id, side, qty)
    }

    pub fn cancel_order(&mut self, order_id: u64) -> Result<CancelOrderResult, Error> {
        if !self.running {
            return Err(Error::EngineStopped);
        }
        self.ingress.cancel_order(order_id)
    }

    pub fn snapshot(&mut self) -> Result<BookSnapshot, Error> {
        if !self.running {
            return Err(Error::EngineStopped);
        }
        self.ingress.snapshot()
    }

    /// Returns `ThreadPanicked` if a pipeline thread panicked.
    pub fn stop(&mut self) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }
        self.running = false;

        // Err only means the WAL already exited; the joins still report a panic.
        let _ = self.ingress.shutdown();

        let wal = join(self.wal_handle.take(), "wal");
        let book = join(self.book_handle.take(), "book");
        wal.and(book)
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
