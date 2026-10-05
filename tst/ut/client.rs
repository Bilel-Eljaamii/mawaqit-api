//! Unit tests for the client's pure helpers and validation logic (moved
//! out of `src/client.rs`), exercised through the **public** API.
//!
//! The private `resolve_timeouts` matrix is not ported: that behavior is
//! documented and only observable through the transport itself.

use std::time::Duration;

use mawaqit_api::{
    MawaqitClient, MawaqitError, is_valid_slug, minutes_between,
};

#[test]
fn minutes_between_handles_wrap() {
    assert_eq!(minutes_between("10:00", "10:30"), Some(30));
    assert_eq!(minutes_between("23:30", "00:10"), Some(40));
}

#[test]
fn slug_length_is_bounded() {
    // review M1: an all-lowercase blob used to be "valid" at any size
    assert!(is_valid_slug(&"a".repeat(128)));
    assert!(!is_valid_slug(&"a".repeat(129)));
    assert!(!is_valid_slug(&"x".repeat(100_000)));
}

// ------------------------------------------------- SOCKS5/Tor proxy

#[test]
fn proxy_validation_accepts_socks5h() {
    for addr in [
        "socks5h://127.0.0.1", // system tor: port defaults to 9050
        "socks5h://127.0.0.1:9050",
        "socks5h://localhost:9150", // Tor Browser
        "socks5h://[::1]:9050",
        "socks5h://user:pass@host:1080",
    ] {
        let client = MawaqitClient::new().with_socks_proxy(addr);
        assert!(client.is_ok(), "{addr:?} must be accepted: {client:?}");
    }
}

#[test]
fn proxy_validation_rejects_everything_that_is_not_socks5h() {
    for addr in [
        "",
        "   ",
        "garbage",
        "127.0.0.1:9050",          // no scheme
        "socks5://127.0.0.1:9050", // local DNS — defeats Tor
        "http://127.0.0.1:8080",
        "https://127.0.0.1:443",
        "ftp://host",
        "socks5h://", // empty host
        "socks5h://host:9050/path",
        "socks5h://host/?x=1",
        "socks5h://host#frag",
    ] {
        let err = MawaqitClient::new().with_socks_proxy(addr).expect_err(addr);
        assert!(
            matches!(err, MawaqitError::InvalidProxy(_)),
            "{addr:?}: {err}"
        );
    }
}

#[test]
fn builders_compose_in_any_order() {
    // proxy first, offline layer after
    let _a = MawaqitClient::new()
        .with_socks_proxy("socks5h://127.0.0.1:9050")
        .expect("valid proxy")
        .with_disk_cache(std::env::temp_dir().join("mawaqit-ut-proxy-a"));
    // offline layer and explicit timeouts first, proxy last
    let _b = MawaqitClient::with_base_urls(
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .with_disk_cache(std::env::temp_dir().join("mawaqit-ut-proxy-b"))
    .with_timeouts(Duration::from_secs(5), Duration::from_secs(15))
    .with_socks_proxy("socks5h://localhost")
    .expect("valid proxy");
}

#[test]
fn invalid_proxy_fails_fast() {
    let err = MawaqitClient::new()
        .with_socks_proxy("socks5://127.0.0.1:9050")
        .expect_err("plain socks5 must be rejected");
    assert!(matches!(err, MawaqitError::InvalidProxy(_)));
}
