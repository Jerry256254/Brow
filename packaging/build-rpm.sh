#!/bin/sh
# Build the KucLab Brow .rpm.
# Prefers rpmbuild (packaging/kucclab-brow.spec) when available, otherwise
# falls back to the dependency-free packaging/mkrpm.py writer.
set -eu
cd "$(dirname "$0")/.."

echo "==> cargo build --release (with tls)"
cargo build --release -p brow --features tls

STAGE=build/rpm-root
rm -rf build dist
mkdir -p "$STAGE/usr/bin" "$STAGE/usr/share/doc/kucclab-brow" "$STAGE/usr/share/kucclab-brow" "$STAGE/usr/share/applications" dist
install -m 0755 target/release/brow "$STAGE/usr/bin/brow"
install -m 0644 README.md LICENSE "$STAGE/usr/share/doc/kucclab-brow/"
install -m 0644 filters/blocklist.txt "$STAGE/usr/share/kucclab-brow/blocklist.txt"
install -m 0644 packaging/kucclab-brow.desktop "$STAGE/usr/share/applications/kucclab-brow.desktop"

OUT="dist/kucclab-brow-0.1.0-1.x86_64.rpm"
if command -v rpmbuild >/dev/null 2>&1; then
    echo "==> rpmbuild"
    rpmbuild -bb packaging/kucclab-brow.spec --define "_topdir $PWD/build" --buildroot "$PWD/$STAGE"
    cp build/RPMS/x86_64/kucclab-brow-0.1.0-1.*.rpm "$OUT"
else
    echo "==> rpmbuild not found, using packaging/mkrpm.py"
    python3 packaging/mkrpm.py \
        --name kucclab-brow --version 0.1.0 --release 1 --arch x86_64 \
        --summary "KucLab Brow - a small browser with its own engine (Brownie) and an ad blocker" \
        --description "KucLab Brow renders every page with Brownie, its own HTML/CSS/layout/rasterizer engine in pure Rust. Built-in ad blocker, HTTP/1.1 + HTTPS (rustls), terminal UI. No bloat." \
        --license "Apache-2.0" --url "https://github.com/Jerry256254/Brow" \
        --out "$OUT" "$STAGE"
fi
sha256sum "$OUT" | tee "$OUT.sha256"
echo "==> done: $OUT"
