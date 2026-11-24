//! Resources: one value per type shared by the whole game (asset server, scene registry, physics
//! world...). Plugins insert theirs in `build`; behaviours and other plugins reach them through
//! `ctx.resources`.

use std::any::{Any, TypeId};
use std::collections::HashMap;

#[derive(Default)]
pub struct Resources {
    map: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    pub fn insert<T: 'static>(&mut self, value: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(value));
    }
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.map.get(&TypeId::of::<T>()).and_then(|b| b.downcast_ref())
    }
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.map.get_mut(&TypeId::of::<T>()).and_then(|b| b.downcast_mut())
    }
    /// The resource, created with `Default` the first time.
    pub fn get_or_default<T: Default + 'static>(&mut self) -> &mut T {
        self.map.entry(TypeId::of::<T>()).or_insert_with(|| Box::new(T::default())).downcast_mut().expect("resource type")
    }
    pub fn remove<T: 'static>(&mut self) -> Option<T> {
        self.map.remove(&TypeId::of::<T>()).and_then(|b| b.downcast().ok()).map(|b| *b)
    }
    /// Takes a resource out while code that also needs `&mut Context` uses it (put it back after).
    pub fn take<T: 'static>(&mut self) -> Option<Box<T>> {
        self.map.remove(&TypeId::of::<T>()).and_then(|b| b.downcast().ok())
    }
    pub fn put_back<T: 'static>(&mut self, value: Box<T>) {
        self.map.insert(TypeId::of::<T>(), value);
    }
}
