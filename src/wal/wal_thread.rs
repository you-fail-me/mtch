use crate::core::messages::{IngressCmd, MatchCmd, OrderBookResponse};
use crate::wal::messages::{WalAck, WalEntryType};
use crate::wal::sequencer::Sequencer;
use rtrb::{Consumer, Producer};

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
            if let Ok(entry) = self.ingress_rx.pop() {
                match entry {
                    IngressCmd::AddLimitOrder { order_id, trader_id, side, px, qty, slot_id } => {
                        let seq = self.seqr.next();

                        // TODO: Persist
                        #[cfg(debug_assertions)]
                        println!("[WAL] Received ADD order_id: {}, seq: {}", order_id, seq);

                        // Send ack back
                        let ack = WalAck {
                            order_id,
                            entry_type: WalEntryType::Add,
                            seq,
                            snapshot: None,
                        };
                        let _ = self.ingress_tx.push(ack);

                        // Pass on to matcher with slot_id
                        let match_cmd = MatchCmd::AddLimitOrder {
                            order_id,
                            trader_id,
                            side,
                            px,
                            qty,
                            seq,
                            slot_id,
                        };
                        let _ = self.book_tx.push(match_cmd);
                    }
                    IngressCmd::CancelOrder { order_id } => {
                        let seq = self.seqr.next();

                        // TODO: Persist
                        #[cfg(debug_assertions)]
                        println!("[WAL] Received CANCEL order_id: {}, seq: {}", order_id, seq);

                        // Send ack back
                        let ack = WalAck {
                            order_id,
                            entry_type: WalEntryType::Cancel,
                            seq,
                            snapshot: None,
                        };
                        let _ = self.ingress_tx.push(ack);

                        // Pass on to matcher
                        let match_cmd = MatchCmd::CancelOrder { order_id, seq: self.seqr.next() };
                        let _ = self.book_tx.push(match_cmd);
                    }
                    IngressCmd::Shutdown => {
                        // Tell matcher to shutdown
                        let _ = self.book_tx.push(MatchCmd::Shutdown);
                        // Exit the loop to terminate thread
                        break;
                    }
                    // Debug method, querying should not go to the matcher IRL
                    IngressCmd::GetSnapshot => {
                        // No persistence needed for snapshot
                        let _ = self.book_tx.push(MatchCmd::GetSnapshot);

                        loop {
                            if let Ok(res) = self.book_rx.pop() {
                                match res {
                                    OrderBookResponse::Snapshot { snapshot } => {
                                        let ack = WalAck {
                                            order_id: 0,
                                            entry_type: WalEntryType::Query,
                                            seq: 0,
                                            snapshot: Some(snapshot),
                                        };
                                        let _ = self.ingress_tx.push(ack);
                                        break;
                                    }
                                    OrderBookResponse::Error { msg: _ } => {
                                        let ack = WalAck {
                                            order_id: 0,
                                            entry_type: WalEntryType::Query,
                                            seq: 0,
                                            snapshot: None,
                                        };
                                        let _ = self.ingress_tx.push(ack);
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    IngressCmd::AddMarketOrder { order_id, trader_id, side, qty } => {
                        let seq = self.seqr.next();

                        // TODO: Persist
                        #[cfg(debug_assertions)]
                        println!("[WAL] Received ADD market order_id: {}, seq: {}", order_id, seq);

                        // Send ack back
                        let ack = WalAck {
                            order_id,
                            entry_type: WalEntryType::Add,
                            seq,
                            snapshot: None,
                        };
                        let _ = self.ingress_tx.push(ack);

                        // Pass on to matcher
                        let match_cmd = MatchCmd::AddMarketOrder { order_id, trader_id, side, qty, seq };
                        let _ = self.book_tx.push(match_cmd);
                    }
                }
            } else {
                std::hint::spin_loop();
                // std::thread::yield_now();
            }
        }
    }
}
