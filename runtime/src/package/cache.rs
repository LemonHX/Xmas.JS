//! A cache whose misses are filled by one task and awaited by the rest.

use async_lock::OnceCell;
use dashmap::DashMap;
use std::future::Future;
use std::pin::Pin;
use std::{fmt::Debug, hash::Hash, sync::Arc};

type Loader<K, V> = Box<dyn Fn(K) -> Pin<Box<dyn Future<Output = V> + Send>> + Send + Sync>;

pub struct Cache<K: Eq + Hash + Clone + Send + Debug + 'static, V: Clone + Send + 'static> {
    loader: Loader<K, V>,
    map: DashMap<K, Arc<OnceCell<V>>>,
}

impl<K: Eq + Hash + Clone + Send + Debug + 'static, V: Clone + Send + 'static> Cache<K, V> {
    pub fn new(
        loader: impl Fn(K) -> Pin<Box<dyn Future<Output = V> + Send>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            loader: Box::new(loader),
            map: DashMap::new(),
        }
    }

    /// The value for a key: one task loads it, and every task that asks meanwhile waits on that
    /// one instead of starting a second request.
    pub async fn get(&self, key: K) -> V {
        let cell = self
            .map
            .entry(key.clone())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();

        let loader = &self.loader;
        let value = cell.get_or_init(|| loader(key)).await;
        value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Twenty tasks ask for the same key at the same time: one loader call, twenty answers.
    #[test]
    fn a_miss_is_filled_once_for_everyone_waiting() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);

        let cache = Arc::new(Cache::new(|_key: String| {
            Box::pin(async move {
                CALLS.fetch_add(1, Ordering::SeqCst);
                smol::Timer::after(Duration::from_millis(50)).await;
                7usize
            })
        }));

        smol::block_on(async {
            let mut tasks = vec![];
            for _ in 0..20 {
                let cache = Arc::clone(&cache);
                tasks.push(smol::spawn(async move { cache.get("key".to_string()).await }));
            }
            for task in tasks {
                assert_eq!(task.await, 7);
            }
        });

        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    }
}
