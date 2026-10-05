//! The adhan voice catalog: the muadhin recordings Mawaqit's mosque screens
//! play, served keylessly from the public CDN at
//! `https://cdn.mawaqit.net/audio/{id}.mp3`. The list is static and
//! enumerable (the same set the official apps stream), so it is a constant
//! here — no endpoint, no API key, no discovery at runtime.
//!
//! Al Afasy is omitted by product decision (2026-10-05).

#[cfg(any(feature = "std", feature = "alloc"))]
use alloc::{format, string::String};
#[cfg(feature = "std")]
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(any(feature = "std", feature = "alloc"))]
use crate::models::ConfData;
#[cfg(feature = "std")]
use crate::{
    MawaqitClient,
    error::{self, BOUNDED_DIAGNOSTIC, BOUNDED_ID, MawaqitError, Result},
};

/// The public CDN root the mosque-screen voices are served from.
pub const CDN_URL_BASE: &str = "https://cdn.mawaqit.net/audio";

/// Hard cap for one downloaded voice file (real files are ~2–5 MB).
#[cfg(feature = "std")]
const MAX_VOICE_BYTES: usize = 8 * 1024 * 1024;

/// The temp file for an atomic write of `dest` — per-writer unique (pid +
/// sequence, FINDING F24): two concurrent downloads of one voice must
/// never share a temp path. `#[doc(hidden)]` test instrumentation, not
/// semver surface.
#[cfg(feature = "std")]
#[doc(hidden)]
pub fn unique_tmp_path(dest: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut s = dest.as_os_str().to_os_string();
    s.push(format!(".{}-{n}.tmp", std::process::id()));
    PathBuf::from(s)
}

/// One selectable adhan recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct AdhanVoice {
    /// Stable identifier, also the CDN file stem (`{id}.mp3`) and the value
    /// stored in the mosque page's `adhanVoice` field.
    pub id: &'static str,
    /// Display name, matching how the official apps label them.
    pub name: &'static str,
    /// True for the Fajr-recitation variants — UIs show them only on the
    /// Fajr tab, like the official apps do.
    pub fajr_variant: bool,
}

/// Every selectable voice. Fajr variants are separate entries — the official
/// apps list them that way ("Makkah (Fajr)"), and a consumer decides per
/// prayer which entry to use.
pub const ADHAN_VOICES: [AdhanVoice; 8] = [
    AdhanVoice { id: "adhan-maquah", name: "Makkah", fajr_variant: false },
    AdhanVoice {
        id: "adhan-maquah-fajr",
        name: "Makkah (Fajr)",
        fajr_variant: true,
    },
    AdhanVoice { id: "adhan-madina", name: "Madinah", fajr_variant: false },
    AdhanVoice {
        id: "adhan-madina-fajr",
        name: "Madinah (Fajr)",
        fajr_variant: true,
    },
    AdhanVoice {
        id: "adhan-quds",
        name: "Al-Aqsa (Qods)",
        fajr_variant: false,
    },
    AdhanVoice {
        id: "adhan-quds-fajr",
        name: "Al-Aqsa (Qods, Fajr)",
        fajr_variant: true,
    },
    AdhanVoice { id: "adhan-algeria", name: "Algeria", fajr_variant: false },
    AdhanVoice { id: "adhan-egypt", name: "Egypt", fajr_variant: false },
];

/// The CDN URL for a catalog voice. Unknown ids are rejected: a caller may
/// pass a hostile string here, and a URL is only ever built from the static
/// catalog.
#[cfg(any(feature = "std", feature = "alloc"))]
pub fn adhan_voice_url(id: &str) -> Option<String> {
    ADHAN_VOICES
        .iter()
        .find(|v| v.id == id)
        .map(|v| format!("{CDN_URL_BASE}/{}.mp3", v.id))
}

/// The voice the mosque itself uses (`adhanVoice` on the mosque page),
/// validated against the catalog; `None` when the mosque uses the default
/// (`null`) or publishes an unknown id.
#[cfg(any(feature = "std", feature = "alloc"))]
pub fn voice_id_from_conf(conf: &ConfData) -> Option<&'static str> {
    let raw = conf.raw.get("adhanVoice")?.as_str()?;
    ADHAN_VOICES.iter().find(|v| v.id == raw).map(|v| v.id)
}

/// Download a catalog voice into `dest_dir` (as `{id}.mp3`, atomically) and
/// return the path. Skips the download when a valid file is already cached
/// (validated size, FINDING F24 — a cached file is never trusted past the
/// cap). The download goes through the client's HTTP stack, so a proxy
/// (Tor) applies. Playback fallback is the caller's concern: on `Err`, play
/// the builtin instead.
#[cfg(feature = "std")]
pub async fn download_voice(
    client: &MawaqitClient,
    id: &str,
    dest_dir: &Path,
) -> Result<PathBuf> {
    let Some(url) = client.voice_url(id) else {
        return Err(MawaqitError::InvalidVoice(error::bounded(id, BOUNDED_ID)));
    };
    let dest = dest_dir.join(format!("{id}.mp3"));

    // A directory (or any non-file) at the destination is not a cached
    // voice — the download proceeds and the later steps fail loudly. The
    // same for an oversized "cached" file (FINDING F24): the destination
    // directory is attacker-writable, so a planted blob is replaced by a
    // fresh download instead of being served forever.
    if std::fs::metadata(&dest).is_ok_and(|meta| {
        meta.is_file() && meta.len() > 0 && meta.len() <= MAX_VOICE_BYTES as u64
    }) {
        return Ok(dest);
    }

    // Explicit matches instead of `?`: the Try-desugaring of `?` inside an
    // async fn instruments generator-glue closures that never execute as
    // functions, which permanently shows as uncovered lines.
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(e) => return Err(MawaqitError::Http(e)),
    };
    let status = response.status();
    if !status.is_success() {
        return Err(MawaqitError::Api { status: status.as_u16(), url });
    }
    // The CDN sends Content-Length; a larger claim is rejected before any
    // transfer. A missing header falls through to the mid-stream cap.
    if response
        .content_length()
        .is_some_and(|len| len as usize > MAX_VOICE_BYTES)
    {
        return Err(MawaqitError::InvalidVoice(error::bounded(
            &format!("voice file exceeds the {MAX_VOICE_BYTES} byte cap"),
            BOUNDED_DIAGNOSTIC,
        )));
    }
    // FINDING F24: the body is streamed with the cap enforced per chunk —
    // it is never buffered past the cap, and a hostile CDN serving an
    // endless body without Content-Length is cut off mid-stream instead of
    // being held to the request timeout.
    let mut response = response;
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if bytes.len() + chunk.len() > MAX_VOICE_BYTES {
                    return Err(MawaqitError::InvalidVoice(error::bounded(
                        &format!(
                            "voice file exceeds the {MAX_VOICE_BYTES} byte cap"
                        ),
                        BOUNDED_DIAGNOSTIC,
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => return Err(MawaqitError::Http(e)),
        }
    }
    if bytes.is_empty() {
        return Err(MawaqitError::InvalidVoice("empty voice file".to_string()));
    }

    std::fs::create_dir_all(dest_dir)
        .map_err(|e| MawaqitError::Parse(e.to_string()))?;
    // FINDING F24: the temp name is per-writer unique — two concurrent
    // downloads of one voice must never share a temp path, or one renames
    // a torn file into place.
    let tmp = unique_tmp_path(&dest);
    std::fs::write(&tmp, &bytes)
        .map_err(|e| MawaqitError::Parse(e.to_string()))?;
    std::fs::rename(&tmp, &dest)
        .map_err(|e| MawaqitError::Parse(e.to_string()))?;
    Ok(dest)
}
