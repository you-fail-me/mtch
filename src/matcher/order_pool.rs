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

/// Fixed-capacity order storage with generational handles.
pub struct OrderPool {
    slots: Box<[Slot]>,
    pub live_count: usize,
    capacity: Arc<Capacity>,
}

impl OrderPool {
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

    pub fn dealloc(&mut self, handle: Handle) {
        let idx = handle.idx as usize;
        if idx < self.slots.len() {
            let slot = &mut self.slots[idx];
            if slot.alive {
                slot.alive = false;
                self.live_count = self.live_count.saturating_sub(1);
                self.capacity.release(handle.idx);
            }
        }
    }

    /// Gets an immutable reference to an order.
    pub fn get(&self, handle: Handle) -> Option<&Order> {
        let idx = handle.idx as usize;
        if idx >= self.slots.len() {
            return None;
        }

        let slot = &self.slots[idx];

        if !slot.alive {
            return None;
        }

        if slot.generation != handle.generation {
            return None;
        }

        Some(&slot.order)
    }

    /// Gets a mutable reference to an order.
    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut Order> {
        let idx = handle.idx as usize;
        if idx >= self.slots.len() {
            return None;
        }

        let slot = &mut self.slots[idx];

        if !slot.alive {
            return None;
        }

        if slot.generation != handle.generation {
            return None;
        }

        Some(&mut slot.order)
    }

    /// Allocate an order at a specific slot index (for pre-reserved slots from Capacity)
    pub fn alloc_at_slot(&mut self, slot_id: u64, order: Order) -> Option<(Handle, &mut Order)> {
        let idx = slot_id as usize;
        if idx >= self.slots.len() {
            return None;
        }

        let slot = &mut self.slots[idx];
        slot.generation = slot.generation.wrapping_add(1);
        slot.alive = true;
        slot.order = order;
        self.live_count += 1;

        let handle = Handle { idx: slot_id, generation: slot.generation };

        Some((handle, &mut slot.order))
    }
}
