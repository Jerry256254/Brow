#!/usr/bin/env python3
"""Minimal RPM writer (no rpmbuild needed): lead + signature header + header +
gzip'd cpio (newc) payload. Produces a valid binary .rpm that `rpm -i` accepts.

Usage: mkrpm.py --name N --version V --release R --arch A --summary S --description D
               --license L --url U --out OUT.rpm STAGE_ROOT
STAGE_ROOT contains the installed tree (e.g. usr/bin/brow).
"""
import argparse, gzip, hashlib, io, os, struct, time

# header types
INT16, INT32, INT64, STRING, BIN, STRING_ARRAY, I18NSTRING = 3, 4, 5, 6, 7, 8, 9

def pad8(b):
    return b + b"\0" * ((8 - len(b) % 8) % 8)

def pack_header(entries):
    """entries: list of (tag, type, values). Returns header bytes (unpadded)."""
    entries = sorted(entries, key=lambda e: e[0])
    store = b""
    index = b""
    for tag, typ, vals in entries:
        if typ == INT32:
            data = b"".join(struct.pack(">I", v) for v in vals)
            count = len(vals)
        elif typ == INT16:
            data = b"".join(struct.pack(">H", v) for v in vals)
            count = len(vals)
        elif typ == INT64:
            data = b"".join(struct.pack(">Q", v) for v in vals)
            count = len(vals)
        elif typ == STRING:
            data = vals[0].encode() + b"\0"
            count = 1
        elif typ == I18NSTRING:
            data = b"".join(v.encode() + b"\0" for v in vals)
            count = len(vals)
        elif typ == STRING_ARRAY:
            data = b"".join(v.encode() + b"\0" for v in vals)
            count = len(vals)
        elif typ == BIN:
            data = vals[0]
            count = len(vals[0])
        else:
            raise ValueError(typ)
        offset = len(store)
        index += struct.pack(">IIII", tag, typ, offset, count)
        store += data
    n = len(entries)
    return b"\x8e\xad\xe8\x01" + b"\0" * 4 + struct.pack(">II", n, len(store)) + index + store

def cpio_newc(entries):
    """entries: list of (path, mode, data-bytes-or-None-for-dir)."""
    out = io.BytesIO()
    ino = 1
    for path, mode, data in entries:
        name = path.encode() + b"\0"
        data = data or b""
        nlink = 2 if (mode & 0o170000) == 0o040000 else 1
        fields = (ino, mode, 0, 0, nlink, int(time.time()), len(data), 3, 1, 0, 0, len(name), 0)
        hdr = b"070701" + b"".join(format(v, "08x").encode() for v in fields)
        rec = hdr + name
        rec += b"\0" * ((4 - len(rec) % 4) % 4)
        rec += data
        rec += b"\0" * ((4 - len(rec) % 4) % 4)
        out.write(rec)
        ino += 1
    # trailer
    name = b"TRAILER!!!\0"
    hdr = b"070701" + b"".join(format(v, "08x").encode() for v in (0, 0, 0, 0, 1, 0, 0, 3, 1, 0, 0, len(name), 0))
    rec = hdr + name
    rec += b"\0" * ((4 - len(rec) % 4) % 4)
    out.write(rec)
    out.write(b"\0" * 1024)
    return out.getvalue()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--release", default="1")
    ap.add_argument("--arch", default="x86_64")
    ap.add_argument("--summary", required=True)
    ap.add_argument("--description", default="")
    ap.add_argument("--license", default="Apache-2.0")
    ap.add_argument("--url", default="")
    ap.add_argument("--packager", default="KucLab <dev@kucclab.local>")
    ap.add_argument("--out", required=True)
    ap.add_argument("stage_root")
    args = ap.parse_args()

    # collect files and dirs from the staging root
    paths = []  # (relpath, mode, data)
    for root, dirs, files in os.walk(args.stage_root):
        dirs.sort()
        for d in dirs:
            full = os.path.join(root, d)
            rel = os.path.relpath(full, args.stage_root)
            paths.append((rel, 0o040755, None))
        for f in sorted(files):
            full = os.path.join(root, f)
            rel = os.path.relpath(full, args.stage_root)
            mode = 0o100755 if os.access(full, os.X_OK) else 0o100644
            with open(full, "rb") as fh:
                paths.append((rel, mode, fh.read()))
    paths.sort(key=lambda p: p[0])

    # cpio entries: dirs first as ./usr etc, then files
    cpio_entries = []
    for rel, mode, data in paths:
        cpio_entries.append(("./" + rel, mode, data))
    payload_plain = cpio_newc(cpio_entries)
    payload = gzip.compress(payload_plain, compresslevel=9, mtime=0)

    # main header
    basenames, dirnames, dirindexes = [], [], []
    dirset = {}
    for rel, mode, data in paths:
        d, b = os.path.split(rel)
        if d not in dirset:
            dirset[d] = len(dirnames)
            dirnames.append(d + "/")
        dirindexes.append(dirset[d])
        basenames.append(b)
    filesizes = [len(p[2]) if p[2] is not None else 0 for p in paths]
    filemodes = [p[1] for p in paths]
    filedigests = [hashlib.md5(p[2]).hexdigest() if p[2] is not None else "" for p in paths]
    fileflags = [0] * len(paths)
    filelinktos = [""] * len(paths)
    now = int(time.time())
    total_size = sum(filesizes)

    header_entries = [
        (1000, STRING, [args.name]),
        (1001, STRING, [args.version]),
        (1002, STRING, [args.release]),
        (1004, I18NSTRING, [args.summary]),
        (1005, I18NSTRING, [args.description]),
        (1006, INT32, [now]),
        (1007, STRING, ["kucclab-brow-packaging"]),
        (1009, INT32, [total_size]),
        (1014, STRING, [args.license]),
        (1015, STRING, [args.packager]),
        (1016, STRING, ["Applications/Internet"]),
        (1020, STRING, [args.url]),
        (1021, STRING, ["linux"]),
        (1022, STRING, [args.arch]),
        (1028, INT32, filesizes),
        (1030, INT16, filemodes),
        (1034, INT32, [now] * len(paths)),
        (1035, STRING_ARRAY, filedigests),
        (1036, STRING_ARRAY, filelinktos),
        (1037, INT32, fileflags),
        (1116, INT32, dirindexes),
        (1117, STRING_ARRAY, basenames),
        (1118, STRING_ARRAY, dirnames),
        (1124, STRING, ["cpio"]),
        (1125, STRING, ["gzip"]),
        (1126, STRING, ["9"]),
        (5012, INT32, [1]),  # md5 digests
    ]
    header = pack_header(header_entries)

    # signature header: SIZE (1000) + MD5 (1004) over header+payload
    sig_entries = [
        (1000, INT32, [len(header) + len(payload)]),
        (1004, BIN, [hashlib.md5(header + payload).digest()]),
    ]
    sig_header = pad8(pack_header(sig_entries))

    # lead
    lead = struct.pack(">4sBBHH", b"\xed\xab\xee\xdb", 3, 0, 0, 1)
    name = f"{args.name}-{args.version}-{args.release}".encode()[:65]
    lead += name + b"\0" * (66 - len(name))
    lead += struct.pack(">HH", 1, 5)  # osnum=linux, sigtype=HEADERSIG
    lead += b"\0" * 16
    assert len(lead) == 96

    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    with open(args.out, "wb") as fh:
        fh.write(lead + sig_header + header + payload)
    print(f"wrote {args.out} ({os.path.getsize(args.out)} bytes, {len(paths)} entries)")

if __name__ == "__main__":
    main()
