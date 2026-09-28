"""Stream-extract the first models of an ABC dataset 7z chunk without storing the archive.

The NYU archive server does not support HTTP range requests and a single chunk is
several GB, so we stream it once:

* the raw LZMA2 stream(s) starting at offset 32 are decoded on the fly, and only the
  first ``--max-mb`` of decoded output is written to ``OUT_DIR/_prefix.bin``;
* the tail of the archive (where 7z keeps its header) is kept in memory, and the parsed
  file listing is saved to ``OUT_DIR/_listing.json``;
* the prefix is then trimmed to exactly the first ``--models`` model folders and split
  into files, each verified against the CRC32 stored in the archive.

Disk safety: nothing is written if it would leave less than ``--reserve-gb`` free, and the
prefix is truncated *before* splitting so peak usage is about twice the kept data.

Usage:
    python abc_stream.py URL OUT_DIR [--max-mb 600] [--models 200]
    python abc_stream.py URL OUT_DIR --reuse-prefix   # prefix exists; fetch listing only
"""

from __future__ import annotations

import argparse
import io
import json
import lzma
import os
import shutil
import struct
import sys
import time
import urllib.request
import zlib
from pathlib import Path

import py7zr

SIGNATURE = b"7z\xbc\xaf\x27\x1c"
CHUNK = 1 << 20
TAIL_KEEP = 64 << 20


class SparseArchive(io.RawIOBase):
    """Read-only view of an archive where only the signature header and the tail are known."""

    def __init__(self, head: bytes, tail: bytes, total: int):
        self.head, self.tail, self.total = head, tail, total
        self.tail_start = total - len(tail)
        self.pos = 0

    def readable(self):
        return True

    def seekable(self):
        return True

    def tell(self):
        return self.pos

    def seek(self, offset, whence=0):
        base = {0: 0, 1: self.pos, 2: self.total}[whence]
        self.pos = base + offset
        return self.pos

    def read(self, n=-1):
        if n is None or n < 0:
            n = self.total - self.pos
        end = min(self.pos + n, self.total)
        out = bytearray()
        p = self.pos
        while p < end:
            if p < len(self.head):
                take = min(end, len(self.head)) - p
                out += self.head[p : p + take]
            elif p >= self.tail_start:
                take = end - p
                out += self.tail[p - self.tail_start : p - self.tail_start + take]
            else:
                raise IOError(f"read of unknown region at {p}; increase TAIL_KEEP")
            p += take
        self.pos = end
        return bytes(out)

    def readinto(self, b):
        data = self.read(len(b))
        b[: len(data)] = data
        return len(data)


def new_decoder():
    return lzma.LZMADecompressor(lzma.FORMAT_RAW, filters=[{"id": lzma.FILTER_LZMA2, "dict_size": 1 << 30}])


def free_bytes(path: Path) -> int:
    return shutil.disk_usage(path).free


def stream(url: str, out_dir: Path, max_bytes: int, reuse_prefix: bool, reserve: int) -> list[dict]:
    """Download once; returns the archive listing. Writes the decoded prefix unless reusing."""
    out_dir.mkdir(parents=True, exist_ok=True)
    prefix_path = out_dir / "_prefix.bin"
    if reuse_prefix and not prefix_path.exists():
        raise SystemExit(f"--reuse-prefix but {prefix_path} is missing")
    req = urllib.request.Request(url, headers={"User-Agent": "obj2cad-corpus/1.0"})
    prefix = None if reuse_prefix else open(prefix_path, "wb")
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            head = resp.read(32)
            if head[:6] != SIGNATURE:
                raise SystemExit("not a 7z archive")
            next_off, next_size = struct.unpack("<QQ", head[12:28])
            total = 32 + next_off + next_size
            tail_start = max(32, total - TAIL_KEEP)
            tail = bytearray()
            pos, decoded, dec = 32, 0, new_decoder()
            t0, last = time.time(), 0.0
            while True:
                buf = resp.read(CHUNK)
                if not buf:
                    break
                if prefix is not None and decoded < max_bytes:
                    data = buf
                    while data and decoded < max_bytes:
                        out = dec.decompress(data)
                        if free_bytes(out_dir) - len(out) < reserve:
                            raise SystemExit("stopping: disk free space would drop below the reserve")
                        prefix.write(out)
                        decoded += len(out)
                        data = dec.unused_data if dec.eof else b""
                        if dec.eof:  # next folder starts a fresh LZMA2 stream
                            dec = new_decoder()
                    if decoded >= max_bytes:
                        prefix.close()
                        prefix = None
                if pos + len(buf) > tail_start:
                    tail += buf[max(0, tail_start - pos) :]
                pos += len(buf)
                now = time.time()
                if now - last > 15:
                    rate = pos / (now - t0) / 1e6
                    print(f"{pos / 1e9:6.2f}/{total / 1e9:.2f} GB  {rate:5.1f} MB/s  decoded {decoded / 1e6:.0f} MB", flush=True)
                    last = now
    finally:
        if prefix is not None:
            prefix.close()
    if pos != total:
        raise SystemExit(f"short download: {pos} of {total} bytes")

    archive = py7zr.SevenZipFile(SparseArchive(head, bytes(tail), total), mode="r")
    listing = [
        {"file": i.filename, "bytes": i.uncompressed, "crc32": i.crc32, "dir": i.is_directory}
        for i in archive.list()
    ]
    (out_dir / "_listing.json").write_text(json.dumps(listing))
    return listing


def split(listing: list[dict], out_dir: Path, models: int, reserve: int, max_file: int) -> None:
    prefix_path = out_dir / "_prefix.bin"
    avail = prefix_path.stat().st_size
    files = [f for f in listing if not f["dir"] and f["bytes"] > 0]

    # Files are stored in stream order; keep whole model folders only. Models with a file
    # over max_file are skipped (they still occupy their place in the stream).
    keep, offset, folders, skipped = [], 0, [], []
    for f in files:
        folder = f["file"].split("/", 1)[0]
        if folder in skipped:
            offset += f["bytes"]
            continue
        if folder not in folders:
            if len(folders) == models:
                break
            folders.append(folder)
        if offset + f["bytes"] > avail:
            folders.pop()  # incomplete folder
            keep = [k for k in keep if not k[0]["file"].startswith(folder + "/")]
            break
        if f["bytes"] > max_file:
            folders.remove(folder)
            skipped.append(folder)
            keep = [k for k in keep if not k[0]["file"].startswith(folder + "/")]
            offset += f["bytes"]
            continue
        keep.append((f, offset))
        offset += f["bytes"]
    if skipped:
        print(f"skipped {len(skipped)} models with a file over {max_file / 1e6:.0f} MB: {', '.join(skipped)}")
    needed = sum(f["bytes"] for f, _ in keep)
    end = max((o + f["bytes"] for f, o in keep), default=0)
    largest = max((f["bytes"] for f, _ in keep), default=0)

    # Work from the last file backwards, truncating the prefix after each one, so the
    # extra space needed at any moment is a single file, not the whole selection.
    os.truncate(prefix_path, end)
    if free_bytes(out_dir) - largest < reserve:
        raise SystemExit(f"a {largest / 1e6:.0f} MB file would drop free space below the reserve")
    index = []
    for f, o in reversed(keep):
        with open(prefix_path, "rb") as prefix:
            prefix.seek(o)
            data = prefix.read(f["bytes"])
        crc = zlib.crc32(data)
        if f["crc32"] is not None and crc != f["crc32"]:
            raise SystemExit(f"CRC mismatch for {f['file']}: stream order assumption broken")
        dest = out_dir / f["file"]
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        os.truncate(prefix_path, o)
        index.append({"file": f["file"], "bytes": f["bytes"], "crc32": f"{crc:08x}"})
    index.reverse()
    prefix_path.unlink()
    (out_dir / "_index.json").write_text(json.dumps(index, indent=1))
    print(f"extracted {len(index)} files from {len(folders)} models ({needed / 1e6:.0f} MB, CRC verified)")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("url")
    ap.add_argument("out_dir", type=Path)
    ap.add_argument("--max-mb", type=int, default=600, help="decoded bytes to keep while streaming")
    ap.add_argument("--models", type=int, default=200, help="number of model folders to extract")
    ap.add_argument("--reuse-prefix", action="store_true", help="prefix already on disk: only fetch the listing")
    ap.add_argument("--max-file-mb", type=int, default=60, help="skip models with a larger file")
    ap.add_argument("--no-split", action="store_true", help="only download and save the listing")
    ap.add_argument("--reserve-gb", type=float, default=1.0, help="never let free disk space drop below this")
    a = ap.parse_args()
    reserve = int(a.reserve_gb * 1e9)
    listing_path = a.out_dir / "_listing.json"
    if a.reuse_prefix and listing_path.exists():
        listing = json.loads(listing_path.read_text())
    else:
        listing = stream(a.url, a.out_dir, a.max_mb << 20, a.reuse_prefix, reserve)
    if not a.no_split:
        split(listing, a.out_dir, a.models, reserve, a.max_file_mb << 20)
    sys.exit(0)
