use std::ops::ControlFlow;

use crate::matcher::messages::{IngressCmd, MatchCmd, OrderBookResponse};
use crate::wal::messages::{WalAck, WalEntryType};
use crate::wal::sequencer::Sequencer;
use rtrb::{Consumer, Producer, PushError};

/// Spins through backpressure until `value` is queued. Returns `false` (value dropped) once the
/// consumer is gone; checked up front because rtrb accepts pushes into an abandoned queue.
#[inline]
pub(crate) fn push_reliable<T>(tx: &mut Producer<T>, value: T) -> bool {
    if tx.is_abandoned() {
        return false;
    }
    let mut value = value;
    loop {
        match tx.push(value) {
            Ok(()) => return true,
            Err(PushError::Full(back)) => {
                if tx.is_abandoned() {
                    return false;
                }
                value = back;
                std::hint::spin_loop();
            }
        }
    }
}

/// Nothing is dropped on a full queue; the WAL spins until there is room. If ingress goes away the
/// WAL forwards `Shutdown` and exits; if the matcher goes away it exits, so ingress observes
/// `EngineStopped` instead of hanging.
pub struct WalThread {
    ingress_rx: Consumer<IngressCmd>,
    ingress_tx: Producer<WalAck>,
    book_rx: Consumer<OrderBookResponse>,
    book_tx: Producer<MatchCmd>,
    seqr: Sequencer,
}

impl WalThread {
    pub fn new(ingress_rx: Consumer<IngressCmd>, ingress_tx: Producer<WalAck>, book_rx: Consumer<OrderBookResponse>, book_tx: Producer<MatchCmd>) -> Self {
        Self {
            ingress_rx,
            ingress_tx,
            book_rx,
            book_tx,
            seqr: Sequencer::new(1),
        }
    }

    pub fn run(mut self) {
        loop {
            match self.ingress_rx.pop() {
                Ok(cmd) => {
                    if self.handle(&cmd).is_break() {
                        break;
                    }
                }
                // Abandoned, then empty: ingress is gone and nothing more can arrive.
                Err(_) if self.ingress_rx.is_abandoned() && self.ingress_rx.is_empty() => break,
                Err(_) => std::hint::spin_loop(),
            }
        }
        // Tell matcher to shutdown (no-op if it is already gone)
        push_reliable(&mut self.book_tx, MatchCmd::Shutdown);
    }

    /// Receipt ACK first, then forward: the ACK confirms sequencing, not execution.
    fn ack_and_forward(&mut self, order_id: u64, entry_type: WalEntryType, seq: u64, match_cmd: MatchCmd) -> ControlFlow<()> {
        // Don't acknowledge a command the matcher can no longer receive.
        if self.book_tx.is_abandoned() {
            return ControlFlow::Break(());
        }
        let ack = WalAck { order_id, entry_type, seq, snapshot: None };
        if push_reliable(&mut self.ingress_tx, ack) && push_reliable(&mut self.book_tx, match_cmd) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    /// `Break` on shutdown or when a peer is gone.
    fn handle(&mut self, cmd: &IngressCmd) -> ControlFlow<()> {
        match *cmd {
            IngressCmd::AddLimitOrder { order_id, trader_id, side, px, qty, slot_id } => {
                let seq = self.seqr.next();

                // TODO: Persist
                #[cfg(debug_assertions)]
                println!("[WAL] Received ADD order_id: {}, seq: {}", order_id, seq);

                let match_cmd = MatchCmd::AddLimitOrder {
                    order_id,
                    trader_id,
                    side,
                    px,
                    qty,
                    seq,
                    slot_id,
                };
                self.ack_and_forward(order_id, WalEntryType::Add, seq, match_cmd)
            }
            IngressCmd::CancelOrder { order_id } => {
                let seq = self.seqr.next();

                // TODO: Persist
                #[cfg(debug_assertions)]
                println!("[WAL] Received CANCEL order_id: {}, seq: {}", order_id, seq);

                self.ack_and_forward(order_id, WalEntryType::Cancel, seq, MatchCmd::CancelOrder { order_id, seq })
            }
            IngressCmd::AddMarketOrder { order_id, trader_id, side, qty } => {
                let seq = self.seqr.next();

                // TODO: Persist
                #[cfg(debug_assertions)]
                println!("[WAL] Received ADD market order_id: {}, seq: {}", order_id, seq);

                let match_cmd = MatchCmd::AddMarketOrder { order_id, trader_id, side, qty, seq };
                self.ack_and_forward(order_id, WalEntryType::Add, seq, match_cmd)
            }
            IngressCmd::Shutdown => ControlFlow::Break(()),
            // Debug method, querying should not go to the matcher IRL
            IngressCmd::GetSnapshot => {
                // No persistence needed for snapshot
                if !push_reliable(&mut self.book_tx, MatchCmd::GetSnapshot) {
                    return ControlFlow::Break(());
                }
                let snapshot = loop {
                    match self.book_rx.pop() {
                        Ok(OrderBookResponse::Snapshot { snapshot }) => break Some(snapshot),
                        Ok(OrderBookResponse::Error { msg: _ }) => break None,
                        Ok(_) => {}
                        Err(_) if self.book_rx.is_abandoned() && self.book_rx.is_empty() => return ControlFlow::Break(()),
                        Err(_) => std::hint::spin_loop(),
                    }
                };
                let ack = WalAck {
                    order_id: 0,
                    entry_type: WalEntryType::Query,
                    seq: 0,
                    snapshot,
                };
                if push_reliable(&mut self.ingress_tx, ack) { ControlFlow::Continue(()) } else { ControlFlow::Break(()) }
            }
        }
    }
}
