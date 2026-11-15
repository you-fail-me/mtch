use crate::error::Error;
use crate::matcher::order::OrderLocation;

/// Fixed-capacity order index addressed directly by order id: valid ids are `0..capacity`.
pub struct OrderIndex {
    entries: Box<[Option<OrderLocation>]>,
    live_count: usize,
}

impl OrderIndex {
    pub fn new(capacity: u64) -> Self {
        Self {
            entries: vec![None; capacity as usize].into_boxed_slice(),
            live_count: 0,
        }
    }

    /// # Errors
    ///
    /// Index unchanged on error: `OrderIdOutOfRange` if `id >= capacity`, `DuplicateOrderId` if `id` is present.
    pub fn insert(&mut self, id: u64, location: OrderLocation) -> Result<(), Error> {
        let entry = self.entries.get_mut(id as usize).ok_or(Error::OrderIdOutOfRange(id))?;
        if entry.is_some() {
            return Err(Error::DuplicateOrderId(id));
        }
        *entry = Some(location);
        self.live_count += 1;
        Ok(())
    }

    pub fn get(&self, id: u64) -> Option<&OrderLocation> {
        self.entries.get(id as usize)?.as_ref()
    }

    pub fn remove(&mut self, id: u64) -> Option<OrderLocation> {
        let location = self.entries.get_mut(id as usize)?.take();
        if location.is_some() {
            self.live_count -= 1;
        }
        location
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.live_count
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live_count == 0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::matcher::order::{Handle, Side};

    fn loc(px: u64) -> OrderLocation {
        OrderLocation {
            side: Side::Bid,
            px,
            handle: Handle { idx: px, generation: 1 },
        }
    }

    #[test]
    fn test_index_rejects_out_of_range_and_aliasing_ids() {
        // Old index used id % len: with capacity 8, remove(9) removed order 1.
        let mut index = OrderIndex::new(8);
        index.insert(1, loc(1)).unwrap();
        assert!(index.remove(9).is_none());
        assert!(index.get(9).is_none());
        assert!(matches!(index.insert(9, loc(9)), Err(Error::OrderIdOutOfRange(9))));
        assert!(matches!(index.insert(u64::MAX, loc(9)), Err(Error::OrderIdOutOfRange(u64::MAX))));
        assert_eq!(index.get(1).unwrap().px, 1);
        assert_eq!(index.len(), 1);

        let mut empty = OrderIndex::new(0);
        assert!(empty.get(0).is_none());
        assert!(empty.remove(0).is_none());
        assert!(matches!(empty.insert(0, loc(0)), Err(Error::OrderIdOutOfRange(0))));
    }

    #[test]
    fn test_index_duplicate_rejected_without_mutation() {
        let mut index = OrderIndex::new(8);
        index.insert(7, loc(1)).unwrap();
        assert!(matches!(index.insert(7, loc(2)), Err(Error::DuplicateOrderId(7))));
        assert_eq!(index.get(7).unwrap().px, 1);
        assert_eq!(index.len(), 1);
        assert_eq!(index.remove(7).unwrap().px, 1);
        assert!(index.remove(7).is_none());
        assert!(index.is_empty());
        index.insert(7, loc(3)).unwrap();
        assert_eq!(index.get(7).unwrap().px, 3);
    }
}
