use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

/// Minimal in-process TTL cache with an entry cap (the reference
/// mawaqit-api uses Redis for this). The cap keeps a long-running client —
/// the desktop loop shares one across every mosque the user browses — from
/// growing without bound (review H1); eviction is FIFO over still-present
/// keys, so the freshest lookups survive.
pub(crate) struct TtlCache<V> {
    entries: Mutex<Entries<V>>,
    ttl: Duration,
    max_entries: usize,
}

struct Entries<V> {
    map: HashMap<String, (Instant, V)>,
    /// Insertion order of keys, including keys already gone from `map`
    /// (expired via `get`, invalidated). Skipped during eviction.
    order: VecDeque<String>,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(Entries {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
            ttl,
            max_entries: max_entries.max(1),
        }
    }

    pub fn get(&self, key: &str) -> Option<V> {
        let mut entries = self.entries.lock().ok()?;
        let (created, value) = entries.map.get(key)?;
        if created.elapsed() >= self.ttl {
            entries.map.remove(key);
            return None;
        }
        Some(value.clone())
    }

    pub fn insert(&self, key: String, value: V) {
        let mut entries = match self.entries.lock() {
            Ok(entries) => entries,
            Err(_) => return,
        };
        if !entries.map.contains_key(&key) {
            entries.order.push_back(key.clone());
        }
        entries.map.insert(key, (Instant::now(), value));
        while entries.map.len() > self.max_entries {
            match entries.order.pop_front() {
                // Evicted one live entry; stale order entries are skipped
                // and the loop keeps going.
                Some(k) => {
                    entries.map.remove(&k);
                }
                None => break,
            }
        }
    }

    pub fn invalidate(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.map.remove(key);
        }
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.map.clear();
            entries.order.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn stores_and_expires() {
        let cache: TtlCache<u32> = TtlCache::new(Duration::from_secs(60), 16);
        cache.insert("a".into(), 1);
        assert_eq!(cache.get("a"), Some(1));
        cache.invalidate("a");
        assert_eq!(cache.get("a"), None);
    }

    #[test]
    fn zero_ttl_expires_immediately() {
        let cache: TtlCache<u32> = TtlCache::new(Duration::ZERO, 16);
        cache.insert("a".into(), 1);
        assert_eq!(cache.get("a"), None);
    }

    #[test]
    fn cap_evicts_the_oldest_entries() {
        let cache: TtlCache<u32> = TtlCache::new(Duration::from_secs(60), 2);
        cache.insert("a".into(), 1);
        cache.insert("b".into(), 2);
        cache.insert("c".into(), 3);
        assert_eq!(cache.get("a"), None, "oldest evicted");
        assert_eq!(cache.get("b"), Some(2));
        assert_eq!(cache.get("c"), Some(3));
    }

    #[test]
    fn overwrite_does_not_evict_or_grow() {
        let cache: TtlCache<u32> = TtlCache::new(Duration::from_secs(60), 2);
        cache.insert("a".into(), 1);
        cache.insert("b".into(), 2);
        cache.insert("a".into(), 10);
        assert_eq!(cache.get("a"), Some(10));
        assert_eq!(cache.get("b"), Some(2));
    }

    #[test]
    fn eviction_survives_stale_order_entries() {
        // invalidate() removes a key from the map but leaves its order
        // entry; eviction must skip the stale entry and still evict a
        // live one.
        let cache: TtlCache<u32> = TtlCache::new(Duration::from_secs(60), 2);
        cache.insert("a".into(), 1);
        cache.insert("b".into(), 2);
        cache.invalidate("a"); // map={b}, order=[a, b] — `a` is stale
        cache.insert("c".into(), 3);
        cache.insert("d".into(), 4); // evicts b, after skipping stale a
        cache.insert("e".into(), 5); // evicts c
        assert_eq!(cache.get("a"), None, "invalidated");
        assert_eq!(cache.get("b"), None, "evicted through a stale entry");
        assert_eq!(cache.get("c"), None, "evicted");
        assert_eq!(cache.get("d"), Some(4));
        assert_eq!(cache.get("e"), Some(5));
    }
}
