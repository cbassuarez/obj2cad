"""Generate the DXF R2018 document skeleton used by obj2cad-dxf.

The skeleton (header variables, tables, blocks, layouts, dictionaries) is produced by
ezdxf (MIT), whose output is tested against AutoCAD and BricsCAD. We then:

* remove ezdxf-specific bookkeeping (EZDXF appid, EZDXF_META dictionary),
* make it deterministic (fixed timestamps, GUIDs filled in by the writer),
* add the MESH class AutoCAD declares for AcDbSubDMesh entities,
* insert @@MARKERS@@ the Rust writer fills in.

Run:  python tools/dxf-template/gen_template.py
Output: crates/obj2cad-dxf/templates/r2018.dxf (committed; regenerate only on purpose).
"""

from __future__ import annotations

import io
from pathlib import Path

import ezdxf

OUT = Path(__file__).resolve().parents[2] / "crates" / "obj2cad-dxf" / "templates" / "r2018.dxf"

# header variable -> marker (value lines replaced wholesale by the writer)
HEADER_MARKERS = {
    "$INSUNITS": "@@INSUNITS@@",
    "$MEASUREMENT": "@@MEASUREMENT@@",
    "$HANDSEED": "@@HANDSEED@@",
    "$EXTMIN": "@@EXTMIN@@",
    "$EXTMAX": "@@EXTMAX@@",
    "$FINGERPRINTGUID": "@@FINGERPRINTGUID@@",
    "$VERSIONGUID": "@@VERSIONGUID@@",
    "$LASTSAVEDBY": "@@LASTSAVEDBY@@",
}
# Dates come from the source file (deterministic per input), filled in by the writer.
DATE_MARKERS = {
    "$TDCREATE": "@@TDCREATE@@",
    "$TDUCREATE": "@@TDUCREATE@@",
    "$TDUPDATE": "@@TDUPDATE@@",
    "$TDUUPDATE": "@@TDUUPDATE@@",
}
# The opening view (VPORT *Active) is fitted to the model by the writer: one marker per
# point/value, placed where its first pair was; the pairs' other coordinates are dropped.
VPORT_MARKERS = {"12": "@@VPORT_CENTER@@", "16": "@@VPORT_DIRECTION@@", "17": "@@VPORT_TARGET@@", "40": "@@VPORT_HEIGHT@@"}
VPORT_DROP = {"22", "26", "36", "27", "37"}


def pairs_of(text: str) -> list[tuple[str, str]]:
    lines = text.splitlines()
    return [(lines[i].strip(), lines[i + 1]) for i in range(0, len(lines) - 1, 2)]


def entity_spans(pairs, start, end):
    """Split pairs[start:end] into spans that each begin with a (0, TYPE) pair."""
    spans, cur = [], None
    for i in range(start, end):
        if pairs[i][0] == "0":
            if cur is not None:
                spans.append(cur)
            cur = [i, i + 1]
        else:
            cur[1] = i + 1
    if cur is not None:
        spans.append(cur)
    return spans


def main() -> None:
    doc = ezdxf.new("R2018", setup=False)
    buf = io.StringIO()
    doc.write(buf)
    pairs = pairs_of(buf.getvalue())

    # --- drop ezdxf bookkeeping ------------------------------------------------------
    drop: set[int] = set()
    ezdxf_handles = set()
    for s, e in entity_spans(pairs, 0, len(pairs)):
        kind = pairs[s][1]
        body = dict(pairs[s:e])
        if kind == "APPID" and body.get("2") == "EZDXF":
            drop.update(range(s, e))
        if kind == "DICTIONARY" and any(v == "CREATED_BY_EZDXF" for _, v in pairs[s:e]):
            drop.update(range(s, e))
            ezdxf_handles.add(body["5"])
            ezdxf_handles.update(v for c, v in pairs[s:e] if c == "350")
        if kind == "DICTIONARYVAR":
            drop.update(range(s, e))
    # root dictionary entry pointing at EZDXF_META
    for i, (c, v) in enumerate(pairs):
        if c == "3" and v == "EZDXF_META":
            drop.update({i, i + 1})
    pairs = [p for i, p in enumerate(pairs) if i not in drop]
    # the APPID table count
    for i, (c, v) in enumerate(pairs):
        if (c, v) == ("2", "APPID") and pairs[i - 1] == ("0", "TABLE"):
            j = next(k for k in range(i, i + 8) if pairs[k][0] == "70")
            pairs[j] = ("70", str(int(pairs[j][1]) - 1))
            break

    # --- markers -----------------------------------------------------------------------
    out: list[str] = []
    i = 0
    in_header = False
    while i < len(pairs):
        c, v = pairs[i]
        if (c, v) == ("2", "HEADER"):
            in_header = True
        if in_header and c == "9" and v in HEADER_MARKERS:
            out.append(HEADER_MARKERS[v])
            i += 1
            while i < len(pairs) and pairs[i][0] not in ("9", "0"):
                i += 1
            continue
        if in_header and c == "9" and v in DATE_MARKERS:
            out.append(DATE_MARKERS[v])
            i += 2
            continue
        if (c, v) == ("2", "*Active") and ("0", "VPORT") in pairs[i - 6 : i]:
            out.append(f"{c:>3}\n{v}")
            i += 1
            while pairs[i][0] != "0":
                code = pairs[i][0]
                if code in VPORT_MARKERS:
                    out.append(VPORT_MARKERS[code])
                elif code not in VPORT_DROP:
                    out.append(f"{code:>3}\n{pairs[i][1]}")
                i += 1
            continue
        if in_header and (c, v) == ("0", "ENDSEC"):
            out.append("@@CUSTOMPROPERTIES@@")
            in_header = False
        if (c, v) == ("2", "CLASSES"):
            out.append(f"{c:>3}\n{v}")
            out.append(
                "  0\nCLASS\n  1\nMESH\n  2\nAcDbSubDMesh\n  3\nObjectDBX Classes\n"
                " 90\n4095\n 91\n0\n280\n0\n281\n1"
            )
            i += 1
            continue
        if (c, v) == ("2", "LAYER") and pairs[i - 1] == ("0", "TABLE"):
            out.append(f"{c:>3}\n{v}")
            i += 1
            while pairs[i][0] != "70":
                out.append(f"{pairs[i][0]:>3}\n{pairs[i][1]}")
                i += 1
            out.append("@@LAYERCOUNT@@")
            i += 1
            continue
        if (c, v) == ("0", "ENDTAB") and "@@LAYERCOUNT@@" in out and "@@LAYERS@@" not in out:
            out.append("@@LAYERS@@")
        if (c, v) == ("2", "ENTITIES"):
            out.append(f"{c:>3}\n{v}")
            out.append("@@ENTITIES@@")
            i += 1
            continue
        out.append(f"{c:>3}\n{v}")
        i += 1

    text = "\n".join(out) + "\n"
    markers = (
        list(HEADER_MARKERS.values())
        + list(DATE_MARKERS.values())
        + list(VPORT_MARKERS.values())
        + ["@@CUSTOMPROPERTIES@@", "@@LAYERS@@", "@@LAYERCOUNT@@", "@@ENTITIES@@"]
    )
    for marker in markers:
        assert text.count(marker) == 1, marker
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, newline="\n")
    print(f"wrote {OUT} ({len(text)} bytes); removed ezdxf handles {sorted(ezdxf_handles)}")


if __name__ == "__main__":
    main()
