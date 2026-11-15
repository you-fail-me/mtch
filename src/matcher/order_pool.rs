use std::sync::Arc;

use crate::matcher::capacity::Capacity;
use crate::matcher::order::{Handle, Order, Side};

// Not pretty but avoids the unsafe of MaybeUninit and the overhead of Option
fn placeholder_order() -> Order {
    Order::new_limit(0, 0, Side::Ask, 0, 0, 0)
}

#[derive(Debug)]
struct Slot {
    order: Order,
    generation: u64,
    alive: bool,
}

/// Fixed-capacity order storage with generational handles. A handle is honoured only while its
/// slot is alive with the same generation, so stale handles can neither read nor free a reused slot.
#[derive(Debug)]
pub struct OrderPool {
    slots: Box<[Slot]>,
    live_count: usize,
    capacity: Arc<Capacity>,
}

impl OrderPool {
    #[must_use]
    pub fn new(pool_capacity: u64, capacity: Arc<Capacity>) -> Self {
        let mut vec: Vec<Slot> = Vec::with_capacity(pool_capacity as usize);

        for _ in 0..pool_capacity as usize {
            vec.push(Slot {
                order: placeholder_order(),
                generation: 0,
                alive: false,
            });
        }
        let slots = vec.into_boxed_slice();

        OrderPool { slots, live_count: 0, capacity }
    }

    /// Frees the order and releases its slot reservation. Returns `false` (no-op) for stale,
    /// already-freed or out-of-range handles.
    pub fn dealloc(&mut self, handle: Handle) -> bool {
        if self.get(handle).is_none() {
            return false;
        }
        self.slots[handle.idx as usize].alive = false;
        self.live_count -= 1;
        self.capacity.release(handle.idx);
        true
    }

    /// Gets an immutable reference to an order.
    pub fn get(&self, handle: Handle) -> Option<&Order> {
        let slot = self.slots.get(handle.idx as usize)?;
        (slot.alive && slot.generation == handle.generation).then_some(&slot.order)
    }

    /// Gets a mutable reference to an order.
    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut Order> {
        let slot = self.slots.get_mut(handle.idx as usize)?;
        (slot.alive && slot.generation == handle.generation).then_some(&mut slot.order)
    }

    /// Allocate an order at a specific slot index (for pre-reserved slots from Capacity).
    /// Returns `None` without touching the pool if the slot is out of range or alive.
    pub fn alloc_at_slot(&mut self, slot_id: u64, order: Order) -> Option<(Handle, &mut Order)> {
        let slot = self.slots.get_mut(slot_id as usize)?;
        if slot.alive {
            return None;
        }
        slot.generation = slot.generation.wrapping_add(1);
        slot.alive = true;
        slot.order = order;
        self.live_count += 1;

        let handle = Handle { idx: slot_id, generation: slot.generation };

        Some((handle, &mut slot.order))
    }

    /// Releases a reservation that never became a resting order (fully matched or rejected).
    /// No-op if the slot is alive, since its reservation belongs to that order.
    pub fn release_unused_slot(&self, slot_id: u64) {
        if self.slots.get(slot_id as usize).is_some_and(|slot| !slot.alive) {
            self.capacity.release(slot_id);
        }
    }

    #[must_use]
    pub fn live_count(&self) -> usize {
        self.live_count
    }

    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::matcher::capacity::ReserveResult;

    fn order(id: u64) -> Order {
        Order::new_limit(id, 1, Side::Bid, 100, 10, id)
    }

    #[test]
    fn test_alloc_on_live_slot_rejected() {
        let cap = Arc::new(Capacity::new(2));
        let mut pool = OrderPool::new(2, Arc::clone(&cap));
        let (h1, _) = pool.alloc_at_slot(0, order(1)).unwrap();
        assert!(pool.alloc_at_slot(0, order(2)).is_none(), "used to overwrite the live order");
        assert!(pool.alloc_at_slot(2, order(3)).is_none());
        assert_eq!(pool.get(h1).unwrap().id, 1);
        assert_eq!(pool.live_count(), 1);
    }

    #[test]
    fn test_stale_handle_cannot_free_reused_slot() {
        let cap = Arc::new(Capacity::new(1));
        let mut pool = OrderPool::new(1, Arc::clone(&cap));
        assert_eq!(cap.reserve(), ReserveResult::Reserved(0));
        let (old, _) = pool.alloc_at_slot(0, order(1)).unwrap();
        assert!(pool.dealloc(old));
        assert!(!pool.dealloc(old), "double free");
        assert_eq!((pool.live_count(), cap.reserved_count()), (0, 0));

        assert_eq!(cap.reserve(), ReserveResult::Reserved(0));
        let (new, _) = pool.alloc_at_slot(0, order(2)).unwrap();
        assert!(pool.get(old).is_none());
        assert!(!pool.dealloc(old), "stale handle used to free the new occupant");
        pool.release_unused_slot(0);
        assert_eq!((pool.live_count(), cap.reserved_count()), (1, 1), "live order owns its reservation");
        assert_eq!(pool.get(new).unwrap().id, 2);

        assert!(pool.dealloc(new));
        assert_eq!(cap.reserve(), ReserveResult::Reserved(0));
        pool.release_unused_slot(0);
        assert_eq!((pool.live_count(), cap.reserved_count()), (0, 0));
    }
}
