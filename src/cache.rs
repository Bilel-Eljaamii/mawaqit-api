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
///
/// # Internal
///
/// Exposed `#[doc(hidden)]` only so the unit tests live in `tst/ut/cache.rs`
/// instead of `src/`. Not part of the public API; not covered by semver.
pub struct TtlCache<V> {
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
