# AutoCAD acceptance checklist

Run before approving each release PR. Automated checks (independent read-back, ezdxf
audit, parity hashes) already pass in CI; this checklist confirms the files behave in the
program people actually use. It takes about 15 minutes.

**Setup:** download the `acceptance-<version>.zip` artifact from the release PR's CI run
(or run `python tests/harness/parity.py --keep out tests/fixtures` locally). Use a recent
AutoCAD (note the version below).

For each file in the table:

1. **Open** it (`OPEN`). ☐ No error or "drawing needs recovery" dialog.
2. **Audit**: `AUDIT` → `Y`. ☐ "0 errors found".
3. **Look**: `ZOOM` → `E`, then `3DORBIT` / `VSCURRENT` → Shaded. ☐ Shape matches the preview
   in obj2cad (same orientation, nothing missing, no stray geometry).
4. **Structure**: `LAYER`. ☐ Layer names match the report's layer map.
5. **Properties**: `DWGPROPS` → Custom. ☐ `obj2cad.parity_hash` equals the report's
   `parity_hash`.
6. **Units**: `UNITS`. ☐ "Units to scale inserted content" matches the unit chosen.
7. **Spot-check a coordinate**: `ID`, snap to a vertex listed in the notes below.
   ☐ Value matches the OBJ text (AutoCAD shows limited decimals; set `LUPREC` to 8).
8. **Save round-trip**: `SAVEAS` → DWG 2018, reopen. ☐ Still opens and audits clean.

| File | Why it's in the set | Vertex to spot-check | Result |
|---|---|---|---|
| `cube_materials.dxf` | layers + true colors from MTL | `1.000000, 1.000000, -1.000000` | |
| `ngons_nonplanar.dxf` | 6-gon and non-planar quad in a MESH | `3, 1, 0` | |
| `precision.dxf` | extreme/subnormal values, 17-digit values | `0.1, 0.2, 0.30000000000000004` | |
| `names_layers.dxf` | non-ASCII and sanitized layer names | – | |
| `lines_points.dxf` | 3D POLYLINE and POINT entities | `1, 1, 1` | |
| `degenerate.dxf` | face with a repeated vertex | – | |
| `georeferenced.dxf` | survey-scale coordinates | `500123.456, 4649876.543, 1234.567` | |
| One large ABC model | performance, >100k faces | – | |

**First acceptance round: also confirm specifically**
- ☐ MESH entities open without a proxy/"unknown object" warning (we declare the `MESH`
  class the way AutoCAD writes it; ezdxf omits it).
- ☐ A face with a repeated vertex (`degenerate.dxf`) does not make AutoCAD reject the MESH.
  If it does, the converter must split or report it. File an issue.

Record: AutoCAD version ______, OS ______, tester ______, date ______, all boxes ticked ☐.
Paste this checklist, filled in, as a comment on the release PR before merging.
