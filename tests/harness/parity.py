"""Independent parity verification for obj2cad.

For every OBJ in the given folders this script:

1. runs the obj2cad CLI to produce a DXF (ASCII or binary) or DWG, and a report;
2. parses the OBJ with its *own* reader (written separately from the Rust parser);
3. reads the DXF back with ezdxf (an independent DXF implementation) and runs its audit,
   or the DWG with acadrust's reader (`obj2cad dwg-dump`, exact bit patterns; acadrust's
   reader shares no code with obj2cad's writer);
4. checks **geometry**: the obj2cad-parity-v1 hash (see crates/obj2cad-core/src/hash.rs)
   computed from the OBJ and from the DXF must both equal the hash in the Rust report;
5. checks **structure**: every face, line and point must be on the expected layer with the
   expected color, computed here from the OBJ and MTL with independently written rules.

Files under a folder named ``invalid`` must be rejected by the CLI *and* by this reader.

Usage:
    python tests/harness/parity.py [--bin target/release/obj2cad] [--up auto|as-is|y-to-z]
                                   [--format dxf|dxf-binary|dwg] [--keep DIR] DIR...
Exit status is non-zero if any file fails.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
import subprocess
import sys
import tempfile
import time
import unicodedata
from collections import Counter
from pathlib import Path

import ezdxf


class ObjError(Exception):
    pass


# ---------------------------------------------------------------- independent OBJ reader
KEYWORDS = {
    "v", "vt", "vn", "vp", "f", "fo", "l", "p", "o", "g", "s", "usemtl", "mtllib",
    "cstype", "deg", "bmat", "step", "curv", "curv2", "surf", "parm", "trim", "hole", "scrv", "sp", "end", "con", "mg",
    "usemap", "maplib", "lod", "bevel", "c_interp", "d_interp", "shadow_obj", "trace_obj", "ctech", "stech",
}


def read_obj(path: Path) -> dict:
    """Geometry and naming state of an OBJ, as this harness understands the spec."""
    raw = path.read_bytes()
    if raw.startswith((b"\xff\xfe", b"\xfe\xff")):
        raise ObjError("UTF-16")
    text = raw.decode("utf-8-sig", errors="replace")  # -sig: a BOM is not content
    if "\n" not in text and "\r" in text:
        lines_raw = text.split("\r")  # classic Mac
    else:
        lines_raw = text.replace("\r\n", "\n").split("\n")
    logical, buf = [], ""
    for raw_line in lines_raw:
        if raw_line.endswith("\\"):
            buf += raw_line[:-1] + " "
            continue
        logical.append(buf + raw_line)
        buf = ""
    if buf:
        logical.append(buf)

    pos: list[tuple[float, float, float]] = []
    elements = []  # (kind, [vertex indices], object, group, material)
    nvt = nvn = 0
    forward = []
    obj = grp = mat = None
    mtllibs = []

    def ref(tok: str, n: int, slot: int) -> int:
        i = int(tok)
        if i == 0:
            raise ObjError("index 0")
        if i < 0:
            r = n + i
            if r < 0:
                raise ObjError("negative index out of range")
            return r
        if i > n:
            forward.append((i - 1, slot))
        return i - 1

    for line in logical:
        stripped = line.strip(" \t\v\f")
        if not stripped or stripped.startswith("#"):
            continue
        body = stripped
        for k in range(1, len(body)):
            if body[k] == "#" and body[k - 1] in " \t":
                body = body[:k]
                break
        parts = body.split()
        kw, args = parts[0], parts[1:]
        if not all(c.isascii() and c.isprintable() and not c.isspace() for c in kw):
            visible = "".join(c for c in kw if c.isascii() and c.isprintable() and not c.isspace())
            if visible in KEYWORDS:
                raise ObjError("hidden characters before a keyword")
        if kw == "v":
            if len(args) < 3:
                raise ObjError("short vertex")
            xyz = tuple(float(a) for a in args[:3])
            if any(math.isnan(c) or math.isinf(c) for c in xyz):
                raise ObjError("non-finite")
            pos.append(xyz)
        elif kw == "vt":
            nvt += 1
        elif kw == "vn":
            nvn += 1
        elif kw == "o":
            obj, grp = line.split(None, 1)[1].strip() if len(parts) > 1 else "", None
        elif kw == "g":
            grp = args[0] if args else "default"
        elif kw == "usemtl":
            mat = line.split(None, 1)[1].strip() if len(parts) > 1 else ""
        elif kw == "mtllib":
            mtllibs += args if args and all(a.lower().endswith(".mtl") for a in args) else ([body.split(None, 1)[1]] if len(parts) > 1 else [])
        elif kw in ("f", "fo", "l", "p"):
            idx = []
            for a in args:
                comps = a.split("/")
                if len(comps) > 3 or not comps[0]:
                    raise ObjError("malformed reference")
                idx.append(ref(comps[0], len(pos), 0))
                if len(comps) > 1 and comps[1]:
                    ref(comps[1], nvt, 1)
                if len(comps) > 2 and comps[2]:
                    ref(comps[2], nvn, 2)
            kind = {"f": "f", "fo": "f", "l": "l", "p": "p"}[kw]
            if kind == "f" and len(idx) < 3:
                continue  # skipped (reported by the engine)
            if kind == "l" and len(idx) < 2 or kind == "p" and not idx:
                raise ObjError("element too short")
            elements.append((kind, idx, obj, grp, mat))
    totals = [len(pos), nvt, nvn]
    for i, slot in forward:
        if i >= totals[slot]:
            raise ObjError("forward reference out of range")
    return {"positions": pos, "elements": elements, "mtllibs": mtllibs}


def read_mtl(path: Path) -> dict[str, tuple[int, int, int]]:
    colors, current = {}, None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if line.startswith("newmtl") and (len(line) == 6 or line[6].isspace()):
            current = line[6:].strip()
        elif line.startswith("Kd") and current is not None:
            vals = []
            for t in line[2:].split():
                try:
                    vals.append(float(t))
                except ValueError:
                    pass
            if len(vals) == 3 and all(math.isfinite(v) for v in vals):
                # round half away from zero, like Rust's f64::round
                colors[current] = tuple(int(math.floor(min(max(c, 0.0), 1.0) * 255.0 + 0.5)) for c in vals)
    return colors


# ---------------------------------------------------------------- expected layers and colors
INVALID_NAME_CHARS = set('<>/\\":;?*|=`,')


def dxf_safe(name: str) -> str:
    s = "".join("_" if c in INVALID_NAME_CHARS or unicodedata.category(c) == "Cc" else c for c in name.strip())
    s = s[:255]
    return s or "unnamed"


def expected_structure(obj: dict, stem: str, up: str, palette: dict) -> list[bytes]:
    """(layer, color, geometry) records the DXF should contain, in obj2cad's default
    layer mode (objects; groups when the file has no objects)."""
    P = [transform(p, up) for p in obj["positions"]]
    has_objects = any(e[2] is not None for e in obj["elements"])
    taken, by_source, default = {"0"}, {}, [None]

    def unique(base: str) -> str:
        base = dxf_safe(base)
        name, n = base, 2
        while name.lower() in taken:
            name, n = f"{base}~{n}", n + 1
        taken.add(name.lower())
        return name

    def default_layer() -> str:
        if default[0] is None:
            default[0] = unique(stem.strip() or "OBJ")
        return default[0]

    def layer_of(o, g) -> str:
        source = o if has_objects else g
        if source is None or not source.strip():
            return default_layer()
        if source not in by_source:
            by_source[source] = unique(source)
        return by_source[source]

    records, used = [], set()
    for kind, idx, o, g, m in obj["elements"]:
        layer = layer_of(o, g)
        color = palette.get(m) if m is not None else None
        used.update(idx)
        if kind == "p":
            records += [structure_record(layer, color, "p", [P[i]]) for i in idx]
        else:
            records.append(structure_record(layer, color, kind, [P[i] for i in idx]))
    if not obj["elements"] and P:  # point cloud
        layer = default_layer()
        records += [structure_record(layer, None, "p", [p]) for p in P]
    return records


def structure_record(layer: str, color, kind: str, verts) -> bytes:
    c = "-" if color is None else "%02x%02x%02x" % tuple(color)
    return f"{layer}\0{c}\0{kind}\0".encode() + record(verts)


# ---------------------------------------------------------------- canonical geometry hash
def transform(p, up: str):
    x, y, z = p
    return (x, -z, y) if up == "y_up_to_z_up" else (x, y, z)


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


def hash_from_obj(obj: dict, up: str) -> str:
    P = [transform(p, up) for p in obj["positions"]]
    faces = [record(P[i] for i in idx) for k, idx, *_ in obj["elements"] if k == "f"]
    lines = [record(P[i] for i in idx) for k, idx, *_ in obj["elements"] if k == "l"]
    points = [record([P[i]]) for k, idx, *_ in obj["elements"] if k == "p" for i in idx]
    if not obj["elements"]:
        points = [record([p]) for p in P]  # point cloud
    return parity_hash(faces, lines, points)


def read_dxf(path: Path):
    doc = ezdxf.readfile(path)
    auditor = doc.audit()
    faces, lines, points, structure = [], [], [], []
    for e in doc.modelspace():
        t = e.dxftype()
        color = e.dxf.true_color if e.dxf.hasattr("true_color") else None
        rgb = None if color is None else ((color >> 16) & 255, (color >> 8) & 255, color & 255)
        layer = e.dxf.layer
        if t == "MESH":
            data = e.get_data()
            vs = [tuple(v) for v in data.vertices]
            for f in data.faces:
                verts = [vs[i] for i in f]
                faces.append(record(verts))
                structure.append(structure_record(layer, rgb, "f", verts))
        elif t == "POLYLINE":
            verts = [tuple(v.dxf.location) for v in e.vertices]
            lines.append(record(verts))
            structure.append(structure_record(layer, rgb, "l", verts))
        elif t == "POINT":
            verts = [tuple(e.dxf.location)]
            points.append(record(verts))
            structure.append(structure_record(layer, rgb, "p", verts))
        else:
            raise AssertionError(f"unexpected entity {t}")
    layer_colors = {l.dxf.name: l.dxf.get("true_color") for l in doc.layers}
    header = {
        "custom": dict(doc.header.custom_vars),
        "acadver": doc.header.get("$ACADVER"),
    }
    return parity_hash(faces, lines, points), auditor, structure, layer_colors, header


def read_dwg(path: Path, binary: Path):
    """Same results as read_dxf, from `obj2cad dwg-dump` (acadrust's DWG reader)."""
    proc = subprocess.run([str(binary), "dwg-dump", str(path)], capture_output=True, text=True, encoding="utf-8")
    if proc.returncode != 0:
        raise AssertionError("DWG does not read back: " + proc.stderr.strip())
    dump = json.loads(proc.stdout)
    unpack = lambda v: tuple(struct.unpack(">d", bytes.fromhex(c))[0] for c in v)
    faces, lines, points, structure = [], [], [], []
    for e in dump["entities"]:
        t, layer = e["t"], e.get("layer")
        rgb = None if e.get("color") is None else tuple(bytes.fromhex(e["color"]))
        vs = [unpack(v) for v in e.get("v", [])]
        if t == "mesh":
            for f in e["faces"]:
                verts = [vs[i] for i in f]
                faces.append(record(verts))
                structure.append(structure_record(layer, rgb, "f", verts))
        elif t == "polyline":
            lines.append(record(vs))
            structure.append(structure_record(layer, rgb, "l", vs))
        elif t == "point":
            points.append(record(vs))
            structure.append(structure_record(layer, rgb, "p", vs))
        else:
            raise AssertionError(f"unexpected entity {t}")
    layer_colors = {name: None if c is None or len(c) != 6 else int(c, 16) for name, c in dump["layers"].items()}

    class Audit:
        has_errors = dump["problems"] > 0
        errors = [None] * dump["problems"]

    header = {"custom": dump["custom"], "acadver": dump["version"]}
    return parity_hash(faces, lines, points), Audit, structure, layer_colors, header


# ---------------------------------------------------------------- driver
def check(binary: Path, obj_path: Path, out_dir: Path, up: str, fmt: str, expect_invalid: bool) -> tuple[bool, str]:
    stem = f"{obj_path.parent.name}_{obj_path.stem}" if obj_path.stem == "model" else obj_path.stem
    ext = ".dwg" if fmt == "dwg" else ".dxf"
    dxf = out_dir / (stem + ext)
    rep = out_dir / (stem + ".report.json")
    cmd = [str(binary), "convert", str(obj_path), "-o", str(dxf), "--report", str(rep), "--format", fmt, "--quiet"]
    if up != "auto":
        cmd += ["--up", up]
    t0 = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8")
    elapsed = time.perf_counter() - t0
    if expect_invalid:
        if proc.returncode == 0:
            return False, "accepted an invalid file"
        try:
            read_obj(obj_path)
            return False, "independent reader accepted it (reader disagreement)"
        except (ObjError, ValueError):
            pass
        return True, "rejected: " + proc.stderr.strip().splitlines()[0].split(": ", 1)[-1][:90]
    if proc.returncode != 0:
        return False, "CLI failed: " + proc.stderr.strip()

    report = json.loads(rep.read_text(encoding="utf-8"))
    applied_up = report["options"]["up_axis"]
    obj = read_obj(obj_path)
    mtl = next((obj_path.parent / m for m in obj["mtllibs"] if (obj_path.parent / m).is_file()), None)
    palette = read_mtl(mtl) if mtl else {}

    h_rust = report["parity_hash"]
    h_obj = hash_from_obj(obj, applied_up)
    h_dxf, auditor, structure, layer_colors, header = read_dwg(dxf, binary) if fmt == "dwg" else read_dxf(dxf)
    problems = []
    if h_obj != h_rust:
        problems.append("OBJ readers disagree on geometry")
    if h_dxf != h_rust:
        problems.append(f"{fmt.upper()} geometry differs from source")
    expected = sorted(expected_structure(obj, obj_path.stem, applied_up, palette))
    if sorted(structure) != expected:
        got, want = Counter(structure), Counter(expected)
        diff = next(iter((got - want) or (want - got)))
        layer, color = diff.split(b"\0")[:2]
        problems.append(f"layer/color mismatch (e.g. layer {layer.decode()!r}, color {color.decode()})")
    used_layers = {r.split(b"\0")[0].decode() for r in structure}
    if any(layer_colors.get(name) in (None, 0xFFFFFF) for name in used_layers):
        problems.append("a used layer has no distinct color")
    if auditor.has_errors:
        problems.append(f"{'acadrust read' if fmt == 'dwg' else 'ezdxf audit'}: {len(auditor.errors)} errors")
    if header["custom"].get("obj2cad.parity_hash") != h_rust:
        problems.append("custom property parity hash missing/wrong")
    if header["acadver"] != "AC1032":
        problems.append(f"unexpected version {header['acadver']}")
    if problems:
        return False, "; ".join(problems)
    up_note = "upright" if applied_up == "y_up_to_z_up" else "as-is"
    return True, f"{report['output']['faces']} faces, {len(used_layers)} layers, {up_note}, {elapsed:.2f}s"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+", type=Path)
    ap.add_argument("--bin", type=Path, default=Path("target/release/obj2cad"))
    ap.add_argument("--up", default="auto", choices=["auto", "as-is", "y-to-z"])
    ap.add_argument("--format", default="dxf", choices=["dxf", "dxf-binary", "dwg"])
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
            try:
                ok, msg = check(binary, obj, out, a.up, a.format, "invalid" in obj.parts)
            except Exception as e:  # a harness crash is a failure, not a pass
                ok, msg = False, f"harness error: {type(e).__name__}: {e}"
            failures += not ok
            label = f"{obj.parent.name}/{obj.name}" if obj.stem == "model" else obj.name
            print(f"{'PASS' if ok else 'FAIL'}  {label:<40} {msg}", flush=True)
    print(f"\n{len(objs) - failures}/{len(objs)} passed (up {a.up}, {a.format})")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
