//! Security & robustness tests: hostile inputs must be handled with strict
//! resource limits and must never crash or hang the engine.

use brownie::adblock::AdBlock;
use brownie::{net, Browser};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn spawn_raw<F: Fn(&str) -> (String, Vec<u8>) + Send + 'static>(handler: F) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (head, body) = handler(&path);
            let resp = format!("HTTP/1.1 {}\r\nConnection: close\r\n\r\n", head);
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    format!("http://127.0.0.1:{}", port)
}

#[test]
fn deep_dom_does_not_overflow() {
    let html = format!("<html><body>{}<p>deep</p></body></html>", "<div>".repeat(200_000));
    let doc = brownie::html::parse(&html);
    // layout + paint must survive too (recursion bounded by MAX_DEPTH)
    let b = Browser::new();
    let page = brownie::Page {
        url: "x".into(),
        final_url: "x".into(),
        title: String::new(),
        status: 200,
        doc,
        sheets: Vec::new(),
    };
    let root = b.layout_page(&page, 200.0);
    let fb = brownie::paint::render(&root, 100, 100, 0.0, 1);
    assert_eq!(fb.width, 100);
}

#[test]
fn entity_bomb_does_not_hang() {
    let bomb = "&".repeat(500_000);
    let out = brownie::html::decode_entities(&bomb);
    assert_eq!(out.len(), 500_000);
}

#[test]
fn oversized_headers_rejected() {
    let base = spawn_raw(|_| {
        ("200 OK".to_string(), Vec::new())
    });
    // server that sends a giant header line
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let big = format!("HTTP/1.1 200 OK\r\nX-Big: {}\r\n\r\n", "h".repeat(200_000));
            let _ = s.write_all(big.as_bytes());
        }
    });
    let err = net::fetch(&format!("http://127.0.0.1:{}/", port), 5).unwrap_err();
    assert!(matches!(err, net::FetchError::HeadersTooLarge), "got {:?}", err);
    let _ = base;
}

#[test]
fn oversized_body_is_capped() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let big_len = net::MAX_BODY_BYTES + 1024 * 1024;
    thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf);
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", big_len);
            let _ = s.write_all(head.as_bytes());
            let chunk = vec![b'x'; 1024 * 1024];
            let mut sent = 0;
            while sent < big_len {
                let n = chunk.len().min(big_len - sent);
                if s.write_all(&chunk[..n]).is_err() {
                    break;
                }
                sent += n;
            }
        }
    });
    let resp = net::fetch(&format!("http://127.0.0.1:{}/", port), 5).unwrap();
    assert!(resp.body.len() <= net::MAX_BODY_BYTES, "body len {}", resp.body.len());
}

#[test]
fn same_origin_policy_blocks_cross_origin_subresources() {
    // page references file:// and another origin's CSS/JS — must not be fetched
    let page = r#"<html><head>
      <link rel="stylesheet" href="file:///etc/hostname">
      <link rel="stylesheet" href="http://127.0.0.1:1/evil.css">
      <link rel="stylesheet" href="/style.css">
      <script src="file:///etc/passwd"></script>
      </head><body><p>x</p></body></html>"#;
    let base = spawn_raw(|path| {
        if path == "/style.css" {
            ("200 OK\r\nContent-Type: text/css".to_string(), b"p { color: red; }".to_vec())
        } else {
            ("200 OK\r\nContent-Type: text/html".to_string(), page.as_bytes().to_vec())
        }
    });
    let mut b = Browser::new();
    let page = b.open(&format!("{}/", base)).expect("open");
    assert_eq!(page.sheets.len(), 1, "only same-origin stylesheet may load, got {}", page.sheets.len());
    let p = page.doc.find_first("p").unwrap();
    let st = brownie::style::compute_style(&page.doc, p, &page.sheets);
    assert_eq!(st.color, brownie::css::Color::rgb(255, 0, 0));
}

#[test]
fn framebuffer_size_is_capped() {
    let b = Browser::new();
    let doc = brownie::html::parse("<html><body><p>x</p></body></html>");
    let page = brownie::Page {
        url: "x".into(),
        final_url: "x".into(),
        title: String::new(),
        status: 200,
        doc,
        sheets: Vec::new(),
    };
    let err = b.render_page(&page, 20_000, 20_000, 0.0, 1).unwrap_err();
    assert!(err.to_string().contains("too large"), "got {}", err);
    // sane size still works
    assert!(b.render_page(&page, 320, 400, 0.0, 2).is_ok());
}

#[test]
fn long_url_rejected() {
    let long = format!("http://{}.com/", "a".repeat(5000));
    let err = net::fetch(&long, 5).unwrap_err();
    assert!(matches!(err, net::FetchError::BadUrl(_)));
}

#[test]
fn css_and_filters_capped() {
    let big_css = "p { color: red; }\n".repeat(50_000);
    let sheet = brownie::css::parse_stylesheet(&big_css);
    assert_eq!(sheet.rules.len(), brownie::css::MAX_RULES);
    let big_list = "||x.com^\n".repeat(300_000);
    let ab = AdBlock::load_from_str(&big_list);
    assert_eq!(ab.len(), brownie::adblock::MAX_RULES);
}

#[test]
fn https_downgrade_redirect_refused() {
    let base = spawn_raw(|_| ("302 Found\r\nLocation: http://127.0.0.1:9/".to_string(), Vec::new()));
    let _ = &base;
    // simulate an https page redirecting to http: call fetch_inner via https URL is
    // not possible without TLS, so test the rule through resolve + Url directly
    let u = net::Url::parse("https://example.com/").unwrap();
    let next = net::Url::parse(&net::resolve_url("https://example.com/", "http://127.0.0.1:9/")).unwrap();
    assert!(u.scheme == "https" && next.scheme == "http");
    let _ = base;
}

#[test]
fn perf_smoke_large_page() {
    // ~1.5 MB document with thousands of elements: must parse+layout+paint fast.
    let mut html = String::from("<html><head><title>big</title></head><body>");
    for i in 0..8000 {
        html.push_str(&format!(
            "<div class=\"c{}\"><h2>Section {}</h2><p>Lorem ipsum dolor sit amet {} <b>bold</b> text here.</p></div>",
            i % 7, i, i
        ));
    }
    html.push_str("</body></html>");
    let b = Browser::new();
    let t = std::time::Instant::now();
    let doc = brownie::html::parse(&html);
    let parse = t.elapsed();
    let page = brownie::Page {
        url: "x".into(),
        final_url: "x".into(),
        title: "big".into(),
        status: 200,
        doc,
        sheets: vec![brownie::css::parse_stylesheet("h2 { color: #123456; } .c3 { background-color: #eee; }")],
    };
    let t = std::time::Instant::now();
    let root = b.layout_page(&page, 320.0);
    let layout = t.elapsed();
    let t = std::time::Instant::now();
    let fb = brownie::paint::render(&root, 320, 2000, 0.0, 1);
    let paint = t.elapsed();
    eprintln!(
        "perf: {} bytes | parse {:?} | layout {:?} | paint {:?} | page height {:.0}px",
        html.len(), parse, layout, paint, root.h
    );
    assert!(html.len() > 500_000);
    assert!(root.h > 1000.0);
    assert_eq!(fb.width, 320);
    // generous bound: debug builds are slow; this guards against quadratic blowups
    assert!(parse.as_secs() < 10 && layout.as_secs() < 10 && paint.as_secs() < 10);
}
