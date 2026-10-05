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
        // A poisoned mutex only means "no caching"; never propagate.
        let _ = self.insert_if_healthy(key, value);
    }

    fn insert_if_healthy(&self, key: String, value: V) -> Option<()> {
        let mut entries = self.entries.lock().ok()?;
        if !entries.map.contains_key(&key) {
            entries.order.push_back(key.clone());
        }
        entries.map.insert(key, (Instant::now(), value));
        // Every live entry keeps an order entry (pushed on first insert),
        // so the loop always finds a victim while over the cap; stale
        // order entries (expired/invalidated keys) are skipped for free.
        while entries.map.len() > self.max_entries
            && let Some(k) = entries.order.pop_front()
        {
            entries.map.remove(&k);
        }
        // FINDING F25: expired/invalidated keys leave their order entries
        // behind forever, so a long-running client churning TTLs grows the
        // queue without bound even though the map stays capped. Compact
        // once the queue passes twice the cap — bounded work, amortized
        // over the churn that caused it.
        if entries.order.len() > self.max_entries.saturating_mul(2) {
            let Entries { map, order } = &mut *entries;
            order.retain(|k| map.contains_key(k));
        }
        Some(())
    }

    pub fn invalidate(&self, key: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.map.remove(key);
        }
    }

    /// Length of the internal insertion-order queue — `#[doc(hidden)]`
    /// test instrumentation for the FINDING F25 compaction pin, same
    /// status as the module itself: not public API, not semver.
    #[doc(hidden)]
    pub fn order_len(&self) -> usize {
        self.entries.lock().map(|e| e.order.len()).unwrap_or(0)
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.map.clear();
            entries.order.clear();
        }
    }
}
