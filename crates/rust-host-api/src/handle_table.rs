/// A generic typed handle table that stores values and hands out i32 handles.
/// Used to manage WASI resource lifetimes across the Rust/C++ FFI boundary.

pub struct HandleTable<T> {
    entries: Vec<Option<T>>,
    free_list: Vec<i32>,
}

impl<T> HandleTable<T> {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            free_list: Vec::new(),
        }
    }

    pub fn insert(&mut self, value: T) -> i32 {
        if let Some(idx) = self.free_list.pop() {
            self.entries[idx as usize] = Some(value);
            idx
        } else {
            let idx = self.entries.len() as i32;
            self.entries.push(Some(value));
            idx
        }
    }

    pub fn get(&self, handle: i32) -> Option<&T> {
        self.entries.get(handle as usize).and_then(|e| e.as_ref())
    }

    pub fn get_mut(&mut self, handle: i32) -> Option<&mut T> {
        self.entries.get_mut(handle as usize).and_then(|e| e.as_mut())
    }

    pub fn remove(&mut self, handle: i32) -> Option<T> {
        if let Some(entry) = self.entries.get_mut(handle as usize) {
            let val = entry.take();
            if val.is_some() {
                self.free_list.push(handle);
            }
            val
        } else {
            None
        }
    }
}
