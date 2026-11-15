use crate::matcher::order::Side;
use crate::matcher::order_book::BookConfigError;
use crate::matcher::price_level::PriceLevel;

/// One side's price window; index 0 is `base_px` and lower indices are better prices.
/// `best_idx` is always the lowest non-empty index (`None` iff empty); owners keep it exact via
/// `on_order_added` / `on_level_emptied`.
#[derive(Debug)]
pub struct TickArray {
    pub levels: Box<[PriceLevel]>,
    pub base_px: u64,
    /// Non-zero; validated at construction.
    tick_size: u64,
    /// Determines the price direction of increasing indices.
    side: Side,
    best_idx: Option<usize>,
    non_empty_count: usize,
}

impl TickArray {
    /// # Errors
    /// See [`TickArray::validate`].
    pub fn try_new(capacity: u64, base_px: u64, tick_size: u64, side: Side) -> Result<Self, BookConfigError> {
        Self::validate(capacity, base_px, tick_size, side)?;
        let levels = (0..capacity)
            .map(|i| {
                PriceLevel::new(match side {
                    Side::Ask => base_px + i * tick_size,
                    Side::Bid => base_px - i * tick_size,
                })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(Self {
            levels,
            base_px,
            tick_size,
            side,
            best_idx: None,
            non_empty_count: 0,
        })
    }

    /// Every level must have a distinct representable price (no ask overflow, no bid saturation at 0).
    ///
    /// # Errors
    /// Returns the first invalid parameter.
    pub fn validate(capacity: u64, base_px: u64, tick_size: u64, side: Side) -> Result<(), BookConfigError> {
        if tick_size == 0 {
            return Err(BookConfigError::ZeroTickSize);
        }
        if capacity == 0 {
            return Err(BookConfigError::ZeroTickCapacity);
        }
        let span = (capacity - 1).checked_mul(tick_size);
        match side {
            Side::Ask => span.and_then(|s| base_px.checked_add(s)).map(|_| ()).ok_or(BookConfigError::AskWindowOverflow),
            Side::Bid => span.and_then(|s| base_px.checked_sub(s)).map(|_| ()).ok_or(BookConfigError::BidWindowUnderflow),
        }
    }

    /// On the tick grid (`px ≡ base_px mod tick_size`), whether or not inside the window.
    #[must_use]
    pub fn is_aligned(&self, px: u64) -> bool {
        px % self.tick_size == self.base_px % self.tick_size
    }

    /// Exact level for an on-grid price inside the window; off-grid prices are not floored.
    #[must_use]
    pub fn index(&self, px: u64) -> Option<usize> {
        let diff = match self.side {
            Side::Ask => px.checked_sub(self.base_px)?,
            Side::Bid => self.base_px.checked_sub(px)?,
        };
        if diff % self.tick_size != 0 {
            return None;
        }
        let idx = diff / self.tick_size;
        (idx < self.levels.len() as u64).then_some(idx as usize)
    }

    #[must_use]
    pub fn best_idx(&self) -> Option<usize> {
        self.best_idx
    }

    /// First non-empty level at or after `from` (i.e. equal or worse price).
    #[must_use]
    pub fn next_non_empty(&self, from: usize) -> Option<usize> {
        let tail = self.levels.get(from..)?;
        tail.iter().position(|l| !l.is_empty()).map(|i| from + i)
    }

    pub fn on_order_added(&mut self, idx: usize, became_non_empty: bool) {
        debug_assert!(self.levels.get(idx).is_some_and(|l| !l.is_empty()));
        if became_non_empty {
            self.non_empty_count += 1;
        }
        match self.best_idx {
            Some(best) if best <= idx => {}
            _ => self.best_idx = Some(idx),
        }
    }

    pub fn on_level_emptied(&mut self, idx: usize) {
        debug_assert!(self.levels.get(idx).is_some_and(PriceLevel::is_empty));
        debug_assert!(self.non_empty_count > 0, "non_empty_count underflow");
        self.non_empty_count -= 1;
        if self.best_idx == Some(idx) {
            self.best_idx = if self.non_empty_count == 0 { None } else { self.next_non_empty(idx + 1) };
        }
    }

    /// # Errors
    /// Describes the first violated invariant.
    pub fn check_invariants(&self) -> Result<(), &'static str> {
        let mut non_empty = 0usize;
        let mut first = None;
        for (i, level) in self.levels.iter().enumerate() {
            let expected_px = match self.side {
                Side::Ask => self.base_px + i as u64 * self.tick_size,
                Side::Bid => self.base_px - i as u64 * self.tick_size,
            };
            if level.px != expected_px {
                return Err("level price does not match its index");
            }
            if !level.is_empty() {
                non_empty += 1;
                first.get_or_insert(i);
            }
        }
        if non_empty != self.non_empty_count {
            return Err("non_empty_count mismatch");
        }
        if first != self.best_idx {
            return Err("best_idx is not the best non-empty level");
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_index_requires_exact_tick_alignment() {
        let asks = TickArray::try_new(4, 1000, 10, Side::Ask).unwrap();
        assert_eq!(asks.index(1000), Some(0));
        assert_eq!(asks.index(1030), Some(3));
        assert_eq!(asks.index(1005), None);
        assert_eq!(asks.index(1040), None);
        assert_eq!(asks.index(990), None);
        let bids = TickArray::try_new(4, 1000, 10, Side::Bid).unwrap();
        assert_eq!(bids.index(970), Some(3));
        assert_eq!(bids.index(995), None);
        assert_eq!(bids.index(1010), None);
        assert!(bids.is_aligned(1010) && !bids.is_aligned(1011));
    }

    #[test]
    fn test_invalid_windows_rejected() {
        assert_eq!(TickArray::try_new(4, 100, 0, Side::Ask).err(), Some(BookConfigError::ZeroTickSize));
        assert_eq!(TickArray::try_new(0, 100, 1, Side::Ask).err(), Some(BookConfigError::ZeroTickCapacity));
        assert_eq!(TickArray::try_new(4, u64::MAX - 2, 1, Side::Ask).err(), Some(BookConfigError::AskWindowOverflow));
        assert_eq!(TickArray::try_new(u64::MAX, 2, u64::MAX, Side::Ask).err(), Some(BookConfigError::AskWindowOverflow));
        // Old code saturated these to duplicate price-0 levels.
        assert_eq!(TickArray::try_new(10, 5, 1, Side::Bid).err(), Some(BookConfigError::BidWindowUnderflow));
        assert!(TickArray::try_new(6, 5, 1, Side::Bid).is_ok());
        assert!(TickArray::try_new(4, u64::MAX - 3, 1, Side::Ask).is_ok());
    }

    #[test]
    fn test_full_window_has_distinct_prices() {
        let bids = TickArray::try_new(6, 5, 1, Side::Bid).unwrap();
        let px: Vec<u64> = bids.levels.iter().map(|l| l.px).collect();
        assert_eq!(px, vec![5, 4, 3, 2, 1, 0]);
        assert_eq!(bids.index(0), Some(5));
        assert!(bids.check_invariants().is_ok());
    }
}
