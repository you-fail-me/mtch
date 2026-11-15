use crate::matcher::order_book::Fill;

#[derive(Debug)]
pub struct FillBuffer {
    slots: Box<[Fill]>,
    count: usize,
}

fn placeholder_fill() -> Fill {
    Fill {
        taker_order_id: 0,
        maker_order_id: 0,
        px: 0,
        qty: 0,
        seq: 0,
    }
}

impl FillBuffer {
    pub fn new(capacity: u64) -> Self {
        let mut vec = Vec::with_capacity(capacity as usize);
        for _ in 0..capacity as usize {
            vec.push(placeholder_fill());
        }
        Self { slots: vec.into_boxed_slice(), count: 0 }
    }

    /// Callers check `is_full` before executing the trade being recorded.
    pub fn push(&mut self, fill: Fill) {
        self.slots[self.count] = fill;
        self.count += 1;
    }

    #[must_use]
    pub fn is_full(&self) -> bool {
        self.count == self.slots.len()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Fill] {
        &self.slots[..self.count]
    }

    pub fn drain(&mut self) -> &[Fill] {
        let fills = &self.slots[..self.count];
        self.count = 0;
        fills
    }
}
