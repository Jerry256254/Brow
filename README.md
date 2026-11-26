# KucLab Brow

A smooth, modern, **bloat-free browser with its own rendering engine** and a built-in **ad blocker**.

Everything you see is rendered by `brow-engine` — our own HTML parser, CSS cascade,
layout engine, software rasterizer and PNG encoder — written in pure Rust with
**zero external dependencies**. No Blink, no WebKit, no bloat.

## Features (v0.1)

- Own HTML parser (elements, attributes, entities, void/raw-text elements, implied end tags)
- Own CSS parser + cascade (UA sheet, author sheets, inline styles, specificity)
- Own layout engine: block + inline boxes, word wrap, text-align, margins/padding/borders
- Own software rasterizer with a built-in bitmap font (no system fonts needed)
- Own PNG encoder (zlib stored blocks) — screenshots without any image crate
- Own HTTP/1.1 client (redirects, chunked encoding) + `file://` support
- Own ad blocker: hosts-style entries, `||domain^` anchored patterns, `*/ads/*` wildcards, `@@` exceptions
- Terminal shell: tabs, address bar, scrolling, screenshots

## Build & run

```sh
cargo build --release

# interactive terminal browser
./target/release/brow https://example.com

# headless helpers
./target/release/brow --screenshot https://example.com shot.png --width 480 --height 600 --scale 2
./target/release/brow --dump-dom https://example.com
./target/release/brow --dump-layout https://example.com --width 80
./target/release/brow --fetch https://example.com

# try the bundled demo page locally (ads.js / ads/banner.png get blocked)
./target/release/brow --screenshot "file://$PWD/demo/site/index.html" demo.png --width 320 --height 400 --scale 2
```

Ad-block filters default to `./filters/blocklist.txt` (override with `--filters PATH`,
disable with `--no-adblock`).

## Architecture

```
shell/    brow binary — terminal UI (tabs, URL bar) + CLI modes
engine/   brow-engine library
  html.rs     HTML tokenizer + tree builder (arena DOM)
  css.rs      CSS parser, selectors, colors
  style.rs    UA stylesheet + cascade -> computed style
  layout.rs   box tree + block/inline layout with word wrap
  font.rs     built-in 5x7 bitmap font
  paint.rs    RGBA framebuffer rasterizer
  png.rs      PNG encoder
  net.rs      HTTP/1.1 client, redirects, chunked, file://
  adblock.rs  filter-list engine (hosts, ||anchored, wildcards, @@exceptions)
demo/     local demo site
filters/  default blocklist
```

## Roadmap

1. Packaging, in this order: **.rpm → .apk → .exe → .deb**
2. HTTPS via a pluggable TLS transport (rustls) — transport only; rendering stays ours
3. Image decoding (PNG/JPEG) into the rasterizer
4. JavaScript engine (own interpreter)
5. GUI shell (native window) on top of the same engine

See `docs/ARCHITECTURE.md` for engine details.

## License

Apache-2.0 (see `LICENSE`).
