//! A cache whose misses are filled by one thread and waited on by the rest.

use dashmap::DashMap;
use std::{
    fmt::Debug,
    hash::Hash,
    sync::{Arc, OnceLock},
};

pub struct Cache<K: Eq + Hash + Clone + Send + Debug + 'static, V: Clone + Send + 'static> {
    loader: Box<dyn Fn(&K) -> V + Send + Sync + 'static>,
    map: DashMap<K, Arc<OnceLock<V>>>,
}

impl<K: Eq + Hash + Clone + Send + Debug + 'static, V: Clone + Send + 'static> Cache<K, V> {
    pub fn new(loader: impl Fn(&K) -> V + Send + Sync + 'static) -> Self {
        Self {
            loader: Box::new(loader),
            map: DashMap::new(),
        }
    }

    /// The value for a key, loaded once no matter how many threads ask at the same time.
    pub fn get(&self, key: K) -> V {
        let cell = self
            .map
            .entry(key.clone())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone();
        cell.get_or_init(|| (self.loader)(&key)).clone()
    }
}
