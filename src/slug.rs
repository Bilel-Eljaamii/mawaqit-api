//! Domain-core mosque-slug policy: the namespace rules that keep hostile
//! strings from escaping the mosque page path (FINDING F2). Pure functions,
//! no allocator below `page_url` (which returns an owned `String` and so
//! lives at the `alloc` tier).

/// The language segment of every mosque page URL the client fetches.
#[cfg(any(feature = "std", feature = "alloc"))]
pub(crate) const PAGE_LANG: &str = "en";

/// Whether `slug` is a mosque page identifier in the shape mawaqit.net
/// publishes (`grande-mosquee-de-paris`): lowercase letters and digits,
/// single hyphens between segments, at most 128 bytes (review M1 — a
/// hostile all-lowercase blob would otherwise be "valid" and reach the
/// wire as a giant URL). FINDING F2: anything else must never reach the
/// network verbatim — `../` escapes the mosque namespace and `?`/`#` swap
/// the page under a legit-looking slug.
///
/// # Examples
///
/// ```rust
/// use mawaqit_api::is_valid_slug;
///
/// assert!(is_valid_slug("grande-mosquee-de-paris"));
/// assert!(!is_valid_slug("../admin"));      // path escape
/// assert!(!is_valid_slug("paris?next=#x")); // query/fragment injection
/// assert!(!is_valid_slug("-leading-dash")); // not a published shape
/// ```
pub fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 128
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
}

/// The exact URL [`crate::MawaqitClient::conf_data`] fetches for a slug.
/// Exposed as a pure function so hostile-slug handling (`../`, `?`, `#`,
/// giant or non-ASCII slugs) can be asserted without touching the network.
///
/// # Examples
///
/// ```rust
/// use mawaqit_api::page_url;
///
/// assert_eq!(
///     page_url("https://mawaqit.net", "grande-mosquee-de-paris"),
///     "https://mawaqit.net/en/grande-mosquee-de-paris",
/// );
/// ```
#[cfg(any(feature = "std", feature = "alloc"))]
pub fn page_url(site_base: &str, mosque_id: &str) -> alloc::string::String {
    alloc::format!("{site_base}/{PAGE_LANG}/{mosque_id}")
}
