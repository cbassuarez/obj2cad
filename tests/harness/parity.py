"""Independent parity verification for obj2cad.

For every OBJ in the given folders this script:

1. runs the obj2cad CLI to produce a DXF and a report;
2. parses the OBJ with its *own* reader (written separately from the Rust parser);
3. reads the DXF back with ezdxf (an independent DXF implementation) and runs its audit;
4. computes the obj2cad-parity-v1 hash (see crates/obj2cad-core/src/hash.rs) from the
   OBJ and from the DXF, and requires both to equal the hash in the Rust report.

Files under a folder named ``invalid`` must be rejected by the CLI.

Usage:
    python tests/harness/parity.py [--bin target/release/obj2cad] [--up as-is|y-to-z] DIR...
Exit status is non-zero if any file fails.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import subprocess
import sys
import tempfile
import time
from collections import Counter
from pathlib import Path

import ezdxf


# ---------------------------------------------------------------- independent OBJ reader
class ObjError(Exception):
    pass


def read_obj(path: Path):
    """Return (positions, faces, lines, points) with 0-based indices; only geometry."""
    data = path.read_bytes().decode("utf-8", errors="replace")
    # join backslash continuations, normalise newlines
    logical, buf = [], ""
    for raw in data.replace("\r\n", "\n").split("\n"):
        if raw.endswith("\\"):
            buf += raw[:-1] + " "
            continue
        logical.append(buf + raw)
        buf = ""
    if buf:
        logical.append(buf)

    pos: list[tuple[float, float, float]] = []
    faces, lines, points = [], [], []
    nvt = nvn = 0

    def ref(tok: str, n: int) -> int:
        i = int(tok)
        if i == 0:
            raise ObjError("index 0")
        r = i - 1 if i > 0 else n + i
        if not 0 <= r < n:
            raise ObjError(f"index {i} out of range")
        return r

    for line in logical:
        parts = line.split("#", 1)[0].split() if not line.lstrip().startswith("#") else []
        if not parts:
            continue
        kw, args = parts[0], parts[1:]
        if kw == "v":
            if len(args) < 3:
                raise ObjError("short vertex")
            xyz = tuple(float(a) for a in args[:3])
            if any(c != c or c in (float("inf"), float("-inf")) for c in xyz):
                raise ObjError("non-finite")
            pos.append(xyz)
        elif kw == "vt":
            nvt += 1
        elif kw == "vn":
            nvn += 1
        elif kw in ("f", "fo", "l", "p"):
            idx = []
            for a in args:
                comps = a.split("/")
                idx.append(ref(comps[0], len(pos)))
                if len(comps) > 1 and comps[1]:
                    ref(comps[1], nvt)
                if len(comps) > 2 and comps[2]:
                    ref(comps[2], nvn)
            if kw in ("f", "fo"):
                if len(idx) >= 3:
                    faces.append(idx)
            elif kw == "l":
                lines.append(idx)
            else:
                points.extend([i] for i in idx)
    return pos, faces, lines, points


def transform(p, up: str):
    x, y, z = p
    return (x, -z, y) if up == "y-to-z" else (x, y, z)


# ---------------------------------------------------------------- canonical hash
def record(vertices) -> bytes:
    return b"".join(struct.pack(">d", c) for v in vertices for c in v)


def parity_hash(faces, lines, points) -> str:
    h = hashlib.sha256(b"obj2cad-parity-v1\0")
    for tag, recs in ((b"f", faces), (b"l", lines), (b"p", points)):
        recs = sorted(recs)
        h.update(tag + struct.pack("<Q", len(recs)))
        for r in recs:
            h.update(struct.pack("<I", len(r)) + r)
    return h.hexdigest()


def hash_from_obj(path: Path, up: str) -> str:
    pos, faces, lines, points = read_obj(path)
    P = [transform(p, up) for p in pos]
    return parity_hash(
        [record(P[i] for i in f) for f in faces],
        [record(P[i] for i in l) for l in lines],
        [record(P[i] for i in p) for p in points],
    )


def hash_from_dxf(path: Path):
    doc = ezdxf.readfile(path)
    auditor = doc.audit()
    faces, lines, points = [], [], []
    per_layer = Counter()
    for e in doc.modelspace():
        t = e.dxftype()
        if t == "MESH":
            data = e.get_data()
            vs = [tuple(v) for v in data.vertices]
            for f in data.faces:
                faces.append(record(vs[i] for i in f))
            per_layer[e.dxf.layer] += len(data.faces)
        elif t == "POLYLINE":
            lines.append(record(tuple(v.dxf.location) for v in e.vertices))
        elif t == "POINT":
            points.append(record([tuple(e.dxf.location)]))
        else:
            raise AssertionError(f"unexpected entity {t}")
    header = {
        "insunits": doc.header.get("$INSUNITS"),
        "custom": dict(doc.header.custom_vars),
        "acadver": doc.header.get("$ACADVER"),
    }
    return parity_hash(faces, lines, points), auditor, per_layer, header


# ---------------------------------------------------------------- driver
def check(binary: Path, obj: Path, out_dir: Path, up: str, expect_invalid: bool) -> tuple[bool, str]:
    stem = f"{obj.parent.name}_{obj.stem}" if obj.stem == "model" else obj.stem
    dxf = out_dir / (stem + ".dxf")
    rep = out_dir / (stem + ".report.json")
    t0 = time.perf_counter()
    proc = subprocess.run(
        [str(binary), "convert", str(obj), "-o", str(dxf), "--report", str(rep), "--up", up],
        capture_output=True, text=True, encoding="utf-8",
    )
    elapsed = time.perf_counter() - t0
    if expect_invalid:
        if proc.returncode == 0:
            return False, "accepted an invalid file"
        try:
            read_obj(obj)
            return False, "independent reader accepted it (reader disagreement)"
        except (ObjError, ValueError):
            pass
        return True, "rejected: " + proc.stderr.strip().splitlines()[-1].split(": ", 1)[-1]
    if proc.returncode != 0:
        return False, "CLI failed: " + proc.stderr.strip()

    report = json.loads(rep.read_text(encoding="utf-8"))
    h_rust = report["parity_hash"]
    h_obj = hash_from_obj(obj, up)
    h_dxf, auditor, per_layer, header = hash_from_dxf(dxf)
    problems = []
    if h_obj != h_rust:
        problems.append("OBJ readers disagree")
    if h_dxf != h_rust:
        problems.append("DXF read-back differs from source")
    if auditor.has_errors:
        problems.append(f"ezdxf audit: {len(auditor.errors)} errors")
    if header["custom"].get("obj2cad.parity_hash") != h_rust:
        problems.append("custom property parity hash missing/wrong")
    if header["acadver"] != "AC1032":
        problems.append(f"unexpected version {header['acadver']}")
    if sum(per_layer.values()) != report["output"]["faces"]:
        problems.append("face count mismatch")
    if problems:
        return False, "; ".join(problems)
    n_diag = len(report["diagnostics"])
    return True, f"{report['output']['faces']} faces, {len(per_layer)} layers, {n_diag} notes, {elapsed:.2f}s"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+", type=Path)
    ap.add_argument("--bin", type=Path, default=Path("target/release/obj2cad"))
    ap.add_argument("--up", default="as-is", choices=["as-is", "y-to-z"])
    ap.add_argument("--keep", type=Path, help="keep outputs in this folder")
    a = ap.parse_args()

    binary = a.bin if a.bin.exists() else a.bin.with_suffix(".exe")
    objs = sorted(p for d in a.dirs for p in d.rglob("*.obj"))
    if not objs:
        print("no .obj files found", file=sys.stderr)
        return 2
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        out = a.keep or Path(tmp)
        out.mkdir(parents=True, exist_ok=True)
        for obj in objs:
            ok, msg = check(binary, obj, out, a.up, "invalid" in obj.parts)
            failures += not ok
            label = f"{obj.parent.name}/{obj.name}" if obj.stem == "model" else obj.name
            print(f"{'PASS' if ok else 'FAIL'}  {label:<40} {msg}", flush=True)
    print(f"\n{len(objs) - failures}/{len(objs)} passed ({a.up})")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
