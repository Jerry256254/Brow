//! KucLab Brow engine — a from-scratch, zero-dependency rendering engine:
//! HTML parser, CSS parser + cascade, layout, software rasterizer, PNG encoder,
//! HTTP/1.1 client and ad-block filter engine.

pub mod adblock;
pub mod css;
pub mod font;
pub mod html;
pub mod layout;
pub mod net;
pub mod paint;
pub mod png;
pub mod style;

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
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Net(e) => write!(f, "{}", e),
            EngineError::Blocked(u) => write!(f, "request blocked by ad blocker: {}", u),
            EngineError::Http(s) => write!(f, "HTTP error {}", s),
            EngineError::Io(e) => write!(f, "io error: {}", e),
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
}

pub fn normalize_url(input: &str) -> String {
    let t = input.trim();
    if t.contains("://") {
        t.to_string()
    } else {
        format!("http://{}", t)
    }
}

impl Browser {
    pub fn new() -> Browser {
        Browser {
            adblock: AdBlock::load_from_str(""),
            blocked_count: 0,
            max_redirects: 5,
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

    /// Load a URL: fetch, parse HTML, collect <style> and <link rel=stylesheet>,
    /// and count blocked subresources (script/img/link).
    pub fn open(&mut self, url: &str) -> Result<Page, EngineError> {
        let url = normalize_url(url);
        let resp = self.fetch_checked(&url)?;
        if resp.status >= 400 {
            return Err(EngineError::Http(resp.status));
        }
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
                        match self.fetch_checked(&abs) {
                            Ok(r) => sheets.push(css::parse_stylesheet(&r.text())),
                            Err(EngineError::Blocked(_)) => {}
                            Err(e) => return Err(e),
                        }
                    }
                }
            }
        }
        // count blocked subresources (scripts, images) without loading them
        for tag in ["script", "img", "iframe", "video", "audio", "source"] {
            for i in doc.find_all(tag) {
                if let Some(e) = doc.element(i) {
                    let attr = if tag == "img" || tag == "source" { "src" } else { "src" };
                    if let Some(src) = e.attr(attr) {
                        let abs = net::resolve_url(&resp.final_url, src);
                        if self.adblock.is_blocked(&abs) {
                            self.blocked_count += 1;
                        }
                    }
                }
            }
        }

        Ok(Page {
            url,
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
    /// `width*device_scale` x `height*device_scale` device px.
    pub fn render_page(&self, page: &Page, width: usize, height: usize, scroll_y: f32, device_scale: u32) -> (Framebuffer, LayoutBox) {
        let ds = device_scale.max(1);
        let root = self.layout_page(page, width as f32);
        let fb = paint::render(&root, width * ds as usize, height * ds as usize, scroll_y * ds as f32, ds);
        (fb, root)
    }

    pub fn screenshot(&mut self, url: &str, out_path: &str, width: usize, height: usize, scale: u32) -> Result<Page, EngineError> {
        let page = self.open(url)?;
        let (fb, _) = self.render_page(&page, width, height, 0.0, scale);
        png::write_png(out_path, &fb)?;
        Ok(page)
    }
}

impl Default for Browser {
    fn default() -> Self {
        Browser::new()
    }
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
