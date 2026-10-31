use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Residency {
    Unloaded,
    Loading,
    CpuReady,
    GpuReady,
    Failed,
}
struct Entry<V> {
    value: Arc<V>,
    bytes: usize,
    used: u64,
}
pub struct ByteLru<K, V> {
    entries: HashMap<K, Entry<V>>,
    pinned: HashSet<K>,
    budget: usize,
    clock: u64,
    bytes: usize,
}
impl<K: Eq + Hash + Clone, V> ByteLru<K, V> {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            pinned: HashSet::new(),
            budget,
            clock: 0,
            bytes: 0,
        }
    }
    pub fn get(&mut self, key: &K) -> Option<Arc<V>> {
        self.clock += 1;
        self.entries.get_mut(key).map(|e| {
            e.used = self.clock;
            Arc::clone(&e.value)
        })
    }
    pub fn insert(&mut self, key: K, value: Arc<V>, bytes: usize) {
        self.clock += 1;
        if let Some(e) = self.entries.remove(&key) {
            self.bytes -= e.bytes;
        }
        self.entries.insert(
            key,
            Entry {
                value,
                bytes,
                used: self.clock,
            },
        );
        self.bytes += bytes;
        self.evict();
    }
    pub fn pin(&mut self, keys: impl IntoIterator<Item = K>) {
        self.pinned = keys.into_iter().collect();
        self.evict();
    }
    fn evict(&mut self) {
        while self.bytes > self.budget {
            let key = self
                .entries
                .iter()
                .filter(|(k, _)| !self.pinned.contains(*k))
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone());
            let Some(key) = key else {
                break;
            };
            self.bytes -= self.entries.remove(&key).expect("resident").bytes;
        }
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }
    /// Inspect resident metadata without changing eviction order.
    pub fn peek(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|entry| entry.value.as_ref())
    }
}
#[derive(Default)]
pub struct Generation(pub u64);
impl Generation {
    pub fn advance(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
    pub fn accepts(&self, result: u64) -> bool {
        self.0 == result
    }
}
