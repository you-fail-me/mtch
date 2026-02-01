use crate::core::order::{Handle, Order};
use crate::core::order_pool::OrderPool;

// TODO: use Result<(), E> and propagate errors instead of Option<()>

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

    // Adds an order to the tail, to maintain price-time priority
    pub fn add_order(&mut self, order: Order, pool: &mut OrderPool) -> Option<(Handle, bool)> {
        let was_empty = self.is_empty();

        let qty = order.qty;
        // Allocate
        let (handle, order_ptr) = pool.alloc(order)?;
        // Update price level metrics
        self.qty += qty;
        self.count += 1;

        // Insert at the end = set the new order's prev and next indices
        order_ptr.prev = self.tail;
        order_ptr.next = None;

        // Price level not empty, has tail set - set the old tail's next index to this order
        if let Some(tail) = self.tail {
            // TODO: what to do if it's None? Potentially should shut down as it would mean the allocation is broken
            let current_tail = pool.get_mut(tail).unwrap();
            current_tail.next = Some(handle);
        } else {
            // It's an empty price level - just need to set the head of it
            self.head = Some(handle);
        }

        // This order becomes the tail in any case
        self.tail = Some(handle);

        let became_non_empty = was_empty && !self.is_empty();
        Some((handle, became_non_empty))
    }

    // Remove (cancelation) - O(1)
    pub fn remove_order(&mut self, handle: Handle, pool: &mut OrderPool) -> Option<bool> {
        let was_empty = self.is_empty();

        // Extract data and drop the borrow
        let (prev, next, qty) = {
            let order = pool.get_mut(handle)?;
            (order.prev, order.next, order.qty)
        };

        // Update price level metrics
        self.qty -= qty;
        self.count -= 1;

        // Fix first link
        if let Some(prev_handle) = prev {
            // Order has previous node (is not head) - link the previous node to the next one
            pool.get_mut(prev_handle).unwrap().next = next;
        } else {
            // Order has no previous node (is head) - set its next node as level's head
            self.head = next;
        }

        // Fix next link
        if let Some(next_handle) = next {
            // Order has next node (is not tail) - link its next node to its previous node
            pool.get_mut(next_handle).unwrap().prev = prev;
        } else {
            // Order has no next node (is tail) - set its previous node as level's tail
            self.tail = prev;
        }

        // Clean up removed node
        let order = pool.get_mut(handle).unwrap();
        order.prev = None;
        order.next = None;

        let became_empty = !was_empty && self.is_empty();
        Some(became_empty)
    }

    // Get best price
    pub fn peek_head<'a>(&self, pool: &'a OrderPool) -> Option<&'a Order> {
        self.head.and_then(|head| pool.get(head))
    }

    pub fn remove_head(&mut self, pool: &mut OrderPool) -> Option<()> {
        if let Some(old_head_handle) = self.head {
            // Extract data and drop the borrow
            let (old_head_next, old_head_qty) = {
                let order = pool.get_mut(old_head_handle)?;
                (order.next, order.qty)
            };

            // Update metrics
            self.qty -= old_head_qty;
            self.count -= 1;

            // Move head pseudopointer to the next order
            self.head = old_head_next;

            // Update new head's prev if exists
            if let Some(new_head_handle) = self.head {
                pool.get_mut(new_head_handle).unwrap().prev = None;
            } else {
                // List is now empty
                self.tail = None;
            }

            // Clean up removed node
            let old_head = pool.get_mut(old_head_handle).unwrap();
            old_head.prev = None;
            old_head.next = None;

            Some(())
        } else {
            return None;
        }
    }

    pub fn update_head_qty(&mut self, new_qty: u64, pool: &mut OrderPool) {
        if let Some(head_handle) = self.head {
            if let Some(head) = pool.get_mut(head_handle) {
                let qty_diff = head.qty.saturating_sub(new_qty);

                self.qty = self.qty.saturating_sub(qty_diff);

                head.qty = new_qty;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.qty == 0
    }

    pub fn order_count(&self) -> usize {
        self.count
    }
}
