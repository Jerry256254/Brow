//! Integration tests: real HTTP over loopback, rendering, screenshots, adblock.

use brow_engine::adblock::AdBlock;
use brow_engine::{layout, Browser};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

const PAGE: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>Test Page</title>
  <link rel="stylesheet" href="/style.css">
  <script src="/ads.js"></script>
</head>
<body>
  <h1 id="head">Hello Brow</h1>
  <p>some text with <b>bold</b> words to wrap around the line width limit</p>
</body>
</html>"#;

const CSS: &str = "h1 { color: #ff0000; } p { font-size: 16px; }";

fn spawn_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{}", port);
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            let (status, headers, body): (&str, String, Vec<u8>) = match path.as_str() {
                "/" => ("200 OK", "Content-Type: text/html\r\n".to_string(), PAGE.as_bytes().to_vec()),
                "/style.css" => ("200 OK", "Content-Type: text/css\r\n".to_string(), CSS.as_bytes().to_vec()),
                "/ads.js" => ("200 OK", "Content-Type: application/javascript\r\n".to_string(), b"evil()".to_vec()),
                "/chunked" => (
                    "200 OK",
                    "Content-Type: text/plain\r\nTransfer-Encoding: chunked\r\n".to_string(),
                    b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n".to_vec(),
                ),
                "/redir" => ("302 Found", "Location: /\r\n".to_string(), Vec::new()),
                _ => ("404 Not Found", "Content-Type: text/plain\r\n".to_string(), b"nope".to_vec()),
            };
            let resp = format!(
                "HTTP/1.1 {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n",
                status,
                headers,
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    (base, handle)
}

fn browser_with(filters: &str) -> Browser {
    let mut b = Browser::new();
    b.adblock = AdBlock::load_from_str(filters);
    b
}

#[test]
fn loads_page_parses_title_and_applies_css() {
    let (base, _h) = spawn_server();
    let mut b = browser_with("");
    let page = b.open(&format!("{}/", base)).expect("open");
    assert_eq!(page.status, 200);
    assert_eq!(page.title, "Test Page");
    assert_eq!(page.final_url, format!("{}/", base));
    let h1 = page.doc.find_first("h1").expect("h1");
    let style = brow_engine::style::compute_style(&page.doc, h1, &page.sheets);
    assert_eq!(style.color, brow_engine::css::Color::rgb(255, 0, 0), "author css must apply");
}

#[test]
fn adblock_blocks_subresource_and_counts_it() {
    let (base, _h) = spawn_server();
    let mut b = browser_with("||127.0.0.1/ads.js\n");
    let page = b.open(&format!("{}/", base)).expect("open");
    assert_eq!(b.blocked_count, 1, "ads.js must be counted as blocked");
    let _ = page;

    // without the filter nothing is blocked
    let (base2, _h2) = spawn_server();
    let mut b2 = browser_with("");
    let _ = b2.open(&format!("{}/", base2)).expect("open");
    assert_eq!(b2.blocked_count, 0);
}

#[test]
fn redirects_are_followed() {
    let (base, _h) = spawn_server();
    let mut b = browser_with("");
    let page = b.open(&format!("{}/redir", base)).expect("open");
    assert_eq!(page.final_url, format!("{}/", base));
    assert_eq!(page.title, "Test Page");
}

#[test]
fn chunked_body_decoded() {
    let (base, _h) = spawn_server();
    let resp = brow_engine::net::fetch(&format!("{}/chunked", base), 5).expect("fetch");
    assert_eq!(resp.text(), "hello world");
}

#[test]
fn http_404_is_an_error() {
    let (base, _h) = spawn_server();
    let mut b = browser_with("");
    match b.open(&format!("{}/missing", base)) {
        Err(e) => assert!(e.to_string().contains("404"), "got: {}", e),
        Ok(_) => panic!("expected 404 error"),
    }
}

#[test]
fn layout_dimensions() {
    let (base, _h) = spawn_server();
    let mut b = browser_with("");
    let page = b.open(&format!("{}/", base)).expect("open");
    let root = b.layout_page(&page, 400.0);
    assert_eq!(root.w, 400.0);
    let html_box = &root.children[0];
    let body = &html_box.children[0];
    assert!((body.w - 384.0).abs() < 0.01, "body width {} (viewport 400 - 2*8 margin)", body.w);
    let h1 = &body.children[0];
    assert_eq!(h1.kind, layout::BoxKind::Block);
    assert!(h1.h > 0.0);
}

#[test]
fn screenshot_writes_valid_png() {
    let (base, _h) = spawn_server();
    let mut b = browser_with("");
    let dir = std::env::temp_dir();
    let out = dir.join(format!("brow-test-{}.png", std::process::id()));
    let path = out.to_str().unwrap().to_string();
    let page = b.screenshot(&format!("{}/", base), &path, 200, 300, 2).expect("screenshot");
    let _ = page;
    let data = std::fs::read(&path).unwrap();
    assert_eq!(&data[..8], &[137, 80, 78, 71, 13, 10, 26, 10], "PNG signature");
    assert!(data.windows(4).any(|w| w == b"IEND"));
    assert_eq!(&data[16..20], &400u32.to_be_bytes(), "width = 200*2");
    assert_eq!(&data[20..24], &600u32.to_be_bytes(), "height = 300*2");
    std::fs::remove_file(&path).ok();
}

#[test]
fn file_scheme_loads_local_demo() {
    let mut b = browser_with("*/ads.js\n*/ads/*\n");
    let demo = concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/site/index.html");
    let page = b.open(&format!("file://{}", demo)).expect("open file");
    assert_eq!(page.title, "KucLab Brow demo");
    // style.css must have been picked up via <link>
    let h1 = page.doc.find_first("h1").unwrap();
    let style = brow_engine::style::compute_style(&page.doc, h1, &page.sheets);
    assert_eq!(style.color, brow_engine::css::Color::rgb(0x3b, 0x2f, 0xd4));
    // ads.js and ads/banner.png are blocked by the filter
    assert_eq!(b.blocked_count, 2, "script + img blocked, got {}", b.blocked_count);
}

#[test]
fn rendered_framebuffer_has_content() {
    let (base, _h) = spawn_server();
    let mut b = browser_with("");
    let page = b.open(&format!("{}/", base)).expect("open");
    let (fb, _root) = b.render_page(&page, 120, 200, 0.0, 1);
    let non_white = fb
        .data
        .chunks_exact(4)
        .filter(|p| !(p[0] == 255 && p[1] == 255 && p[2] == 255))
        .count();
    assert!(non_white > 50, "expected rendered pixels, got {}", non_white);
}
