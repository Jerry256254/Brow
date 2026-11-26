//! Own HTTP/1.1 client over raw TCP (plus file:// for local pages), with
//! hard resource limits. HTTPS is supported behind the `tls` feature via
//! rustls (TLS itself is never hand-rolled) — the rendering pipeline stays ours.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[cfg(feature = "tls")]
use std::sync::{Arc, OnceLock};
#[cfg(feature = "tls")]
use rustls::{ClientConfig, RootCertStore};
#[cfg(feature = "tls")]
use rustls_pki_types::ServerName;

pub const USER_AGENT: &str = "KucLab-Brow/0.1 (Brownie engine)";

/// Security/resource limits (defense against hostile servers and inputs).
pub const MAX_URL_LEN: usize = 4096;
pub const MAX_HEADER_BYTES: usize = 64 * 1024;
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_REDIRECTS: usize = 5;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const IO_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub final_url: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn content_type(&self) -> &str {
        self.header("content-type").unwrap_or("application/octet-stream")
    }
}

#[derive(Debug)]
pub enum FetchError {
    UnsupportedScheme(String),
    BadUrl(String),
    Blocked,
    Io(std::io::Error),
    Http(u16),
    TooManyRedirects,
    Timeout,
    HeadersTooLarge,
    Tls(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::UnsupportedScheme(s) => write!(f, "unsupported scheme '{}' (build with --features tls for https)", s),
            FetchError::BadUrl(u) => write!(f, "bad URL: {}", u),
            FetchError::Blocked => write!(f, "blocked by ad blocker"),
            FetchError::Io(e) => write!(f, "io error: {}", e),
            FetchError::Http(s) => write!(f, "HTTP status {}", s),
            FetchError::TooManyRedirects => write!(f, "too many redirects (>{})", MAX_REDIRECTS),
            FetchError::Timeout => write!(f, "timed out"),
            FetchError::HeadersTooLarge => write!(f, "response headers exceed {} bytes", MAX_HEADER_BYTES),
            FetchError::Tls(e) => write!(f, "TLS error: {}", e),
        }
    }
}

impl std::error::Error for FetchError {}

impl From<std::io::Error> for FetchError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::TimedOut {
            FetchError::Timeout
        } else {
            FetchError::Io(e)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Url {
    pub fn parse(input: &str) -> Result<Url, FetchError> {
        if input.len() > MAX_URL_LEN {
            return Err(FetchError::BadUrl(format!("URL exceeds {} bytes", MAX_URL_LEN)));
        }
        let (scheme, rest) = input
            .split_once("://")
            .ok_or_else(|| FetchError::BadUrl(input.to_string()))?;
        let scheme = scheme.to_lowercase();
        if scheme != "http" && scheme != "https" && scheme != "file" {
            return Err(FetchError::UnsupportedScheme(scheme));
        }
        if scheme == "file" {
            return Ok(Url {
                scheme,
                host: String::new(),
                port: 0,
                path: rest.to_string(),
            });
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let default_port = if scheme == "https" { 443 } else { 80 };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
                (h.to_string(), p.parse::<u16>().map_err(|_| FetchError::BadUrl(input.to_string()))?)
            }
            _ => (authority.to_string(), default_port),
        };
        if host.is_empty() || host.len() > 253 {
            return Err(FetchError::BadUrl(input.to_string()));
        }
        Ok(Url {
            scheme,
            host,
            port,
            path: path.to_string(),
        })
    }

    pub fn origin(&self) -> String {
        if self.scheme == "file" {
            return "file://".to_string();
        }
        if (self.scheme == "http" && self.port == 80) || (self.scheme == "https" && self.port == 443) {
            return format!("{}://{}", self.scheme, self.host);
        }
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    /// Same-origin check used before fetching subresources.
    pub fn same_origin(&self, other: &Url) -> bool {
        self.scheme == other.scheme && self.host == other.host && self.port == other.port
    }
}

/// Resolve a possibly-relative href against a base URL string.
pub fn resolve_url(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.len() > MAX_URL_LEN {
        return String::new();
    }
    if href.contains("://") {
        return href.to_string();
    }
    let Ok(b) = Url::parse(base) else {
        return href.to_string();
    };
    if let Some(rest) = href.strip_prefix("//") {
        return format!("{}://{}", b.scheme, rest);
    }
    if let Some(p) = href.strip_prefix('/') {
        return format!("{}/{}", b.origin(), p);
    }
    let dir = match b.path.rfind('/') {
        Some(i) => &b.path[..i + 1],
        None => "/",
    };
    format!("{}{}{}", b.origin(), dir, href)
}

fn fetch_file(url: &Url) -> Result<Response, FetchError> {
    let path = url.path.replace("%20", " ");
    let meta = std::fs::metadata(&path)?;
    if meta.len() > MAX_BODY_BYTES as u64 {
        return Err(FetchError::BadUrl(format!("file too large ({} bytes)", meta.len())));
    }
    let body = std::fs::read(&path)?;
    let ct = if path.ends_with(".html") || path.ends_with(".htm") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    };
    Ok(Response {
        status: 200,
        headers: vec![("content-type".to_string(), ct.to_string())],
        body,
        final_url: format!("file://{}", path),
    })
}

/// Read a full HTTP response with header/body caps. Stops at header end first,
/// then reads at most MAX_BODY_BYTES of body.
fn read_response_capped<S: Read>(stream: &mut S) -> Result<Vec<u8>, FetchError> {
    let mut raw: Vec<u8> = Vec::new();
    let mut buf = [0u8; 8192];
    let mut scanned = 0usize;
    let header_end;
    loop {
        if let Some(p) = raw[scanned.saturating_sub(3)..]
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
        {
            header_end = scanned.saturating_sub(3) + p;
            break;
        }
        if raw.len() > MAX_HEADER_BYTES {
            return Err(FetchError::HeadersTooLarge);
        }
        scanned = raw.len();
        match stream.read(&mut buf) {
            Ok(0) => {
                header_end = raw.len();
                break;
            }
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            // servers that close without TLS close_notify are common; treat as EOF
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                header_end = raw.len();
                break;
            }
            Err(e) => return Err(e.into()),
        }
    }
    let have_body = raw.len().saturating_sub(header_end + 4);
    let mut remaining = MAX_BODY_BYTES.saturating_sub(have_body);
    let mut chunk = [0u8; 16384];
    while remaining > 0 {
        let want = remaining.min(chunk.len());
        match stream.read(&mut chunk[..want]) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                remaining -= n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(raw)
}

fn http_exchange<S: Read + Write>(stream: &mut S, url: &Url) -> Result<Response, FetchError> {
    let host_header = if (url.scheme == "http" && url.port == 80) || (url.scheme == "https" && url.port == 443) {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        url.path, host_header, USER_AGENT
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;
    let raw = read_response_capped(stream)?;
    parse_response(&raw, &format!("{}://{}:{}", url.scheme, url.host, url.port))
}

fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, FetchError> {
    let addr = (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| FetchError::BadUrl(host.to_string()))?;
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    Ok(stream)
}

fn http_once(url: &Url) -> Result<Response, FetchError> {
    let mut stream = tcp_connect(&url.host, url.port)?;
    http_exchange(&mut stream, url)
}

#[cfg(feature = "tls")]
pub fn default_root_store() -> &'static RootCertStore {
    static ROOTS: OnceLock<RootCertStore> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        roots
    })
}

#[cfg(feature = "tls")]
fn https_once(url: &Url, roots: &RootCertStore) -> Result<Response, FetchError> {
    let tcp = tcp_connect(&url.host, url.port)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| FetchError::Tls(e.to_string()))?
        .with_root_certificates(roots.clone())
        .with_no_client_auth();
    let server_name = ServerName::try_from(url.host.clone())
        .map_err(|_| FetchError::BadUrl(format!("invalid server name '{}'", url.host)))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| FetchError::Tls(e.to_string()))?;
    // Drive the handshake eagerly so certificate errors surface as FetchError::Tls.
    let mut tcp = tcp;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)
            .map_err(|e| FetchError::Tls(e.to_string()))?;
    }
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    http_exchange(&mut tls, url)
}

fn parse_response(raw: &[u8], origin: &str) -> Result<Response, FetchError> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| FetchError::BadUrl("malformed HTTP response".to_string()))?;
    let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| FetchError::BadUrl("bad status line".to_string()))?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let resp = Response {
        status,
        headers,
        body: Vec::new(),
        final_url: origin.to_string(),
    };
    let chunked = resp
        .header("transfer-encoding")
        .map(|v| v.to_lowercase().contains("chunked"))
        .unwrap_or(false);
    let body_raw = &raw[header_end + 4..];
    let body = if chunked {
        dechunk(body_raw)
    } else if let Some(cl) = resp.header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body_raw[..cl.min(body_raw.len()).min(MAX_BODY_BYTES)].to_vec()
    } else {
        body_raw[..body_raw.len().min(MAX_BODY_BYTES)].to_vec()
    };
    Ok(Response { body, ..resp })
}

fn dechunk(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line_end = match data.windows(2).position(|w| w == b"\r\n") {
            Some(i) => i,
            None => break,
        };
        let size_str = String::from_utf8_lossy(&data[..line_end]);
        let size_str = size_str.split(';').next().unwrap_or("").trim();
        let Ok(mut size) = usize::from_str_radix(size_str, 16) else { break };
        data = &data[line_end + 2..];
        if size == 0 {
            break;
        }
        if out.len() + size > MAX_BODY_BYTES {
            size = MAX_BODY_BYTES - out.len();
        }
        let take = size.min(data.len());
        out.extend_from_slice(&data[..take]);
        data = &data[take..];
        if data.starts_with(b"\r\n") {
            data = &data[2..];
        }
        if out.len() >= MAX_BODY_BYTES {
            break;
        }
    }
    out
}

/// Fetch a URL following redirects (max MAX_REDIRECTS).
/// With the `tls` feature, https:// is supported via rustls + webpki roots.
pub fn fetch(url: &str, max_redirects: usize) -> Result<Response, FetchError> {
    fetch_inner(url, max_redirects, None)
}

/// Fetch with a custom TLS root store (used by tests with self-signed certs).
#[cfg(feature = "tls")]
pub fn fetch_with_roots(url: &str, max_redirects: usize, roots: &RootCertStore) -> Result<Response, FetchError> {
    fetch_inner(url, max_redirects, Some(roots))
}

#[cfg(feature = "tls")]
type Roots<'a> = Option<&'a RootCertStore>;
#[cfg(not(feature = "tls"))]
type Roots<'a> = Option<&'a ()>;

fn fetch_inner(url: &str, max_redirects: usize, roots: Roots<'_>) -> Result<Response, FetchError> {
    if url.len() > MAX_URL_LEN {
        return Err(FetchError::BadUrl(format!("URL exceeds {} bytes", MAX_URL_LEN)));
    }
    let mut current = url.to_string();
    let mut redirects_left = max_redirects.min(MAX_REDIRECTS);
    loop {
        let u = Url::parse(&current)?;
        let resp = match u.scheme.as_str() {
            "file" => return fetch_file(&u),
            "http" => http_once(&u)?,
            "https" => {
                #[cfg(feature = "tls")]
                {
                    let roots = match roots {
                        Some(r) => r,
                        None => default_root_store(),
                    };
                    https_once(&u, roots)?
                }
                #[cfg(not(feature = "tls"))]
                {
                    let _ = roots;
                    return Err(FetchError::UnsupportedScheme("https".to_string()));
                }
            }
            other => return Err(FetchError::UnsupportedScheme(other.to_string())),
        };
        if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
            if let Some(loc) = resp.header("location") {
                if redirects_left == 0 {
                    return Err(FetchError::TooManyRedirects);
                }
                redirects_left -= 1;
                let next = resolve_url(&current, loc);
                if next.is_empty() {
                    return Err(FetchError::BadUrl("empty redirect location".to_string()));
                }
                let next_url = Url::parse(&next)?;
                // Never silently downgrade https -> http via redirect.
                if u.scheme == "https" && next_url.scheme == "http" {
                    return Err(FetchError::BadUrl("refusing https -> http downgrade redirect".to_string()));
                }
                current = next;
                continue;
            }
        }
        return Ok(Response {
            final_url: current.clone(),
            ..resp
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parsing() {
        let u = Url::parse("http://example.com:8080/a/b?x=1").unwrap();
        assert_eq!(u.host, "example.com");
        assert_eq!(u.port, 8080);
        assert_eq!(u.path, "/a/b?x=1");
        let u = Url::parse("http://example.com").unwrap();
        assert_eq!(u.port, 80);
        assert_eq!(u.path, "/");
        assert_eq!(u.origin(), "http://example.com");
        assert!(Url::parse("http://").is_err());
        assert!(matches!(Url::parse("ftp://x/").unwrap_err(), FetchError::UnsupportedScheme(_)));
        let long = format!("http://{}", "a".repeat(5000));
        assert!(matches!(Url::parse(&long).unwrap_err(), FetchError::BadUrl(_)));
    }

    #[test]
    fn same_origin_rules() {
        let a = Url::parse("http://x.com:80/a").unwrap();
        let b = Url::parse("http://x.com/b").unwrap();
        let c = Url::parse("http://x.com:8080/b").unwrap();
        let d = Url::parse("https://x.com/b").unwrap();
        assert!(a.same_origin(&b));
        assert!(!a.same_origin(&c));
        assert!(!a.same_origin(&d));
    }

    #[test]
    fn resolve() {
        assert_eq!(resolve_url("http://a.com/x/y.html", "z.css"), "http://a.com/x/z.css");
        assert_eq!(resolve_url("http://a.com/x/y.html", "/s.css"), "http://a.com/s.css");
        assert_eq!(resolve_url("http://a.com/x/", "https://b.com/c"), "https://b.com/c");
        assert_eq!(resolve_url("file:///d/page.html", "s.css"), "file:///d/s.css");
    }

    #[test]
    fn dechunk_works_and_caps() {
        let data = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(dechunk(data), b"Wikipedia");
    }
}
