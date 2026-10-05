"""Fetch acadrust (the DWG writer we use) from crates.io, verify it, and apply our patches.

Why patches:
  1. acadrust 0.5.5 encodes DWG bit-doubles with `value == 0.0` and `value == 1.0`
     tests. In IEEE-754 `-0.0 == 0.0`, so every negative zero is written as positive
     zero, and obj2cad promises bit-for-bit coordinates (Blender writes `-0.000000` all
     the time). The fix compares bit patterns instead.
  2. It writes an empty SummaryInfo section whatever the document holds, so drawing
     properties (DWGPROPS: custom properties such as `obj2cad.parity`) are lost. The fix
     writes the document's summary in the layout acadrust's own reader parses.
Both should go upstream; until then this script produces the patched source in
`vendor/acadrust` (git-ignored, except its Cargo.toml so Dependabot can resolve the patch), which the workspace uses through `[patch.crates-io]` in
the root Cargo.toml.

acadrust is MPL-2.0: the modified files keep their license, and the modifications are
exactly the PATCHES below.

Usage:  python tools/vendor/fetch_acadrust.py      (idempotent; offline once fetched)
"""

from __future__ import annotations

import hashlib
import io
import json
import shutil
import sys
import tarfile
import urllib.request
from pathlib import Path

CRATE, VERSION = "acadrust", "0.5.5"
# From Cargo.lock: the checksum crates.io publishes for this exact version.
SHA256 = "6298485f7afd00af7880f285f01ab387143a1fbb20c42f9048830c95b19dda5d"
URL = f"https://static.crates.io/crates/{CRATE}/{CRATE}-{VERSION}.crate"

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / "vendor" / CRATE
MARKER = DEST / ".obj2cad-patched.json"

BIT_WRITER = "src/io/dwg/dwg_stream_writers/bit_writer.rs"
DWG_WRITER = "src/io/dwg/dwg_writer.rs"
# (file, exact old text, new text[, occurrences]). Old text must occur exactly as often
# as stated, so a changed upstream fails loudly instead of half-patching.
PATCHES = [
    (
        BIT_WRITER,
        "        if value == 0.0 {\n            self.write_2bits(2);\n        } else if value == 1.0 {\n            self.write_2bits(1);",
        "        // obj2cad patch: compare bits, so -0.0 is written as a full double instead of\n"
        "        // collapsing into the +0.0 short code (IEEE-754: -0.0 == 0.0).\n"
        "        if value.to_bits() == 0.0f64.to_bits() {\n            self.write_2bits(2);\n"
        "        } else if value.to_bits() == 1.0f64.to_bits() {\n            self.write_2bits(1);",
    ),
    (
        BIT_WRITER,
        "    pub fn write_bit_double_with_default(&mut self, def: f64, value: f64) {\n        if def == value {",
        "    pub fn write_bit_double_with_default(&mut self, def: f64, value: f64) {\n"
        "        // obj2cad patch: bit equality, so -0.0 never reuses a +0.0 default.\n"
        "        if def.to_bits() == value.to_bits() {",
    ),
    (
        DWG_WRITER,
        """fn build_summary_info(version: DxfVersion) -> Vec<u8> {
    let mut data = Vec::with_capacity(128);
    let is_utf16 = version >= DxfVersion::AC1021;

    // 8 × empty strings
    // Title, Subject, Author, Keywords, Comments, LastSavedBy, RevisionNumber, HyperlinkBase
    for _ in 0..8 {
        data.extend_from_slice(&1u16.to_le_bytes()); // char/byte count including null
        if is_utf16 {
            // UTF-16LE null terminator: 2 bytes
            data.push(0);
            data.push(0);
        } else {
            // ANSI null terminator: 1 byte
            data.push(0);
        }
    }
""",
        """fn build_summary_info(version: DxfVersion, info: &crate::document::SummaryInfo) -> Vec<u8> {
    // obj2cad patch: write the document's summary and custom properties (DWGPROPS)
    // instead of empty fields, in the layout `parse_summary_info` reads back.
    let mut data = Vec::with_capacity(128);
    let is_utf16 = version >= DxfVersion::AC1021;
    let t16 = |data: &mut Vec<u8>, s: &str| {
        let max = u16::MAX as usize - 1;
        if is_utf16 {
            let units: Vec<u16> = s.encode_utf16().take(max).chain(std::iter::once(0)).collect();
            data.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for unit in units {
                data.extend_from_slice(&unit.to_le_bytes());
            }
        } else {
            let bytes: Vec<u8> = s.chars().take(max).map(|c| if c.is_ascii() { c as u8 } else { b'?' }).chain(std::iter::once(0)).collect();
            data.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            data.extend_from_slice(&bytes);
        }
    };
    for s in [&info.title, &info.subject, &info.author, &info.keywords, &info.comments, &info.last_saved_by, &info.revision_number, &info.hyperlink_base] {
        t16(&mut data, s);
    }
""",
    ),
    (
        DWG_WRITER,
        """    // Property count: Int16 (0)
    data.extend_from_slice(&0u16.to_le_bytes());
""",
        """    // Property count: Int16, then the name/value pairs.
    let properties = &info.custom_properties[..info.custom_properties.len().min(u16::MAX as usize)];
    data.extend_from_slice(&(properties.len() as u16).to_le_bytes());
    for (name, value) in properties {
        t16(&mut data, name);
        t16(&mut data, value);
    }
""",
    ),
    (DWG_WRITER, "build_summary_info(version);", "build_summary_info(version, &document.summary_info);", 2),
    (DWG_WRITER, "build_summary_info(DxfVersion::AC1018);", "build_summary_info(DxfVersion::AC1018, &Default::default());"),
    (DWG_WRITER, "build_summary_info(DxfVersion::AC1021);", "build_summary_info(DxfVersion::AC1021, &Default::default());"),
]
STAMP = hashlib.sha256(json.dumps([SHA256, PATCHES]).encode()).hexdigest()


def main() -> int:
    if MARKER.exists() and json.loads(MARKER.read_text()).get("stamp") == STAMP:
        print(f"{DEST.relative_to(ROOT)} is up to date")
        return 0
    print(f"fetching {URL}")
    with urllib.request.urlopen(URL, timeout=120) as r:
        data = r.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != SHA256:
        print(f"checksum mismatch: expected {SHA256}, got {digest}", file=sys.stderr)
        return 1
    if DEST.exists():
        shutil.rmtree(DEST)
    DEST.parent.mkdir(parents=True, exist_ok=True)
    prefix = f"{CRATE}-{VERSION}/"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        for member in tar.getmembers():
            if not member.name.startswith(prefix) or ".." in Path(member.name).parts:
                continue
            rel = member.name[len(prefix):]
            if not rel or rel.startswith(("tests/", "src/docs/", "benches/")):
                continue  # not needed to build the library
            target = DEST / rel
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(tar.extractfile(member).read())
    for rel, old, new, *count in PATCHES:
        path = DEST / rel
        text = path.read_text(encoding="utf-8")
        expected = count[0] if count else 1
        if text.count(old) != expected:
            print(f"patch target found {text.count(old)}x (expected {expected}) in {rel}; acadrust changed?", file=sys.stderr)
            return 1
        path.write_text(text.replace(old, new), encoding="utf-8", newline="\n")
    MARKER.write_text(json.dumps({"crate": CRATE, "version": VERSION, "sha256": SHA256, "stamp": STAMP, "patches": len(PATCHES)}, indent=1))
    print(f"patched {CRATE} {VERSION} into {DEST.relative_to(ROOT)} ({len(PATCHES)} patches)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
