"""Check the files the web app actually downloads.

parity.py proves the command-line tool's output correct. This proves the browser's
downloads are the same files, and that what the app says about them is true:

1. every download is byte-identical to the CLI's output for the same model and settings
   (DXF, binary DXF and DWG; the visible-layers download against --exclude-layer; each
   entry of the batch .zip);
2. the report the app saves describes that download: it equals the CLI's report and its
   output SHA-256 is the downloaded file's;
3. independently of both: the geometry read back from the download matches this
   harness's own OBJ reader, the embedded parity hash matches, every entity is on the
   expected layer with the expected color, and the file audits clean;
4. every file the CLI rejects, the app rejects too.

Usage (after `node web/scripts/download-fixtures.mjs <web-dir> <fixture-dirs>`):
    python tests/harness/web_parity.py [--bin target/release/obj2cad] <web-dir>
Exit status is non-zero if any check fails.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

import parity as H


def convert(binary: Path, obj: Path, out: Path, fmt: str, extra: list[str]) -> tuple[bytes, dict] | None:
    report = out.with_suffix(".report.json")
    proc = subprocess.run(
        [str(binary), "convert", str(obj), "-o", str(out), "--report", str(report), "--format", fmt, "--quiet", *extra],
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if proc.returncode != 0:
        return None
    return out.read_bytes(), json.loads(report.read_text(encoding="utf-8"))


def normalized(report: dict) -> dict:
    """The report with hidden-layer names in a fixed order (the app lists them in layer
    order, the CLI in flag order; the drawing is the same)."""
    r = json.loads(json.dumps(report))
    r["options"]["exclude_layers"] = sorted(r["options"]["exclude_layers"])
    return r


def verify(binary: Path, obj: Path, web_file: Path, web_report: dict, cli: tuple[bytes, dict], fmt: str, hidden: list[str]) -> list[str]:
    problems = []
    data, cli_report = cli
    got = web_file.read_bytes()
    if got != data:
        at = next((i for i, (a, b) in enumerate(zip(got, data)) if a != b), min(len(got), len(data)))
        problems.append(f"bytes differ from the CLI's (sizes {len(got)}/{len(data)}, first difference at byte {at})")
    if normalized(web_report) != normalized(cli_report):
        keys = sorted(k for k in web_report if web_report[k] != cli_report.get(k))
        problems.append(f"report differs from the CLI's in {keys}")
    if web_report["output"]["sha256"] != hashlib.sha256(got).hexdigest():
        problems.append("report's output SHA-256 is not the downloaded file's")

    up = web_report["options"]["up_axis"]
    o = H.read_obj(obj)
    mtl = next((obj.parent / m for m in o["mtllibs"] if (obj.parent / m).is_file()), None)
    expected = H.expected_structure(o, obj.stem, up, H.read_mtl(mtl) if mtl else {})
    expected = [r for r in expected if r.split(b"\0")[0].decode() not in hidden]
    h_file, auditor, structure, _, header = H.read_dwg(web_file, binary) if fmt == "dwg" else H.read_dxf(web_file)
    if not hidden and H.hash_from_obj(o, up) != h_file:
        problems.append("geometry read back from the download differs from the OBJ")
    if h_file != web_report["parity_hash"] or header["custom"].get("obj2cad.parity_hash") != web_report["parity_hash"]:
        problems.append("parity hash: download and report disagree")
    if sorted(structure) != sorted(expected):
        problems.append("entities not on the expected layers/colors")
    if auditor.has_errors:
        problems.append(f"audit: {len(auditor.errors)} errors")
    return problems


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("web", type=Path, help="folder written by web/scripts/download-fixtures.mjs")
    ap.add_argument("--bin", type=Path, default=Path("target/release/obj2cad"))
    a = ap.parse_args()
    binary = a.bin if a.bin.exists() else a.bin.with_suffix(".exe")
    manifest = json.loads((a.web / "manifest.json").read_text(encoding="utf-8"))
    results: list[tuple[str, list[str]]] = []

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        cli_out: dict[tuple[str, str], tuple[bytes, dict] | None] = {}
        for fmt, entries in manifest["formats"].items():
            ext = ".dwg" if fmt == "dwg" else ".dxf"
            for obj_s, entry in entries.items():
                obj = Path(obj_s)
                label = f"{fmt:<10} {obj.name}"
                cli = convert(binary, obj, tmp / f"{fmt}-{obj.stem}{ext}", fmt, [])
                cli_out[(fmt, obj_s)] = cli
                if "rejected" in entry:
                    results.append((label, [] if cli is None else ["the app rejected a file the CLI converts"]))
                    continue
                if cli is None:
                    results.append((label, ["the app converted a file the CLI rejects"]))
                    continue
                web_report = json.loads((a.web / fmt / entry["report"]).read_text(encoding="utf-8"))
                results.append((label, verify(binary, obj, a.web / fmt / entry["drawing"], web_report, cli, fmt, [])))

        if v := manifest.get("visible"):
            obj = Path(v["obj"])
            extra = [x for name in v["hidden"] for x in ("--exclude-layer", name)]
            cli = convert(binary, obj, tmp / "visible.dxf", "dxf", extra)
            web_report = json.loads((a.web / "visible" / v["report"]).read_text(encoding="utf-8"))
            label = f"{'visible':<10} {obj.name} without {', '.join(v['hidden'])}"
            results.append((label, ["the CLI rejects it"] if cli is None else verify(binary, obj, a.web / "visible" / v["drawing"], web_report, cli, "dxf", v["hidden"])))

        if b := manifest.get("batch"):
            with zipfile.ZipFile(a.web / b["zip"]) as z:
                names = set(z.namelist())
                for obj_s in b["objs"]:
                    obj = Path(obj_s)
                    name = obj.stem + ".dxf"
                    cli = cli_out.get(("dxf", obj_s))
                    problems = []
                    if name not in names:
                        problems.append("missing from the .zip")
                    elif cli is None or z.read(name) != cli[0]:
                        problems.append("differs from the CLI's output")
                    results.append((f"{'batch zip':<10} {name}", problems))

    for label, problems in results:
        print(f"{'PASS' if not problems else 'FAIL'}  {label:<50} {'; '.join(problems)}")
    failed = sum(bool(p) for _, p in results)
    print(f"\n{len(results) - failed}/{len(results)} passed (web downloads)")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
