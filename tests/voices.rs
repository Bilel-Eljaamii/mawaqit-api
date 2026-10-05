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
