//! Own HTTP/1.1 client over raw TCP (plus file:// for local pages).
//! HTTPS/TLS is intentionally not hand-rolled; it is on the roadmap behind a
//! pluggable TLS transport.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const USER_AGENT: &str = "KucLab-Brow/0.1 (+own engine)";

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
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::UnsupportedScheme(s) => write!(f, "unsupported scheme '{}' (TLS not built yet)", s),
            FetchError::BadUrl(u) => write!(f, "bad URL: {}", u),
            FetchError::Blocked => write!(f, "blocked by ad blocker"),
            FetchError::Io(e) => write!(f, "io error: {}", e),
            FetchError::Http(s) => write!(f, "HTTP status {}", s),
            FetchError::TooManyRedirects => write!(f, "too many redirects"),
            FetchError::Timeout => write!(f, "timed out"),
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
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                (h.to_string(), p.parse::<u16>().map_err(|_| FetchError::BadUrl(input.to_string()))?)
            }
            _ => (authority.to_string(), if scheme == "https" { 443 } else { 80 }),
        };
        if host.is_empty() {
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
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }
}

/// Resolve a possibly-relative href against a base URL string.
pub fn resolve_url(base: &str, href: &str) -> String {
    let href = href.trim();
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

fn http_once(url: &Url) -> Result<Response, FetchError> {
    let addr = (url.host.as_str(), url.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| FetchError::BadUrl(url.host.clone()))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        url.path,
        if url.port == 80 { url.host.clone() } else { format!("{}:{}", url.host, url.port) },
        USER_AGENT
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw, &format!("{}://{}:{}", url.scheme, url.host, url.port))
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
        body_raw[..cl.min(body_raw.len())].to_vec()
    } else {
        body_raw.to_vec()
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
        let Ok(size) = usize::from_str_radix(size_str, 16) else { break };
        data = &data[line_end + 2..];
        if size == 0 {
            break;
        }
        let take = size.min(data.len());
        out.extend_from_slice(&data[..take]);
        data = &data[take..];
        if data.starts_with(b"\r\n") {
            data = &data[2..];
        }
    }
    out
}

/// Fetch a URL following redirects. `https` is rejected for now (TLS roadmap).
pub fn fetch(url: &str, max_redirects: usize) -> Result<Response, FetchError> {
    let mut current = url.to_string();
    for _ in 0..=max_redirects {
        let u = Url::parse(&current)?;
        match u.scheme.as_str() {
            "file" => return fetch_file(&u),
            "http" => {
                let resp = http_once(&u)?;
                let final_url = if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                    match resp.header("location") {
                        Some(loc) => resolve_url(&current, loc),
                        None => current.clone(),
                    }
                } else {
                    current.clone()
                };
                if matches!(resp.status, 301 | 302 | 303 | 307 | 308) && resp.header("location").is_some() {
                    current = final_url;
                    continue;
                }
                return Ok(Response { final_url: current.clone(), ..resp });
            }
            other => return Err(FetchError::UnsupportedScheme(other.to_string())),
        }
    }
    Err(FetchError::TooManyRedirects)
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
        assert!(Url::parse("http://").is_err());
        assert!(matches!(Url::parse("ftp://x/").unwrap_err(), FetchError::UnsupportedScheme(_)));
    }

    #[test]
    fn resolve() {
        assert_eq!(resolve_url("http://a.com/x/y.html", "z.css"), "http://a.com:80/x/z.css");
        assert_eq!(resolve_url("http://a.com/x/y.html", "/s.css"), "http://a.com:80/s.css");
        assert_eq!(resolve_url("http://a.com/x/", "https://b.com/c"), "https://b.com/c");
    }

    #[test]
    fn dechunk_works() {
        let data = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(dechunk(data), b"Wikipedia");
    }
}
