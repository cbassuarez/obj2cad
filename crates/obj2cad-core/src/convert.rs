//! OBJ → format-neutral CAD model.
//!
//! The model keeps references to the source vertices rather than copies, so writers can
//! emit each coordinate's original text. Every transformation offered here is exact in
//! IEEE-754 arithmetic (axis permutation and negation); units are recorded as metadata,
//! never applied by scaling.

use crate::diag::{Code, Diagnostic, Diagnostics, Severity};
use crate::mtl::Palette;
use crate::obj::ObjDocument;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashMap;

/// AutoCAD's default SMOOTHMESHMAXFACE; larger meshes are split so they open with default settings.
pub const MAX_FACES_PER_MESH: usize = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    Unitless,
    Millimeters,
    Centimeters,
    Meters,
    Inches,
    Feet,
}

impl Units {
    /// DXF `$INSUNITS` code.
    pub fn insunits(self) -> i32 {
        match self {
            Units::Unitless => 0,
            Units::Inches => 1,
            Units::Feet => 2,
            Units::Millimeters => 4,
            Units::Centimeters => 5,
            Units::Meters => 6,
        }
    }

    pub fn is_metric(self) -> bool {
        !matches!(self, Units::Inches | Units::Feet)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpAxis {
    /// Coordinates are written exactly as in the OBJ.
    AsIs,
    /// OBJ Y-up to CAD Z-up: (x, y, z) → (x, −z, y). Exact: a permutation and a sign flip.
    YUpToZUp,
}

#[derive(Debug, Clone, Serialize)]
pub struct Options {
    pub units: Units,
    pub up_axis: UpAxis,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            units: Units::Unitless,
            up_axis: UpAxis::AsIs,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Layer {
    /// DXF-safe, unique (case-insensitively) name.
    pub name: String,
    /// The OBJ object/group this layer came from, verbatim.
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct MeshEntity {
    pub layer: u32,
    pub color: Option<[u8; 3]>,
    /// Source vertex ids, in the order they are written.
    pub vertices: Vec<u32>,
    /// Faces as indices into `vertices`: `face_offsets[i]..face_offsets[i+1]`.
    pub face_offsets: Vec<u32>,
    pub face_indices: Vec<u32>,
}

impl MeshEntity {
    pub fn face_count(&self) -> usize {
        self.face_offsets.len() - 1
    }

    pub fn faces(&self) -> impl Iterator<Item = &[u32]> + '_ {
        self.face_offsets
            .windows(2)
            .map(|w| &self.face_indices[w[0] as usize..w[1] as usize])
    }
}

#[derive(Debug, Clone)]
pub struct PolylineEntity {
    pub layer: u32,
    pub color: Option<[u8; 3]>,
    pub vertices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct PointEntity {
    pub layer: u32,
    pub color: Option<[u8; 3]>,
    pub vertex: u32,
}

pub struct CadModel<'a> {
    pub doc: &'a ObjDocument,
    pub options: Options,
    pub layers: Vec<Layer>,
    pub meshes: Vec<MeshEntity>,
    pub polylines: Vec<PolylineEntity>,
    pub points: Vec<PointEntity>,
    /// Parser diagnostics plus everything conversion could not carry over.
    pub diagnostics: Vec<Diagnostic>,
    pub unreferenced_vertices: u64,
}

impl CadModel<'_> {
    /// Output position of source vertex `v` (after the exact axis mapping).
    pub fn position(&self, v: u32) -> [f64; 3] {
        let p = self.doc.positions[v as usize];
        match self.options.up_axis {
            UpAxis::AsIs => p,
            UpAxis::YUpToZUp => [p[0], -p[2], p[1]],
        }
    }

    /// Output text of coordinate `axis` of source vertex `v`: the original token, with
    /// the sign flipped textually where the axis mapping negates.
    pub fn coord_text(&self, v: u32, axis: usize) -> Cow<'_, str> {
        let (t, negate) = self.coord_source(v, axis);
        if !negate {
            Cow::Borrowed(t)
        } else if let Some(rest) = t.strip_prefix('-') {
            Cow::Borrowed(rest)
        } else {
            Cow::Owned(format!("-{}", t.strip_prefix('+').unwrap_or(t)))
        }
    }

    /// The original token behind output coordinate `axis` of vertex `v`, and whether the
    /// axis mapping negates it. Lets writers emit text without allocating.
    pub fn coord_source(&self, v: u32, axis: usize) -> (&str, bool) {
        let (src_axis, negate) = match (self.options.up_axis, axis) {
            (UpAxis::AsIs, a) => (a, false),
            (UpAxis::YUpToZUp, 0) => (0, false),
            (UpAxis::YUpToZUp, 1) => (2, true),
            (UpAxis::YUpToZUp, _) => (1, false),
        };
        (self.doc.coord_text(v as usize, src_axis), negate)
    }

    /// Axis-aligned bounds of all written vertices, or `None` if nothing is written.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut any = false;
        let mut take = |v: u32| {
            let p = self.position(v);
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
            any = true;
        };
        for m in &self.meshes {
            m.vertices.iter().for_each(|&v| take(v));
        }
        for l in &self.polylines {
            l.vertices.iter().for_each(|&v| take(v));
        }
        for p in &self.points {
            take(p.vertex);
        }
        any.then_some((lo, hi))
    }
}

/// Characters AutoCAD rejects in symbol-table names.
const INVALID_NAME_CHARS: &[char] = &[
    '<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', '=', '`', ',',
];

fn dxf_safe_name(raw: &str) -> String {
    let mut s: String = raw
        .trim()
        .chars()
        .map(|c| {
            if INVALID_NAME_CHARS.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.chars().count() > 255 {
        s = s.chars().take(255).collect();
    }
    if s.is_empty() {
        s.push_str("unnamed");
    }
    s
}

fn kd_to_rgb(kd: [f64; 3]) -> [u8; 3] {
    kd.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn convert<'a>(
    doc: &'a ObjDocument,
    palette: Option<&Palette>,
    options: Options,
) -> CadModel<'a> {
    let mut diags = Diagnostics::default();
    note_dropped(doc, &mut diags);
    if palette.is_none() && !doc.materials.is_empty() {
        diags.push(Severity::Info, Code::MaterialColorsUnavailable, 0, || {
            "materials are referenced but no MTL file was provided; entities use layer color".into()
        });
    }

    // ---- layers ----------------------------------------------------------------------
    let mut layers = vec![Layer {
        name: "0".into(),
        source: String::new(),
    }];
    let mut layer_of_attr: Vec<u32> = Vec::with_capacity(doc.attrs.len());
    let mut by_source: HashMap<String, u32> = HashMap::new();
    let mut taken: HashMap<String, ()> = HashMap::from([("0".to_owned(), ())]);
    for a in &doc.attrs {
        let obj = a.object.map(|i| doc.objects[i as usize].as_str());
        let grp = a.group.map(|i| doc.groups[i as usize].as_str());
        let source = match (obj, grp) {
            (Some(o), Some(g)) if o != g => format!("{o}.{g}"),
            (Some(o), _) => o.to_owned(),
            (None, Some(g)) => g.to_owned(),
            (None, None) => {
                layer_of_attr.push(0);
                continue;
            }
        };
        let id = *by_source.entry(source.clone()).or_insert_with(|| {
            let base = dxf_safe_name(&source);
            let mut name = base.clone();
            let mut n = 2;
            while taken.contains_key(&name.to_lowercase()) {
                name = format!("{base}~{n}");
                n += 1;
            }
            taken.insert(name.to_lowercase(), ());
            layers.push(Layer { name, source });
            (layers.len() - 1) as u32
        });
        layer_of_attr.push(id);
    }
    if layers
        .iter()
        .any(|l| !l.source.is_empty() && l.name != l.source)
    {
        diags.push(Severity::Info, Code::LayerRenamed, 0, || {
            "some object/group names were adjusted to be valid, unique layer names (see layer map)".into()
        });
    }

    let color_of_attr = |attr: u32| -> Option<[u8; 3]> {
        let m = doc.attrs[attr as usize].material?;
        palette?
            .get(&doc.materials[m as usize])
            .copied()
            .map(kd_to_rgb)
    };

    // ---- meshes: one per (layer, color) in order of first use, chunked ------------------
    let mut referenced = vec![false; doc.positions.len()];
    let mut meshes: Vec<MeshEntity> = Vec::new();
    let mut open: HashMap<(u32, Option<[u8; 3]>), usize> = HashMap::new();
    // Per-attribute (layer, color) keys, computed once rather than per face.
    let keys: Vec<(u32, Option<[u8; 3]>)> = (0..doc.attrs.len() as u32)
        .map(|a| (layer_of_attr[a as usize], color_of_attr(a)))
        .collect();
    // Source vertex -> (mesh, local index) for the first mesh that used it; vertices shared
    // with other meshes fall back to a map (rare: only on layer/material borders).
    let mut slot: Vec<(u32, u32)> = vec![(u32::MAX, 0); doc.positions.len()];
    let mut shared: HashMap<(u32, u32), u32> = HashMap::new();
    let (mut last_attr, mut last_mi) = (u32::MAX, 0usize);
    for (fi, face) in doc.faces.iter().enumerate() {
        let attr = doc.faces.attr[fi];
        let full = meshes
            .get(last_mi)
            .is_some_and(|m| m.face_count() >= MAX_FACES_PER_MESH);
        if attr != last_attr || full {
            let key = keys[attr as usize];
            last_mi = match open.get(&key) {
                Some(&mi) if meshes[mi].face_count() < MAX_FACES_PER_MESH => mi,
                _ => {
                    meshes.push(MeshEntity {
                        layer: key.0,
                        color: key.1,
                        vertices: Vec::new(),
                        face_offsets: vec![0],
                        face_indices: Vec::new(),
                    });
                    open.insert(key, meshes.len() - 1);
                    meshes.len() - 1
                }
            };
            last_attr = attr;
        }
        let mi = last_mi;
        let m = &mut meshes[mi];
        for &v in face {
            referenced[v as usize] = true;
            let (owner, li) = slot[v as usize];
            let li = if owner == mi as u32 {
                li
            } else if owner == u32::MAX {
                let li = m.vertices.len() as u32;
                m.vertices.push(v);
                slot[v as usize] = (mi as u32, li);
                li
            } else {
                *shared.entry((mi as u32, v)).or_insert_with(|| {
                    m.vertices.push(v);
                    (m.vertices.len() - 1) as u32
                })
            };
            m.face_indices.push(li);
        }
        m.face_offsets.push(m.face_indices.len() as u32);
    }
    let face_vertices = referenced.iter().filter(|r| **r).count();
    let written: usize = meshes.iter().map(|m| m.vertices.len()).sum();
    if written > face_vertices {
        let (n, extra) = (meshes.len(), written - face_vertices);
        diags.push(Severity::Info, Code::SharedVerticesRepeated, 0, || {
            format!(
                "faces are grouped into {n} mesh entities by layer and color; {extra} vertices on their \
                 borders are written once per entity (identical coordinates)"
            )
        });
    }
    let splits = meshes.len() - open.len();
    if splits > 0 {
        diags.push(Severity::Info, Code::MeshSplit, 0, || {
            format!("large meshes were split into parts of at most {MAX_FACES_PER_MESH} faces ({splits} extra entities)")
        });
    }

    let mut polylines = Vec::new();
    for (i, l) in doc.lines.iter().enumerate() {
        let attr = doc.lines.attr[i];
        l.iter().for_each(|&v| referenced[v as usize] = true);
        polylines.push(PolylineEntity {
            layer: layer_of_attr[attr as usize],
            color: color_of_attr(attr),
            vertices: l.to_vec(),
        });
    }
    let mut points = Vec::new();
    for (i, p) in doc.points.iter().enumerate() {
        let attr = doc.points.attr[i];
        for &v in p {
            referenced[v as usize] = true;
            points.push(PointEntity {
                layer: layer_of_attr[attr as usize],
                color: color_of_attr(attr),
                vertex: v,
            });
        }
    }

    let unreferenced = referenced.iter().filter(|r| !**r).count() as u64;
    if unreferenced > 0 {
        diags.push(Severity::Warning, Code::UnreferencedVertices, 0, || {
            format!("{unreferenced} vertices are not used by any face, line or point and were not written")
        });
    }

    let mut diagnostics = doc.diagnostics.clone();
    diagnostics.extend(diags.into_vec());
    diagnostics.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));

    CadModel {
        doc,
        options,
        layers,
        meshes,
        polylines,
        points,
        diagnostics,
        unreferenced_vertices: unreferenced,
    }
}

fn note_dropped(doc: &ObjDocument, d: &mut Diagnostics) {
    let c = &doc.counts;
    let drops: [(u64, Code, &str); 6] = [
        (
            c.texcoords,
            Code::TexcoordsDropped,
            "texture coordinates (vt) are not stored in DXF/DWG",
        ),
        (
            c.normals,
            Code::NormalsDropped,
            "vertex normals (vn) are not stored in DXF/DWG",
        ),
        (
            c.param_vertices,
            Code::ParamVerticesDropped,
            "parameter-space vertices (vp) were not written",
        ),
        (
            c.weighted_vertices,
            Code::VertexWeightDropped,
            "homogeneous vertex weights (w ≠ 1) were not written",
        ),
        (
            c.vertices_with_color,
            Code::VertexColorsDropped,
            "per-vertex colors are not written yet",
        ),
        (
            c.smoothing_statements,
            Code::SmoothingGroupsIgnored,
            "smoothing groups (s) have no DXF/DWG equivalent",
        ),
    ];
    for (n, code, msg) in drops {
        if n > 0 {
            let sev = if code == Code::SmoothingGroupsIgnored {
                Severity::Info
            } else {
                Severity::Warning
            };
            d.push(sev, code, 0, || format!("{msg} ({n} in source)"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obj::parse;

    #[test]
    fn axis_swap_is_exact_including_text_and_signed_zero() {
        let doc = parse(b"v 1.5 -0.000000 2.25\nv 0 3 -4\np 1 2\n").unwrap();
        let m = convert(
            &doc,
            None,
            Options {
                up_axis: UpAxis::YUpToZUp,
                ..Default::default()
            },
        );
        assert_eq!(m.position(0), [1.5, -2.25, -0.0]);
        assert!(m.position(0)[2].is_sign_negative());
        assert_eq!(m.coord_text(0, 1), "-2.25");
        assert_eq!(m.coord_text(0, 2), "-0.000000");
        assert_eq!(m.coord_text(1, 1), "4");
    }

    #[test]
    fn layers_are_valid_and_unique() {
        let doc =
            parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\no a/b\nf 1 2 3\no a_b\nf 1 2 3\no A_B\nf 1 2 3\n")
                .unwrap();
        let m = convert(&doc, None, Options::default());
        let names: Vec<_> = m.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["0", "a_b", "a_b~2", "A_B~3"]);
        assert!(m.diagnostics.iter().any(|d| d.code == Code::LayerRenamed));
    }

    #[test]
    fn materials_split_meshes_by_color() {
        let doc = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl red\nf 1 2 3\nusemtl blue\nf 1 2 3\nusemtl red\nf 3 2 1\n").unwrap();
        let pal = crate::mtl::parse(b"newmtl red\nKd 1 0 0\nnewmtl blue\nKd 0 0 1\n");
        let m = convert(&doc, Some(&pal), Options::default());
        assert_eq!(m.meshes.len(), 2);
        assert_eq!(m.meshes[0].color, Some([255, 0, 0]));
        assert_eq!(m.meshes[0].face_count(), 2);
    }

    #[test]
    fn unreferenced_vertices_are_reported() {
        let doc = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nv 9 9 9\nf 1 2 3\n").unwrap();
        let m = convert(&doc, None, Options::default());
        assert_eq!(m.unreferenced_vertices, 1);
    }
}
