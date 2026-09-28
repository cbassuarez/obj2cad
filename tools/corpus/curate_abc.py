"""Assemble the ABC test corpus from stream-extracted chunks (see abc_stream.py).

For every model that has an OBJ, a feature file and a STEP file, copies them to
``OUT/<model id>/`` as ``model.obj``, ``features.yml`` and ``model.step`` and writes
``OUT/manifest.json`` with per-model ground truth: counts of each true surface type
(plane, cylinder, cone, sphere, torus, B-spline, ...) and of each feature-curve type.
That ground truth is what the curve-recovery benchmark scores against.

Usage:
    python curate_abc.py --obj DIR --feat DIR --step DIR --out tests/corpus/cache/abc [--models 200]
"""

from __future__ import annotations

import argparse
import json
import shutil
from collections import Counter
from pathlib import Path

import yaml

try:
    from yaml import CSafeLoader as Loader
except ImportError:  # pure-Python fallback
    from yaml import SafeLoader as Loader


def by_model(root: Path, suffix: str) -> dict[str, Path]:
    """model id -> first file with the suffix (models with several parts use part 000)."""
    out: dict[str, Path] = {}
    for p in sorted(root.glob(f"*/*{suffix}")):
        out.setdefault(p.parent.name, p)
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--obj", type=Path, required=True)
    ap.add_argument("--feat", type=Path, required=True)
    ap.add_argument("--step", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--models", type=int, default=200)
    ap.add_argument("--move", action="store_true", help="move instead of copy (saves disk space)")
    a = ap.parse_args()

    objs, feats, steps = by_model(a.obj, ".obj"), by_model(a.feat, ".yml"), by_model(a.step, ".step")
    ids = sorted(set(objs) & set(feats) & set(steps))[: a.models]
    a.out.mkdir(parents=True, exist_ok=True)
    put = shutil.move if a.move else shutil.copyfile

    manifest = []
    totals_s, totals_c = Counter(), Counter()
    for mid in ids:
        with open(feats[mid], "rb") as f:
            feat = yaml.load(f, Loader=Loader) or {}
        surfaces = Counter(s.get("type", "Unknown") for s in feat.get("surfaces", []) or [])
        curves = Counter(c.get("type", "Unknown") for c in feat.get("curves", []) or [])
        totals_s.update(surfaces)
        totals_c.update(curves)
        d = a.out / mid
        d.mkdir(exist_ok=True)
        for src, name in ((objs[mid], "model.obj"), (feats[mid], "features.yml"), (steps[mid], "model.step")):
            put(str(src), str(d / name))
        manifest.append({
            "id": mid,
            "source": {"obj": objs[mid].name, "feat": feats[mid].name, "step": steps[mid].name},
            "obj_bytes": (d / "model.obj").stat().st_size,
            "surfaces": dict(surfaces),
            "curves": dict(curves),
        })

    (a.out / "manifest.json").write_text(json.dumps({
        "dataset": "ABC v00, chunk 0000",
        "license": "MIT, (c) 2019 Deep Geometry Processing; https://deep-geometry.github.io/abc-dataset/",
        "models": manifest,
        "surface_totals": dict(totals_s.most_common()),
        "curve_totals": dict(totals_c.most_common()),
    }, indent=1))
    print(f"{len(ids)} models -> {a.out}")
    print("surface types:", dict(totals_s.most_common()))
    print("curve types:  ", dict(totals_c.most_common()))


if __name__ == "__main__":
    main()
