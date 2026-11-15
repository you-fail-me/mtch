use crate::matcher::order::{Handle, Order};
use crate::matcher::order_pool::OrderPool;

// TODO: use Result<(), E> and propagate errors instead of Option<()>

/// Fallible operations validate every handle and counter they touch before mutating, so `None` leaves the
/// level and pool unchanged. Counters are checked, not saturating: underflow would mean corruption.
#[derive(Debug, Clone)]
pub struct PriceLevel {
    pub px: u64,
    pub qty: u64,
    pub count: usize,
    pub head: Option<Handle>,
    tail: Option<Handle>,
}

impl PriceLevel {
    pub fn new(px: u64) -> Self {
        Self { px, qty: 0, count: 0, head: None, tail: None }
    }

    // Adds an order to the tail to maintain price-time priority. Returns (handle, became_non_empty).
    pub fn add_order(&mut self, order: Order, pool: &mut OrderPool, slot_id: u64) -> Option<(Handle, bool)> {
        if order.qty == 0 {
            return None;
        }
        let new_qty = self.qty.checked_add(order.qty)?;

        if let Some(tail) = self.tail
            && (tail.idx == slot_id || pool.get(tail).is_none())
        {
            return None;
        }

        let was_empty = self.is_empty();
        let tail = self.tail;
        let (handle, new_order) = pool.alloc_at_slot(slot_id, order)?;
        new_order.prev = tail;
        new_order.next = None;

        match tail {
            Some(tail) => match pool.get_mut(tail) {
                Some(old_tail) => old_tail.next = Some(handle),
                // Unreachable: tail was validated and `alloc_at_slot` only touches `slot_id != tail.idx`.
                None => debug_assert!(false, "price level tail vanished during insertion"),
            },
            None => self.head = Some(handle),
        }

        self.tail = Some(handle);
        self.qty = new_qty;
        self.count += 1;

        Some((handle, was_empty))
    }

    // Remove (cancelation) - O(1). Returns whether the level became empty. Does not dealloc.
    pub fn remove_order(&mut self, handle: Handle, pool: &mut OrderPool) -> Option<bool> {
        let (prev, next, qty) = {
            let order = pool.get(handle)?;
            (order.prev, order.next, order.qty)
        };

        // Validate neighbour links (or level ends) first. This rejects stale or unlinked handles, but does not
        // prove a middle node belongs to this level; callers pass the level the order was indexed at.
        match prev {
            Some(p) if pool.get(p)?.next != Some(handle) => return None,
            None if self.head != Some(handle) => return None,
            _ => {}
        }
        match next {
            Some(n) if pool.get(n)?.prev != Some(handle) => return None,
            None if self.tail != Some(handle) => return None,
            _ => {}
        }
        let new_qty = self.qty.checked_sub(qty)?;
        let new_count = self.count.checked_sub(1)?;

        match prev {
            Some(p) => {
                if let Some(o) = pool.get_mut(p) {
                    o.next = next;
                }
            }
            None => self.head = next,
        }
        match next {
            Some(n) => {
                if let Some(o) = pool.get_mut(n) {
                    o.prev = prev;
                }
            }
            None => self.tail = prev,
        }
        if let Some(o) = pool.get_mut(handle) {
            o.prev = None;
            o.next = None;
        }

        self.qty = new_qty;
        self.count = new_count;
        Some(self.is_empty())
    }

    /// Partial fill: `0 < by < order.qty`. A full fill must use `remove_order`.
    pub fn reduce_order_qty(&mut self, handle: Handle, by: u64, pool: &mut OrderPool) -> Option<()> {
        let new_level_qty = self.qty.checked_sub(by)?;
        let order = pool.get_mut(handle)?;
        if by == 0 || by >= order.qty {
            return None;
        }
        order.qty -= by;
        self.qty = new_level_qty;
        Some(())
    }

    // Get best price
    pub fn peek_head<'a>(&self, pool: &'a OrderPool) -> Option<&'a Order> {
        self.head.and_then(|head| pool.get(head))
    }

    pub fn remove_head(&mut self, pool: &mut OrderPool) -> Option<()> {
        let head = self.head?;
        self.remove_order(head, pool).map(|_| ())
    }

    /// Reduce-only (`0 < new_qty <= head.qty`): growing an order would need to forfeit time priority.
    pub fn update_head_qty(&mut self, new_qty: u64, pool: &mut OrderPool) -> Option<()> {
        let head = self.head?;
        let by = pool.get(head)?.qty.checked_sub(new_qty)?;
        if by == 0 {
            return Some(());
        }
        self.reduce_order_qty(head, by, pool)
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn order_count(&self) -> usize {
        self.count
    }

    #[must_use]
    pub fn tail(&self) -> Option<Handle> {
        self.tail
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::matcher::capacity::Capacity;
    use crate::matcher::order::Side;

    fn setup(n: u64) -> (OrderPool, PriceLevel, Vec<Handle>) {
        let mut pool = OrderPool::new(16, Arc::new(Capacity::new(16)));
        let mut level = PriceLevel::new(100);
        let mut handles = Vec::new();
        for i in 0..n {
            let (h, _) = level.add_order(Order::new_limit(i, 1, Side::Bid, 100, 10 + i, i), &mut pool, i).unwrap();
            handles.push(h);
        }
        (pool, level, handles)
    }

    fn ids(level: &PriceLevel, pool: &OrderPool) -> Vec<u64> {
        let mut out = Vec::new();
        let mut cur = level.head;
        while let Some(h) = cur {
            let o = pool.get(h).unwrap();
            out.push(o.id);
            cur = o.next;
        }
        out
    }

    #[test]
    fn test_remove_middle_head_tail_keeps_links_and_totals() {
        let (mut pool, mut level, h) = setup(4);
        assert_eq!(level.qty, 10 + 11 + 12 + 13);
        assert_eq!(level.remove_order(h[1], &mut pool), Some(false));
        assert_eq!(ids(&level, &pool), vec![0, 2, 3]);
        assert_eq!(level.remove_order(h[0], &mut pool), Some(false));
        assert_eq!(level.remove_order(h[3], &mut pool), Some(false));
        assert_eq!(ids(&level, &pool), vec![2]);
        assert_eq!((level.qty, level.order_count()), (12, 1));
        assert_eq!(level.remove_order(h[2], &mut pool), Some(true));
        assert!(level.is_empty() && level.head.is_none() && level.tail().is_none());
    }

    #[test]
    fn test_remove_twice_is_rejected_without_mutation() {
        let (mut pool, mut level, h) = setup(3);
        assert_eq!(level.remove_order(h[1], &mut pool), Some(false));
        // h[1] is still alive in the pool but no longer linked into this level.
        assert_eq!(level.remove_order(h[1], &mut pool), None);
        assert_eq!(ids(&level, &pool), vec![0, 2]);
        assert_eq!((level.qty, level.order_count()), (22, 2));
    }

    #[test]
    fn test_remove_order_from_wrong_level_is_rejected() {
        let (mut pool, mut level, h) = setup(2);
        let mut other = PriceLevel::new(101);
        assert_eq!(other.remove_order(h[0], &mut pool), None);
        assert_eq!(other.remove_order(h[1], &mut pool), None);
        assert_eq!(ids(&level, &pool), vec![0, 1]);
        assert_eq!(level.remove_order(h[0], &mut pool), Some(false));
    }

    #[test]
    fn test_add_zero_qty_or_reused_tail_slot_rejected_without_mutation() {
        let (mut pool, mut level, _) = setup(2);
        assert!(level.add_order(Order::new_limit(9, 1, Side::Bid, 100, 0, 9), &mut pool, 9).is_none());
        // Slot 1 is the live tail: re-allocating it would corrupt the list.
        assert!(level.add_order(Order::new_limit(9, 1, Side::Bid, 100, 5, 9), &mut pool, 1).is_none());
        assert!(level.add_order(Order::new_limit(9, 1, Side::Bid, 100, 5, 9), &mut pool, 99).is_none());
        assert_eq!(ids(&level, &pool), vec![0, 1]);
        assert_eq!((level.qty, level.order_count()), (21, 2));
    }

    #[test]
    fn test_reduce_and_update_head_qty_bounds() {
        let (mut pool, mut level, h) = setup(2);
        assert_eq!(level.reduce_order_qty(h[1], 0, &mut pool), None);
        assert_eq!(level.reduce_order_qty(h[1], 11, &mut pool), None);
        assert_eq!(level.reduce_order_qty(h[1], 4, &mut pool), Some(()));
        assert_eq!(level.qty, 10 + 7);
        assert_eq!(level.update_head_qty(11, &mut pool), None);
        assert_eq!(level.update_head_qty(0, &mut pool), None);
        assert_eq!(level.update_head_qty(3, &mut pool), Some(()));
        assert_eq!(level.qty, 3 + 7);
        assert_eq!(level.remove_head(&mut pool), Some(()));
        assert_eq!(ids(&level, &pool), vec![1]);
        assert_eq!(level.qty, 7);
    }
}
