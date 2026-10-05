//! Unit tests for the in-process TTL cache (moved out of `src/cache.rs`).
//!
//! [`TtlCache`] is `#[doc(hidden)]`-public purely so these live in the `ut`
//! tier: the cap keeps a long-running client from growing without bound
//! (review H1), and eviction is FIFO over still-present keys.

use std::time::Duration;

use mawaqit_api::cache::TtlCache;

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

/// FINDING F25 — expired keys leave their `order` entries behind forever:
/// the map stays capped, but a long-running client churning TTLs grows the
/// order queue without bound (every churned key pushes exactly one entry
/// nothing ever removes). The queue compacts once it passes twice the cap.
#[test]
fn finding_f25_order_queue_compacts_under_ttl_churn() {
    let cache: TtlCache<u32> = TtlCache::new(Duration::ZERO, 4);
    for i in 0..200 {
        cache.insert(format!("key-{i}"), i);
        // A get past the TTL expires the entry: map row removed, order
        // row stays — the churn pattern of the 6h/30min client caches.
        let _ = cache.get(&format!("key-{i}"));
    }
    assert!(
        cache.order_len() <= 2 * 4,
        "order queue must compact under churn, got {} entries for a cap-4 cache",
        cache.order_len()
    );
    // Compaction preserves eviction correctness on a live cache.
    let live: TtlCache<u32> = TtlCache::new(Duration::from_secs(60), 2);
    live.insert("a".into(), 1);
    live.insert("b".into(), 2);
    live.insert("c".into(), 3);
    assert_eq!(live.get("a"), None, "oldest evicted");
    assert_eq!(live.get("c"), Some(3));
}
