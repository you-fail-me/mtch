#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use mtch::matcher::messages::{IngressCmd, MatchCmd, OrderBookResponse};
use mtch::matcher::{Capacity, ReserveResult, Side};
use mtch::wal::{WalAck, WalThread};
use mtch::{Engine, EngineConfig, Error, ingress::Ingress, ingress::IngressConfig};
use rtrb::RingBuffer;

fn config() -> EngineConfig {
    EngineConfig {
        symbol: 1,
        tick_size: 1,
        base_px: 100,
        rb_cap: 1024,
        tick_capacity: 64,
        pool_capacity: 128,
        min_order_qty: 1,
        max_order_qty: 1000,
    }
}

fn ingress_config() -> IngressConfig {
    IngressConfig { min_order_qty: 1, max_order_qty: 1000 }
}

#[test]
fn test_capacity_concurrent_reservations_are_unique() {
    const CAP: u64 = 65;
    let cap = Arc::new(Capacity::new(CAP));
    let owned: Arc<Vec<AtomicBool>> = Arc::new((0..CAP).map(|_| AtomicBool::new(false)).collect());

    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (cap, owned) = (Arc::clone(&cap), Arc::clone(&owned));
            thread::spawn(move || {
                let mut held = Vec::new();
                for i in 0..20_000 {
                    if let ReserveResult::Reserved(id) = cap.reserve() {
                        assert!(!owned[id as usize].swap(true, Ordering::SeqCst), "slot {id} reserved twice");
                        held.push(id);
                    }
                    if (i % 3 == 0 || held.len() > 20)
                        && let Some(id) = held.pop()
                    {
                        owned[id as usize].store(false, Ordering::SeqCst);
                        cap.release(id);
                    }
                }
                for id in held {
                    owned[id as usize].store(false, Ordering::SeqCst);
                    cap.release(id);
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    assert_eq!(cap.reserved_count(), 0);
}

#[test]
fn test_ingress_rolls_back_reservation_when_enqueue_fails() {
    let cap = Arc::new(Capacity::new(4));

    // Full command queue -> BufferFull, reservation returned.
    let (mut cmd_tx, _cmd_rx) = RingBuffer::new(1);
    cmd_tx.push(IngressCmd::GetSnapshot).unwrap();
    let (_resp_tx, resp_rx) = RingBuffer::<WalAck>::new(1);
    let mut ingress = Ingress::new(cmd_tx, resp_rx, ingress_config(), Arc::clone(&cap));
    assert!(matches!(ingress.add_limit_order(1, 1, Side::Bid, 100, 10), Err(Error::BufferFull)));
    assert_eq!(cap.reserved_count(), 0);

    // WAL gone -> EngineStopped, reservation returned, no hang.
    let (cmd_tx, cmd_rx) = RingBuffer::new(1);
    let (resp_tx, resp_rx) = RingBuffer::<WalAck>::new(1);
    drop((cmd_rx, resp_tx));
    let mut ingress = Ingress::new(cmd_tx, resp_rx, ingress_config(), Arc::clone(&cap));
    assert!(matches!(ingress.add_limit_order(2, 1, Side::Bid, 100, 10), Err(Error::EngineStopped)));
    assert!(matches!(ingress.cancel_order(2), Err(Error::EngineStopped)));
    assert_eq!(cap.reserved_count(), 0);
}

#[test]
fn test_ingress_returns_when_wal_dies_before_ack() {
    let (cmd_tx, mut cmd_rx) = RingBuffer::new(1);
    let (resp_tx, resp_rx) = RingBuffer::<WalAck>::new(1);
    let wal = thread::spawn(move || {
        while cmd_rx.pop().is_err() {
            std::hint::spin_loop();
        }
        drop((cmd_rx, resp_tx));
    });
    let mut ingress = Ingress::new(cmd_tx, resp_rx, ingress_config(), Arc::new(Capacity::new(1)));
    assert!(matches!(ingress.cancel_order(7), Err(Error::EngineStopped)));
    wal.join().unwrap();
}

#[test]
fn test_wal_backpressure_loses_no_commands_or_acks() {
    const N: u64 = 2_000;
    let (mut cmd_tx, cmd_rx) = RingBuffer::new(1);
    let (resp_tx, mut resp_rx) = RingBuffer::new(1);
    let (_book_resp_tx, book_resp_rx) = RingBuffer::<OrderBookResponse>::new(1);
    let (book_tx, mut book_rx) = RingBuffer::new(1);
    let wal = thread::spawn(move || WalThread::new(cmd_rx, resp_tx, book_resp_rx, book_tx).run());

    // Slow matcher: the WAL must wait for room instead of dropping.
    let book = thread::spawn(move || {
        let mut received = Vec::new();
        loop {
            match book_rx.pop() {
                Ok(MatchCmd::CancelOrder { order_id, seq }) => {
                    received.push((order_id, seq));
                    if order_id % 64 == 0 {
                        thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
                Ok(MatchCmd::Shutdown) => return received,
                Ok(other) => panic!("unexpected {other:?}"),
                Err(_) => std::hint::spin_loop(),
            }
        }
    });

    // Pipelined producer (unlike Ingress) so the ack queue also fills.
    let mut acks = Vec::new();
    let mut next = 0;
    while acks.len() < N as usize {
        if next < N && cmd_tx.push(IngressCmd::CancelOrder { order_id: next }).is_ok() {
            next += 1;
        }
        if let Ok(ack) = resp_rx.pop() {
            acks.push(ack.order_id);
        }
    }
    while cmd_tx.push(IngressCmd::Shutdown).is_err() {
        std::hint::spin_loop();
    }

    let received = book.join().unwrap();
    wal.join().unwrap();
    assert_eq!(acks, (0..N).collect::<Vec<_>>());
    assert_eq!(received, (0..N).map(|id| (id, id + 1)).collect::<Vec<_>>());
}

#[test]
fn test_wal_exits_when_matcher_gone_without_acking() {
    let (mut cmd_tx, cmd_rx) = RingBuffer::new(4);
    let (resp_tx, mut resp_rx) = RingBuffer::<WalAck>::new(4);
    let (book_resp_tx, book_resp_rx) = RingBuffer::<OrderBookResponse>::new(4);
    let (book_tx, book_rx) = RingBuffer::<MatchCmd>::new(4);
    drop((book_rx, book_resp_tx));
    let wal = thread::spawn(move || WalThread::new(cmd_rx, resp_tx, book_resp_rx, book_tx).run());

    cmd_tx.push(IngressCmd::CancelOrder { order_id: 1 }).unwrap();
    wal.join().unwrap();
    assert!(resp_rx.pop().is_err(), "receipt ACK for a command that cannot reach the matcher");
    assert!(resp_rx.is_abandoned());
}

#[test]
fn test_wal_forwards_shutdown_when_ingress_gone() {
    let (cmd_tx, cmd_rx) = RingBuffer::<IngressCmd>::new(4);
    let (resp_tx, _resp_rx) = RingBuffer::<WalAck>::new(4);
    let (_book_resp_tx, book_resp_rx) = RingBuffer::<OrderBookResponse>::new(4);
    let (book_tx, mut book_rx) = RingBuffer::new(4);
    drop(cmd_tx);
    WalThread::new(cmd_rx, resp_tx, book_resp_rx, book_tx).run();
    assert!(matches!(book_rx.pop(), Ok(MatchCmd::Shutdown)));
}

#[test]
fn test_snapshot_does_not_hang_when_matcher_dies() {
    let (cmd_tx, cmd_rx) = RingBuffer::new(4);
    let (resp_tx, resp_rx) = RingBuffer::new(4);
    let (book_resp_tx, book_resp_rx) = RingBuffer::<OrderBookResponse>::new(4);
    let (book_tx, mut book_rx) = RingBuffer::<MatchCmd>::new(4);
    let wal = thread::spawn(move || WalThread::new(cmd_rx, resp_tx, book_resp_rx, book_tx).run());
    let book = thread::spawn(move || {
        while !matches!(book_rx.pop(), Ok(MatchCmd::GetSnapshot)) {
            std::hint::spin_loop();
        }
        drop((book_rx, book_resp_tx));
    });

    let mut ingress = Ingress::new(cmd_tx, resp_rx, ingress_config(), Arc::new(Capacity::new(1)));
    assert!(matches!(ingress.snapshot(), Err(Error::EngineStopped)));
    book.join().unwrap();
    wal.join().unwrap();
}

#[test]
fn test_engine_rejects_invalid_config() {
    let cases: [fn(&mut EngineConfig); 9] = [
        |c| c.tick_size = 0,
        |c| c.min_order_qty = 0,
        |c| c.min_order_qty = c.max_order_qty + 1,
        |c| c.rb_cap = 0,
        |c| c.tick_capacity = 0,
        |c| c.pool_capacity = 0,
        |c| c.tick_size = u64::MAX,
        |c| (c.base_px, c.tick_size) = (u64::MAX - 10, 1),
        |c| c.base_px = 62,
    ];
    for (i, mutate) in cases.iter().enumerate() {
        let mut cfg = config();
        mutate(&mut cfg);
        assert!(matches!(Engine::new(cfg), Err(Error::InvalidConfig(_))), "case {i}");
    }
    let mut edge = config();
    edge.base_px = 63;
    assert!(Engine::new(edge).is_ok(), "bid range ending exactly at 0 is valid");
}

#[test]
fn test_engine_single_slot_queues_and_stop() {
    let mut engine = Engine::new(EngineConfig { rb_cap: 1, ..config() }).unwrap();
    for id in 1..=50 {
        engine.add_limit_order(id, id, Side::Bid, 100 - id, 10).unwrap();
    }
    for id in 1..=25 {
        engine.cancel_order(id).unwrap();
    }
    let snap = engine.snapshot().unwrap();
    assert_eq!(snap.bids.len(), 25);
    assert_eq!(snap.bids[0].px, 74);

    engine.stop().unwrap();
    engine.stop().unwrap();
    assert!(matches!(engine.add_limit_order(99, 1, Side::Bid, 100, 10), Err(Error::EngineStopped)));
}
