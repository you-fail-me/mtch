use crate::matcher::order::Side;
use crate::matcher::price_level::PriceLevel;
use std::cmp::Ordering;

pub struct TickArray {
    pub levels: Box<[PriceLevel]>,
    pub base_px: u64,
    best_idx: Option<usize>,
    non_empty_count: usize,
}

impl TickArray {
    pub fn new(capacity: u64, base_px: u64, tick_size: u64, ordering: Ordering) -> Self {
        let mut levels_vec: Vec<PriceLevel> = Vec::with_capacity(capacity as usize);

        for i in 0..capacity as usize {
            let price = match ordering {
                Ordering::Less => base_px.saturating_sub((i as u64) * tick_size), // Bids: decreasing prices
                Ordering::Greater => base_px + (i as u64) * tick_size,            // Asks: increasing prices
                Ordering::Equal => panic!("Ordering::Equal not supported for TickArray initialization"),
            };
            levels_vec.push(PriceLevel::new(price));
        }

        let levels = levels_vec.into_boxed_slice();

        Self {
            levels,
            base_px,
            best_idx: None,
            non_empty_count: 0,
        }
    }

    pub fn index(&self, px: u64, tick_size: u64, side: Side) -> Option<usize> {
        match side {
            Side::Ask => {
                if px < self.base_px {
                    return None;
                }
                let idx = ((px - self.base_px) / tick_size) as usize;

                if idx < self.levels.len() { Some(idx) } else { None }
            }
            Side::Bid => {
                if px > self.base_px {
                    return None;
                }
                let idx = ((self.base_px - px) / tick_size) as usize;

                if idx < self.levels.len() { Some(idx) } else { None }
            }
        }
    }

    pub fn level_mut(&mut self, px: u64, tick_size: u64, side: Side) -> Option<&mut PriceLevel> {
        let idx = self.index(px, tick_size, side)?;
        let level = &mut self.levels[idx];

        // TODO: ensure invalid prices don't reach the matcher so we can tackle it later

        // Update best if this is better (lower index = better price)
        match self.best_idx {
            None => self.best_idx = Some(idx),
            Some(best) if idx < best => self.best_idx = Some(idx),
            _ => {}
        }

        Some(level)
    }

    // Return best non-empty price level
    pub fn best_level_mut(&mut self, tick_size: u64, side: Side) -> Option<(u64, &mut PriceLevel)> {
        let idx = self.best_idx?;
        let level = &mut self.levels[idx];

        let px = match side {
            Side::Ask => self.base_px + (idx as u64) * tick_size,
            Side::Bid => self.base_px - (idx as u64) * tick_size,
        };

        Some((px, level))
    }

    /// Advances best_idx if current best is empty
    pub fn update_best(&mut self) {
        if self.non_empty_count == 0 {
            self.best_idx = None;
            return;
        }

        let start = match self.best_idx {
            Some(idx) => idx,
            None => return,
        };

        for i in start..self.levels.len() {
            if !self.levels[i].is_empty() {
                self.best_idx = Some(i);
                return;
            }
        }
        self.best_idx = None;
    }

    pub fn increment_non_empty(&mut self) {
        self.non_empty_count += 1;
    }

    pub fn decrement_non_empty(&mut self) {
        self.non_empty_count = self.non_empty_count.saturating_sub(1);
    }
}
