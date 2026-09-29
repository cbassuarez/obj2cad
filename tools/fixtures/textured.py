"""A textured and vertex-colored model for checking face colors: textures with fine detail
(averaging in linear light), transparency, `-clamp`, `-s`/`-o` and tiling, and faces and
lines colored by their vertices.

Run:  python tools/fixtures/textured.py   (writes tests/fixtures/bundle/textured/)
"""

from __future__ import annotations

import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "bundle" / "textured"


def png(path: Path, w: int, h: int, pixel, alpha: bool = False) -> None:
    """Write an 8-bit RGB or RGBA PNG; `pixel(x, y)` gives a tuple, rows top to bottom."""
    raw = b"".join(b"\0" + bytes(c for x in range(w) for c in pixel(x, y)) for y in range(h))

    def chunk(kind: bytes, body: bytes) -> bytes:
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6 if alpha else 2, 0, 0, 0)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    # 32 × 32: one-pixel black and white stripes, with a red block in the top-left corner.
    png(OUT / "stripes.png", 32, 32, lambda x, y: (200, 30, 30) if x < 8 and y < 8 else (255,) * 3 if x % 2 else (0,) * 3)
    # 16 × 16: an opaque green disc on transparent magenta (which must not show).
    png(OUT / "leaf.png", 16, 16,
        lambda x, y: (40, 150, 60, 255) if (x - 7.5) ** 2 + (y - 7.5) ** 2 < 36 else (255, 0, 255, 0), alpha=True)
    (OUT / "textured.mtl").write_text(
        "newmtl stripes\nKd 1 1 1\nmap_Kd stripes.png\n"
        "newmtl edge\nKd 1 1 1\nmap_Kd -clamp on stripes.png\n"
        "newmtl scaled\nKd 1 1 1\nmap_Kd -s 0.25 0.25 1 -o 0 0.75 stripes.png\n"
        "newmtl leaf\nKd 1 1 1\nmap_Kd leaf.png\n",
        encoding="utf-8",
    )
    lines = ["# textured and vertex-colored faces", "mtllib textured.mtl"]
    vt = 0

    def quad(i: int, uvs, mat: str, name: str) -> None:
        nonlocal vt
        base = i * 4 + 1
        lines.append(f"o {name}")
        lines.append(f"usemtl {mat}")
        for u, v in uvs:
            lines.append(f"vt {u} {v}")
        lines.append(f"f {base}/{vt + 1} {base + 1}/{vt + 2} {base + 2}/{vt + 3} {base + 3}/{vt + 4}")
        vt += 4

    # Six unit squares side by side, each with its own four vertices.
    for i in range(6):
        lines += [f"v {i * 2} 0 0", f"v {i * 2 + 1} 0 0", f"v {i * 2 + 1} 1 0", f"v {i * 2} 1 0"]
    square = [(0, 0), (1, 0), (1, 1), (0, 1)]
    quad(0, square, "stripes", "Stripes")  # mostly stripes: mid gray, plus some red
    quad(1, [(0, 0.75), (0.25, 0.75), (0.25, 1), (0, 1)], "stripes", "Red")  # the red block
    quad(2, [(3 * u, 3 * v) for u, v in square], "stripes", "Tiled")  # repeats three times
    quad(3, [(3 * u - 1, 3 * v - 1) for u, v in square], "edge", "Clamped")  # edges stretch
    quad(4, square, "scaled", "Scaled")  # -s/-o: the red block
    quad(5, square, "leaf", "Leaf")  # only the green disc
    # A 3 × 2 grid colored by its vertices, and a line.
    lines.append("o Scan")
    first = 25
    colors = {(0, 0): (1, 0, 0), (1, 0): (0, 1, 0), (2, 0): (0, 0, 1), (3, 0): (1, 1, 0),
              (0, 1): (0, 1, 1), (1, 1): (1, 0, 1), (2, 1): (1, 1, 1), (3, 1): (0, 0, 0),
              (0, 2): (0.5, 0.25, 0.125), (1, 2): (0.2, 0.4, 0.6), (2, 2): (0.9, 0.8, 0.1), (3, 2): (0.3, 0.3, 0.3)}
    for y in range(3):
        for x in range(4):
            r, g, b = colors[(x, y)]
            lines.append(f"v {x} {y + 3} 0 {r} {g} {b}")
    for y in range(2):
        for x in range(3):
            a = first + y * 4 + x
            lines.append(f"f {a} {a + 1} {a + 5} {a + 4}")
    lines.append(f"l {first} {first + 3} {first + 11}")
    (OUT / "textured.obj").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
