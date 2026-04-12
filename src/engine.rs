use std::sync::Arc;
use std::thread::{self, JoinHandle};

use core_affinity::{CoreId, get_core_ids, set_for_current};
use num_cpus;
use thread_priority::{ThreadBuilderExt, ThreadPriority};

use crate::error::Error;
use crate::ingress::{CancelOrderResult, Ingress, IngressConfig};
use crate::matcher::Capacity;
use crate::matcher::OrderBookThread;
use crate::matcher::messages::OrderBookResponse;
use crate::matcher::order::Side;
use crate::matcher::order_book::BookSnapshot;
use crate::wal::WalThread;

fn get_separate_physical_cores() -> (CoreId, CoreId, CoreId) {
    let physical_cores = num_cpus::get_physical();
    if physical_cores < 3 {
        panic!("Need at least 3 physical cores, found: {}", physical_cores);
    }

    let all_cores = get_core_ids().unwrap();
    // Step by 2 to avoid hyperthreaded siblings
    let main_core = all_cores[0];
    let wal_core = all_cores[2];
    let book_core = all_cores[4];

    (main_core, wal_core, book_core)
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub symbol: u32,
    pub tick_size: u64,
    pub base_px: u64,
    pub rb_cap: usize,
    pub tick_capacity: usize,
    pub pool_capacity: usize,
    pub min_order_qty: u64,
    pub max_order_qty: u64,
}

pub struct Engine {
    ingress: Ingress,
    wal_handle: Option<JoinHandle<()>>,
    book_handle: Option<JoinHandle<()>>,
    running: bool,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, Error> {
        // Get separate physical cores
        let (main_core, wal_core, book_core) = get_separate_physical_cores();

        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(config.rb_cap);
        let (resp_tx, resp_rx) = rtrb::RingBuffer::new(config.rb_cap);
        let (wal_tx, wal_rx) = rtrb::RingBuffer::new(config.rb_cap);
        let (wal_ack_tx, wal_ack_rx) = rtrb::RingBuffer::new(config.rb_cap);

        // Pin Main Thread to Core 0
        set_for_current(main_core);

        // Create shared Capacity for slot reservation
        let capacity = Arc::new(Capacity::new(config.pool_capacity as u64));
        let capacity_for_ingress = Arc::clone(&capacity);
        let capacity_for_book = Arc::clone(&capacity);

        let wal_thread = WalThread::new(cmd_rx, resp_tx, wal_ack_rx, wal_tx);
        let wal_handle = thread::Builder::new()
            .name("mtch-wal".to_string())
            .spawn_with_priority(ThreadPriority::Max, move |_| {
                set_for_current(wal_core);
                wal_thread.run()
            })
            .unwrap();

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
        let book_handle = thread::Builder::new()
            .name("mtch-book".to_string())
            .spawn_with_priority(ThreadPriority::Max, move |_| {
                set_for_current(book_core);
                ob_thread.run(capacity_for_book)
            })
            .unwrap();

        let ingress_config = IngressConfig {
            min_order_qty: config.min_order_qty,
            max_order_qty: config.max_order_qty,
        };
        let ingress = Ingress::new(cmd_tx, resp_rx, ingress_config, capacity_for_ingress);

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
        self.ingress.add_limit_order(order_id, trader_id, side, px, qty).map_err(|_| Error::BufferFull)
    }

    pub fn add_market_order(&mut self, order_id: u64, trader_id: u64, side: Side, qty: u64) -> Result<OrderBookResponse, Error> {
        if !self.running {
            return Err(Error::EngineStopped);
        }
        self.ingress.add_market_order(order_id, trader_id, side, qty).map_err(|_| Error::BufferFull)
    }

    pub fn cancel_order(&mut self, order_id: u64) -> Result<CancelOrderResult, Error> {
        if !self.running {
            return Err(Error::EngineStopped);
        }
        self.ingress.cancel_order(order_id).map_err(|_| Error::BufferFull)
    }

    pub fn snapshot(&mut self) -> Result<BookSnapshot, Error> {
        if !self.running {
            return Err(Error::EngineStopped);
        }
        self.ingress.snapshot().map_err(|_| Error::BufferFull)
    }

    pub fn stop(&mut self) -> Result<(), Error> {
        if !self.running {
            return Ok(());
        }

        let _ = self.ingress.shutdown();

        if let Some(handle) = self.wal_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.book_handle.take() {
            let _ = handle.join();
        }

        self.running = false;
        Ok(())
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
