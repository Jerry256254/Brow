# KucLab Brow - engine architecture

The engine is **Brownie** (`engine/`, crate `brownie`) - a from-scratch, zero-dependency
rendering pipeline:

```
URL ──▶ net.rs (HTTP/1.1 | file://) ──▶ adblock.rs (filter check)
     ──▶ html.rs (tokenizer + tree builder) ──▶ DOM (arena)
     ──▶ css.rs / style.rs (parse + cascade) ──▶ Computed styles
     ──▶ layout.rs (box tree, block/inline layout, word wrap) ──▶ LayoutBox tree
     ──▶ paint.rs (software rasterizer, built-in font) ──▶ RGBA framebuffer
     ──▶ png.rs (PNG encoder) or terminal blit (shell)
```

## Pipeline stages

1. **Fetch** - `net::fetch` speaks HTTP/1.1 over raw `TcpStream` (redirects,
   chunked transfer decoding, `Connection: close`). `file://` reads local files.
   Before any connection, `adblock::AdBlock::is_blocked` is consulted; blocked
   requests are never sent and are counted (`Browser::blocked_count`).
2. **Parse** - `html::parse` builds an arena DOM (`Vec<Node>` + parent links),
   handling comments, doctype, attributes, entities, void elements, raw-text
   elements (`script`/`style`/...) and implied end tags (`<p>`, `<li>`, ...).
3. **Style** - `style::compute_style` cascades the built-in UA sheet, all author
   sheets (`<style>` blocks + `<link rel=stylesheet>`, fetched through the
   ad blocker) and inline `style` attributes, ordered by specificity.
   Supported properties: `display`, `color`, `background-color`, `width`,
   `height`, `margin`, `padding`, `border`, `border-color`, `border-width`,
   `font-size`, `font-weight`, `text-align`.
4. **Layout** - `layout::build` creates a `LayoutBox` tree: block boxes stack
   vertically; runs of inline content become anonymous blocks; text is wrapped
   greedily using the built-in font metrics (`font::text_width`).
5. **Paint** - `paint::render` rasterizes backgrounds, borders and glyphs into an
   RGBA `Framebuffer` at an integer device scale.
6. **Output** - `png::write_png` encodes the framebuffer as PNG (own zlib stored
   blocks + CRC32/Adler32); the shell blits it to the terminal with truecolor
   half-block characters.

## Performance notes

- UA stylesheet is parsed once per process (`style::ua_stylesheet`, `OnceLock`).
- Computed styles are memoized per element during a layout pass
  (`layout::StyleCache`) - each element is styled exactly once.
- `Framebuffer::fill_rect` writes whole row spans instead of per-pixel calls.
- `paint::walk` culls subtrees outside the viewport, so painting a huge page
  costs only the visible part (~0.7 ms for a 1 MB page at 320x2000).
- `brow --bench URL` reports fetch/parse/layout/paint per-op timings.

## Security limits

| limit | value | where |
|---|---|---|
| URL length | 4096 B | `net::MAX_URL_LEN` |
| response headers | 64 KB | `net::MAX_HEADER_BYTES` |
| response body | 32 MB | `net::MAX_BODY_BYTES` (also chunked) |
| redirects | 5 | `net::MAX_REDIRECTS` |
| DOM depth | 512 | `html::MAX_DEPTH` |
| CSS rules / sheet | 10 000 | `css::MAX_RULES` |
| adblock rules / pattern | 250 000 / 1 KB | `adblock::MAX_RULES`, `MAX_PATTERN_LEN` |
| framebuffer | 64 Mpx | `MAX_FRAMEBUFFER_PIXELS` |
| timeouts | 10 s connect / 15 s IO | `net::CONNECT_TIMEOUT`, `IO_TIMEOUT` |

Plus: same-origin policy for subresources, no https→http redirect downgrade,
adblock checked before connecting, linear-time entity decoding (no `&&&&…`
bombs). HTTPS (rustls, webpki roots) is behind the `tls` cargo feature.

## Ad blocker filter syntax

- `0.0.0.0 ads.example.com` / `127.0.0.1 tracker.io` - hosts-style (host + subdomains)
- `||ads.example.com^` - domain-anchored (`^` = separator or end); optional path
  pattern after the domain, e.g. `||127.0.0.1/ads.js`
- `*/ads/*`, `*/banner.js` - wildcard patterns matched against the whole URL
- `@@||example.com/allowed.js` - exception (wins over block rules)
- `!` / `[Adblock Plus 2.0]` - comments/headers, ignored

## Deliberate v0.1 scope

- `http://` and `file://` only; `https://` returns a clear error until the TLS
  transport lands (roadmap item, right after packaging).
- No JavaScript, no images yet; `<script>`/`<img>` are fetched only to be
  blocked/counted.
- Layout is a simplified box model (no floats, no flex/grid, no margin collapsing).
