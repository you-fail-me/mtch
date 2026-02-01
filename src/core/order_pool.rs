use crate::core::free_list::FreeList;
use crate::core::order::{Handle, Order, Side};

// Not pretty but avoids the unsafe of MaybeUninit and the overhead of Option
fn placeholder_order() -> Order {
    Order::new_limit(0, 0, Side::Ask, 0, 0, 0)
}

#[derive(Debug)]
struct Slot {
    order: Order,
    generation: usize,
    alive: bool,
}

/// Fixed-capacity order allocator with generational handles, O(1) alloc/dealloc/get.
pub struct OrderPool {
    slots: Box<[Slot]>,
    pub free_list: FreeList,
    pub bump_idx: usize,
    pub live_count: usize,
}

impl OrderPool {
    pub fn new(capacity: usize) -> Self {
        let mut vec: Vec<Slot> = Vec::with_capacity(capacity);

        for _ in 0..capacity {
            vec.push(Slot {
                order: placeholder_order(),
                generation: 0,
                alive: false,
            });
        }
        let slots = vec.into_boxed_slice();

        OrderPool {
            slots,
            bump_idx: 0,
            free_list: FreeList::new(capacity),
            live_count: 0,
        }
    }

    /// Allocates space for an order and returns a handle.
    pub fn alloc(&mut self, order: Order) -> Option<(Handle, &mut Order)> {
        if let Some(recycled_idx) = self.free_list.pop() {
            return self.alloc_at(recycled_idx, order);
        }

        if self.bump_idx < self.slots.len() {
            let idx = self.bump_idx;
            self.bump_idx += 1;
            return self.alloc_at(idx, order);
        }

        None
    }

    pub fn dealloc(&mut self, handle: Handle) {
        let idx = handle.idx;
        if idx < self.slots.len() {
            let slot = &mut self.slots[idx];
            slot.alive = false;
            self.free_list.push(handle.idx);
            self.live_count = self.live_count.saturating_sub(1);
        }
    }

    /// Gets an immutable reference to an order.
    pub fn get(&self, handle: Handle) -> Option<&Order> {
        let idx = handle.idx;
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
        let idx = handle.idx;
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

    fn alloc_at(&mut self, idx: usize, order: Order) -> Option<(Handle, &mut Order)> {
        if idx >= self.slots.len() {
            return None;
        }

        let slot = &mut self.slots[idx];
        slot.generation = slot.generation.wrapping_add(1);
        slot.alive = true;
        slot.order = order;
        self.live_count += 1;

        let handle = Handle { idx, generation: slot.generation };

        Some((handle, &mut slot.order))
    }
}
