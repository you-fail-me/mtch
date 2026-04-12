pub struct FreeList {
    bits: Box<[usize]>,
    bump_idx: usize,
    capacity: usize,
}

impl FreeList {
    pub fn new(capacity: usize) -> Self {
        let words = (capacity + 63) / 64;
        let bits = vec![0usize; words].into_boxed_slice();
        Self { bits, bump_idx: 0, capacity }
    }

    pub fn pop(&mut self) -> Option<usize> {
        if let Some(free_idx) = self.find_first_free() {
            self.set_bit(free_idx);
            Some(free_idx)
        } else {
            None
        }
    }

    pub fn push(&mut self, idx: usize) {
        if idx < self.capacity {
            self.clear_bit(idx);
        }
    }

    pub fn len(&self) -> usize {
        self.bits.iter().map(|word| word.count_ones() as usize).sum()
    }

    pub fn bump_next(&mut self) -> Option<usize> {
        if self.bump_idx < self.capacity {
            let idx = self.bump_idx;
            self.bump_idx += 1;
            Some(idx)
        } else {
            None
        }
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
            if word != usize::MAX {
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
