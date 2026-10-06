use std::{path::PathBuf, sync::Arc, time::Duration};

use chrono::{Local, NaiveDate};

use crate::{
    cache::TtlCache,
    calendar, disk,
    error::{self, BOUNDED_DIAGNOSTIC, BOUNDED_ID, MawaqitError, Result},
    models::{ConfData, MonthIqamaTimes, MonthTimes, Mosque, TodayTimes},
    sanitize,
};

const API_URL_BASE: &str = "https://mawaqit.net/api";
const SITE_URL_BASE: &str = "https://mawaqit.net";
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
/// The search word becomes a cache key and a wire URL; the same bound as a
/// mosque slug (review M1) applies (FINDING F29). Longer words are refused
/// before any request.
const MAX_SEARCH_WORD_BYTES: usize = 128;

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
    /// Adhan-voice CDN root (`voices.rs`). Overridable for tests.
    cdn_base: String,
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
    /// A client against the real mawaqit.net endpoints — no API key.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo() -> Result<(), mawaqit_api::MawaqitError> {
    /// let client = mawaqit_api::MawaqitClient::new();
    /// let mosques = client.search_mosques("Paris").await?;
    /// println!("{}", mosques[0].display_name());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Self {
        Self::with_base_urls(
            API_URL_BASE.to_string(),
            SITE_URL_BASE.to_string(),
        )
    }

    /// Same client against custom base URLs — the seam the hostile HTTP
    /// tests use to point the client at a local mock server.
    pub fn with_base_urls(api_base: String, site_base: String) -> Self {
        Self::from_parts(
            api_base,
            site_base,
            crate::voices::CDN_URL_BASE.to_string(),
            None,
            None,
            None,
        )
        .expect("default construction cannot fail: no proxy to configure")
    }

    /// Single construction path: every builder funnels here so base URLs,
    /// the disk cache, the SOCKS proxy and the timeouts compose in any
    /// order. `explicit_timeouts` is `None` for the defaults (raised
    /// automatically when a proxy is set).
    fn from_parts(
        api_base: String,
        site_base: String,
        cdn_base: String,
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
                cdn_base,
                pages: TtlCache::new(CONF_TTL, MAX_CACHED_PAGES),
                searches: TtlCache::new(SEARCH_TTL, MAX_CACHED_SEARCHES),
                proxy,
                explicit_timeouts,
            }),
            disk,
        })
    }

    /// Raw GET through this client's HTTP stack (proxy, timeouts, UA) —
    /// the seam the voice downloader uses.
    pub(crate) fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.inner.http.get(url)
    }

    /// Override the adhan-voice CDN root (default `https://cdn.mawaqit.net`)
    /// — the seam the voice tests use. Composes with every other builder.
    pub fn with_cdn_base(self, url: String) -> Self {
        Self::from_parts(
            self.inner.api_base.clone(),
            self.inner.site_base.clone(),
            url,
            self.disk.clone(),
            self.inner.proxy.clone(),
            self.inner.explicit_timeouts,
        )
        .expect("rebuilding with an already-validated proxy cannot fail")
    }

    /// The CDN URL for a catalog voice, honoring this client's CDN base.
    /// Unknown ids are rejected (see [`crate::voices::adhan_voice_url`]).
    pub fn voice_url(&self, id: &str) -> Option<String> {
        crate::voices::adhan_voice_url(id).map(|u| {
            u.replace(crate::voices::CDN_URL_BASE, &self.inner.cdn_base)
        })
    }

    /// Serve [`Self::conf_data`] from a disk snapshot when the network is
    /// unavailable, and refresh the snapshot on every successful fetch —
    /// the offline layer. See [`crate::disk`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::PathBuf;
    ///
    /// let client = mawaqit_api::MawaqitClient::new()
    ///     .with_disk_cache(PathBuf::from("/var/lib/myapp/snapshots"));
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let client = mawaqit_api::MawaqitClient::new()
    ///     .with_socks_proxy("socks5h://127.0.0.1:9050")
    ///     .expect("tor daemon address");
    /// ```
    pub fn with_socks_proxy(self, addr: impl Into<String>) -> Result<Self> {
        let proxy = validate_socks_proxy(&addr.into())?;
        Self::from_parts(
            self.inner.api_base.clone(),
            self.inner.site_base.clone(),
            self.inner.cdn_base.clone(),
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
            self.inner.cdn_base.clone(),
            self.disk.clone(),
            self.inner.proxy.clone(),
            Some((connect, request)),
        )
        .expect("rebuilding with an already-validated proxy cannot fail")
    }

    /// `GET /api/2.0/mosque/search?word=...` — keyword search, no auth.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo() -> Result<(), mawaqit_api::MawaqitError> {
    /// let client = mawaqit_api::MawaqitClient::new();
    /// for mosque in client.search_mosques("Paris").await? {
    ///     println!("{} ({})", mosque.display_name(),
    ///         mosque.place().unwrap_or_default());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn search_mosques(&self, word: &str) -> Result<Vec<Mosque>> {
        let word = word.trim();
        if word.is_empty() {
            return Ok(Vec::new());
        }
        // FINDING F29: the word would become a giant cache key and a giant
        // wire URL; the slug bound applies equally.
        if word.len() > MAX_SEARCH_WORD_BYTES {
            return Err(MawaqitError::SearchWordTooLong);
        }
        // FINDING F30: the key is the exact request string. Lowercasing it
        // collided case-confusable words ("Paris" vs "paris", Turkish İ
        // forms), so a second query could be served the first's cached
        // results even though the wire asked a different question.
        let cache_key = word.to_string();
        if let Some(cached) = self.inner.searches.get(&cache_key) {
            return Ok(cached);
        }

        let url = format!("{}/2.0/mosque/search", self.inner.api_base);
        let response =
            self.inner.http.get(&url).query(&[("word", word)]).send().await?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(MawaqitError::MosqueNotFound(error::bounded(
                word, BOUNDED_ID,
            )));
        }
        let body = read_capped(response).await?;
        if !status.is_success() {
            return Err(MawaqitError::Api { status: status.as_u16(), url });
        }

        let mut mosques: Vec<Mosque> =
            serde_json::from_str(&body).map_err(|e| {
                MawaqitError::Parse(error::bounded(
                    &format!("search: {e}"),
                    BOUNDED_DIAGNOSTIC,
                ))
            })?;
        // FINDING F22: the search response is free text from the same
        // hostile wire as the page — the shared sanitizer applies at this
        // ingress exactly like the page parser's.
        for mosque in &mut mosques {
            sanitize::mosque(mosque);
        }
        self.inner.searches.insert(cache_key, mosques.clone());
        Ok(mosques)
    }

    /// Fetch (or take from cache) the confData of a mosque page. `mosque_id`
    /// is the page slug, e.g. `grande-mosquee-de-paris`. When the network
    /// fails and a disk snapshot exists ([`Self::with_disk_cache`]), the
    /// snapshot is served instead.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo() -> Result<(), mawaqit_api::MawaqitError> {
    /// let client = mawaqit_api::MawaqitClient::new();
    /// let conf = client.conf_data("grande-mosquee-de-paris").await?;
    /// println!("{}", conf.name.as_deref().unwrap_or("?"));
    /// # Ok(())
    /// # }
    /// ```
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
        let slug = if crate::slug::is_valid_slug(mosque_id) {
            mosque_id.to_string()
        } else {
            "-".repeat(mosque_id.len().clamp(4, 64))
        };
        let url = crate::slug::page_url(&inner.site_base, &slug);
        let response = inner.http.get(&url).send().await?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(MawaqitError::MosqueNotFound(error::bounded(
                mosque_id, BOUNDED_ID,
            )));
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
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn demo() -> Result<(), mawaqit_api::MawaqitError> {
    /// let client = mawaqit_api::MawaqitClient::new();
    /// let today = client.today("grande-mosquee-de-paris").await?;
    /// println!("Fajr {} — iqama {}", today.adhan.fajr,
    ///     today.iqama.as_ref().map(|i| i.fajr.as_str()).unwrap_or("?"));
    /// # Ok(())
    /// # }
    /// ```
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
/// network like [`crate::slug::is_valid_slug`]. Rules: scheme `socks5h`
/// exactly
/// (remote DNS — plain `socks5` or an http(s) proxy leaks resolution),
/// non-empty host, no path/query/fragment, port defaulting to
/// [`SOCKS_DEFAULT_PORT`].
fn validate_socks_proxy(addr: &str) -> Result<String> {
    // FINDING F29: the payload echoes caller input — bounded like any
    // other error payload.
    let reject = |why: String| {
        MawaqitError::InvalidProxy(error::bounded(&why, BOUNDED_DIAGNOSTIC))
    };
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
    if url.port().is_none() {
        // set_port only errors on cannot-be-a-base URLs, which the host
        // check above has already excluded.
        url.set_port(Some(SOCKS_DEFAULT_PORT))
            .expect("socks5h host checked; set_port cannot fail");
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
    String::from_utf8(buf).map_err(|e| {
        MawaqitError::Parse(error::bounded(
            &format!("response is not UTF-8: {e}"),
            BOUNDED_DIAGNOSTIC,
        ))
    })
}
