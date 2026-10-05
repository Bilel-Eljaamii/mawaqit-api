//! Unit tests for the page scraper (moved out of `src/scraper.rs`),
//! through the public [`parse_page`] alias — the same entry point network
//! data takes.

use mawaqit_api::{MawaqitError, parse_page};

fn sample_page() -> String {
    let conf = r#"{"times":["06:09","07:41","13:47","16:58","19:45"],"shuruq":"07:41","calendar":[{"1":["07:05","08:44","12:59","14:48","17:08","18:35"]}],"iqamaCalendar":[{"1":["+8","+8","+8","+0","+8"]}],"name":"GRANDE MOSQUÉE DE PARIS","jumua":"13:50","jumua2":"14:30","announcements":[{"id":1,"title":"Hello"}]}"#;
    format!(
        "<html><head><script>var a=1;</script></head><body>\
         <script>var confData = {conf};</script></body></html>"
    )
}

#[test]
fn extracts_conf_data_from_page() {
    let conf = parse_page(&sample_page(), "grande-mosquee-de-paris").unwrap();
    assert_eq!(conf.name.as_deref(), Some("GRANDE MOSQUÉE DE PARIS"));
    assert_eq!(conf.times.len(), 5);
    assert_eq!(conf.shuruq.as_deref(), Some("07:41"));
    assert_eq!(conf.calendar.len(), 1);
    assert_eq!(conf.iqama_calendar.as_ref().unwrap().len(), 1);
    assert_eq!(conf.jumua.as_deref(), Some("13:50"));
    assert_eq!(conf.announcements.len(), 1);
}

#[test]
fn handles_multiline_and_semicolons_in_strings() {
    let conf = "{\n  \"times\": [\"06:09\", \"07:41\", \"13:47\", \"16:58\", \"19:45\"],\n  \"calendar\": [{\"1\": [\"06:09\", \"07:41\", \"13:47\", \"16:58\", \"19:45\", \"21:12\"]}],\n  \"image\": \"https://x.test/a.jpg?q=1;s=2\"\n}";
    let page = format!(
        "<script>\n  var x = 0;\n  let confData =\n    {conf};\n  alert(x);\n</script>"
    );
    let conf = parse_page(&page, "x").unwrap();
    assert_eq!(conf.times.len(), 5);
    assert_eq!(conf.calendar.len(), 1);
    assert!(conf.raw["image"].as_str().unwrap().contains(';'));
}

#[test]
fn ignores_conf_data_mentions_that_are_not_assignments() {
    let times = r#""times":["01:01","01:01","01:01","01:01","01:01"]"#;
    let calendar = r#""calendar":[{"1":["01:01","01:01","01:01","01:01","01:01","01:01"]}]"#;
    let page = format!(
        r#"<script>if (confData === undefined) {{}} var confData = {{{times},{calendar}}};</script>"#
    );
    let conf = parse_page(&page, "x").unwrap();
    assert_eq!(conf.times.len(), 5);
}

#[test]
fn missing_conf_data_is_an_error() {
    let page = "<html><script>var other = 1;</script></html>".to_string();
    let err = parse_page(&page, "some-mosque").unwrap_err();
    assert!(matches!(err, MawaqitError::ConfDataNotFound(_)));
}

/// Review H3 follow-up: the exotic invisible-Cf ranges (variation tags,
/// Egyptian format controls, musical/annotation controls) must not survive
/// into display strings either.
#[test]
fn exotic_invisible_characters_are_stripped_from_display_fields() {
    let evil = "a\u{E0001}b\u{E007F}c\u{13430}d\u{1D173}e";
    let page = format!(
        r#"<html><script>var confData = {{"times":["06:09","07:41","13:47","16:58","19:45"],"shuruq":"07:41","calendar":[{{"1":["07:05","08:44","12:59","14:48","17:08","18:35"]}}],"name":"{evil}"}};</script></html>"#
    );
    let parsed = parse_page(&page, "x").expect("parses");
    let name = parsed.name.as_deref();
    assert_eq!(name, Some("abcde"), "{name:?}");
}

/// F6 across every announcement text field: title, content, image and video
/// are all sanitized at the boundary — a field with no hostile characters
/// passes through untouched.
#[test]
fn announcement_text_fields_are_all_sanitized() {
    let hostile_title = "\u{202E}eltitle\u{202C}";
    let hostile_body = "body\u{200B}text\u{202D}";
    let page = format!(
        r#"<html><script>var confData = {{"times":["06:09","07:41","13:47","16:58","19:45"],"calendar":[{{"1":["07:05","08:44","12:59","14:48","17:08","18:35"]}}],"announcements":[{{"id":7,"title":"{hostile_title}","content":"{hostile_body}","image":"https://x.test/a.jpg","video":"https://x.test/v.mp4"}}]}};</script></html>"#
    );
    let conf = parse_page(&page, "x").unwrap();
    assert_eq!(conf.announcements.len(), 1);
    let ann = &conf.announcements[0];
    assert_eq!(ann.title.as_deref(), Some("eltitle"));
    assert_eq!(ann.content.as_deref(), Some("bodytext"));
    assert_eq!(ann.image.as_deref(), Some("https://x.test/a.jpg"));
    assert_eq!(ann.video.as_deref(), Some("https://x.test/v.mp4"));
}
