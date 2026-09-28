//! Suggestions for settings the OBJ format does not record (units, up axis).
//!
//! These are only ever *suggestions* shown to the user with the reason; the tool never
//! applies them without confirmation.

use crate::convert::{Units, UpAxis};
use crate::obj::ObjDocument;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Hints {
    /// Exporter name if the leading comments identify one.
    pub exporter: Option<String>,
    pub units: Units,
    pub units_reason: String,
    pub up_axis: UpAxis,
    pub up_axis_reason: String,
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
    let (units, units_reason) = match (known.and_then(|e| e.2), size) {
        (Some(u), _) => (
            u,
            format!("{} usually exports in {}", known.unwrap().1, unit_name(u)),
        ),
        (None, Some(s)) if s > 0.0 && s < 5.0 => (
            Units::Meters,
            format!(
                "the model is about {} units across, typical of meters",
                human(s)
            ),
        ),
        (None, Some(s)) if (5.0..50_000.0).contains(&s) => (
            Units::Millimeters,
            format!(
                "the model is about {} units across, typical of millimeters",
                human(s)
            ),
        ),
        _ => (
            Units::Unitless,
            "the model's size doesn't suggest a unit".to_owned(),
        ),
    };
    let (up_axis, up_axis_reason) = match known.and_then(|e| e.3.map(|u| (u, e.1))) {
        Some((UpAxis::YUpToZUp, name)) => (
            UpAxis::YUpToZUp,
            format!("{name} exports Y-up; CAD is Z-up"),
        ),
        Some((UpAxis::AsIs, name)) => (UpAxis::AsIs, format!("{name} already exports Z-up")),
        None => (
            UpAxis::AsIs,
            "exporter unknown; keeping coordinates as-is".to_owned(),
        ),
    };
    Hints {
        exporter: known.map(|e| e.1.to_owned()),
        units,
        units_reason,
        up_axis,
        up_axis_reason,
        bounds,
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

    #[test]
    fn unknown_uses_size() {
        let d = parse(b"v 0 0 0\nv 120 30 4\n").unwrap();
        assert_eq!(hints(&d).units, Units::Millimeters);
        assert_eq!(hints(&d).up_axis, UpAxis::AsIs);
    }
}
