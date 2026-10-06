//! Hostile HTTP tests: the server on the other end is the attacker.
//!
//! `MawaqitClient` is pointed (via [`MawaqitClient::with_base_urls`]) at a
//! raw-TCP mock that answers with crafted responses: garbage JSON, lies about
//! content length, non-UTF8 bodies, redirects to attacker hosts, oversized
//! bodies. The client must always answer with a clean `Err` or well-formed
//! data — never panic, never hang, never follow the attacker somewhere else.
//!
//! Two kinds of tests live here:
//! - always-run contract tests: behavior that must stay true (green);
//! - finding regression tests: each started as an `#[ignore]`d red test
//!   documenting a red-team finding, and turned green with its fix (F1:
//!   redirects are never followed; F2: hostile slugs stay in the mosque
//!   namespace).

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};

use mawaqit_api::{MawaqitClient, MawaqitError};
use serde_json::json;

// ---------------------------------------------------------------- mock server

struct MockServer {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
}

/// Serve canned HTTP responses over raw TCP: route path (query stripped) ->
/// exact response bytes. Every request's full target is logged.
fn spawn_mock(routes: Vec<(&str, Vec<u8>)>) -> MockServer {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("mock binds an ephemeral port");
    let base = format!("http://{}", listener.local_addr().expect("local addr"));
    let routes: HashMap<String, Vec<u8>> =
        routes.into_iter().map(|(p, b)| (p.to_string(), b)).collect();
    let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let log = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let target = match read_request_target(&mut stream) {
                Some(t) => t,
                None => continue,
            };
            if let Ok(mut log) = log.lock() {
                log.push(target.clone());
            }
            let path = target.split('?').next().unwrap_or("").to_string();
            let response = routes.get(&path).cloned().unwrap_or_else(http_404);
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
    });

    MockServer { base, requests }
}

impl MockServer {
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("mock log").clone()
    }

    fn client(&self) -> MawaqitClient {
        MawaqitClient::with_base_urls(self.base.clone(), self.base.clone())
    }
}

/// Read one HTTP request head, return the request target (path + query).
fn read_request_target(stream: &mut std::net::TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 64 * 1024 {
            break;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    head.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1).map(str::to_string))
}

fn http_bytes(status_line: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut r = format!("{status_line}\r\nContent-Type: {content_type}\r\n");
    r.push_str(&format!("Content-Length: {}\r\n", body.len()));
    r.push_str("Connection: close\r\n\r\n");
    let mut bytes = r.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn ok_json(body: &str) -> Vec<u8> {
    http_bytes("HTTP/1.1 200 OK", "application/json", body.as_bytes())
}

fn ok_html(body: &str) -> Vec<u8> {
    http_bytes("HTTP/1.1 200 OK", "text/html", body.as_bytes())
}

fn status_with_body(status_line: &str, body: &str) -> Vec<u8> {
    http_bytes(status_line, "text/plain", body.as_bytes())
}

fn redirect_to(location: &str) -> Vec<u8> {
    let mut r = String::from("HTTP/1.1 302 Found\r\n");
    r.push_str(&format!("Location: {location}\r\n"));
    r.push_str("Content-Length: 0\r\nConnection: close\r\n\r\n");
    r.into_bytes()
}

fn http_404() -> Vec<u8> {
    status_with_body("HTTP/1.1 404 Not Found", "nope")
}

/// A valid mosque page whose confData is completely attacker-authored.
fn trap_page() -> String {
    let conf = r#"{
        "times": ["00:00", "00:00", "00:00", "00:00", "00:00"],
        "calendar": [{"1": ["00:00","06:00","00:00","00:00","00:00","00:00"]}],
        "name": "EVIL TRAP MOSQUE",
        "image": "https://evil.test/pwn.jpg"
    }"#;
    format!("<html><script>var confData = {conf};</script></html>")
}

/// Minimal valid search result with one clean slug.
fn search_ok() -> String {
    r#"[{"slug":"grande-mosquee-de-paris","name":"Grande Mosquée","locality":"Paris","country":"France"}]"#.to_string()
}

// ------------------------------------------------------------ contract tests

#[tokio::test]
async fn search_garbage_bodies_are_errors_never_panics() {
    let bodies = [
        "[1,2,",                        // truncated
        "{\"mosques\": []}",            // object, not array
        "null",                         // wrong kind
        "\"array?\"",                   // wrong kind
        "12345",                        // wrong kind
        "",                             // empty body
        "<html>blocked</html>",         // WAF page
        "{\"slug\": 1}, {\"slug\": 2}", // half object
    ];
    for body in bodies {
        let mock = spawn_mock(vec![("/2.0/mosque/search", ok_json(body))]);
        let result = mock.client().search_mosques("paris").await;
        assert!(
            result.is_err(),
            "body {body:?} must not deserialize into mosques"
        );
    }
}

#[tokio::test]
async fn search_error_statuses_surface_as_errors() {
    let cases = [
        ("HTTP/1.1 404 Not Found", true), // 404 is special: MosqueNotFound
        ("HTTP/1.1 403 Forbidden", false),
        ("HTTP/1.1 429 Too Many Requests", false),
        ("HTTP/1.1 500 Internal Server Error", false),
        ("HTTP/1.1 503 Service Unavailable", false),
    ];
    for (status_line, is_404) in cases {
        let mock = spawn_mock(vec![(
            "/2.0/mosque/search",
            status_with_body(status_line, "no"),
        )]);
        let err = mock.client().search_mosques("paris").await.unwrap_err();
        if is_404 {
            assert!(matches!(err, MawaqitError::MosqueNotFound(_)), "{err}");
        } else {
            assert!(matches!(err, MawaqitError::Api { .. }), "{err}");
        }
    }
}

#[tokio::test]
async fn search_bounded_input_stays_bounded() {
    // 10k entries of minimal (all-optional) mosques: large but legal — must
    // parse, since the 20 MB cap is what bounds hostility, not entry count.
    let body = format!("[{}]", vec!["{}"; 10_000].join(","));
    let mock = spawn_mock(vec![("/2.0/mosque/search", ok_json(&body))]);
    let mosques =
        mock.client().search_mosques("x").await.expect("10k entries parse");
    assert_eq!(mosques.len(), 10_000);

    // One hostile element anywhere in the array poisons the whole response —
    // that must be an Err, not a partial result.
    let poisoned = format!("[{},null]", "{}".repeat(100));
    let mock = spawn_mock(vec![("/2.0/mosque/search", ok_json(&poisoned))]);
    assert!(mock.client().search_mosques("x").await.is_err());
}

#[tokio::test]
async fn search_non_utf8_and_bom_bodies_are_rejected() {
    let mock = spawn_mock(vec![(
        "/2.0/mosque/search",
        http_bytes(
            "HTTP/1.1 200 OK",
            "application/json",
            &[0xFF, 0xFE, 0x5B, 0x5D],
        ),
    )]);
    let err = mock.client().search_mosques("x").await.unwrap_err();
    assert!(matches!(err, MawaqitError::Parse(_)), "{err}");

    let mock = spawn_mock(vec![(
        "/2.0/mosque/search",
        http_bytes("HTTP/1.1 200 OK", "application/json", b"\xEF\xBB\xBF[]"),
    )]);
    assert!(
        mock.client().search_mosques("x").await.is_err(),
        "BOM is not JSON"
    );
}

#[tokio::test]
async fn search_query_is_percent_encoded_crlf_never_reach_the_wire() {
    // Classic request-smuggling probe: CRLF in the search word must never
    // terminate the request line early.
    let evil = "paris\r\nX-Injected: 1\r\n\r\nGET /admin HTTP/1.1";
    let mock = spawn_mock(vec![("/2.0/mosque/search", ok_json(&search_ok()))]);
    let _ = mock.client().search_mosques(evil).await;

    let requests = mock.requests();
    assert_eq!(
        requests.len(),
        1,
        "one request, no smuggled second one: {requests:?}"
    );
    let line = &requests[0];
    assert_eq!(line.lines().count(), 1, "target is a single line: {line:?}");
}

#[tokio::test]
async fn conf_page_hostile_xss_content_parses_structurally() {
    // The attacker fully controls confData. The parser's job is structure,
    // not sanitation: it may accept hostile strings, and the render layer
    // (frontend tests) must neutralize them. What the parser must NEVER do
    // is panic or mis-shape the data.
    let conf = r#"{
        "times": ["05:27", "06:37", "13:21", "16:37", "19:24"],
        "shuruq": "06:37",
        "calendar": [{"1": ["05:27","06:37","13:21","16:37","19:24","20:51"]}],
        "name": "<script>alert('xss')</script>",
        "jumua": "<img src=x onerror=alert(1)>",
        "image": "javascript:alert(document.cookie)",
        "announcements": [{"title": "</textarea><script>alert(1)</script>"}]
    }"#;
    let page = format!("<html><script>var confData = {conf};</script></html>");
    let mock = spawn_mock(vec![("/en/victim", ok_html(&page))]);
    let conf = mock.client().conf_data("victim").await.expect("parses");
    assert_eq!(conf.name.as_deref(), Some("<script>alert('xss')</script>"));
}

#[tokio::test]
async fn conf_page_garbage_html_is_conf_data_not_found() {
    for body in [
        "<html>no confData here</html>",
        "",
        "confData = not json;",
        "<script>var confData = [1,2,3];</script>",
    ] {
        let mock = spawn_mock(vec![("/en/victim", ok_html(body))]);
        let err = mock.client().conf_data("victim").await.unwrap_err();
        assert!(
            matches!(err, MawaqitError::ConfDataNotFound(_)),
            "{body:?}: {err}"
        );
    }
}

#[tokio::test]
async fn conf_page_lax_content_type_is_documented_behavior() {
    // The parser does not check Content-Type — a text/plain or
    // application/octet-stream body with valid confData still parses. Not a
    // finding by itself (the payload is what matters), but pinned here so the
    // decision stays deliberate.
    let page = trap_page();
    for ctype in ["text/plain", "application/octet-stream", "weird/vendor-type"]
    {
        let mock = spawn_mock(vec![(
            "/en/victim",
            http_bytes("HTTP/1.1 200 OK", ctype, page.as_bytes()),
        )]);
        assert!(
            mock.client().conf_data("victim").await.is_ok(),
            "{ctype} with valid confData parses"
        );
    }
}

#[tokio::test]
async fn truncated_body_and_connection_reset_are_errors() {
    // Headers promise 10 000 bytes, 50 arrive, socket dies.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 10000\r\n\r\n";
        let _ = stream.write_all(head.as_bytes());
        let partial: &[u8] = b"<html><script>var confData";
        let _ = stream.write_all(&partial[..partial.len().min(27)]);
        let _ = stream.flush();
        drop(stream);
    });
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone());
    assert!(
        client.conf_data("victim").await.is_err(),
        "truncated body is an Err"
    );
}

#[tokio::test]
async fn oversized_body_is_rejected_by_the_cap() {
    let over_cap = "A".repeat(1024 * 1024 + 1);
    let mock = spawn_mock(vec![(
        "/en/victim",
        http_bytes("HTTP/1.1 200 OK", "text/html", over_cap.as_bytes()),
    )]);
    let err = mock.client().conf_data("victim").await.unwrap_err();
    assert!(err.to_string().contains("cap"), "{err}");
}

/// Review H1: `invalidate(None)` must drop the search cache too, not just
/// the page cache — the doc always claimed "everything".
#[tokio::test]
async fn invalidate_none_also_drops_the_search_cache() {
    let mock = spawn_mock(vec![("/2.0/mosque/search", ok_json(&search_ok()))]);
    let client = mock.client();
    let _ = client.search_mosques("paris").await.unwrap();
    let _ = client.search_mosques("paris").await.unwrap();
    assert_eq!(mock.requests().len(), 1, "second search is cached");

    client.invalidate(None);
    let _ = client.search_mosques("paris").await.unwrap();
    assert_eq!(
        mock.requests().len(),
        2,
        "invalidate(None) must clear the search cache"
    );
}

#[tokio::test]
async fn redirect_loop_stays_bounded_by_the_http_client() {
    let mock = spawn_mock(vec![("/en/loop", redirect_to("/en/loop"))]);
    assert!(
        mock.client().conf_data("loop").await.is_err(),
        "redirect loop must end"
    );
}

#[tokio::test]
async fn cache_never_confuses_two_slugs() {
    let mock = spawn_mock(vec![
        ("/en/mosque-a", ok_html(&trap_page())),
        (
            "/en/mosque-b",
            ok_html(
                "<html><script>var confData = {\"times\":[\"06:00\",\"07:00\",\"13:00\",\"16:00\",\"19:00\",\"21:00\"],\"calendar\":[{\"1\":[\"06:00\",\"07:00\",\"13:00\",\"16:00\",\"19:00\",\"21:00\"]}]};</script></html>",
            ),
        ),
    ]);
    let client = mock.client();
    let a = client.conf_data("mosque-a").await.unwrap();
    let b = client.conf_data("mosque-b").await.unwrap();
    assert_ne!(a.name, b.name, "two slugs never share a cache entry");
    assert_eq!(mock.requests().len(), 2, "each slug fetched exactly once");
    // Same slug again is served from cache: still exactly 2 requests.
    let _ = client.conf_data("mosque-a").await.unwrap();
    assert_eq!(mock.requests().len(), 2);
}

// ------------------------------------------------ findings (regression-pinned)

/// FINDING F1 — the client followed redirects anywhere, including
/// cross-origin. A single open redirect (or a compromise) on mawaqit.net
/// turned `conf_data` into "parse whatever evil.test serves" — attacker-
/// authored prayer times, mosque name and image URL on the user's screen.
/// FIXED: the reqwest client pins `redirect::Policy::none()`, so a 302 —
/// same-origin or not — surfaces as `Api { status: 302 }` instead.
#[tokio::test]
async fn finding_f1_cross_origin_redirect_is_not_followed() {
    let mock = spawn_mock(vec![
        ("/en/victim", redirect_to("http://attacker.invalid/en/trap")),
        ("/en/trap", ok_html(&trap_page())),
    ]);
    // Not following means the 302 itself surfaces as an Api error.
    let err = mock.client().conf_data("victim").await.unwrap_err();
    assert!(
        matches!(err, MawaqitError::Api { status: 302, .. }),
        "client followed the redirect instead of surfacing the 302: {err}"
    );
}

/// FINDING F2 — slugs are never validated between the search response and
/// the URL: `update_config` persists anything, and `conf_data` interpolates
/// it verbatim into `…/{lang}/{slug}`. A hostile/compromised search response
/// (or a tampered config file) can point the fetch at arbitrary mawaqit.net
/// paths: `../` escapes the mosque namespace, `?`/`#` swap the page under a
/// legit-looking slug.
/// FIX: validate the slug once (e.g. `^[a-z0-9]+(-[a-z0-9]+)*$`) at the
/// `Mosque::mosque_id` boundary and in `update_config`, then un-ignore.
#[tokio::test]
async fn finding_f2_hostile_slug_never_leaves_the_mosque_namespace() {
    let hostile_slugs = [
        "../trap",      // dot-segment traversal
        "..%2Ftrap",    // encoded traversal
        "%2e%2e/trap",  // encoded dot-segment
        "victim?x=1",   // query injection swaps nothing visible…
        "victim#frag",  // fragment injection
        "victim/extra", // path extension
        "victim%00",    // NUL byte
    ];
    let mock = spawn_mock(vec![
        (
            "/en/victim",
            ok_html(
                "<html><script>var confData = {\"times\":[\"06:00\",\"07:00\",\"13:00\",\"16:00\",\"19:00\",\"21:00\"],\"calendar\":[{\"1\":[\"06:00\",\"07:00\",\"13:00\",\"16:00\",\"19:00\",\"21:00\"]}]};</script></html>",
            ),
        ),
        ("/trap", ok_html(&trap_page())),
    ]);
    for slug in hostile_slugs {
        let result = mock.client().conf_data(slug).await;
        let path = mock.requests().last().cloned().unwrap_or_default();
        // Secure contract: the request stays a single page under /en/, and a
        // hostile slug never yields attacker content.
        let decoded =
            path.replace("%2F", "/").replace("%2e", ".").replace("%2E", ".");
        assert!(
            decoded.starts_with("/en/") && !decoded.contains(".."),
            "slug {slug:?} escaped the mosque namespace, request path {path:?}"
        );
        assert!(
            result.is_err(),
            "slug {slug:?} must be rejected, got parsed data"
        );
    }
}

/// FINDING F3 — the cap used to be applied *after* the whole body was
/// buffered (`response.bytes()`): a hostile server streaming gigabytes
/// OOMs the app before the cap trips. FIXED: `read_capped` streams the
/// body chunk-wise and aborts once the running total exceeds the cap, so
/// memory is bounded by cap + one chunk.
#[tokio::test]
async fn finding_f3_documented_cap_rejects_oversized_response() {
    let over_cap = "A".repeat(1024 * 1024 + 1);
    let mock = spawn_mock(vec![(
        "/en/victim",
        http_bytes("HTTP/1.1 200 OK", "text/html", over_cap.as_bytes()),
    )]);
    assert!(mock.client().conf_data("victim").await.is_err());
}

/// A valid page whose calendar covers all 12 months, days 1..=31 — so the
/// `today()`/`month_iqama()` transport wrappers resolve for any real-world
/// local date.
fn full_year_page() -> String {
    let mut months = Vec::new();
    let mut iqama_months = Vec::new();
    for _month in 1..=12 {
        let mut days = serde_json::Map::new();
        let mut iqama_days = serde_json::Map::new();
        for day in 1..=31 {
            days.insert(
                day.to_string(),
                serde_json::json!([
                    "05:00", "06:30", "12:00", "15:30", "18:00", "19:30",
                ]),
            );
            iqama_days.insert(
                day.to_string(),
                serde_json::json!([
                    "05:10", "12:10", "15:40", "18:10", "19:40",
                ]),
            );
        }
        months.push(serde_json::Value::Object(days));
        iqama_months.push(serde_json::Value::Object(iqama_days));
    }
    let conf = serde_json::json!({
        "times": ["05:00", "06:30", "12:00", "15:30", "18:00", "19:30"],
        "calendar": months,
        "iqamaCalendar": iqama_months,
        "name": "Full Year Mosque"
    });
    format!("<html><script>var confData = {conf};</script></html>")
}

#[tokio::test]
async fn search_empty_word_short_circuits_without_a_request() {
    let mock = spawn_mock(vec![("/2.0/mosque/search", ok_json(&search_ok()))]);
    let mosques =
        mock.client().search_mosques("   ").await.expect("empty word");
    assert!(mosques.is_empty(), "an empty word is no search at all");
    assert!(
        mock.requests().is_empty(),
        "an empty word must not touch the network"
    );
}

#[tokio::test]
async fn today_and_month_iqama_resolve_through_the_public_methods() {
    let mock = spawn_mock(vec![("/en/fullyear", ok_html(&full_year_page()))]);
    let client = mock.client();

    let today = client.today("fullyear").await.expect("today");
    assert_eq!(today.adhan.fajr.len(), 5, "strict HH:MM surfaced");
    assert!(today.iqama.is_some(), "iqama resolves for today");

    let month = client.month_iqama("fullyear", 1).await.expect("iqama month");
    assert_eq!(month.days.len(), 31);
    assert_eq!(month.days[0].times.dhuhr, "12:10");
}

#[tokio::test]
async fn invalidate_drops_one_slug_and_keeps_the_others() {
    let mock = spawn_mock(vec![
        ("/en/mosque-a", ok_html(&trap_page())),
        ("/en/mosque-b", ok_html(&trap_page())),
    ]);
    let client = mock.client();
    let _ = client.conf_data("mosque-a").await.unwrap();
    let _ = client.conf_data("mosque-b").await.unwrap();
    assert_eq!(mock.requests().len(), 2, "each slug fetched once");

    client.invalidate(Some("mosque-a"));
    let _ = client.conf_data("mosque-a").await.unwrap();
    let _ = client.conf_data("mosque-b").await.unwrap();
    assert_eq!(
        mock.requests().len(),
        3,
        "mosque-a refetched after invalidation, mosque-b still cached"
    );
}

// ------------------------------------------------ round-2 findings
// (F22/F29/F30)

/// FINDING F22 — the search response is free text from the same hostile
/// wire as the mosque page, but `Vec<Mosque>` was deserialized verbatim:
/// hostile labels reached the tray/UI through `display_name()`. The shared
/// sanitizer now applies at this ingress — modeled fields, the string `id`,
/// and every string inside the flattened `extra` map.
#[tokio::test]
async fn finding_f22_search_results_are_sanitized_at_the_ingress() {
    let body = serde_json::to_string(&json!([{
        "slug": "clean-slug",
        "name": "\u{202E}EVIL\u{200B}",
        "label": "A\u{200B}B",
        "locality": "\u{202D}C",
        "country": "D\u{FEFF}E",
        "id": "i\u{2060}d",
        "extra_note": "\u{200B}x",
        // Nested structures in the unmodeled extras go through the same
        // recursion: arrays, objects, and strings inside them.
        "tags": ["\u{200B}t1", {"deep": "\u{202E}d", "n": 7}],
    }]))
    .unwrap();
    let server = spawn_mock(vec![("/2.0/mosque/search", ok_json(&body))]);
    let client = server.client();

    let mosques = client.search_mosques("paris").await.expect("search");
    assert_eq!(mosques.len(), 1);
    let m = &mosques[0];
    assert_eq!(m.name.as_deref(), Some("EVIL"));
    assert_eq!(m.label.as_deref(), Some("AB"));
    assert_eq!(m.locality.as_deref(), Some("C"));
    assert_eq!(m.country.as_deref(), Some("DE"));
    assert_eq!(m.id.as_ref().and_then(|v| v.as_str()), Some("id"));
    assert_eq!(
        m.slug.as_deref(),
        Some("clean-slug"),
        "clean data is untouched"
    );
    assert_eq!(
        m.extra.get("extra_note").and_then(|v| v.as_str()),
        Some("x"),
        "unmodeled extra strings are sanitized too"
    );
    let tags = m.extra.get("tags").expect("tags survive in extra");
    assert_eq!(tags[0].as_str(), Some("t1"), "array items are sanitized");
    assert_eq!(
        tags[1].get("deep").and_then(|v| v.as_str()),
        Some("d"),
        "nested object strings are sanitized"
    );
    assert_eq!(
        tags[1].get("n").and_then(|v| v.as_i64()),
        Some(7),
        "numbers pass through"
    );
    // And what the caller actually displays is clean end to end.
    assert!(!m.display_name().chars().any(char::is_control));
}

/// FINDING F29b — an unbounded search word became a giant cache key and a
/// giant wire URL. Words over 128 bytes (the slug bound) are refused at
/// the client, before any request leaves the process.
#[tokio::test]
async fn finding_f29b_oversized_search_word_is_refused_before_the_wire() {
    let server = spawn_mock(vec![("/2.0/mosque/search", ok_json("[]"))]);
    let client = server.client();

    let err = client.search_mosques(&"a".repeat(129)).await.unwrap_err();
    assert!(matches!(err, MawaqitError::SearchWordTooLong), "got {err:?}");
    assert!(
        server.requests().is_empty(),
        "an oversized word must never reach the wire"
    );

    // Exactly at the bound is legal.
    let ok = client
        .search_mosques(&"a".repeat(128))
        .await
        .expect("128 bytes is legal");
    assert!(ok.is_empty());
}

/// FINDING F30 — the cache key was the lowercased word while the wire
/// query carried the original: case-confusable words ("Paris" vs "paris",
/// Turkish İ forms) collided on one key, so a second query could be served
/// the first's cached results even though the server was asked a different
/// question. FIXED: the key is the exact request string — an identical
/// repeat stays cached, a distinct request string is a fresh wire query.
#[tokio::test]
async fn finding_f30_cache_key_is_the_exact_request_string() {
    let server =
        spawn_mock(vec![("/2.0/mosque/search", ok_json(&search_ok()))]);
    let client = server.client();

    client.search_mosques("Paris").await.expect("first query");
    client.search_mosques("Paris").await.expect("identical repeat");
    assert_eq!(server.requests().len(), 1, "an identical repeat stays cached");

    client.search_mosques("paris").await.expect("case variant");
    assert_eq!(
        server.requests().len(),
        2,
        "a distinct request string must not be served another word's cache"
    );
}
