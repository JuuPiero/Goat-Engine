//! Typed, generational handles into pools: resources (meshes, textures, materials) are owned by
//! their pool and referred to by small `Copy` values — no lifetimes, no raw pointers, and a stale
//! handle (its slot reused) is detected instead of reading the wrong object.

use std::marker::PhantomData;

/// A handle to a `T` in a [`Pool<T>`].
pub struct Handle<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    pub fn index(self) -> u32 {
        self.index
    }
    /// A raw id (index + generation) for logging / hashing.
    pub fn id(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Handle<T> {}
impl<T> PartialEq for Handle<T> {
    fn eq(&self, o: &Self) -> bool {
        self.index == o.index && self.generation == o.generation
    }
}
impl<T> Eq for Handle<T> {}
impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.id().hash(h)
    }
}
impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Handle<{}>({}v{})", std::any::type_name::<T>().rsplit("::").next().unwrap_or("?"), self.index, self.generation)
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Owns values of `T`; hands out [`Handle`]s.
pub struct Pool<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Pool { slots: Vec::new(), free: Vec::new() }
    }
}

impl<T> Pool<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, value: T) -> Handle<T> {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.value = Some(value);
            Handle { index, generation: slot.generation, _marker: PhantomData }
        } else {
            self.slots.push(Slot { generation: 0, value: Some(value) });
            Handle { index: (self.slots.len() - 1) as u32, generation: 0, _marker: PhantomData }
        }
    }

    pub fn get(&self, h: Handle<T>) -> Option<&T> {
        let s = self.slots.get(h.index as usize)?;
        if s.generation == h.generation { s.value.as_ref() } else { None }
    }

    pub fn get_mut(&mut self, h: Handle<T>) -> Option<&mut T> {
        let s = self.slots.get_mut(h.index as usize)?;
        if s.generation == h.generation { s.value.as_mut() } else { None }
    }

    /// Removes the value; the handle (and copies of it) become stale.
    pub fn remove(&mut self, h: Handle<T>) -> Option<T> {
        let s = self.slots.get_mut(h.index as usize)?;
        if s.generation != h.generation || s.value.is_none() {
            return None;
        }
        s.generation = s.generation.wrapping_add(1);
        self.free.push(h.index);
        s.value.take()
    }

    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.value.as_ref().map(|v| (Handle { index: i as u32, generation: s.generation, _marker: PhantomData }, v))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_handles_are_rejected() {
        let mut p = Pool::new();
        let a = p.insert(1);
        assert_eq!(p.get(a), Some(&1));
        assert_eq!(p.remove(a), Some(1));
        let b = p.insert(2);
        assert_eq!(a.index(), b.index());
        assert_eq!(p.get(a), None);
        assert_eq!(p.get(b), Some(&2));
        assert_eq!(p.len(), 1);
    }
}
