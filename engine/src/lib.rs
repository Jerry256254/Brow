//! Brownie - the from-scratch, zero-dependency rendering engine of KucLab Brow:
//! HTML parser, CSS parser + cascade, layout, software rasterizer, PNG encoder,
//! HTTP/1.1 client and ad-block filter engine. Hardened with resource limits;
//! optional `tls` feature adds HTTPS via rustls.

pub mod adblock;
pub mod css;
pub mod font;
pub mod html;
pub mod layout;
pub mod net;
pub mod paint;
pub mod png;
pub mod style;
pub mod theme;

use std::fmt;

pub use adblock::AdBlock;
pub use css::Stylesheet;
pub use html::Document;
pub use layout::LayoutBox;
pub use net::Response;
pub use paint::Framebuffer;

#[derive(Debug)]
pub enum EngineError {
    Net(net::FetchError),
    Blocked(String),
    Http(u16),
    Io(std::io::Error),
    TooLarge(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Net(e) => write!(f, "{}", e),
            EngineError::Blocked(u) => write!(f, "request blocked by ad blocker: {}", u),
            EngineError::Http(s) => write!(f, "HTTP error {}", s),
            EngineError::Io(e) => write!(f, "io error: {}", e),
            EngineError::TooLarge(m) => write!(f, "too large: {}", m),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<net::FetchError> for EngineError {
    fn from(e: net::FetchError) -> Self {
        EngineError::Net(e)
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        EngineError::Io(e)
    }
}

/// Max framebuffer size in pixels (anti memory-exhaustion on huge viewports).
pub const MAX_FRAMEBUFFER_PIXELS: usize = 64 * 1024 * 1024;

/// A loaded page: DOM + author stylesheets + metadata.
pub struct Page {
    pub url: String,
    pub final_url: String,
    pub title: String,
    pub status: u16,
    pub doc: Document,
    pub sheets: Vec<Stylesheet>,
}

/// The browser: owns the ad blocker and fetches/renders pages.
pub struct Browser {
    pub adblock: AdBlock,
    pub blocked_count: u64,
    pub max_redirects: usize,
    /// Cross-origin subresources are not fetched (same-origin policy).
    pub same_origin_only: bool,
}

pub fn normalize_url(input: &str) -> String {
    let t = input.trim();
    if t.contains("://") {
        t.to_string()
    } else {
        format!("http://{}", t)
    }
}

/// Same-origin policy: subresources may only come from the page's own origin.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (net::Url::parse(a), net::Url::parse(b)) {
        (Ok(x), Ok(y)) => x.same_origin(&y),
        _ => false,
    }
}

impl Browser {
    pub fn new() -> Browser {
        Browser {
            adblock: AdBlock::load_from_str(""),
            blocked_count: 0,
            max_redirects: net::MAX_REDIRECTS,
            same_origin_only: true,
        }
    }

    pub fn with_filters_file(path: &str) -> std::io::Result<Browser> {
        let mut b = Browser::new();
        b.adblock = AdBlock::load_from_file(path)?;
        Ok(b)
    }

    pub fn set_filters(&mut self, filters: AdBlock) {
        self.adblock = filters;
    }

    fn fetch_checked(&mut self, url: &str) -> Result<Response, EngineError> {
        if self.adblock.is_blocked(url) {
            self.blocked_count += 1;
            return Err(EngineError::Blocked(url.to_string()));
        }
        Ok(net::fetch(url, self.max_redirects)?)
    }

    /// Load a URL: fetch, parse HTML, collect <style> and <link rel=stylesheet>
    /// (same-origin only), and count blocked subresources (script/img/...).
    pub fn open(&mut self, url: &str) -> Result<Page, EngineError> {
        let url = normalize_url(url);
        let resp = self.fetch_checked(&url)?;
        if resp.status >= 400 {
            return Err(EngineError::Http(resp.status));
        }
        self.build_page(&url, &resp)
    }

    /// Load an in-memory HTML document (used for the built-in start page).
    pub fn open_source(&mut self, url: &str, html: &str) -> Result<Page, EngineError> {
        let resp = Response {
            status: 200,
            headers: vec![("content-type".to_string(), "text/html; charset=utf-8".to_string())],
            body: html.as_bytes().to_vec(),
            final_url: url.to_string(),
        };
        self.build_page(url, &resp)
    }

    fn build_page(&mut self, url: &str, resp: &Response) -> Result<Page, EngineError> {
        let is_html = resp.content_type().contains("html");
        let text = resp.text();
        let doc = if is_html { html::parse(&text) } else { html::parse(&format!("<html><head><title>raw</title></head><body><pre>{}</pre></body></html>", html_escape(&text))) };

        let title = doc
            .find_first("title")
            .map(|i| doc.text_content(i).trim().to_string())
            .unwrap_or_default();

        let mut sheets: Vec<Stylesheet> = Vec::new();
        for i in doc.find_all("style") {
            for &c in doc.children(i) {
                if let Some(t) = doc.text(c) {
                    sheets.push(css::parse_stylesheet(t));
                }
            }
        }
        for i in doc.find_all("link") {
            if let Some(e) = doc.element(i) {
                let rel = e.attr("rel").unwrap_or("").to_lowercase();
                if rel.contains("stylesheet") {
                    if let Some(href) = e.attr("href") {
                        let abs = net::resolve_url(&resp.final_url, href);
                        if abs.is_empty() {
                            continue;
                        }
                        if self.same_origin_only && !same_origin(&resp.final_url, &abs) {
                            continue;
                        }
                        match self.fetch_checked(&abs) {
                            Ok(r) => sheets.push(css::parse_stylesheet(&r.text())),
                            Err(EngineError::Blocked(_)) => {}
                            Err(e) => return Err(e),
                        }
                    }
                }
            }
        }
        // Count blocked subresources (scripts, images, ...) without loading them.
        for tag in ["script", "img", "iframe", "video", "audio", "source"] {
            for i in doc.find_all(tag) {
                if let Some(e) = doc.element(i) {
                    if let Some(src) = e.attr("src") {
                        let abs = net::resolve_url(&resp.final_url, src);
                        if abs.is_empty() {
                            continue;
                        }
                        if self.same_origin_only && !same_origin(&resp.final_url, &abs) {
                            continue;
                        }
                        if self.adblock.is_blocked(&abs) {
                            self.blocked_count += 1;
                        }
                    }
                }
            }
        }

        Ok(Page {
            url: url.to_string(),
            final_url: resp.final_url.clone(),
            title,
            status: resp.status,
            doc,
            sheets,
        })
    }

    pub fn layout_page(&self, page: &Page, viewport_width: f32) -> LayoutBox {
        layout::build(&page.doc, &page.sheets, viewport_width)
    }

    /// Render a page: `width`/`height` are CSS px (viewport), the framebuffer is
    /// `width*device_scale` x `height*device_scale` device px. Fails if the
    /// framebuffer would exceed MAX_FRAMEBUFFER_PIXELS.
    pub fn render_page(&self, page: &Page, width: usize, height: usize, scroll_y: f32, device_scale: u32) -> Result<(Framebuffer, LayoutBox), EngineError> {
        let ds = device_scale.max(1) as usize;
        let w = width.checked_mul(ds).ok_or_else(|| EngineError::TooLarge("width overflow".into()))?;
        let h = height.checked_mul(ds).ok_or_else(|| EngineError::TooLarge("height overflow".into()))?;
        let px = w.checked_mul(h).ok_or_else(|| EngineError::TooLarge("framebuffer overflow".into()))?;
        if px > MAX_FRAMEBUFFER_PIXELS {
            return Err(EngineError::TooLarge(format!(
                "framebuffer {}x{} = {} px > {} limit",
                w, h, px, MAX_FRAMEBUFFER_PIXELS
            )));
        }
        let root = self.layout_page(page, width as f32);
        let fb = paint::render(&root, w, h, scroll_y * ds as f32, ds as u32);
        Ok((fb, root))
    }

    pub fn screenshot(&mut self, url: &str, out_path: &str, width: usize, height: usize, scale: u32) -> Result<Page, EngineError> {
        let page = self.open(url)?;
        let (fb, _) = self.render_page(&page, width, height, 0.0, scale)?;
        png::write_png(out_path, &fb)?;
        Ok(page)
    }

    /// Micro-benchmark of the pipeline stages (fetch / parse / layout / paint).
    pub fn bench(&mut self, url: &str, width: usize, height: usize, iterations: usize) -> Result<BenchResult, EngineError> {
        use std::time::Instant;
        let url = normalize_url(url);
        let t0 = Instant::now();
        let resp = self.fetch_checked(&url)?;
        let fetch_ms = t0.elapsed().as_millis();
        let text = resp.text();
        let bytes = text.len();
        let n = iterations.max(1);

        let t = Instant::now();
        let mut doc = html::parse(&text);
        for _ in 1..n {
            doc = html::parse(&text);
        }
        std::hint::black_box(&doc);
        let parse_ns = t.elapsed().as_nanos() / n as u128;

        let page = self.open(&url)?;
        let t = Instant::now();
        let mut root = layout::build(&page.doc, &page.sheets, width as f32);
        for _ in 1..n {
            root = layout::build(&page.doc, &page.sheets, width as f32);
        }
        std::hint::black_box(&root);
        let layout_ns = t.elapsed().as_nanos() / n as u128;

        let t = Instant::now();
        for _ in 0..n {
            let fb = paint::render(&root, width, height, 0.0, 1);
            std::hint::black_box(&fb);
        }
        let paint_ns = t.elapsed().as_nanos() / n as u128;

        Ok(BenchResult {
            url,
            bytes,
            iterations: n,
            fetch_ms,
            parse_ns,
            layout_ns,
            paint_ns,
        })
    }
}

impl Default for Browser {
    fn default() -> Self {
        Browser::new()
    }
}

/// Results of `Browser::bench`.
pub struct BenchResult {
    pub url: String,
    pub bytes: usize,
    pub iterations: usize,
    pub fetch_ms: u128,
    pub parse_ns: u128,
    pub layout_ns: u128,
    pub paint_ns: u128,
}

impl fmt::Display for BenchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mbps = |ns: u128| -> f64 {
            if ns == 0 {
                0.0
            } else {
                self.bytes as f64 / (ns as f64 / 1e9) / 1e6
            }
        };
        write!(
            f,
            "bench {} ({} bytes, {} iterations)\n  fetch:  {} ms\n  parse:  {:>8.0} ns/op  ({:.1} MB/s)\n  layout: {:>8.0} ns/op  ({:.1} MB/s)\n  paint:  {:>8.0} ns/op",
            self.url,
            self.bytes,
            self.iterations,
            self.fetch_ms,
            self.parse_ns,
            mbps(self.parse_ns),
            self.layout_ns,
            mbps(self.layout_ns),
            self.paint_ns
        )
    }
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
