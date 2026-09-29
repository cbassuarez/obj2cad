"""Tessellated curved shapes for the curve recognizer, written like CAD exporters write
them: vertices exactly on the surface, printed with 6 decimals.

Run:  python tools/fixtures/curves.py   (writes tests/fixtures/curves/*.obj)
"""

from __future__ import annotations

import math
import random
from pathlib import Path

OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "curves"


class Obj:
    def __init__(self, comment: str):
        self.lines = [f"# {comment}"]
        self.n = 0

    def v(self, x, y, z) -> int:
        self.lines.append(f"v {x:.6f} {y:.6f} {z:.6f}")
        self.n += 1
        return self.n

    def f(self, *idx):
        self.lines.append("f " + " ".join(str(i) for i in idx))

    def o(self, name):
        self.lines.append(f"o {name}")

    def save(self, name):
        (OUT / name).write_text("\n".join(self.lines) + "\n")


def ring(o: Obj, r, z, n, a0=0.0, a1=2 * math.pi, closed=True):
    steps = n if closed else n + 1
    return [o.v(r * math.cos(a0 + (a1 - a0) * k / n), r * math.sin(a0 + (a1 - a0) * k / n), z) for k in range(steps)]


def band(o: Obj, lo, hi, closed=True):
    m = len(lo)
    for k in range(m if closed else m - 1):
        k2 = (k + 1) % m
        o.f(lo[k], lo[k2], hi[k2], hi[k])


def cap(o: Obj, ring_ids, z, up):
    c = o.v(0, 0, z)
    m = len(ring_ids)
    for k in range(m):
        a, b = ring_ids[k], ring_ids[(k + 1) % m]
        o.f(c, a, b) if up else o.f(c, b, a)


def cylinder():
    o = Obj("capped cylinder r=10 h=25, 32 segments")
    o.o("Cylinder")
    lo, hi = ring(o, 10, 0, 32), ring(o, 10, 25, 32)
    band(o, lo, hi)
    cap(o, lo, 0, up=False)
    cap(o, hi, 25, up=True)
    o.save("cylinder_capped.obj")


def cone():
    o = Obj("cone frustum r=12..6 h=18, 32 segments")
    o.o("Cone")
    lo, hi = ring(o, 12, 0, 32), ring(o, 6, 18, 32)
    band(o, lo, hi)
    cap(o, lo, 0, up=False)
    cap(o, hi, 18, up=True)
    o.save("cone_frustum.obj")


def sphere():
    o = Obj("UV sphere r=8, 24 x 12")
    o.o("Sphere")
    r, nu, nv = 8.0, 24, 12
    top = o.v(0, 0, r)
    rows = []
    for j in range(1, nv):
        t = math.pi * j / nv
        rows.append([o.v(r * math.sin(t) * math.cos(2 * math.pi * k / nu), r * math.sin(t) * math.sin(2 * math.pi * k / nu), r * math.cos(t)) for k in range(nu)])
    bot = o.v(0, 0, -r)
    for k in range(nu):
        o.f(top, rows[0][k], rows[0][(k + 1) % nu])
    for j in range(nv - 2):
        band(o, rows[j + 1], rows[j])
    for k in range(nu):
        o.f(bot, rows[-1][(k + 1) % nu], rows[-1][k])
    o.save("sphere_uv.obj")


def torus():
    o = Obj("torus R=15 r=4, 36 x 18")
    o.o("Torus")
    R, r, nu, nv = 15.0, 4.0, 36, 18
    g = [[o.v((R + r * math.cos(2 * math.pi * j / nv)) * math.cos(2 * math.pi * i / nu),
              (R + r * math.cos(2 * math.pi * j / nv)) * math.sin(2 * math.pi * i / nu),
              r * math.sin(2 * math.pi * j / nv)) for j in range(nv)] for i in range(nu)]
    for i in range(nu):
        for j in range(nv):
            a, b = g[i][j], g[(i + 1) % nu][j]
            c, d = g[(i + 1) % nu][(j + 1) % nv], g[i][(j + 1) % nv]
            o.f(a, b, c, d)
    o.save("torus.obj")


def trough():
    o = Obj("half cylinder r=10 length 40, 16 segments, triangulated")
    o.o("Trough")
    n = 16
    a = [o.v(10 * math.cos(math.pi * k / n), 10 * math.sin(math.pi * k / n), 0) for k in range(n + 1)]
    b = [o.v(10 * math.cos(math.pi * k / n), 10 * math.sin(math.pi * k / n), 40) for k in range(n + 1)]
    for k in range(n):
        o.f(a[k], a[k + 1], b[k + 1])
        o.f(a[k], b[k + 1], b[k])
    o.save("half_cylinder.obj")


def noisy():
    random.seed(7)
    o = Obj("cylinder with scanner-like noise (5e-5): must stay faceted")
    o.o("Scan")
    def jit(v):
        return v + random.uniform(-5e-5, 5e-5)
    n = 32
    lo = [o.v(jit(10 * math.cos(2 * math.pi * k / n)), jit(10 * math.sin(2 * math.pi * k / n)), jit(0)) for k in range(n)]
    hi = [o.v(jit(10 * math.cos(2 * math.pi * k / n)), jit(10 * math.sin(2 * math.pi * k / n)), jit(25)) for k in range(n)]
    band(o, lo, hi)
    o.save("noisy_cylinder.obj")


def box():
    o = Obj("a box: nothing curved")
    o.o("Box")
    p = [o.v(x, y, z) for z in (0, 10) for y in (0, 20) for x in (0, 30)]
    for f in [(1, 3, 4, 2), (5, 6, 8, 7), (1, 2, 6, 5), (3, 7, 8, 4), (1, 5, 7, 3), (2, 4, 8, 6)]:
        o.f(*f)
    o.save("box.obj")


def capsule():
    o = Obj("capsule: cylinder r=5 h=20 between two hemispheres, 24 segments")
    o.o("Capsule")
    r, h, nu, nv = 5.0, 20.0, 24, 6
    top = o.v(0, 0, h + r)
    rings = []
    for j in range(1, nv + 1):  # upper hemisphere down to the equator at z = h
        t = (math.pi / 2) * j / nv
        rings.append([o.v(r * math.sin(t) * math.cos(2 * math.pi * k / nu), r * math.sin(t) * math.sin(2 * math.pi * k / nu), h + r * math.cos(t)) for k in range(nu)])
    for j in range(1, nv + 1):  # lower hemisphere from the equator at z = 0
        t = (math.pi / 2) + (math.pi / 2) * j / nv
        if j == nv:
            break
        rings.append([o.v(r * math.sin(t) * math.cos(2 * math.pi * k / nu), r * math.sin(t) * math.sin(2 * math.pi * k / nu), r * math.cos(t)) for k in range(nu)])
    eq_lo = [o.v(r * math.cos(2 * math.pi * k / nu), r * math.sin(2 * math.pi * k / nu), 0) for k in range(nu)]
    rings.insert(nv, eq_lo)
    bot = o.v(0, 0, -r)
    for k in range(nu):
        o.f(top, rings[0][k], rings[0][(k + 1) % nu])
    for j in range(len(rings) - 1):
        band(o, rings[j + 1], rings[j])
    for k in range(nu):
        o.f(bot, rings[-1][(k + 1) % nu], rings[-1][k])
    o.save("capsule.obj")


def boss():
    o = Obj("boss: flat disk, quarter-torus fillet r=2, cylinder r=6 h=10; 32 segments")
    o.o("Boss")
    n, rc, rf, h = 32, 6.0, 2.0, 10.0
    outer = ring(o, 20.0, 0, n)
    rings = [ring(o, rc + rf, 0, n)]
    for j in range(1, 5):  # fillet: tube center at (rc + rf, 0, rf)
        t = (math.pi / 2) * j / 4
        rings.append(ring(o, rc + rf - rf * math.sin(t), rf - rf * math.cos(t), n))
    rings.append(ring(o, rc, h, n))
    band(o, outer, rings[0]) if False else None
    for k in range(n):  # flat annulus, facing up
        k2 = (k + 1) % n
        o.f(outer[k], outer[k2], rings[0][k2], rings[0][k])
    for j in range(len(rings) - 1):
        band(o, rings[j], rings[j + 1])
    cap(o, rings[-1], h, up=True)
    o.save("boss.obj")


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for make in (cylinder, cone, sphere, torus, trough, noisy, box, capsule, boss):
        make()
    print("wrote", sorted(p.name for p in OUT.glob("*.obj")))
