# AutoCAD acceptance checklist

Run before approving each release PR. Automated checks (independent read-back, ezdxf
audit, parity hashes) already pass in CI; this checklist confirms the files behave in the
program people actually use. It takes about 15 minutes.

**Setup:** download the `acceptance-<sha>.zip` artifact from the release PR's CI run
(or run `python tests/harness/parity.py --up auto --keep out tests/fixtures` locally, with
`--format dwg` for DWG). It has three folders, `dxf/`, `dxf-binary/` and `dwg/`, with the
same drawings. Use a recent AutoCAD (note the version below).

For each file in the table, from `dwg/` and from `dxf/` (spot-check `dxf-binary/`):

1. **Open** it (`OPEN`). ☐ No error or "drawing needs recovery" dialog.
2. **Opening view**: before touching anything. ☐ The whole model is on screen, centered,
   in the same isometric view as obj2cad's preview (no blank screen, no dot).
3. **Audit**: `AUDIT` → `Y`. ☐ "0 errors found".
4. **Look**: `3DORBIT` / `VSCURRENT` → Shaded. ☐ Shape matches the preview in obj2cad (same
   orientation, nothing missing, no stray geometry, concave faces filled correctly).
5. **Structure**: `LAYER`. ☐ Layer names match the report's layer map, and each layer's
   color matches its swatch in obj2cad.
6. **Properties**: `DWGPROPS` → Custom. ☐ `obj2cad.parity_hash` equals the report's
   `parity_hash`.
7. **Units**: `UNITS`. ☐ "Units to scale inserted content" matches the unit chosen.
8. **Spot-check a coordinate**: `ID`, snap to a vertex listed in the notes below.
   ☐ Value matches the OBJ text (AutoCAD shows limited decimals; set `LUPREC` to 8).
9. **Save round-trip**: `SAVEAS` → DWG 2018, reopen. ☐ Still opens and audits clean.

| File | Why it's in the set | Vertex to spot-check | Result |
|---|---|---|---|
| `cube_materials.dxf` | layers + true colors from MTL | `1.000000, 1.000000, -1.000000` | |
| `ngons_nonplanar.dxf` | 6-gon and non-planar quad in a MESH | `3, 1, 0` | |
| `precision.dxf` | extreme/subnormal values, 17-digit values | `0.1, 0.2, 0.30000000000000004` | |
| `names_layers.dxf` | non-ASCII and sanitized layer names | – | |
| `lines_points.dxf` | 3D POLYLINE and POINT entities | `1, 1, 1` | |
| `degenerate.dxf` | face with a repeated vertex | – | |
| `georeferenced.dxf` | survey-scale coordinates; opening view far from the origin | `500123.456, 4649876.543, 1234.567` | |
| `concave_ngon.dxf` | L- and U-shaped faces in a MESH | `0, 0, 0` | |
| `point_cloud.dxf` | a vertex-only file as POINT entities | – | |
| `bom.dxf` | a file that starts with a byte-order mark | `0, 0, 0` | |
| `big_terrain.dxf` / `.dwg` | about 1 million faces: open time, orbit, audit | – | |

**First acceptance round: also confirm specifically**
- ☐ MESH entities open without a proxy/"unknown object" warning (we declare the `MESH`
  class the way AutoCAD writes it; ezdxf omits it).
- ☐ A face with a repeated vertex (`degenerate.dxf`) does not make AutoCAD reject the MESH.
  If it does, the converter must split or report it. File an issue.

Record: AutoCAD version ______, OS ______, tester ______, date ______, all boxes ticked ☐.
Paste this checklist, filled in, as a comment on the release PR before merging.
