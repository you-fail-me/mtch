use std::sync::atomic::{AtomicU64, Ordering};

/// Lock-free slot bitmap (set bit = reserved). Bits past `capacity` in the last word start set,
/// so a full bitmap means exhausted and every issued id came from flipping a clear bit.
#[derive(Debug)]
pub struct AtomicFreeList {
    bits: Box<[AtomicU64]>,
    capacity: u64,
}

impl AtomicFreeList {
    pub fn new(capacity: u64) -> Self {
        let bits = (0..capacity.div_ceil(64))
            .map(|word_idx| {
                let valid = (capacity - word_idx * 64).min(64);
                AtomicU64::new(if valid == 64 { 0 } else { u64::MAX << valid })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self { bits, capacity }
    }

    #[must_use]
    pub fn pop(&self) -> Option<u64> {
        for (word_idx, word) in self.bits.iter().enumerate() {
            let mut current = word.load(Ordering::SeqCst);

            while current != u64::MAX {
                let free_bit = (!current).trailing_zeros();
                let new_val = current | (1 << free_bit);

                match word.compare_exchange(current, new_val, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => return Some(word_idx as u64 * 64 + u64::from(free_bit)),
                    Err(observed) => current = observed,
                }
            }
        }

        None
    }

    /// No-op for out-of-range or unreserved slots.
    pub fn push(&self, slot_id: u64) {
        if slot_id < self.capacity {
            self.bits[(slot_id / 64) as usize].fetch_and(!(1 << (slot_id % 64)), Ordering::SeqCst);
        }
    }

    /// O(capacity / 64); for diagnostics and tests.
    #[must_use]
    pub fn reserved_count(&self) -> u64 {
        let set: u64 = self.bits.iter().map(|word| u64::from(word.load(Ordering::SeqCst).count_ones())).sum();
        set - (self.bits.len() as u64 * 64 - self.capacity)
    }
}

#[derive(Debug)]
pub struct Capacity {
    free_list: AtomicFreeList,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReserveResult {
    Reserved(u64),
    CapacityExceeded,
}

impl Capacity {
    pub fn new(capacity: u64) -> Self {
        Self { free_list: AtomicFreeList::new(capacity) }
    }

    #[must_use]
    pub fn reserve(&self) -> ReserveResult {
        match self.free_list.pop() {
            Some(slot_id) => ReserveResult::Reserved(slot_id),
            None => ReserveResult::CapacityExceeded,
        }
    }

    /// No-op for out-of-range or unreserved slots.
    pub fn release(&self, slot_id: u64) {
        self.free_list.push(slot_id);
    }

    #[must_use]
    pub fn reserved_count(&self) -> u64 {
        self.free_list.reserved_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(cap: &Capacity, capacity: u64) -> Vec<u64> {
        let mut ids = Vec::new();
        while let ReserveResult::Reserved(id) = cap.reserve() {
            ids.push(id);
            assert!(ids.len() as u64 <= capacity, "issued more than capacity");
        }
        ids
    }

    #[test]
    fn test_reserve_exhausts_exactly_capacity_without_duplicates() {
        // 64 and 128 used to fall through to a bump counter restarting at 0.
        for capacity in [0u64, 1, 63, 64, 65, 128] {
            let cap = Capacity::new(capacity);
            let mut ids = drain(&cap, capacity);
            ids.sort_unstable();
            assert_eq!(ids, (0..capacity).collect::<Vec<_>>(), "capacity {capacity}");
            assert_eq!(cap.reserve(), ReserveResult::CapacityExceeded);
            assert_eq!(cap.reserved_count(), capacity);

            if capacity > 0 {
                let victim = capacity / 2;
                cap.release(victim);
                cap.release(victim);
                assert_eq!(cap.reserved_count(), capacity - 1, "double release");
                assert_eq!(cap.reserve(), ReserveResult::Reserved(victim));
                assert_eq!(cap.reserve(), ReserveResult::CapacityExceeded);
            }
            cap.release(capacity);
            cap.release(u64::MAX);
            assert_eq!(cap.reserved_count(), capacity, "out-of-range release");
        }
    }
}
