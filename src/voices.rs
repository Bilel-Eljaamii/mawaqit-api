//! The adhan voice catalog: the muadhin recordings Mawaqit's mosque screens
//! play, served keylessly from the public CDN at
//! `https://cdn.mawaqit.net/audio/{id}.mp3`. The list is static and
//! enumerable (the same set the official apps stream), so it is a constant
//! here — no endpoint, no API key, no discovery at runtime.
//!
//! Al Afasy is omitted by product decision (2026-10-05).

use std::path::{Path, PathBuf};

use crate::{
    MawaqitClient,
    error::{MawaqitError, Result},
    models::ConfData,
};

/// The public CDN root the mosque-screen voices are served from.
pub const CDN_URL_BASE: &str = "https://cdn.mawaqit.net/audio";

/// Hard cap for one downloaded voice file (real files are ~2–5 MB).
const MAX_VOICE_BYTES: usize = 8 * 1024 * 1024;

/// One selectable adhan recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdhanVoice {
    /// Stable identifier, also the CDN file stem (`{id}.mp3`) and the value
    /// stored in the mosque page's `adhanVoice` field.
    pub id: &'static str,
    /// Display name, matching how the official apps label them.
    pub name: &'static str,
}

/// Every selectable voice. Fajr variants are separate entries — the official
/// apps list them that way ("Makkah (Fajr)"), and a consumer decides per
/// prayer which entry to use.
pub const ADHAN_VOICES: [AdhanVoice; 8] = [
    AdhanVoice { id: "adhan-maquah", name: "Makkah" },
    AdhanVoice { id: "adhan-maquah-fajr", name: "Makkah (Fajr)" },
    AdhanVoice { id: "adhan-madina", name: "Madinah" },
    AdhanVoice { id: "adhan-madina-fajr", name: "Madinah (Fajr)" },
    AdhanVoice { id: "adhan-quds", name: "Al-Aqsa (Qods)" },
    AdhanVoice { id: "adhan-quds-fajr", name: "Al-Aqsa (Qods, Fajr)" },
    AdhanVoice { id: "adhan-algeria", name: "Algeria" },
    AdhanVoice { id: "adhan-egypt", name: "Egypt" },
];

/// The CDN URL for a catalog voice. Unknown ids are rejected: a caller may
/// pass a hostile string here, and a URL is only ever built from the static
/// catalog.
pub fn adhan_voice_url(id: &str) -> Option<String> {
    ADHAN_VOICES
        .iter()
        .find(|v| v.id == id)
        .map(|v| format!("{CDN_URL_BASE}/{}.mp3", v.id))
}

/// The voice the mosque itself uses (`adhanVoice` on the mosque page),
/// validated against the catalog; `None` when the mosque uses the default
/// (`null`) or publishes an unknown id.
pub fn voice_id_from_conf(conf: &ConfData) -> Option<&'static str> {
    let raw = conf.raw.get("adhanVoice")?.as_str()?;
    ADHAN_VOICES.iter().find(|v| v.id == raw).map(|v| v.id)
}

/// Download a catalog voice into `dest_dir` (as `{id}.mp3`, atomically) and
/// return the path. Skips the download when a non-empty file is already
/// cached. The download goes through the client's HTTP stack, so a proxy
/// (Tor) applies. Playback fallback is the caller's concern: on `Err`, play
/// the builtin instead.
pub async fn download_voice(
    client: &MawaqitClient,
    id: &str,
    dest_dir: &Path,
) -> Result<PathBuf> {
    let Some(url) = client.voice_url(id) else {
        return Err(MawaqitError::InvalidVoice(id.to_string()));
    };
    let dest = dest_dir.join(format!("{id}.mp3"));

    if let Ok(meta) = std::fs::metadata(&dest) {
        if meta.len() > 0 {
            return Ok(dest);
        }
    }

    let response = client.get(&url).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(MawaqitError::Api { status: status.as_u16(), url });
    }
    // The CDN sends Content-Length; a larger claim is rejected before any
    // buffering. A missing header falls through to the post-download check.
    if let Some(len) = response.content_length() {
        if len as usize > MAX_VOICE_BYTES {
            return Err(MawaqitError::InvalidVoice(format!(
                "voice file of {len} bytes exceeds the {MAX_VOICE_BYTES} byte cap"
            )));
        }
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_VOICE_BYTES {
        return Err(MawaqitError::InvalidVoice(format!(
            "voice file of {} bytes exceeds the {MAX_VOICE_BYTES} byte cap",
            bytes.len()
        )));
    }
    if bytes.is_empty() {
        return Err(MawaqitError::InvalidVoice("empty voice file".into()));
    }

    std::fs::create_dir_all(dest_dir)
        .map_err(|e| MawaqitError::Parse(e.to_string()))?;
    let tmp = dest.with_extension("mp3.tmp");
    std::fs::write(&tmp, &bytes)
        .map_err(|e| MawaqitError::Parse(e.to_string()))?;
    std::fs::rename(&tmp, &dest)
        .map_err(|e| MawaqitError::Parse(e.to_string()))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_built_only_from_the_catalog() {
        assert_eq!(
            adhan_voice_url("adhan-maquah-fajr").as_deref(),
            Some("https://cdn.mawaqit.net/audio/adhan-maquah-fajr.mp3")
        );
        // A hostile id can never shape the URL.
        for bad in [
            "",
            "../../etc/passwd",
            "adhan-afassy", // omitted by product decision
            "adhan-maquah?x=1",
            "adhan-maquah#f",
            "ADHAN-MAQUAH",
        ] {
            assert!(adhan_voice_url(bad).is_none(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn catalog_has_no_duplicates() {
        let mut ids: Vec<_> = ADHAN_VOICES.iter().map(|v| v.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ADHAN_VOICES.len());
    }

    #[test]
    fn voice_id_from_conf_validates_against_the_catalog() {
        let conf_from = |json: &str| {
            crate::parse_page(
                &format!(
                    "<html><script>var confData = {json};</script></html>"
                ),
                "t",
            )
            .unwrap()
        };
        let conf = conf_from(
            r#"{"adhanVoice":"adhan-quds","times":["06:30","08:00","13:00","15:30","17:45","19:15"],"calendar":[{"1":["06:30","08:00","13:00","15:30","17:45","19:15"]}]}"#,
        );
        assert_eq!(voice_id_from_conf(&conf), Some("adhan-quds"));

        // The mosques' default and unknown ids are None, never a guess.
        for json in [
            r#"{"adhanVoice":null,"times":["06:30","08:00","13:00","15:30","17:45","19:15"],"calendar":[{"1":["06:30","08:00","13:00","15:30","17:45","19:15"]}]}"#,
            r#"{"times":["06:30","08:00","13:00","15:30","17:45","19:15"],"calendar":[{"1":["06:30","08:00","13:00","15:30","17:45","19:15"]}]}"#,
            r#"{"adhanVoice":"adhan-afassy","times":["06:30","08:00","13:00","15:30","17:45","19:15"],"calendar":[{"1":["06:30","08:00","13:00","15:30","17:45","19:15"]}]}"#,
            r#"{"adhanVoice":42,"times":["06:30","08:00","13:00","15:30","17:45","19:15"],"calendar":[{"1":["06:30","08:00","13:00","15:30","17:45","19:15"]}]}"#,
        ] {
            let conf = conf_from(json);
            assert!(voice_id_from_conf(&conf).is_none(), "{json:?}");
        }
    }
}
