//! Settings the OBJ format does not record (units, up axis).
//!
//! Both are chosen automatically and shown with the reason; the user can override either.
//! The up axis is detected from the geometry (the rotation is exact). Units are a best
//! guess from the exporter's convention or the model's size; they only label the drawing,
//! so a wrong guess never changes a coordinate. With no basis, the drawing stays unitless.

use crate::convert::{Units, UpAxis};
use crate::obj::ObjDocument;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitsSource {
    Exporter,
    Size,
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hints {
    /// Exporter name if the leading comments identify one.
    pub exporter: Option<String>,
    pub units: Units,
    pub units_reason: String,
    /// Where the unit came from: the exporter's convention, the model's size, or nothing.
    pub units_source: UnitsSource,
    pub up_axis: UpAxis,
    pub up_axis_reason: String,
    /// True when the geometry itself decided the up axis (not just a default).
    pub up_axis_confident: bool,
    /// Bounds of all vertices in file coordinates.
    pub bounds: Option<([f64; 3], [f64; 3])>,
}

const EXPORTERS: &[(&str, &str, Option<Units>, Option<UpAxis>)] = &[
    // (needle in header comments, display name, typical units, typical up axis)
    (
        "blender",
        "Blender",
        Some(Units::Meters),
        Some(UpAxis::YUpToZUp),
    ),
    (
        "maya",
        "Maya",
        Some(Units::Centimeters),
        Some(UpAxis::YUpToZUp),
    ),
    ("3ds max", "3ds Max", None, Some(UpAxis::YUpToZUp)),
    (
        "cinema 4d",
        "Cinema 4D",
        Some(Units::Centimeters),
        Some(UpAxis::YUpToZUp),
    ),
    (
        "houdini",
        "Houdini",
        Some(Units::Meters),
        Some(UpAxis::YUpToZUp),
    ),
    ("zbrush", "ZBrush", None, Some(UpAxis::YUpToZUp)),
    ("meshlab", "MeshLab", None, None),
    ("rhino", "Rhino", None, Some(UpAxis::AsIs)),
    (
        "sketchup",
        "SketchUp",
        Some(Units::Inches),
        Some(UpAxis::AsIs),
    ),
    (
        "solidworks",
        "SolidWorks",
        Some(Units::Millimeters),
        Some(UpAxis::AsIs),
    ),
    (
        "fusion",
        "Fusion",
        Some(Units::Centimeters),
        Some(UpAxis::AsIs),
    ),
];

pub fn hints(doc: &ObjDocument) -> Hints {
    let header = doc.header_comments.join("\n").to_lowercase();
    let known = EXPORTERS.iter().find(|e| header.contains(e.0));

    let bounds = doc
        .positions
        .iter()
        .fold(None, |acc: Option<([f64; 3], [f64; 3])>, p| {
            let (mut lo, mut hi) = acc.unwrap_or((*p, *p));
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
            Some((lo, hi))
        });
    let size = bounds.map(|(lo, hi)| (0..3).map(|a| hi[a] - lo[a]).fold(0.0, f64::max));

    // Only guess from size when it is a plausible physical size; extreme or degenerate
    // extents (surveys in odd units, test files) get no suggestion rather than a bad one.
    let (units, units_reason, units_source) = match (known.and_then(|e| e.2), size) {
        (Some(u), _) => (u, format!("{} usually exports in {}", known.unwrap().1, unit_name(u)), UnitsSource::Exporter),
        (None, Some(s)) if s > 0.0 && s < 5.0 => {
            (Units::Meters, format!("the model is about {} across", units_across(s)), UnitsSource::Size)
        }
        (None, Some(s)) if (5.0..50_000.0).contains(&s) => {
            (Units::Millimeters, format!("the model is about {} across", units_across(s)), UnitsSource::Size)
        }
        _ => (Units::Unitless, "neither the file nor the model's size says".to_owned(), UnitsSource::None),
    };
    let exporter_up = known.and_then(|e| e.3.map(|u| (u, e.1)));
    let (up_axis, up_axis_reason, up_axis_confident) = match (detect_up(doc, bounds), exporter_up) {
        (Some((axis, why)), _) => (axis, why, true),
        (None, Some((axis, name))) => {
            let why = match axis {
                UpAxis::YUpToZUp => format!("{name} files are usually Y-up"),
                UpAxis::AsIs => format!("{name} files are usually Z-up"),
            };
            (axis, why, false)
        }
        (None, None) => (
            UpAxis::AsIs,
            "the shape doesn't show which way is up".to_owned(),
            false,
        ),
    };
    Hints {
        exporter: known.map(|e| e.1.to_owned()),
        units,
        units_reason,
        units_source,
        up_axis,
        up_axis_reason,
        up_axis_confident,
        bounds,
    }
}

/// What the user (or a team default) decided; `None` means "decide automatically".
#[derive(Debug, Clone, Default)]
pub struct Choices {
    pub units: Option<Units>,
    /// The team's "house unit", used when the file's exporter doesn't state one.
    pub default_units: Option<Units>,
    pub up_axis: Option<UpAxis>,
}

/// Resolve units and up axis. The CLI and the web app both call this, so the same file
/// with the same choices always gives the same output.
///
/// Units: an explicit choice, else the exporter's convention, else the house unit, else
/// the size-based guess, else unitless. Up axis: an explicit choice, else detection.
pub fn resolve(h: &Hints, c: &Choices) -> (Units, UpAxis) {
    let units = c
        .units
        .or((h.units_source == UnitsSource::Exporter).then_some(h.units))
        .or(c.default_units)
        .or((h.units_source == UnitsSource::Size).then_some(h.units))
        .unwrap_or(Units::Unitless);
    (units, c.up_axis.unwrap_or(h.up_axis))
}

/// Which way is up, from the geometry alone. Candidates are Y-up (most 3D apps) and Z-up
/// (CAD). Evidence, strongest first:
///
/// 1. **Resting base:** flat area on the model's lowest plane, facing down. A chair's feet,
///    a bracket's base plate, a building's footprint. The axis with clearly more is up.
/// 2. **Thin axis:** a model much thinner along Y or Z than across (terrain, a plate, a
///    ring lying flat) lies flat on that axis.
///
/// `None` when the evidence is weak or balanced (a cube, a sphere); callers then fall back
/// to the exporter's convention.
fn detect_up(doc: &ObjDocument, bounds: Option<([f64; 3], [f64; 3])>) -> Option<(UpAxis, String)> {
    let (lo, hi) = bounds?;
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let size = ext.iter().copied().fold(0.0, f64::max);
    if !(size.is_finite() && size > 0.0) {
        return None;
    }
    let tol = size * 1e-4;

    // Down-facing area on the lowest Y plane and the lowest Z plane (Newell normals, so
    // n-gons and non-planar faces are handled).
    let mut base = [0.0f64; 3]; // indexed by axis; only 1 (Y) and 2 (Z) are used
    let mut total = 0.0f64;
    for f in doc.faces.iter() {
        let mut n = [0.0f64; 3];
        for (k, &i) in f.iter().enumerate() {
            let a = doc.positions[i as usize];
            let b = doc.positions[f[(k + 1) % f.len()] as usize];
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if !(len > 0.0 && len.is_finite()) {
            continue;
        }
        total += len;
        for axis in [1usize, 2] {
            let on_floor = f
                .iter()
                .all(|&i| doc.positions[i as usize][axis] - lo[axis] <= tol);
            if on_floor && n[axis] / len < -0.9 {
                base[axis] += len;
            }
        }
    }
    let (by, bz) = (base[1], base[2]);
    let big = by.max(bz);
    // A real base is a sizable share of the surface (5%) and clearly beats the other axis
    // (3x); small coincidental flats on mechanical parts don't count.
    if total > 0.0 && big / total > 0.05 && big > 3.0 * by.min(bz) {
        return Some(if by > bz {
            (
                UpAxis::YUpToZUp,
                "it sits on a flat base that faces sideways in the file (Y-up)".to_owned(),
            )
        } else {
            (
                UpAxis::AsIs,
                "it sits on a flat base that already faces down (Z-up)".to_owned(),
            )
        });
    }

    // Thin along one of the candidate axes: it lies flat on that axis.
    let (ey, ez) = (ext[1], ext[2]);
    let across = ext[0].max(ey).max(ez);
    if ey < 0.35 * across && ey < 0.5 * ez {
        return Some((
            UpAxis::YUpToZUp,
            "it lies flat along the file's Y axis (Y-up)".to_owned(),
        ));
    }
    if ez < 0.35 * across && ez < 0.5 * ey {
        return Some((UpAxis::AsIs, "it already lies flat (Z-up)".to_owned()));
    }
    None
}

fn units_across(v: f64) -> String {
    let h = human(v);
    if h == "1" {
        "1 unit".to_owned()
    } else {
        format!("{h} units")
    }
}

/// Short, readable size: at most 3 decimals, no trailing zeros.
fn human(v: f64) -> String {
    if v >= 1000.0 {
        return format!("{v:.0}");
    }
    if v < 0.001 {
        return format!("{v:.1e}");
    }
    let t = format!("{v:.3}");
    t.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn unit_name(u: Units) -> &'static str {
    match u {
        Units::Unitless => "no unit",
        Units::Millimeters => "millimeters",
        Units::Centimeters => "centimeters",
        Units::Meters => "meters",
        Units::Inches => "inches",
        Units::Feet => "feet",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obj::parse;

    #[test]
    fn blender_is_meters_and_y_up() {
        let d = parse(b"# Blender 4.2.0\n# www.blender.org\nv 0 0 0\n").unwrap();
        let h = hints(&d);
        assert_eq!(h.exporter.as_deref(), Some("Blender"));
        assert_eq!((h.units, h.up_axis), (Units::Meters, UpAxis::YUpToZUp));
    }

    #[test]
    fn absurd_sizes_get_no_guess() {
        let d = parse(
            b"v -1e308 0 0
v 1e308 0 0
",
        )
        .unwrap();
        let h = hints(&d);
        assert_eq!(h.units, Units::Unitless);
        assert!(h.units_reason.len() < 60, "{}", h.units_reason);
        assert_eq!(human(160.0), "160");
        assert_eq!(human(2.7), "2.7");
    }

    /// Axis-aligned box with its min corner at `lo`, as OBJ quads with outward normals.
    fn cuboid(lo: [f64; 3], hi: [f64; 3], base: usize) -> String {
        let v = |x: usize, y: usize, z: usize| {
            [[lo[0], hi[0]][x], [lo[1], hi[1]][y], [lo[2], hi[2]][z]]
        };
        let mut s = String::new();
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..2 {
                    let p = v(x, y, z);
                    s += &format!(
                        "v {} {} {}
",
                        p[0], p[1], p[2]
                    );
                }
            }
        }
        // vertex index = 1 + x + 2y + 4z
        for f in [
            [1, 3, 4, 2],
            [5, 6, 8, 7],
            [1, 2, 6, 5],
            [3, 7, 8, 4],
            [1, 5, 7, 3],
            [2, 4, 8, 6],
        ] {
            s += &format!(
                "f {} {} {} {}
",
                f[0] + base,
                f[1] + base,
                f[2] + base,
                f[3] + base
            );
        }
        s
    }

    fn up_of(src: &str) -> (UpAxis, bool) {
        let h = hints(&parse(src.as_bytes()).unwrap());
        (h.up_axis, h.up_axis_confident)
    }

    #[test]
    fn detects_z_up_from_resting_base() {
        // An L-bracket: a wide plate on the Z floor with an upright.
        let src = cuboid([0.0, 0.0, 0.0], [160.0, 100.0, 12.0], 0)
            + &cuboid([0.0, 0.0, 12.0], [14.0, 100.0, 110.0], 8);
        assert_eq!(up_of(&src), (UpAxis::AsIs, true));
    }

    #[test]
    fn detects_y_up_from_resting_base_even_against_the_exporter() {
        // Same bracket, modeled Y-up, but the header claims Rhino (which is Z-up).
        let src = "# Rhino
"
        .to_owned()
            + &cuboid([0.0, 0.0, 0.0], [160.0, 12.0, 100.0], 0)
            + &cuboid([0.0, 12.0, 0.0], [14.0, 110.0, 100.0], 8);
        assert_eq!(up_of(&src), (UpAxis::YUpToZUp, true));
    }

    #[test]
    fn thin_models_lie_flat() {
        // A plate thin along Y with no resting base on either candidate plane.
        let flat_y = "v 0 0 0
v 10 0 0
v 10 0.5 10
v 0 0.5 10
f 1 2 3 4
";
        assert_eq!(up_of(flat_y), (UpAxis::YUpToZUp, true));
    }

    #[test]
    fn a_ring_lying_flat_in_y_up_is_detected() {
        // Torus (R = 1, r = 0.35) around the Y axis, as Blender exports it: no flat base,
        // but 0.7 thick against 2.7 across.
        let (nu, nv) = (48, 20);
        let mut src = String::new();
        for i in 0..nu {
            for j in 0..nv {
                let (u, v) = (
                    i as f64 / nu as f64 * std::f64::consts::TAU,
                    j as f64 / nv as f64 * std::f64::consts::TAU,
                );
                let (x, z) = (
                    (1.0 + 0.35 * v.cos()) * u.cos(),
                    (1.0 + 0.35 * v.cos()) * u.sin(),
                );
                src += &format!(
                    "v {x:.6} {:.6} {z:.6}
",
                    0.35 * v.sin()
                );
            }
        }
        let id = |i: usize, j: usize| (i % nu) * nv + (j % nv) + 1;
        for i in 0..nu {
            for j in 0..nv {
                src += &format!(
                    "f {} {} {} {}
",
                    id(i, j),
                    id(i + 1, j),
                    id(i + 1, j + 1),
                    id(i, j + 1)
                );
            }
        }
        assert_eq!(up_of(&src), (UpAxis::YUpToZUp, true));
    }

    #[test]
    fn ambiguous_shapes_fall_back_to_the_exporter() {
        let cube = cuboid([0.0; 3], [1.0; 3], 0);
        assert_eq!(up_of(&cube), (UpAxis::AsIs, false));
        assert_eq!(
            up_of(
                &("# Blender 4.2
"
                .to_owned()
                    + &cube)
            ),
            (UpAxis::YUpToZUp, false)
        );
    }

    #[test]
    fn resolution_order() {
        let blender = hints(&parse(b"# Blender 4.2
v 0 0 0
v 160 0 0
").unwrap());
        let unknown = hints(&parse(b"v 0 0 0
v 30 12 9
").unwrap());
        let house = Choices { default_units: Some(Units::Meters), ..Default::default() };
        // The exporter's convention beats the house unit; the house unit beats a size guess.
        assert_eq!(resolve(&blender, &house).0, Units::Meters);
        assert_eq!(resolve(&unknown, &Choices::default()).0, Units::Millimeters);
        assert_eq!(resolve(&unknown, &house).0, Units::Meters);
        // An explicit choice beats everything.
        let explicit = Choices { units: Some(Units::Inches), up_axis: Some(UpAxis::YUpToZUp), ..house };
        assert_eq!(resolve(&unknown, &explicit), (Units::Inches, UpAxis::YUpToZUp));
    }

    #[test]
    fn unknown_uses_size() {
        let d = parse(b"v 0 0 0\nv 120 30 4\n").unwrap();
        assert_eq!(hints(&d).units, Units::Millimeters);
        assert_eq!(hints(&d).up_axis, UpAxis::AsIs);
    }
}
