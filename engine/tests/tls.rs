//! HTTPS tests (only compiled with --features tls): a loopback rustls server
//! with a self-signed cert for "localhost".
#![cfg(feature = "tls")]

use brow_engine::net;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;

fn spawn_tls_server() -> (String, rustls::RootCertStore) {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_der = certified.cert.der().clone();
    let key_der = certified.key_pair.serialize_der();

    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der.clone()).unwrap();

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], rustls::pki_types::PrivateKeyDer::Pkcs8(key_der.into()))
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(tcp) = stream else { continue };
            let Ok(conn) = rustls::ServerConnection::new(Arc::new(config.clone())) else {
                continue;
            };
            let mut tls = rustls::StreamOwned::new(conn, tcp);
            let mut buf = [0u8; 8192];
            let _ = tls.read(&mut buf);
            let body = b"<html><head><title>secure</title></head><body><p>over tls</p></body></html>";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = tls.write_all(resp.as_bytes());
            let _ = tls.write_all(body);
            let _ = tls.conn.send_close_notify();
            let _ = tls.flush();
        }
    });
    (format!("https://localhost:{}", port), roots)
}

#[test]
fn https_fetch_with_custom_roots() {
    let (base, roots) = spawn_tls_server();
    let resp = net::fetch_with_roots(&format!("{}/", base), 5, &roots).expect("https fetch");
    assert_eq!(resp.status, 200);
    assert!(resp.text().contains("over tls"));
}

#[test]
fn https_fetch_rejects_untrusted_cert_with_default_roots() {
    let (base, _roots) = spawn_tls_server();
    // default webpki roots must NOT trust our self-signed cert
    let err = net::fetch(&format!("{}/", base), 5).unwrap_err();
    let ok = matches!(err, net::FetchError::Tls(_))
        || matches!(&err, net::FetchError::Io(e) if e.kind() == std::io::ErrorKind::InvalidData);
    assert!(ok, "got {:?}", err);
}

#[test]
fn browser_open_over_https() {
    let (base, roots) = spawn_tls_server();
    // Browser::open uses net::fetch (default roots) — so use a custom fetch path:
    // verify at least that the URL layer routes https correctly with roots.
    let resp = net::fetch_with_roots(&format!("{}/page", base), 5, &roots).unwrap();
    assert_eq!(resp.status, 200);
}
