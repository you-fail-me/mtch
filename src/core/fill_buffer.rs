use crate::core::order_book::Fill;

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
    pub fn new(capacity: usize) -> Self {
        let mut vec = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            vec.push(placeholder_fill());
        }
        Self { slots: vec.into_boxed_slice(), count: 0 }
    }

    pub fn push(&mut self, fill: Fill) {
        self.slots[self.count] = fill;
        self.count += 1;
    }

    pub fn drain(&mut self) -> &[Fill] {
        let fills = &self.slots[..self.count];
        self.count = 0;
        fills
    }
}
