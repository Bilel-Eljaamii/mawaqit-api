//! Unit tests for the adhan voice catalog (moved out of `src/voices.rs`),
//! through the public API: URL building, catalog integrity, and
//! page-to-catalog validation.

use mawaqit_api::{
    ADHAN_VOICES, adhan_voice_url, parse_page, voice_id_from_conf,
};

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
        parse_page(
            &format!("<html><script>var confData = {json};</script></html>"),
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
