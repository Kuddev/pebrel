use super::transfer::{catalog_with, copy_verified, decode_preview, install_file};
use super::*;
use crate::theme_library::package::{Author as PackageAuthor, export_package};
use crate::theme_library::{ThemeLibraryStore, builtin_document};
use crate::update_proxy::NetworkFailure;
use crate::update_proxy::test_support::{Server, response};
use nebula_settings::ThemeName;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;

fn row(id: &str) -> Value {
    json!({
        "id": id,
        "name": "宣纸",
        "version": "1.0.0",
        "author": {"name": "XVSHIFU", "github": "XVSHIFU"},
        "source": "https://github.com/XVSHIFU/pebrel-themes",
        "license": "MIT",
        "archive": {
            "url": "https://github.com/XVSHIFU/pebrel-themes/releases/download/v1.0.0/rice-paper.pebrel-theme.zip",
            "sha256": "9419b2303e12f98dfcfe253dca6f1f84753b6ccd42f903cc6b61381803a0e495",
            "bytes": 121082
        },
        "unpacked_bytes": 120724,
        "video_bytes": 0,
        "description": "暖白纸面、墨绿点缀。",
        "preview": "https://raw.githubusercontent.com/XVSHIFU/pebrel-themes/v1.0.0/themes/rice-paper/preview.png",
        "tags": ["light", "colors-only"]
    })
}

fn parse(rows: Vec<Value>) -> Listing {
    Listing::parse(&serde_json::to_vec(&json!({"schema_version": 1, "themes": rows})).unwrap())
        .unwrap()
}

#[test]
fn published_rows_parse_and_keep_their_own_repository_preview() {
    let listing = parse(vec![row("xvshifu/rice-paper")]);
    assert_eq!(listing.skipped, 0);
    let entry = &listing.entries[0];
    assert_eq!(entry.author.github, "XVSHIFU");
    assert!(entry.preview.as_deref().unwrap().ends_with("/rice-paper/preview.png"));
    assert_eq!(entry.origin().catalog, CATALOG);
}

#[test]
fn invalid_rows_are_skipped_without_hiding_valid_ones() {
    let mut cases = Vec::new();
    for (pointer, replacement) in [
        ("/id", json!("XVSHIFU/rice-paper")),
        ("/version", json!("v1")),
        ("/source", json!("https://gitlab.com/x/y")),
        // Release from a repository other than the declared source.
        (
            "/archive/url",
            json!("https://github.com/other/repo/releases/download/v1/x.pebrel-theme.zip"),
        ),
        (
            "/archive/url",
            json!(
                "https://github.com/XVSHIFU/pebrel-themes/releases/download/latest/x.pebrel-theme.zip"
            ),
        ),
        (
            "/archive/url",
            json!("https://github.com/XVSHIFU/pebrel-themes/releases/download/v1/x.zip?raw=1"),
        ),
        (
            "/archive/sha256",
            json!("9419B2303E12F98DFCFE253DCA6F1F84753B6CCD42F903CC6B61381803A0E495"),
        ),
        ("/archive/bytes", json!(MAX_ARCHIVE_BYTES + 1)),
        ("/video_bytes", json!(200_000)),
        ("/tags", json!(["light", "light"])),
        ("/name", json!("bad\u{7}name")),
    ] {
        let mut value = row("xvshifu/broken");
        *value.pointer_mut(pointer).unwrap() = replacement;
        cases.push(value);
    }
    cases.push(row("xvshifu/rice-paper"));
    // A duplicate ID keeps only the first row.
    cases.push(row("xvshifu/rice-paper"));
    let listing = parse(cases);
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.skipped, 12);
}

#[test]
fn previews_outside_the_theme_repository_are_not_fetched() {
    let mut foreign = row("xvshifu/a");
    foreign["preview"] = json!("https://tracker.example/pixel.png");
    let mut other_repo = row("xvshifu/b");
    other_repo["preview"] =
        json!("https://raw.githubusercontent.com/someone/else/main/preview.png");
    let mut escaped = row("xvshifu/c");
    escaped["preview"] =
        json!("https://raw.githubusercontent.com/XVSHIFU/pebrel-themes/main/%2e%2e/preview.png");
    let listing = parse(vec![foreign, other_repo, escaped]);
    assert_eq!(listing.entries.len(), 3);
    assert!(listing.entries.iter().all(|entry| entry.preview.is_none()));
}

#[test]
fn unsupported_schema_and_oversized_lists_fail_the_whole_catalog() {
    assert!(Listing::parse(br#"{"schema_version": 2, "themes": []}"#).is_err());
    let rows = vec![json!({}); MAX_CATALOG_ENTRIES + 1];
    assert!(
        Listing::parse(&serde_json::to_vec(&json!({"schema_version": 1, "themes": rows})).unwrap())
            .is_err()
    );
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn downloads_must_match_the_pinned_size_and_digest() {
    let body = b"pinned archive bytes";
    let sha = digest(body);
    let never = AtomicBool::new(false);
    let mut output = Vec::new();
    copy_verified(&mut &body[..], &mut output, body.len() as u64, &sha, &never).unwrap();
    assert_eq!(output, body);
    for (bytes, expected) in [
        (&body[..body.len() - 1], body.len() as u64),
        (&body[..], body.len() as u64 - 1),
        (&b"pinned archive BYTES"[..], body.len() as u64),
    ] {
        let error = copy_verified(&mut &bytes[..], &mut Vec::new(), expected, &sha, &never);
        assert!(matches!(error, Err(Error::Integrity)), "{error:?}");
    }
    let cancelled = AtomicBool::new(true);
    let error = copy_verified(&mut &body[..], &mut Vec::new(), body.len() as u64, &sha, &cancelled);
    assert!(matches!(error, Err(Error::Cancelled)));
}

#[test]
fn verified_archives_install_with_a_receipt_the_catalog_recognizes() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("nord.pebrel-theme.zip");
    let author = PackageAuthor { name: "Community author".into(), github: None };
    let theme = builtin_document(ThemeName::Nord).unwrap();
    export_package(&theme, author, "1.0.0".into(), "MIT".into(), &archive, None).unwrap();
    let bytes = std::fs::read(&archive).unwrap();
    let mut value = row("xvshifu/nord");
    value["archive"]["sha256"] = json!(digest(&bytes));
    value["archive"]["bytes"] = json!(bytes.len());
    let entry = parse(vec![value]).entries.remove(0);

    let store = ThemeLibraryStore::new(dir.path().join("library"));
    assert_eq!(installed(&entry, &store.list().unwrap().custom), Installed::No);
    let document = install_file(&archive, &entry, &store).unwrap();
    assert_eq!(Origin::of(&document), Some(entry.origin()));
    let library = store.list().unwrap().custom;
    assert_eq!(installed(&entry, &library), Installed::Current(document.id().unwrap().into()));

    let mut update = entry.clone();
    update.version = "1.1.0".into();
    update.archive.sha256 = "0".repeat(64);
    assert_eq!(installed(&update, &library), Installed::Other);
    let mut unrelated = entry;
    unrelated.id = "xvshifu/other".into();
    assert_eq!(installed(&unrelated, &library), Installed::No);
}

#[test]
fn catalog_requests_go_through_the_proxy_and_report_http_and_size_failures() {
    let body =
        serde_json::to_string(&json!({"schema_version": 1, "themes": [row("xvshifu/rice-paper")]}))
            .unwrap();
    let server = Server::start(vec![
        response("200 OK", "Content-Type: application/json\r\n", &body),
        response("404 Not Found", "", ""),
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_CATALOG_BYTES + 1
        ),
    ]);
    let agent = server.network_agent("http", None, &[]);
    let url = "http://catalog.community.invalid/index.json";
    assert_eq!(catalog_with(&agent, url).unwrap().entries.len(), 1);
    assert!(matches!(catalog_with(&agent, url), Err(Error::Network(NetworkFailure::Status(404)))));
    assert!(matches!(catalog_with(&agent, url), Err(Error::TooLarge)));
    let requests = server.finish();
    assert!(requests[0].0.starts_with("CONNECT catalog.community.invalid:80 "));
    assert!(requests[0].1.to_ascii_lowercase().contains("user-agent: pebrel-community"));
}

#[test]
fn previews_decode_to_bounded_thumbnails_and_reject_other_formats() {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(1600, 1000, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let thumbnail = decode_preview(png).unwrap();
    assert_eq!(thumbnail.dimensions(), (480, 300));
    let mut gif = Vec::new();
    image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut gif), image::ImageFormat::Gif)
        .unwrap();
    assert!(decode_preview(gif).is_err());
    assert!(decode_preview(b"<svg/>".to_vec()).is_err());
}
