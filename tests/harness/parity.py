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
   expected color, computed here from the OBJ and MTL with independently written rules;
6. checks **approximate colors** against references computed here: a face colored from
   its vertices' colors against their average in linear light, and a face colored from a
   PNG texture (decoded by this script) against a dense average of the texture over the
   face. JPEG textures are only checked for having some color.

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
import zlib
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
    texts: list[tuple[str, str, str]] = []  # the coordinates as written
    colors: list = []  # per vertex: (r, g, b) 0..255 or None
    elements = []  # (kind, [vertex indices], object, group, material, [vt per corner] or None)
    texcoords: list[tuple[float, float]] = []
    nvt = nvn = 0
    forward = []
    obj = grp = mat = None
    mtllibs = []
    weights: dict[int, float] = {}
    splines, curv_count = [], 0
    cstype = deg = pending = None

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
            texts.append(tuple(args[:3]))
            if len(args) == 4 and float(args[3]) != 1.0:
                weights[len(pos) - 1] = float(args[3])
            # `v x y z r g b`: stored as single precision, then 0..1 → 0..255.
            colors.append(tuple(byte(f32(float(a))) for a in args[3:6]) if len(args) == 6 else None)
        elif kw == "cstype":
            cstype = (len(args) == 2 and args[0] == "rat", args[-1]) if args and len(args) <= 2 else None
        elif kw == "deg":
            deg = int(args[0]) if args else None
        elif kw == "curv":
            curv_count += 1
            pending = {"range": (float(args[0]), float(args[1])), "cps": [ref(a, len(pos), 0) for a in args[2:]], "parm": None}
        elif kw == "parm":
            if args and args[0] == "u" and pending is not None:
                pending["parm"] = [float(a) for a in args[1:]]
        elif kw == "end":
            if pending is not None:
                sp = spline_of(pending, cstype, deg)
                if sp is not None:
                    splines.append({**sp, "o": obj, "g": grp, "m": mat})
            pending = None
        elif kw == "vt":
            nvt += 1
            # Stored as single precision, like the engine.
            uv = [f32(float(a)) for a in args[:2]]
            texcoords.append(tuple(uv + [0.0] * (2 - len(uv))))
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
            idx, uvs = [], []
            for a in args:
                comps = a.split("/")
                if len(comps) > 3 or not comps[0]:
                    raise ObjError("malformed reference")
                idx.append(ref(comps[0], len(pos), 0))
                if len(comps) > 1 and comps[1]:
                    uvs.append(ref(comps[1], nvt, 1))
                if len(comps) > 2 and comps[2]:
                    ref(comps[2], nvn, 2)
            kind = {"f": "f", "fo": "f", "l": "l", "p": "p"}[kw]
            if kind == "f" and len(idx) < 3:
                continue  # skipped (reported by the engine)
            if kind == "l" and len(idx) < 2 or kind == "p" and not idx:
                raise ObjError("element too short")
            elements.append((kind, idx, obj, grp, mat, uvs if len(uvs) == len(idx) else None))
    totals = [len(pos), nvt, nvn]
    for i, slot in forward:
        if i >= totals[slot]:
            raise ObjError("forward reference out of range")
    for sp in splines:
        sp["weights"] = [weights.get(i, 1.0) for i in sp["cps"]] if sp["rational"] else None
    return {"positions": pos, "texts": texts, "colors": colors, "elements": elements, "mtllibs": mtllibs,
            "texcoords": texcoords, "splines": splines, "curves_left_out": curv_count - len(splines)}


def spline_of(p: dict, cstype, deg):
    """A free-form curve as a B-spline, or None when it can't be written exactly: only
    B-spline and Bezier curves whose range is their whole knot domain."""
    if cstype is None or deg is None or p["parm"] is None:
        return None
    rational, kind = cstype
    n = len(p["cps"])
    if deg < 1 or n < deg + 1:
        return None
    if kind == "bspline":
        if len(p["parm"]) != n + deg + 1:
            return None
        knots = p["parm"]
    elif kind == "bezier":
        breaks = p["parm"]
        if (n - 1) % deg or len(breaks) != (n - 1) // deg + 1:
            return None
        knots = [breaks[0]] * (deg + 1) + [b for b in breaks[1:-1] for _ in range(deg)] + [breaks[-1]] * (deg + 1)
    else:
        return None
    if any(b < a for a, b in zip(knots, knots[1:])) or p["range"] != (knots[deg], knots[n]):
        return None
    return {"degree": deg, "knots": knots, "cps": p["cps"], "rational": rational}


def spline_record(degree: int, knots, verts, weights) -> bytes:
    b = struct.pack(">II", degree, len(knots)) + b"".join(struct.pack(">d", k) for k in knots)
    b += record(verts)
    return b + b"".join(struct.pack(">d", w) for w in (weights or []))


def f32(x: float) -> float:
    return struct.unpack("<f", struct.pack("<f", x))[0]


def byte(c: float) -> int:
    """0..1 → 0..255, rounding half away from zero like Rust's f64::round."""
    return int(math.floor(min(max(c, 0.0), 1.0) * 255.0 + 0.5))


# ---------------------------------------------------------------- reference colors
def to_linear(c: int) -> float:
    x = c / 255.0
    return x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4


def to_srgb(lin: float) -> float:
    """Linear light → sRGB, 0..255 (unrounded)."""
    lin = min(max(lin, 0.0), 1.0)
    x = lin * 12.92 if lin <= 0.0031308 else 1.055 * lin ** (1 / 2.4) - 0.055
    return x * 255.0


def mix(colors) -> tuple:
    """The average of 8-bit sRGB colors, taken in linear light."""
    n = len(colors)
    return tuple(to_srgb(sum(to_linear(c[k]) for c in colors) / n) for k in range(3))


def read_png(path: Path):
    """(width, height, rgb rows, alpha rows) for a non-interlaced PNG, or None for anything
    else. Written from the PNG specification, independently of the engine's decoder."""
    data = path.read_bytes()
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        return None
    pos, idat, palette, trns = 8, b"", None, None
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        pos += 12 + n
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
        elif kind == b"PLTE":
            palette = [tuple(body[i:i + 3]) for i in range(0, len(body), 3)]
        elif kind == b"tRNS":
            trns = body
        elif kind == b"IDAT":
            idat += body
    if interlace or (ctype == 3 and depth != 8) or (ctype != 3 and depth not in (8, 16)):
        return None
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    bpp = channels * depth // 8
    stride = w * bpp
    raw = zlib.decompress(idat)
    rows, prev = [], bytearray(stride)
    for y in range(h):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = line
    rgb, alpha = [], []
    for line in rows:
        px = [line[i:i + bpp:depth // 8] for i in range(0, stride, bpp)]  # high bytes
        if ctype == 3:
            rgb.append([palette[p[0]] for p in px])
            alpha.append([trns[p[0]] if trns and p[0] < len(trns) else 255 for p in px])
        elif ctype in (0, 4):
            rgb.append([(p[0],) * 3 for p in px])
            alpha.append([p[1] if ctype == 4 else 255 for p in px])
        else:
            rgb.append([tuple(p[:3]) for p in px])
            alpha.append([p[3] if ctype == 6 else 255 for p in px])
    return w, h, rgb, alpha


def texture_reference(image, uvs, scale, offset, clamp) -> tuple | None:
    """A face's texture color: the texture averaged over the face in linear light, from a
    grid of at least 48 × 48 samples per triangle and 36 per texel (bilinear, transparent
    parts left out)."""
    w, h, rgb, alpha = image
    uv = [(u * scale[0] + offset[0], v * scale[1] + offset[1]) for u, v in uvs]

    def texel(x, y):
        x = min(max(x, 0), w - 1) if clamp else x % w
        y = min(max(y, 0), h - 1) if clamp else y % h
        a = alpha[y][x] / 255.0
        return [to_linear(c) * a for c in rgb[y][x]], a

    def sample(u, v):
        if clamp:
            u, v = min(max(u, 0.0), 1.0), min(max(v, 0.0), 1.0)
        else:
            u, v = u - math.floor(u), v - math.floor(v)
        x, y = u * w - 0.5, (1.0 - v) * h - 0.5
        x0, y0 = math.floor(x), math.floor(y)
        fx, fy = x - x0, y - y0
        c, a = [0.0] * 3, 0.0
        for dx, dy, wt in ((0, 0, (1 - fx) * (1 - fy)), (1, 0, fx * (1 - fy)), (0, 1, (1 - fx) * fy), (1, 1, fx * fy)):
            ci, ai = texel(x0 + dx, y0 + dy)
            c = [c[k] + ci[k] * wt for k in range(3)]
            a += ai * wt
        return c, a

    total, weight = [0.0] * 3, 0.0
    for k in range(1, len(uv) - 1):
        (au, av), (bu, bv), (cu, cv) = uv[0], uv[k], uv[k + 1]
        area = abs((bu - au) * (cv - av) - (bv - av) * (cu - au)) / 2
        n = max(48, math.ceil(6 * math.sqrt(area * w * h)))
        for i in range(n):
            for j in range(n - i):
                for s, t in (((i + 1 / 3) / n, (j + 1 / 3) / n), ((i + 2 / 3) / n, (j + 2 / 3) / n)):
                    if s + t > 1:
                        continue
                    c, a = sample(au + (bu - au) * s + (cu - au) * t, av + (bv - av) * s + (cv - av) * t)
                    total = [total[q] + c[q] * area for q in range(3)]
                    weight += a * area
    if weight == 0:
        c, a = sample(sum(p[0] for p in uv) / len(uv), sum(p[1] for p in uv) / len(uv))
        if a == 0:
            return None
        total, weight = c, a
    return tuple(to_srgb(t / weight) for t in total)


# ---------------------------------------------------------------- independent XYZ reader
def read_xyz(path: Path) -> dict:
    """An ASCII point cloud, as this harness reads the column rules in xyz.rs's docs."""
    raw = path.read_bytes()
    if raw.startswith((b"\xff\xfe", b"\xfe\xff")):
        raise ObjError("UTF-16")
    text = raw.decode("utf-8-sig")
    lines = text.split("\r") if "\n" not in text and "\r" in text else text.replace("\r\n", "\n").split("\n")
    rows, first = [], True
    for line in lines:
        t = line.strip()
        if not t or t.startswith("#") or t.startswith("//"):
            continue
        if first:
            first = False
            if t.isdigit():
                continue  # point count
            low = t.lower()
            if any(c.isalpha() and c not in "eE" for c in t) and "nan" not in low and "inf" not in low:
                continue  # column names
        rows.append(t)
    if not rows:
        return {"positions": [], "texts": [], "colors": [], "elements": [], "mtllibs": [], "texcoords": [], "splines": [], "curves_left_out": 0}
    sep = ";" if ";" in rows[0] else ("," if "," in rows[0] and len(rows[0].split()) < 3 else None)
    split = (lambda r: [x.strip() for x in r.split(sep)]) if sep else (lambda r: r.split())
    table = [split(r) for r in rows]
    n = len(table[0])
    if n not in (3, 4, 6, 7, 9) or any(len(r) != n for r in table):
        raise ObjError("column count")
    vals = [[float(x) for x in r] for r in table]
    if any(not math.isfinite(v) for r in vals for v in r):
        raise ObjError("non-finite")

    def is_byte(c):
        return all(table[i][c].isdigit() and vals[i][c] <= 255 for i in range(len(table)))

    def rgb(a):
        return all(is_byte(c) for c in range(a, a + 3)) and any(vals[i][c] > 1 for i in range(len(table)) for c in range(a, a + 3))

    def normals(a):
        unit = all(abs(sum(r[c] ** 2 for c in range(a, a + 3)) - 1) < 1e-2 for r in vals)
        return unit and not all(is_byte(c) for c in range(a, a + 3))

    rgb_at = None
    if n == 6:
        if rgb(3) == normals(3):
            raise ObjError("ambiguous columns")
        rgb_at = 3 if rgb(3) else None
    elif n == 7:
        if not (rgb(4) and not is_byte(3)):
            raise ObjError("ambiguous columns")
        rgb_at = 4
    elif n == 9:
        a, b = rgb(3) and normals(6), normals(3) and rgb(6)
        if a == b:
            raise ObjError("ambiguous columns")
        rgb_at = 3 if a else 6
    pos = [tuple(r[:3]) for r in vals]
    colors = [tuple(int(v) for v in r[rgb_at:rgb_at + 3]) if rgb_at is not None else None for r in vals]
    stem = path.stem
    texts = [tuple(r[:3]) for r in table]
    return {"positions": pos, "texts": texts, "colors": colors, "elements": [("p", list(range(len(pos))), stem, None, None, None)], "mtllibs": [],
            "texcoords": [], "splines": [], "curves_left_out": 0}


def read_mtl(path: Path) -> tuple[dict, dict, dict]:
    """Material colors (0..255), texture file names (without folders), and each texture's
    options: (scale, offset, clamp)."""
    colors, textures, options, current = {}, {}, {}, None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        parts = line.strip().split(None, 1)
        if not parts:
            continue
        kw, rest = parts[0], (parts[1].strip() if len(parts) > 1 else "")
        if kw == "newmtl":
            current = rest
        elif kw == "Kd" and current is not None:
            vals = []
            for t in rest.split():
                try:
                    vals.append(float(t))
                except ValueError:
                    pass
            if len(vals) == 3 and all(math.isfinite(v) for v in vals):
                colors.setdefault(current, tuple(byte(c) for c in vals))
        elif kw == "map_Kd" and current is not None:
            toks, i = rest.split(), 0
            opts = {"-s": [1.0, 1.0], "-o": [0.0, 0.0], "-clamp": False}
            while i < len(toks) and toks[i].startswith("-") and len(toks[i]) > 1:
                opt = toks[i]
                i += 1
                if opt in ("-o", "-s", "-t"):
                    n = 0
                    while n < 3 and i + 1 < len(toks):
                        try:
                            x = float(toks[i])
                        except ValueError:
                            break
                        if opt in opts and n < 2 and math.isfinite(x):
                            opts[opt][n] = x
                        i, n = i + 1, n + 1
                elif opt == "-clamp":
                    opts["-clamp"] = i < len(toks) and toks[i].lower() == "on"
                    i += 1
                else:
                    i += 2 if opt == "-mm" else 1
            if i < len(toks) and current not in textures:
                textures[current] = " ".join(toks[i:]).replace("\\", "/").split("/")[-1]
                options[current] = (tuple(opts["-s"]), tuple(opts["-o"]), opts["-clamp"])
    return colors, textures, options


# ---------------------------------------------------------------- bundles
def hidden(rel: str) -> bool:
    return any(p.startswith(".") or p == "__MACOSX" for p in rel.replace("\\", "/").split("/"))


def bundle_files(item: Path) -> tuple[list[Path], str]:
    """The files the CLI reads for `item` (an .obj, an .xyz or a folder), and the name a
    folder gives the drawing."""
    if item.is_dir():
        files = [p for p in item.rglob("*") if p.is_file() and not hidden(str(p.relative_to(item)))]
        return files, item.name
    files = [item]
    if item.suffix.lower() == ".obj":
        seen = {item.name.lower()}
        for lib in read_obj(item)["mtllibs"]:
            m = item.parent / lib
            if m.is_file() and m.name.lower() not in seen:
                seen.add(m.name.lower())
                files.append(m)
                for tex in read_mtl(m)[1].values():
                    img = m.parent / tex
                    if img.is_file() and img.name.lower() not in seen:
                        seen.add(img.name.lower())
                        files.append(img)
    return files, ""


def read_bundle(files: list[Path], name: str) -> dict:
    """Every model and cloud in one document, the way bundle.rs promises to combine them."""
    files = sorted(files, key=lambda p: (p.name.lower(), str(p)))
    ext = lambda p: p.suffix.lower()
    geometry = [p for p in files if ext(p) in (".obj", ".xyz")]
    mtls = [p for p in files if ext(p) == ".mtl"]
    images = {p.name.lower(): p for p in files if ext(p) in (".jpg", ".jpeg", ".png")}
    parts = [(p, read_obj(p) if ext(p) == ".obj" else read_xyz(p)) for p in geometry]
    n_obj = sum(ext(p) == ".obj" for p in geometry)
    several = len(parts) > 1

    merged = {"positions": [], "texts": [], "colors": [], "elements": [], "mtllibs": [], "texcoords": [], "splines": [], "curves_left_out": 0}
    palette, textured, maps, meaning = {}, set(), {}, {}
    taken = {"o": set(), "g": set()}  # object/group names an earlier file already used
    for p, doc in parts:
        own = {"o": {}, "g": {}}

        def claim(kind, raw, stem=p.stem, own=own):
            """A name another file already used gets this file's name: "Chair (b)"."""
            if not several or raw is None:
                return raw
            if raw not in own[kind]:
                name, i = raw, 2
                while name in taken[kind]:
                    name = f"{raw} ({stem})" if i == 2 else f"{raw} ({stem} {i})"
                    i += 1
                taken[kind].add(name)
                own[kind][raw] = name
            return own[kind][raw]

        colors, textures, options = {}, {}, {}
        if ext(p) == ".obj":
            libs = [m for lib in doc["mtllibs"] for m in mtls if m.name.lower() == lib.replace("\\", "/").split("/")[-1].lower()]
            used = any(e[4] is not None for e in doc["elements"])
            if not libs and n_obj == 1 and len(mtls) == 1 and (used or not doc["mtllibs"]):
                libs = mtls
            for m in libs:
                c, t, o = read_mtl(m)
                for k, v in c.items():
                    colors.setdefault(k, v)
                for k, v in t.items():
                    if v.lower() in images and k not in textures:
                        textures[k] = v
                        options[k] = o[k]
        # Materials two files define differently get "name (file)".
        rename = {}
        for e in doc["elements"]:
            mat = e[4]
            if mat is None or mat in rename:
                continue
            key = (colors.get(mat), textures.get(mat))
            if not several or meaning.setdefault(mat, key) == key:
                rename[mat] = mat
            else:
                rename[mat] = f"{mat} ({p.stem})"
        for mat, new in rename.items():
            if mat in colors:
                palette.setdefault(new, colors[mat])
            if mat in textures:
                textured.add(new)
                maps.setdefault(new, (images[textures[mat].lower()], *options[mat]))
        v0, t0 = len(merged["positions"]), len(merged["texcoords"])
        merged["positions"] += doc["positions"]
        merged["texcoords"] += doc["texcoords"]
        merged["texts"] += doc["texts"]
        merged["colors"] += doc["colors"]
        for kind, idx, o, g, mat, uv in doc["elements"]:
            if several and o is None:
                o = p.stem
            uv = None if uv is None else [i + t0 for i in uv]
            merged["elements"].append((kind, [i + v0 for i in idx], claim("o", o), claim("g", g), rename.get(mat, mat), uv))
        for sp in doc["splines"]:
            o = sp["o"] if sp["o"] is not None or not several else p.stem
            merged["splines"].append({**sp, "cps": [i + v0 for i in sp["cps"]], "o": claim("o", o), "g": claim("g", sp.get("g")), "m": rename.get(sp["m"], sp["m"])})
        merged["curves_left_out"] += doc["curves_left_out"]
    if len(geometry) == 1:
        stem = geometry[0].stem
    else:
        stem = Path(name).stem if name else (geometry[0].stem if geometry else "bundle")
    merged.update(palette=palette, textured=textured, maps=maps, stem=stem)
    return merged


# ---------------------------------------------------------------- expected layers and colors
INVALID_NAME_CHARS = set('<>/\\":;?*|=`,')


def dxf_safe(name: str) -> str:
    s = "".join("_" if c in INVALID_NAME_CHARS or unicodedata.category(c) == "Cc" else c for c in name.strip())
    s = s[:255]
    return s or "unnamed"


def expected_structure(obj: dict, stem: str, up: str, palette: dict, textured: set = frozenset()) -> list[bytes]:
    """(layer, color, geometry) records the DXF should contain, in obj2cad's default
    layer mode (objects; groups when the file has no objects). Faces colored from a
    texture or from vertex colors get an approximate color "~rrggbb" (checked within a
    tolerance), or "T" when the texture isn't a PNG this harness decodes."""
    P = [transform(p, up) for p in obj["positions"]]
    has_objects = any(e[2] is not None for e in obj["elements"]) or any(sp["o"] is not None for sp in obj.get("splines", []))
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

    vcolor = obj.get("colors") or [None] * len(P)
    maps, images = obj.get("maps", {}), {}
    records = []
    for kind, idx, o, g, m, uv in obj["elements"]:
        layer = layer_of(o, g)
        color = palette.get(m) if m is not None else None
        if kind == "p":
            records += [structure_record(layer, vcolor[i] or color, "p", [P[i]]) for i in idx]
            continue
        if kind == "f" and uv and m in textured:
            path, scale, offset, clamp = maps[m]
            if path not in images:
                images[path] = read_png(path)
            ref = images[path] and texture_reference(images[path], [obj["texcoords"][t] for t in uv], scale, offset, clamp)
            color = "~%02x%02x%02x" % tuple(round(c) for c in ref) if ref else "T"
        elif all(vcolor[i] is not None for i in idx):
            color = "~%02x%02x%02x" % tuple(round(c) for c in mix([vcolor[i] for i in idx]))
        records.append(structure_record(layer, color, kind, [P[i] for i in idx]))
    for sp in obj.get("splines", []):
        layer = layer_of(sp["o"], sp["g"])
        color = palette.get(sp["m"]) if sp["m"] is not None else None
        records.append(structure_record(layer, color, "s", [P[i] for i in sp["cps"]], sp))
    if not obj["elements"] and not obj.get("splines") and P:  # point cloud
        layer = default_layer()
        records += [structure_record(layer, vcolor[i], "p", [p]) for i, p in enumerate(P)]
    return records


# Approximate colors may differ from their reference by rounding, by the engine merging
# colors within 4 levels of each other, and (textures) by its coarser sampling.
COLOR_TOLERANCE = 6


def match_approximate(structure: list[bytes], expected: list[bytes]) -> tuple[list[bytes], int]:
    """Give each written record whose expected color is approximate ("~rrggbb" or "T") that
    expected color when its own is within tolerance, so the exact comparison can follow.
    Returns the records and the largest color difference seen (-1: none approximate)."""
    approx: dict[bytes, list[bytes]] = {}
    for r in expected:
        c = r.split(b"\0")[1]
        if c == b"T" or c.startswith(b"~"):
            approx.setdefault(without_color(r), []).append(c)
    worst, out = -1, []
    for r in structure:
        key, got = without_color(r), r.split(b"\0")[1]
        want = approx.get(key)
        if not want or got == b"-":
            out.append(r)
            continue
        rgb = bytes.fromhex(got.decode())

        def distance(c: bytes) -> int:
            return 0 if c == b"T" else max(abs(a - b) for a, b in zip(rgb, bytes.fromhex(c[1:].decode())))

        best = min(want, key=distance)
        d = distance(best)
        worst = max(worst, d if best != b"T" else 0)
        if d > COLOR_TOLERANCE:
            out.append(r)
            continue
        want.remove(best)
        layer, _, rest = r.split(b"\0", 2)
        out.append(layer + b"\0" + best + b"\0" + rest)
    return out, worst


def without_color(rec: bytes) -> bytes:
    layer, _color, rest = rec.split(b"\0", 2)
    return layer + b"\0" + rest


def structure_record(layer: str, color, kind: str, verts, spline=None) -> bytes:
    c = "-" if color is None else color if isinstance(color, str) else "%02x%02x%02x" % tuple(color)
    body = spline_record(spline["degree"], spline["knots"], verts, spline["weights"]) if spline else record(verts)
    return f"{layer}\0{c}\0{kind}\0".encode() + body


# ---------------------------------------------------------------- canonical geometry hash
def transform(p, up: str):
    x, y, z = p
    return (x, -z, y) if up == "y_up_to_z_up" else (x, y, z)


def record(vertices) -> bytes:
    return b"".join(struct.pack(">d", c) for v in vertices for c in v)


def parity_hash(faces, lines, points, splines=()) -> str:
    h = hashlib.sha256(b"obj2cad-parity-v1\0")
    kinds = [(b"f", faces), (b"l", lines), (b"p", points)] + ([(b"s", splines)] if splines else [])
    for tag, recs in kinds:
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
    splines = [spline_record(sp["degree"], sp["knots"], [P[i] for i in sp["cps"]], sp["weights"]) for sp in obj.get("splines", [])]
    if not obj["elements"] and not splines:
        points = [record([p]) for p in P]  # point cloud
    return parity_hash(faces, lines, points, splines)


def read_dxf(path: Path):
    doc = ezdxf.readfile(path)
    auditor = doc.audit()
    faces, lines, points, structure, acis, splines = [], [], [], [], [], []
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
        elif t in ("SURFACE", "3DSOLID"):
            acis.append((t, layer, bytes(e.sab)))
        elif t == "SPLINE":
            sp = {"degree": e.dxf.degree, "knots": list(e.knots), "weights": list(e.weights) if e.dxf.flags & 4 else None}
            verts = [tuple(v) for v in e.control_points]
            splines.append(spline_record(sp["degree"], sp["knots"], verts, sp["weights"]))
            structure.append(structure_record(layer, rgb, "s", verts, sp))
        else:
            raise AssertionError(f"unexpected entity {t}")
    layer_colors = {l.dxf.name: l.dxf.get("true_color") for l in doc.layers}
    header = {
        "custom": dict(doc.header.custom_vars),
        "acadver": doc.header.get("$ACADVER"),
    }
    return parity_hash(faces, lines, points, splines), auditor, structure, layer_colors, header, acis


def read_dwg(path: Path, binary: Path):
    """Same results as read_dxf, from `obj2cad dwg-dump` (acadrust's DWG reader)."""
    proc = subprocess.run([str(binary), "dwg-dump", str(path)], capture_output=True, text=True, encoding="utf-8")
    if proc.returncode != 0:
        raise AssertionError("DWG does not read back: " + proc.stderr.strip())
    dump = json.loads(proc.stdout)
    unpack = lambda v: tuple(struct.unpack(">d", bytes.fromhex(c))[0] for c in v)
    faces, lines, points, structure, acis, splines = [], [], [], [], [], []
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
        elif t == "spline":
            num = lambda h: struct.unpack(">d", bytes.fromhex(h))[0]
            sp = {"degree": e["degree"], "knots": [num(k) for k in e["knots"]], "weights": [num(w) for w in e["weights"]] if e["rational"] else None}
            splines.append(spline_record(sp["degree"], sp["knots"], vs, sp["weights"]))
            structure.append(structure_record(layer, rgb, "s", vs, sp))
        elif t in ("surface", "solid"):
            acis.append(("SURFACE" if t == "surface" else "3DSOLID", layer, bytes.fromhex(e["sab"])))
        else:
            raise AssertionError(f"unexpected entity {t}")
    layer_colors = {name: None if c is None or len(c) != 6 else int(c, 16) for name, c in dump["layers"].items()}

    class Audit:
        has_errors = dump["problems"] > 0
        errors = [None] * dump["problems"]

    header = {"custom": dump["custom"], "acadver": dump["version"]}
    return parity_hash(faces, lines, points, splines), Audit, structure, layer_colors, header, acis


# ---------------------------------------------------------------- curved surfaces
def quantum(text: str) -> float:
    """The precision a number's text states: 1.234560 → 1e-6, 1.5e-3 → 1e-4, 12 → 1."""
    t = text.lstrip("+-")
    mant, _, exp = t.lower().partition("e")
    decimals = len(mant.split(".", 1)[1]) if "." in mant else 0
    return 10.0 ** ((int(exp) if exp else 0) - decimals)


def surface_of(sab: bytes):
    """The single face's surface in an ACIS body, read with ezdxf's SAB parser, as
    (kind, distance function)."""
    from ezdxf.acis import sab as sabmod

    builder = sabmod.parse_sab(sab)
    faces = [e for e in builder.entities if e.name == "face"]
    if len(faces) != 1:
        raise AssertionError(f"expected one face, got {len(faces)}")
    # face data: pattern, next, loop, shell, subshell, surface, ...
    surf = faces[0].data[5].value
    vals = [t.value for t in surf.data[1:] if t.tag in (0x06, 0x13, 0x14)]
    sub = lambda a, b: tuple(x - y for x, y in zip(a, b))
    dot = lambda a, b: sum(x * y for x, y in zip(a, b))
    norm = lambda a: math.sqrt(dot(a, a))
    if surf.name == "cone-surface":
        center, axis, major, _ratio, sin, cos, _scale = vals[:7]
        r0 = norm(major)

        def d(p):
            v = sub(p, center)
            h = dot(v, axis)
            rho = norm(sub(v, tuple(h * a for a in axis)))
            return (rho - (r0 + h * sin / cos)) * cos

        return ("cylinder" if sin == 0 else "cone"), d
    if surf.name == "sphere-surface":
        center, radius = vals[0], vals[1]
        return "sphere", lambda p: norm(sub(p, center)) - radius
    if surf.name == "torus-surface":
        center, axis, major, minor = vals[:4]

        def d(p):
            v = sub(p, center)
            z = dot(v, axis)
            rho = norm(sub(v, tuple(z * a for a in axis)))
            return math.hypot(rho - major, z) - minor

        return "torus", d
    raise AssertionError(f"unexpected surface {surf.name}")


def check_curves(obj: dict, up: str, report: dict, acis: list) -> list[str]:
    """Every vertex of every face each surface was recognized from lies on the surface
    read back from the file, within its own stated precision (see crates/obj2cad-curves)."""
    regions = report.get("curves", [])
    if len(regions) != len(acis):
        return [f"{len(regions)} regions in the report, {len(acis)} surfaces in the file"]
    faces = [idx for kind, idx, *_ in obj["elements"] if kind == "f"]
    P = [transform(p, up) for p in obj["positions"]]
    used = sorted({i for f in faces for i in f})
    if not used:
        return []
    lo = [min(P[i][a] for i in used) for a in range(3)]
    hi = [max(P[i][a] for i in used) for a in range(3)]
    size = math.dist(lo, hi)
    floor = 1e-12 * (max(abs(c) for i in used for c in P[i]) + size)
    cap = max(1e-6 * size, floor)

    def tol(i):
        t = 0.5 * math.sqrt(sum(quantum(x) ** 2 for x in obj["texts"][i]))
        return max(min(t, cap), floor)

    problems = []
    for region, (_, _, sab) in zip(regions, acis):
        kind, d = surface_of(sab)
        if kind != region["kind"]:
            problems.append(f"report says {region['kind']}, file has {kind}")
        worst = max(abs(d(P[i])) / tol(i) for f in region["faces"] for i in faces[f])
        if worst > 1.0 + 1e-9:
            problems.append(f"{kind}: a vertex is {worst:.3g} tolerances off the surface")
    return problems


# ---------------------------------------------------------------- driver
def check(binary: Path, obj_path: Path, out_dir: Path, up: str, fmt: str, expect_invalid: bool, curves: bool = False) -> tuple[bool, str]:
    """`obj_path`: an .obj or .xyz file, or a folder holding one bundle."""
    stem = f"{obj_path.parent.name}_{obj_path.stem}" if obj_path.stem == "model" else obj_path.stem
    ext = ".dwg" if fmt == "dwg" else ".dxf"
    dxf = out_dir / (stem + ext)
    rep = out_dir / (stem + ".report.json")
    cmd = [str(binary), "convert", str(obj_path), "-o", str(dxf), "--report", str(rep), "--format", fmt, "--quiet"]
    if up != "auto":
        cmd += ["--up", up]
    if curves:
        cmd += ["--curves"]
    t0 = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8")
    elapsed = time.perf_counter() - t0
    if expect_invalid:
        if proc.returncode == 0:
            return False, "accepted an invalid file"
        try:
            read_bundle(*bundle_files(obj_path))
            return False, "independent reader accepted it (reader disagreement)"
        except (ObjError, ValueError, UnicodeDecodeError):
            pass
        return True, "rejected: " + proc.stderr.strip().splitlines()[0].split(": ", 1)[-1][:90]
    if proc.returncode != 0:
        return False, "CLI failed: " + proc.stderr.strip()

    report = json.loads(rep.read_text(encoding="utf-8"))
    applied_up = report["options"]["up_axis"]
    obj = read_bundle(*bundle_files(obj_path))

    h_rust = report["parity_hash"]
    h_obj = hash_from_obj(obj, applied_up)
    h_dxf, auditor, structure, layer_colors, header, acis = read_dwg(dxf, binary) if fmt == "dwg" else read_dxf(dxf)
    problems = []
    if h_obj != h_rust:
        problems.append("OBJ readers disagree on geometry")
    if h_dxf != h_rust:
        problems.append(f"{fmt.upper()} geometry differs from source")
    expected = sorted(expected_structure(obj, obj["stem"], applied_up, obj["palette"], obj["textured"]))
    structure, color_error = match_approximate(structure, expected)
    if color_error > COLOR_TOLERANCE:
        problems.append(f"a face's color is {color_error} levels from its reference (tolerance {COLOR_TOLERANCE})")
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
    if acis and not curves:
        problems.append("surfaces written without --curves")
    if report["omissions"]["freeform_curves"] != obj["curves_left_out"]:
        problems.append(f"{report['omissions']['freeform_curves']} free-form curves reported left out, harness counts {obj['curves_left_out']}")
    problems += check_curves(obj, applied_up, report, acis)
    if problems:
        return False, "; ".join(problems)
    up_note = "upright" if applied_up == "y_up_to_z_up" else "as-is"
    kinds = "".join(f", {r['kind']}" for r in report.get("curves", []))
    colors = f", colors within {color_error}" if color_error >= 0 else ""
    return True, f"{report['output']['faces']} faces, {len(used_layers)} layers, {up_note}{kinds}{colors}, {elapsed:.2f}s"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+", type=Path)
    ap.add_argument("--bin", type=Path, default=Path("target/release/obj2cad"))
    ap.add_argument("--up", default="auto", choices=["auto", "as-is", "y-to-z"])
    ap.add_argument("--format", default="dxf", choices=["dxf", "dxf-binary", "dwg"])
    ap.add_argument("--keep", type=Path, help="keep outputs in this folder")
    ap.add_argument("--curves", action="store_true", help="also recognize curved surfaces and verify them")
    a = ap.parse_args()

    binary = a.bin if a.bin.exists() else a.bin.with_suffix(".exe")
    # Every .obj and .xyz on its own, except inside a `bundle` folder, where each
    # subfolder is one bundle.
    objs = []
    for d in a.dirs:
        for p in d.rglob("*"):
            in_bundle = "bundle" in p.relative_to(d).parts[:-1] or p.parent.name == "bundle"
            if p.is_dir() and p.parent.name == "bundle":
                objs.append(p)
            elif p.is_file() and p.suffix.lower() in (".obj", ".xyz") and not in_bundle:
                objs.append(p)
    objs.sort()
    if not objs:
        print("no .obj or .xyz files found", file=sys.stderr)
        return 2
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        out = a.keep or Path(tmp)
        out.mkdir(parents=True, exist_ok=True)
        for obj in objs:
            try:
                ok, msg = check(binary, obj, out, a.up, a.format, "invalid" in obj.parts, a.curves)
            except Exception as e:  # a harness crash is a failure, not a pass
                ok, msg = False, f"harness error: {type(e).__name__}: {e}"
            failures += not ok
            label = f"{obj.parent.name}/{obj.name}" if obj.stem == "model" else obj.name
            print(f"{'PASS' if ok else 'FAIL'}  {label:<40} {msg}", flush=True)
    print(f"\n{len(objs) - failures}/{len(objs)} passed (up {a.up}, {a.format}{', curves' if a.curves else ''})")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
