pub struct FreeList {
    bits: Box<[u64]>,
    capacity: usize,
}

impl FreeList {
    pub fn new(capacity: u64) -> Self {
        let words = ((capacity + 63) / 64) as usize;
        let bits = vec![0u64; words].into_boxed_slice();
        Self { bits, capacity: capacity as usize }
    }

    pub fn pop(&mut self) -> Option<u64> {
        if let Some(free_idx) = self.find_first_free() {
            self.set_bit(free_idx);
            Some(free_idx as u64)
        } else {
            None
        }
    }

    pub fn push(&mut self, idx: u64) {
        if (idx as usize) < self.capacity {
            self.clear_bit(idx as usize);
        }
    }

    pub fn len(&self) -> usize {
        self.bits.iter().map(|word| word.count_ones() as usize).sum()
    }

    // Draws from the bitmap: a separate bump counter re-issued indices already handed out by `pop`.
    pub fn bump_next(&mut self) -> Option<usize> {
        self.pop().map(|idx| idx as usize)
    }

    fn set_bit(&mut self, idx: usize) {
        if idx < self.capacity {
            let word_idx = idx / 64;
            let bit_idx = idx % 64;
            self.bits[word_idx] |= 1 << bit_idx;
        }
    }

    fn clear_bit(&mut self, idx: usize) {
        if idx < self.capacity {
            let word_idx = idx / 64;
            let bit_idx = idx % 64;
            self.bits[word_idx] &= !(1 << bit_idx);
        }
    }

    fn find_first_free(&self) -> Option<usize> {
        for (word_idx, &word) in self.bits.iter().enumerate() {
            if word != u64::MAX {
                let free_bit = (!word).trailing_zeros() as usize;
                let global_idx = word_idx * 64 + free_bit;
                if global_idx < self.capacity {
                    return Some(global_idx);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bump_next_and_pop_never_reissue() {
        for capacity in [0u64, 1, 64, 65] {
            let mut fl = FreeList::new(capacity);
            let mut seen = Vec::new();
            while let Some(idx) = if seen.len() % 2 == 0 { fl.pop() } else { fl.bump_next().map(|i| i as u64) } {
                assert!(!seen.contains(&idx), "capacity {capacity}: {idx} issued twice");
                seen.push(idx);
            }
            assert_eq!(seen.len() as u64, capacity);
        }
    }
}
