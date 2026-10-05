use std::{path::PathBuf, sync::Arc, time::Duration};

use chrono::{Local, NaiveDate};

use crate::{
    cache::TtlCache,
    calendar, disk,
    error::{MawaqitError, Result},
    models::{ConfData, MonthIqamaTimes, MonthTimes, Mosque, TodayTimes},
};

const API_URL_BASE: &str = "https://mawaqit.net/api";
const SITE_URL_BASE: &str = "https://mawaqit.net";
const PAGE_LANG: &str = "en";
/// Requests without a browser-ish User-Agent get rejected by the site.
const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:132.0) Gecko/20100101 Firefox/132.0";
/// confData carries the whole year; refetching a few times a day is plenty.
const CONF_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const SEARCH_TTL: Duration = Duration::from_secs(30 * 60);
/// Cache caps (review H1): a desktop loop browsing hundreds of mosques must
/// not grow without bound. FIFO eviction keeps the freshest lookups.
const MAX_CACHED_PAGES: usize = 64;
const MAX_CACHED_SEARCHES: usize = 256;
/// Give up rather than hang the caller (the desktop loop shares this client).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Tor circuits are slow: while a SOCKS proxy is set, these replace the
/// defaults unless [`MawaqitClient::with_timeouts`] overrode them.
const PROXY_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const PROXY_REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
/// SOCKS5 default when the proxy address carries no port: the system tor
/// daemon. Tor Browser users pass 9150 explicitly.
const SOCKS_DEFAULT_PORT: u16 = 9050;
/// A real mosque page is ~60 KB; 1 MB is ~15x headroom (review H2 — the old
/// 20 MB cap contradicted its own comment). Enforced while streaming.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Keyless client for mawaqit.net — no account required.
///
/// - Mosque search uses the public endpoint `GET
///   /api/2.0/mosque/search?word=…`.
/// - All prayer data comes from the public mosque page `https://mawaqit.net/{lang}/{slug}`,
///   whose embedded `confData` object holds the daily times, the year calendar,
///   the iqama calendar and the mosque metadata. One fetched page is shared by
///   every data method and cached in memory (6h for pages, 30min for searches).
/// - Optional Tor routing: [`MawaqitClient::with_socks_proxy`] sends every
///   request through a SOCKS5 proxy with remote DNS. The library never enables
///   it by default and never starts or bundles a Tor daemon.
#[derive(Clone)]
pub struct MawaqitClient {
    inner: Arc<Inner>,
    /// Offline snapshot directory (see [`crate::disk`]); shared with clones,
    /// `None` = feature off.
    disk: Option<Arc<PathBuf>>,
}

struct Inner {
    http: reqwest::Client,
    api_base: String,
    site_base: String,
    pages: TtlCache<Arc<ConfData>>,
    searches: TtlCache<Vec<Mosque>>,
    /// Construction knobs, retained so the chainable builders
    /// ([`MawaqitClient::with_socks_proxy`],
    /// [`MawaqitClient::with_timeouts`]) can rebuild the transport.
    /// `explicit_timeouts` is `None` for the defaults, which a proxy
    /// raises automatically.
    proxy: Option<String>,
    explicit_timeouts: Option<(Duration, Duration)>,
}

impl MawaqitClient {
    pub fn new() -> Self {
        Self::with_base_urls(
            API_URL_BASE.to_string(),
            SITE_URL_BASE.to_string(),
        )
    }

    /// Same client against custom base URLs — the seam the hostile HTTP
    /// tests use to point the client at a local mock server.
    pub fn with_base_urls(api_base: String, site_base: String) -> Self {
        Self::from_parts(api_base, site_base, None, None, None)
            .expect("default construction cannot fail: no proxy to configure")
    }

    /// Single construction path: every builder funnels here so base URLs,
    /// the disk cache, the SOCKS proxy and the timeouts compose in any
    /// order. `explicit_timeouts` is `None` for the defaults (raised
    /// automatically when a proxy is set).
    fn from_parts(
        api_base: String,
        site_base: String,
        disk: Option<Arc<PathBuf>>,
        proxy: Option<String>,
        explicit_timeouts: Option<(Duration, Duration)>,
    ) -> Result<Self> {
        let (connect_timeout, request_timeout) =
            resolve_timeouts(proxy.is_some(), explicit_timeouts);
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(request_timeout)
            .connect_timeout(connect_timeout)
            // FINDING F1: never follow redirects. A 302 — same-origin or
            // not — surfaces as `Api { status }` instead of turning
            // conf_data into "parse whatever the redirect target serves".
            .redirect(reqwest::redirect::Policy::none());
        if let Some(proxy) = &proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy)?);
        }
        let http = builder
            .build()
            .expect("reqwest client builds without custom TLS config");
        Ok(Self {
            inner: Arc::new(Inner {
                http,
                api_base,
                site_base,
                pages: TtlCache::new(CONF_TTL, MAX_CACHED_PAGES),
                searches: TtlCache::new(SEARCH_TTL, MAX_CACHED_SEARCHES),
                proxy,
                explicit_timeouts,
            }),
            disk,
        })
    }

    /// Serve [`Self::conf_data`] from a disk snapshot when the network is
    /// unavailable, and refresh the snapshot on every successful fetch —
    /// the offline layer. See [`crate::disk`].
    pub fn with_disk_cache(mut self, dir: PathBuf) -> Self {
        self.disk = Some(Arc::new(dir));
        self
    }

    /// Route all traffic through a SOCKS5 proxy with **remote DNS** — the
    /// Tor opt-in. The address must be `socks5h://host[:port]`:
    /// `socks5://` and any http(s) proxy are rejected
    /// ([`MawaqitError::InvalidProxy`]) because resolving DNS outside the
    /// proxy defeats the purpose. A missing port defaults to 9050 (the
    /// system tor daemon; Tor Browser users pass 9150). Timeouts rise to
    /// 30 s connect / 90 s request unless [`Self::with_timeouts`]
    /// overrode them. Calling this again replaces the previous proxy.
    ///
    /// The library never starts or bundles a Tor daemon — point this at
    /// one that is already running.
    pub fn with_socks_proxy(self, addr: impl Into<String>) -> Result<Self> {
        let proxy = validate_socks_proxy(&addr.into())?;
        Self::from_parts(
            self.inner.api_base.clone(),
            self.inner.site_base.clone(),
            self.disk.clone(),
            Some(proxy),
            self.inner.explicit_timeouts,
        )
    }

    /// Override the transport timeouts (connect, request). Without this,
    /// the defaults are 10 s / 30 s — raised to 30 s / 90 s automatically
    /// when a SOCKS proxy is set. Composes with
    /// [`Self::with_socks_proxy`] in any order.
    pub fn with_timeouts(self, connect: Duration, request: Duration) -> Self {
        Self::from_parts(
            self.inner.api_base.clone(),
            self.inner.site_base.clone(),
            self.disk.clone(),
            self.inner.proxy.clone(),
            Some((connect, request)),
        )
        .expect("rebuilding with an already-validated proxy cannot fail")
    }

    /// `GET /api/2.0/mosque/search?word=...` — keyword search, no auth.
    pub async fn search_mosques(&self, word: &str) -> Result<Vec<Mosque>> {
        let word = word.trim();
        if word.is_empty() {
            return Ok(Vec::new());
        }
        let cache_key = word.to_lowercase();
        if let Some(cached) = self.inner.searches.get(&cache_key) {
            return Ok(cached);
        }

        let url = format!("{}/2.0/mosque/search", self.inner.api_base);
        let response =
            self.inner.http.get(&url).query(&[("word", word)]).send().await?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(MawaqitError::MosqueNotFound(word.to_string()));
        }
        let body = read_capped(response).await?;
        if !status.is_success() {
            return Err(MawaqitError::Api { status: status.as_u16(), url });
        }

        let mosques: Vec<Mosque> = serde_json::from_str(&body)
            .map_err(|e| MawaqitError::Parse(format!("search: {e}")))?;
        self.inner.searches.insert(cache_key, mosques.clone());
        Ok(mosques)
    }

    /// Fetch (or take from cache) the confData of a mosque page. `mosque_id`
    /// is the page slug, e.g. `grande-mosquee-de-paris`. When the network
    /// fails and a disk snapshot exists ([`Self::with_disk_cache`]), the
    /// snapshot is served instead.
    pub async fn conf_data(&self, mosque_id: &str) -> Result<Arc<ConfData>> {
        self.conf_data_dated(mosque_id).await.map(|(conf, _)| conf)
    }

    /// Like [`Self::conf_data`], additionally reporting whether the data
    /// came from the offline disk snapshot: `Some(fetched date)` when it
    /// did, `None` for a fresh (network or in-memory) fetch.
    pub async fn conf_data_dated(
        &self,
        mosque_id: &str,
    ) -> Result<(Arc<ConfData>, Option<NaiveDate>)> {
        if let Some(cached) = self.inner.pages.get(mosque_id) {
            return Ok((cached, None));
        }

        match Self::fetch_conf_data(&self.inner, mosque_id).await {
            Ok(conf) => {
                if let Some(dir) = &self.disk {
                    // Best-effort: a failed snapshot write never breaks an
                    // online fetch.
                    disk::store(dir, mosque_id, &conf);
                }
                Ok((conf, None))
            }
            Err(err) => {
                if let Some(dir) = &self.disk
                    && let Some((fetched_at, conf)) = disk::load(dir, mosque_id)
                {
                    return Ok((Arc::new(conf), Some(fetched_at)));
                }
                Err(err)
            }
        }
    }

    async fn fetch_conf_data(
        inner: &Inner,
        mosque_id: &str,
    ) -> Result<Arc<ConfData>> {
        // FINDING F2: an invalid slug is fetched as a deterministic
        // placeholder under /en/ — it can never escape the mosque namespace
        // (dot-segments, query/fragment injection, encoded variants) and
        // always 404s into MosqueNotFound.
        let slug = if is_valid_slug(mosque_id) {
            mosque_id.to_string()
        } else {
            "-".repeat(mosque_id.len().clamp(4, 64))
        };
        let url = page_url(&inner.site_base, &slug);
        let response = inner.http.get(&url).send().await?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(MawaqitError::MosqueNotFound(mosque_id.to_string()));
        }
        if !status.is_success() {
            return Err(MawaqitError::Api { status: status.as_u16(), url });
        }
        let html = read_capped(response).await?;
        let conf =
            Arc::new(crate::scraper::extract_conf_data(&html, mosque_id)?);
        inner.pages.insert(mosque_id.to_string(), conf.clone());
        Ok(conf)
    }

    /// Adhan + resolved iqama times for today (local timezone).
    pub async fn today(&self, mosque_id: &str) -> Result<TodayTimes> {
        let conf = self.conf_data(mosque_id).await?;
        calendar::times_for_date(&conf, Local::now().date_naive())
    }

    /// Adhan times for a month (1-12).
    pub async fn month(
        &self,
        mosque_id: &str,
        month: u32,
    ) -> Result<MonthTimes> {
        let conf = self.conf_data(mosque_id).await?;
        calendar::month_times(&conf, month)
    }

    /// Resolved iqama times for a month (1-12).
    pub async fn month_iqama(
        &self,
        mosque_id: &str,
        month: u32,
    ) -> Result<MonthIqamaTimes> {
        let conf = self.conf_data(mosque_id).await?;
        calendar::month_iqama_times(&conf, month)
    }

    /// Drop the cached page for a mosque — or every cached page *and*
    /// search when `None` (review H1: the search cache used to survive).
    pub fn invalidate(&self, mosque_id: Option<&str>) {
        match mosque_id {
            Some(id) => self.inner.pages.invalidate(id),
            None => {
                self.inner.pages.clear();
                self.inner.searches.clear();
            }
        }
    }
}

impl Default for MawaqitClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MawaqitClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MawaqitClient")
            .field("api_base", &self.inner.api_base)
            .field("site_base", &self.inner.site_base)
            .field("proxy", &self.inner.proxy)
            .field(
                "timeouts",
                &resolve_timeouts(
                    self.inner.proxy.is_some(),
                    self.inner.explicit_timeouts,
                ),
            )
            .field("disk_cache", &self.disk)
            .finish_non_exhaustive()
    }
}

/// The exact URL [`MawaqitClient::conf_data`] fetches for a slug. Exposed as
/// a pure function so hostile-slug handling (`../`, `?`, `#`, giant or
/// non-ASCII slugs) can be asserted without touching the network.
pub fn page_url(site_base: &str, mosque_id: &str) -> String {
    format!("{site_base}/{PAGE_LANG}/{mosque_id}")
}

/// Whether `slug` is a mosque page identifier in the shape mawaqit.net
/// publishes (`grande-mosquee-de-paris`): lowercase letters and digits,
/// single hyphens between segments, at most 128 bytes (review M1 — a
/// hostile all-lowercase blob would otherwise be "valid" and reach the
/// wire as a giant URL). FINDING F2: anything else must never reach the
/// network verbatim — `../` escapes the mosque namespace and `?`/`#` swap
/// the page under a legit-looking slug.
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

/// Effective (connect, request) timeouts: an explicit
/// [`MawaqitClient::with_timeouts`] wins; otherwise a proxy raises the
/// defaults (Tor circuits are slow).
fn resolve_timeouts(
    proxied: bool,
    explicit: Option<(Duration, Duration)>,
) -> (Duration, Duration) {
    match explicit {
        Some(timeouts) => timeouts,
        None if proxied => (PROXY_CONNECT_TIMEOUT, PROXY_REQUEST_TIMEOUT),
        None => (CONNECT_TIMEOUT, REQUEST_TIMEOUT),
    }
}

/// Validate a SOCKS5 proxy address and return the canonical URL for
/// `reqwest::Proxy`. Pure — the fail-fast half of
/// [`MawaqitClient::with_socks_proxy`], assertable without touching the
/// network like [`is_valid_slug`]. Rules: scheme `socks5h` exactly
/// (remote DNS — plain `socks5` or an http(s) proxy leaks resolution),
/// non-empty host, no path/query/fragment, port defaulting to
/// [`SOCKS_DEFAULT_PORT`].
fn validate_socks_proxy(addr: &str) -> Result<String> {
    let reject = |why: String| MawaqitError::InvalidProxy(why);
    let mut url = reqwest::Url::parse(addr.trim())
        .map_err(|e| reject(format!("{addr:?}: {e}")))?;
    if url.scheme() != "socks5h" {
        return Err(reject(format!(
            "{addr:?}: scheme {:?} is not socks5h — use \
             socks5h://host[:port]; plain socks5/http(s) would resolve \
             DNS outside the proxy",
            url.scheme()
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(reject(format!("{addr:?}: empty host")));
    }
    if (url.path() != "/" && !url.path().is_empty())
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(reject(format!(
            "{addr:?}: a proxy address is scheme://host[:port] only — \
             no path, query or fragment"
        )));
    }
    if url.port().is_none() && url.set_port(Some(SOCKS_DEFAULT_PORT)).is_err() {
        return Err(reject(format!("{addr:?}: cannot apply the default port")));
    }
    Ok(url.as_str().to_string())
}

/// Read a response body with a hard size cap enforced while streaming
/// (review H2), so a hostile/huge response never buffers beyond the cap
/// plus one chunk.
async fn read_capped(response: reqwest::Response) -> Result<String> {
    let mut response = response;
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if buf.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(MawaqitError::Parse(format!(
                "response exceeds the {} byte cap",
                MAX_RESPONSE_BYTES
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf)
        .map_err(|e| MawaqitError::Parse(format!("response is not UTF-8: {e}")))
}

/// Convenience: minutes between two "HH:MM" times (b-a), handling midnight
/// wrap.
pub fn minutes_between(a: &str, b: &str) -> Option<i64> {
    let a = calendar::parse_hhmm(a)?;
    let b = calendar::parse_hhmm(b)?;
    let diff = (b - a).num_minutes();
    Some(if diff < 0 { diff + 24 * 60 } else { diff })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn proxy_validation_accepts_socks5h_and_defaults_the_port() {
        let cases = [
            ("socks5h://127.0.0.1", "127.0.0.1", Some(9050)), // system tor
            ("socks5h://127.0.0.1:9050", "127.0.0.1", Some(9050)),
            ("socks5h://localhost:9150", "localhost", Some(9150)), /* Tor Browser */
            ("socks5h://[::1]:9050", "[::1]", Some(9050)),
            ("socks5h://user:pass@host:1080", "host", Some(1080)),
        ];
        for (addr, host, port) in cases {
            let validated = validate_socks_proxy(addr).expect(addr);
            let url = reqwest::Url::parse(&validated).expect("canonical URL");
            assert_eq!(url.scheme(), "socks5h", "{addr:?}");
            assert_eq!(url.host_str(), Some(host), "{addr:?}");
            assert_eq!(url.port(), port, "{addr:?}");
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
            let err = validate_socks_proxy(addr).expect_err(addr);
            assert!(
                matches!(err, MawaqitError::InvalidProxy(_)),
                "{addr:?}: {err}"
            );
        }
    }

    #[test]
    fn timeout_resolution_explicit_wins_proxy_raises_defaults() {
        // defaults without a proxy
        assert_eq!(
            resolve_timeouts(false, None),
            (Duration::from_secs(10), Duration::from_secs(30))
        );
        // a proxy raises them …
        assert_eq!(
            resolve_timeouts(true, None),
            (Duration::from_secs(30), Duration::from_secs(90))
        );
        // … unless with_timeouts overrode them (either order)
        let explicit = (Duration::from_secs(5), Duration::from_secs(15));
        assert_eq!(resolve_timeouts(true, Some(explicit)), explicit);
        assert_eq!(resolve_timeouts(false, Some(explicit)), explicit);
    }

    #[test]
    fn builders_compose_in_any_order() {
        // proxy first, offline layer after
        let _a = MawaqitClient::new()
            .with_socks_proxy("socks5h://127.0.0.1:9050")
            .expect("valid proxy")
            .with_disk_cache(std::env::temp_dir().join("mawaqit-proxy-a"));
        // offline layer and explicit timeouts first, proxy last
        let _b = MawaqitClient::with_base_urls(
            "http://127.0.0.1:1".to_string(),
            "http://127.0.0.1:1".to_string(),
        )
        .with_disk_cache(std::env::temp_dir().join("mawaqit-proxy-b"))
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

    /// Live end-to-end check against the real site (no account needed):
    /// `cargo test -p mawaqit-api -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "hits the live mawaqit.net site"]
    async fn live_search_and_calendar() {
        let client = MawaqitClient::new();
        let mosques = client.search_mosques("Paris").await.expect("search");
        println!("search hits: {}", mosques.len());
        assert!(!mosques.is_empty(), "expected at least one mosque");

        let slug = mosques
            .iter()
            .find_map(|m| m.mosque_id())
            .expect("a mosque with a slug")
            .to_string();
        println!("mosque: {} ({slug})", mosques[0].display_name());

        let today = client.today(&slug).await.expect("today");
        println!(
            "fajr={} shurouq={} isha={}",
            today.adhan.fajr, today.adhan.shurouq, today.adhan.isha
        );
        if let Some(iq) = &today.iqama {
            println!("iqama: {:?}", iq);
        }

        let month = client.month(&slug, 1).await.expect("month");
        assert!(!month.days.is_empty());
    }
}
