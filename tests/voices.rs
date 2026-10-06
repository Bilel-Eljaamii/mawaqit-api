//! Voice-catalog integration tests: downloading a catalog voice writes it
//! atomically into the destination, skips when already cached, enforces the
//! size cap, and rejects unknown ids before any network use. The mock server
//! is the same raw-TCP seam the hostile HTTP suite uses.

use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    thread,
};

use mawaqit_api::{
    MawaqitClient, MawaqitError, adhan_voice_url, download_voice,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mawaqit-voices-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Serve one canned response for every GET. Byte-exact: voice bodies are
/// binary and must survive the mock untouched.
fn spawn_mock(response: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mock binds");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let head = String::from_utf8_lossy(&buf);
            if !head.starts_with("GET ") {
                continue;
            }
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
    });
    (base, handle)
}

fn mp3_response(body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

#[tokio::test]
async fn download_writes_atomically_and_skips_when_cached() {
    let dir = temp_dir("atomic");
    let body = b"\xff\xfb\x90\x00fake-mp3-frame-data";
    let (base, server) = spawn_mock(mp3_response(body));
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let path =
        download_voice(&client, "adhan-quds", &dir).await.expect("download");
    assert_eq!(path, dir.join("adhan-quds.mp3"));
    assert_eq!(std::fs::read(&path).unwrap(), body);
    // Atomic write left no tmp behind.
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries.len(), 1);

    // Cached: a second call must not touch the (now stopped) network.
    drop(server);
    let path2 =
        download_voice(&client, "adhan-quds", &dir).await.expect("cached");
    assert_eq!(path, path2);
}

#[tokio::test]
async fn download_enforces_the_size_cap() {
    let dir = temp_dir("cap");
    let big = vec![0xFFu8; 9 * 1024 * 1024];
    let (base, _server) = spawn_mock(mp3_response(&big));
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-madina", &dir).await.unwrap_err();
    assert!(matches!(err, MawaqitError::InvalidVoice(_)), "got {err:?}");
}

#[tokio::test]
async fn unknown_id_is_rejected_before_any_network() {
    let dir = temp_dir("unknown");
    // Dead base URL: if the id were not validated first, this would error
    // with Http, not InvalidVoice.
    let client = MawaqitClient::with_base_urls(
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    );
    let err =
        download_voice(&client, "../../etc/passwd", &dir).await.unwrap_err();
    assert!(matches!(err, MawaqitError::InvalidVoice(_)), "got {err:?}");
    assert!(adhan_voice_url("../../etc/passwd").is_none());
}

#[tokio::test]
async fn failing_download_leaves_no_partial_file() {
    let dir = temp_dir("fail");
    let (base, _server) = spawn_mock(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    );
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    assert!(download_voice(&client, "adhan-egypt", &dir).await.is_err());
    assert!(
        std::fs::read_dir(&dir).unwrap().count() == 0,
        "a failed download must not leave files behind"
    );
}

/// The pre-download guard: a 200 whose Content-Length claims more than the
/// cap is rejected before any body is buffered (the real body here is
/// a few dozen bytes).
#[tokio::test]
async fn download_rejects_oversize_content_length_before_downloading() {
    let dir = temp_dir("precheck");
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n".to_vec();
    response.extend_from_slice(b"Content-Length: 9437184\r\n"); // 9 MiB
    response.extend_from_slice(b"Connection: close\r\n\r\n");
    response.extend_from_slice(b"tiny");
    let (base, _server) = spawn_mock(response);
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(
        matches!(&err, MawaqitError::InvalidVoice(m) if m.contains("cap")),
        "got {err:?}"
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "nothing may be written for a rejected download"
    );
}

/// No Content-Length header: the pre-download guard cannot fire, so the
/// cap is enforced on the buffered body instead — 9 MiB delivered, 8 MiB
/// cap.
#[tokio::test]
async fn download_enforces_the_cap_without_content_length() {
    let dir = temp_dir("no-length");
    // HTTP/1.1 without Content-Length + Connection: close: the body runs
    // to EOF, so reqwest reads it all before the cap check.
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n".to_vec();
    response.extend_from_slice(b"Connection: close\r\n\r\n");
    response.extend_from_slice(&vec![0xFFu8; 9 * 1024 * 1024]);
    let (base, _server) = spawn_mock(response);
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(
        matches!(&err, MawaqitError::InvalidVoice(m) if m.contains("cap")),
        "got {err:?}"
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

/// An empty body (no Content-Length) is rejected as an empty file, not
/// written as a 0-byte mp3.
#[tokio::test]
async fn download_rejects_an_empty_body() {
    let dir = temp_dir("empty-body");
    let (base, _server) = spawn_mock(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    );
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(
        matches!(&err, MawaqitError::InvalidVoice(m) if m.contains("empty")),
        "got {err:?}"
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

/// create_dir_all fails: `dest_dir` sits under a regular file. The error is
/// surfaced as Parse and nothing is written.
#[tokio::test]
async fn download_fails_when_the_destination_cannot_be_created() {
    let root = temp_dir("mkdir-fail");
    let blocker = root.join("not-a-dir");
    std::fs::write(&blocker, b"x").unwrap();
    let dir = blocker.join("sub"); // under a file ⇒ create_dir_all fails

    let (base, _server) = spawn_mock(mp3_response(b"\xff\xfbshort"));
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(matches!(err, MawaqitError::Parse(_)), "got {err:?}");
}

/// The atomic-write tmp name is unique per call (the concurrent-download
/// fix), so a stale writer's leftover at any `*.mp3.tmp-*` path — here a
/// directory — can neither block nor poison a fresh download and stays
/// untouched. The suffix below is out of `subsec_nanos()` range so the
/// fresh download's own tmp name can never collide with it.
#[tokio::test]
async fn download_ignores_stale_tmp_artifacts() {
    let dir = temp_dir("tmp-dir");
    std::fs::create_dir_all(dir.join("adhan-quds.mp3.tmp-99999999999"))
        .unwrap();

    let (base, _server) = spawn_mock(mp3_response(b"\xff\xfbshort"));
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let dest = download_voice(&client, "adhan-quds", &dir).await.unwrap();
    assert!(dest.is_file(), "the download lands despite the stale artifact");
    assert!(
        dir.join("adhan-quds.mp3.tmp-99999999999").is_dir(),
        "the stale artifact stays untouched"
    );
}

/// The atomic write fails when the destination directory refuses the write:
/// the error surfaces as Parse and no partial file is left. (The tmp path
/// is per-writer unique since FINDING F24, so blocking one fixed name is
/// no longer possible — a read-only directory fails the same write,
/// deterministically.)
#[cfg(unix)]
#[tokio::test]
async fn download_fails_when_the_tmp_write_is_refused() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("tmp-readonly");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))
        .unwrap();

    let (base, _server) = spawn_mock(mp3_response(b"\xff\xfbshort"));
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(matches!(err, MawaqitError::Parse(_)), "got {err:?}");
    // Restore so the temp dir can be cleaned up by the OS.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
        .unwrap();
    assert!(
        std::fs::read_dir(&dir).unwrap().count() == 0,
        "no partial download file lands in the read-only directory"
    );
}

/// A directory occupies the final destination path: it is not treated as a
/// cached voice (is_file), the download runs, and the atomic rename fails
/// with a surfaced Parse error instead of replacing the directory.
#[tokio::test]
async fn download_fails_when_the_destination_is_a_directory() {
    let dir = temp_dir("dest-dir");
    std::fs::create_dir_all(dir.join("adhan-quds.mp3")).unwrap();

    let (base, _server) = spawn_mock(mp3_response(b"\xff\xfbshort"));
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(matches!(err, MawaqitError::Parse(_)), "got {err:?}");
    assert!(dir.join("adhan-quds.mp3").is_dir(), "the directory survives");
}

/// Connection refused mid-download: `send()` fails and the error surfaces
/// as Http (no file written, no panic).
#[tokio::test]
async fn download_surfaces_a_transport_failure_as_http() {
    let dir = temp_dir("refused");
    // Port 1: connection refused instantly.
    let client = MawaqitClient::with_base_urls(
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .with_cdn_base("http://127.0.0.1:1".into());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(matches!(err, MawaqitError::Http(_)), "got {err:?}");
    assert!(std::fs::read_dir(&dir).unwrap().count() == 0);
}

/// The server lies about Content-Length and closes early: `bytes()` fails
/// on the truncated body and surfaces as Http.
#[tokio::test]
async fn download_surfaces_a_truncated_body_as_http() {
    let dir = temp_dir("truncated");
    // Headers promise 512 bytes; the server sends 8 and closes.
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n".to_vec();
    response.extend_from_slice(b"Content-Length: 512\r\n");
    response.extend_from_slice(b"Connection: close\r\n\r\n");
    response.extend_from_slice(b"\xff\xfb\x90\x00fake");
    let (base, _server) = spawn_mock(response);
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let err = download_voice(&client, "adhan-quds", &dir).await.unwrap_err();
    assert!(matches!(err, MawaqitError::Http(_)), "got {err:?}");
    assert!(std::fs::read_dir(&dir).unwrap().count() == 0);
}

// ------------------------------------------------ round-2 findings (F24)

/// FINDING F24a — a cached file was trusted forever on `len() > 0`: the
/// destination directory is attacker-writable, so a planted oversized
/// "voice" would be served forever without validation. Cached-size
/// validation: a file over the cap is not a cached voice — it is replaced
/// by a fresh download.
#[tokio::test]
async fn finding_f24_oversized_cached_voice_is_replaced() {
    let dir = temp_dir("f24-cache");
    std::fs::write(dir.join("adhan-quds.mp3"), vec![0xFFu8; 9 * 1024 * 1024])
        .unwrap();
    let body = b"\xff\xfb\x90\x00legit-fresh-download";
    let (base, _server) = spawn_mock(mp3_response(body));
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let path =
        download_voice(&client, "adhan-quds", &dir).await.expect("redownload");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        body,
        "an oversized cached file must be replaced, never served"
    );
}

/// FINDING F24b — the body was fully buffered (`bytes().await`) before the
/// 8 MB post-check, and the Content-Length pre-check only fires when the
/// header exists. The cap must hold mid-stream: a hostile CDN serving an
/// endless body is cut off at the cap instead of buffered (with the request
/// timeout as the only backstop).
#[tokio::test]
async fn finding_f24_oversized_stream_is_cut_off_midstream() {
    let dir = temp_dir("f24-stream");
    // Endless chunked body, no Content-Length, connection never closes.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let _server = thread::spawn(move || {
        if let Some(Ok(mut stream)) = listener.incoming().next() {
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nConnection: close\r\n\r\n",
            );
            let chunk = vec![0xFFu8; 64 * 1024];
            loop {
                if stream.write_all(&chunk).is_err() {
                    break; // the client aborted at the cap — that's the point
                }
            }
        }
    });
    let client = MawaqitClient::with_base_urls(base.clone(), base.clone())
        .with_cdn_base(base.clone());

    let start = std::time::Instant::now();
    let result = download_voice(&client, "adhan-egypt", &dir).await;
    assert!(result.is_err(), "an endless body must be rejected");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "the mid-stream cap must abort promptly, took {:?}",
        start.elapsed()
    );
}

/// FINDING F24c — the temp file name was fixed (`X.mp3.tmp`): two
/// concurrent downloads of one voice shared it and one could rename a torn
/// file into place. Temp names are per-writer unique.
#[test]
fn f24_tmp_names_are_unique_per_writer() {
    let dest = std::env::temp_dir().join("f24-voice.mp3");
    let a = mawaqit_api::voices::unique_tmp_path(&dest);
    let b = mawaqit_api::voices::unique_tmp_path(&dest);
    assert_ne!(a, b, "each writer gets its own temp file");
    assert!(a.to_string_lossy().ends_with(".tmp"));
}

/// FINDING P6 (issue #4): the cache-path convention is catalog-validated —
/// the download writes `dir/{id}.mp3` and `cached_path` reads exactly that
/// path for catalog ids; hostile ids never path-join out of the directory.
#[test]
fn p6_cached_path_is_catalog_validated() {
    let dir = temp_dir("p6-cache");
    let path = mawaqit_api::voices::cached_path(&dir, "adhan-quds")
        .expect("catalog id");
    assert_eq!(path, dir.join("adhan-quds.mp3"));
    assert!(path.starts_with(&dir), "stays inside the directory");
    for hostile in
        ["../../etc/passwd", "", "unknown-voice", "adhan-quds/../../x"]
    {
        assert_eq!(
            mawaqit_api::voices::cached_path(&dir, hostile),
            None,
            "{hostile:?} is not a catalog id"
        );
    }
}
