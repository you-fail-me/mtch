pub struct Sequencer {
    next: u64,
}

impl Sequencer {
    pub fn new(next: u64) -> Self {
        Self { next }
    }

    pub fn next(&mut self) -> u64 {
        let seq = self.next;
        self.next += 1;
        seq
    }
}
