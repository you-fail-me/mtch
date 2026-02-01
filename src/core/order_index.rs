use crate::core::free_list::FreeList;
use crate::core::order::OrderLocation;
use crate::error::Error;

/// Fixed-capacity order index with O(1) operations and ID recycling.
pub struct OrderIndex {
    pub entries: Box<[Option<OrderLocation>]>,
    pub next_id: u64,
    pub live_count: usize,
    pub free_list: FreeList,
}

impl OrderIndex {
    pub fn new(capacity: usize) -> Self {
        let mut vec: Vec<Option<OrderLocation>> = Vec::with_capacity(capacity);

        for _ in 0..capacity {
            vec.push(None);
        }
        let entries = vec.into_boxed_slice();

        Self {
            entries,
            next_id: 0,
            live_count: 0,
            free_list: FreeList::new(capacity),
        }
    }

    pub fn next_id(&mut self) -> u64 {
        if let Some(recycled_id) = self.free_list.pop() {
            recycled_id as u64
        } else {
            let id = self.next_id;
            self.next_id += 1;
            id
        }
    }

    pub fn insert(&mut self, id: u64, location: OrderLocation) -> Result<(), Error> {
        let idx = (id as usize) % self.entries.len();
        if self.entries[idx].is_some() {
            return Err(Error::CapacityCollision);
        }
        self.entries[idx] = Some(location);
        self.live_count += 1;
        Ok(())
    }

    pub fn get(&self, id: u64) -> Option<&OrderLocation> {
        let idx = (id as usize) % self.entries.len();
        self.entries[idx].as_ref()
    }

    pub fn remove(&mut self, id: u64) -> Option<OrderLocation> {
        let idx = (id as usize) % self.entries.len();
        let location = self.entries[idx].take();
        if location.is_some() {
            self.live_count = self.live_count.saturating_sub(1);
            self.free_list.push(id as usize);
        }
        location
    }
}
