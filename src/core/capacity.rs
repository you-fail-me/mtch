use std::sync::atomic::{AtomicU64, Ordering};

pub struct AtomicFreeList {
    bits: Box<[AtomicU64]>,
    bump_idx: AtomicU64,
    capacity: u64,
}

impl AtomicFreeList {
    pub fn new(capacity: u64) -> Self {
        let words = (capacity + 63) / 64;
        let bits = (0..words).map(|_| AtomicU64::new(0)).collect::<Vec<_>>().into_boxed_slice();

        Self { bits, bump_idx: AtomicU64::new(0), capacity }
    }

    pub fn pop(&self) -> Option<u64> {
        for word_idx in 0..self.bits.len() {
            let word = &self.bits[word_idx];

            loop {
                let current = word.load(Ordering::SeqCst);

                if current == u64::MAX {
                    break;
                }

                let free_bit = (!current).trailing_zeros() as u64;
                let slot_id = word_idx as u64 * 64 + free_bit;

                if slot_id >= self.capacity {
                    return None;
                }

                let new_val = current | (1 << free_bit);

                match word.compare_exchange(current, new_val, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => return Some(slot_id),
                    Err(_) => continue,
                }
            }
        }

        let idx = self.bump_idx.fetch_add(1, Ordering::SeqCst);
        if idx < self.capacity {
            return Some(idx);
        }

        None
    }

    pub fn push(&self, slot_id: u64) {
        if slot_id >= self.capacity {
            return;
        }

        let word_idx = (slot_id / 64) as usize;
        let bit_idx = (slot_id % 64) as u32;

        if word_idx >= self.bits.len() {
            return;
        }

        let word = &self.bits[word_idx];
        let mask = 1u64 << bit_idx;

        loop {
            let current = word.load(Ordering::SeqCst);
            let new_val = current & !mask;

            match word.compare_exchange(current, new_val, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }
}

pub struct Capacity {
    free_list: AtomicFreeList,
}

pub enum ReserveResult {
    Reserved(u64),
    CapacityExceeded,
}

impl Capacity {
    pub fn new(capacity: u64) -> Self {
        Self { free_list: AtomicFreeList::new(capacity) }
    }

    pub fn reserve(&self) -> ReserveResult {
        match self.free_list.pop() {
            Some(slot_id) => ReserveResult::Reserved(slot_id),
            None => ReserveResult::CapacityExceeded,
        }
    }

    pub fn release(&self, slot_id: u64) {
        self.free_list.push(slot_id);
    }
}
