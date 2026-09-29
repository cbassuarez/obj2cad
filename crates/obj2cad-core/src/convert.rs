//! OBJ → format-neutral CAD model.
//!
//! The model keeps references to the source vertices rather than copies, so writers can
//! emit each coordinate's original text. Every transformation offered here is exact in
//! IEEE-754 arithmetic (axis permutation and negation); units are recorded as metadata,
//! never applied by scaling.

use crate::diag::{Code, Diagnostic, Diagnostics, Severity};
use crate::mtl::Palette;
use crate::obj::{ObjDocument, NO_UV};
use crate::texture::{self, Texture};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// AutoCAD's default SMOOTHMESHMAXFACE; larger meshes are split so they open with default settings.
pub const MAX_FACES_PER_MESH: usize = 1_000_000;

/// Face colors sampled from one texture are reduced to at most this many, so a textured
/// model becomes a few meshes rather than one per face.
pub const TEXTURE_COLORS: usize = 32;

/// Material colors and textures for a conversion.
#[derive(Default)]
pub struct Materials<'a> {
    pub palette: Option<&'a Palette>,
    /// Material name → its diffuse texture.
    pub textures: HashMap<&'a str, Texture<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    /// Short symbol: `mm`, `in`, … (empty for unitless).
    pub fn symbol(self) -> &'static str {
        match self {
            Units::Unitless => "",
            Units::Millimeters => "mm",
            Units::Centimeters => "cm",
            Units::Meters => "m",
            Units::Inches => "in",
            Units::Feet => "ft",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpAxis {
    /// Coordinates are written exactly as in the OBJ.
    AsIs,
    /// OBJ Y-up to CAD Z-up: (x, y, z) → (x, −z, y). Exact: a permutation and a sign flip.
    YUpToZUp,
}

/// What names the layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerMode {
    /// One layer per object (`o`); group names (`g`) when a file has no objects.
    #[default]
    Objects,
    /// One layer per group (`g`); object names when a file has no groups.
    Groups,
    /// One layer per material (`usemtl`).
    Materials,
    /// Everything on one layer.
    Single,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub units: Units,
    pub up_axis: UpAxis,
    #[serde(default)]
    pub layer_mode: LayerMode,
    /// Layer for geometry with no object/group/material name (AutoCAD reserves layer 0
    /// by convention). Usually the file's name.
    #[serde(default = "default_layer")]
    pub default_layer: String,
    /// Write vertices that no face, line or point uses as POINT entities.
    #[serde(default)]
    pub keep_loose_points: bool,
    /// Output layer names to leave out (for "visible layers only").
    #[serde(default)]
    pub exclude_layers: Vec<String>,
    /// Also write curved surfaces recognized in the mesh (the mesh itself is unchanged).
    #[serde(default)]
    pub curves: bool,
}

fn default_layer() -> String {
    "OBJ".to_owned()
}

impl Default for Options {
    fn default() -> Self {
        Self {
            units: Units::Unitless,
            up_axis: UpAxis::AsIs,
            layer_mode: LayerMode::Objects,
            default_layer: default_layer(),
            keep_loose_points: false,
            exclude_layers: Vec::new(),
            curves: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Layer {
    /// DXF-safe, unique (case-insensitively) name.
    pub name: String,
    /// The OBJ object/group/material this layer came from, verbatim ("" for layer 0 and
    /// the default layer).
    pub source: String,
    /// Display color, also written to the DXF layer table.
    pub color: [u8; 3],
    /// The file of a bundle all of this layer's geometry came from; `None` for a single
    /// file, or when the layer holds geometry of several files.
    pub file: Option<String>,
}

/// Distinct, calm layer colors (readable on AutoCAD's dark and light backgrounds).
pub const LAYER_COLORS: [[u8; 3]; 8] = [
    [124, 158, 201],
    [201, 150, 110],
    [128, 180, 150],
    [190, 140, 180],
    [210, 190, 120],
    [120, 170, 180],
    [180, 130, 130],
    [176, 184, 196],
];

/// Color of the n-th layer (0 = AutoCAD's layer "0").
pub fn layer_color(n: u32) -> [u8; 3] {
    if n == 0 {
        return [255, 255, 255];
    }
    LAYER_COLORS[(n as usize - 1) % LAYER_COLORS.len()]
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
    /// Index of each face in the source document (`ObjDocument::faces`).
    pub source_faces: Vec<u32>,
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

/// A curved surface (or solid) recognized in the mesh, as an ACIS body. Written next to
/// the exact mesh, never instead of it.
#[derive(Debug, Clone)]
pub struct SurfaceEntity {
    pub layer: u32,
    pub color: Option<[u8; 3]>,
    pub body: obj2cad_acis::Body,
    /// What it was recognized from, for the report (`None` for test bodies).
    pub region: Option<Region>,
}

/// The mesh region a surface was recognized from. Every vertex of every face in it lies
/// on the surface within that vertex's tolerance.
#[derive(Debug, Clone, Serialize)]
pub struct Region {
    /// `cylinder`, `cone`, `sphere` or `torus`.
    pub kind: &'static str,
    /// Source faces (indices into `ObjDocument::faces`) that lie on the surface.
    pub faces: Vec<u32>,
    /// Largest distance of any of their vertices from the surface, in model units.
    pub max_deviation: f64,
    /// Largest distance allowed for any of them (each vertex's own precision).
    pub tolerance: f64,
}

/// A B-spline from an OBJ free-form curve: the control points are source vertices
/// (exact), the knots and weights the file's own numbers.
#[derive(Debug, Clone)]
pub struct SplineEntity {
    pub layer: u32,
    pub color: Option<[u8; 3]>,
    pub degree: u32,
    pub knots: Vec<f64>,
    pub control: Vec<u32>,
    /// One per control point, for rational curves.
    pub weights: Option<Vec<f64>>,
}

#[derive(Debug, Clone)]
pub struct PointEntity {
    pub layer: u32,
    pub color: Option<[u8; 3]>,
    pub vertex: u32,
}

/// Geometry in the source that is not in the output.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Omissions {
    /// Free-form (NURBS) surfaces and curves: not converted yet.
    pub freeform_surfaces: u64,
    pub freeform_curves: u64,
    /// Faces with fewer than 3 vertices.
    pub broken_faces: u64,
    /// Vertices no element uses, not kept as points.
    pub loose_points: u64,
    /// Layers left out on request, and what they held.
    pub excluded_layers: Vec<String>,
    pub excluded_faces: u64,
    pub excluded_lines: u64,
    pub excluded_points: u64,
}

impl Omissions {
    /// True when geometry was left out that the user didn't ask to leave out.
    pub fn is_partial(&self) -> bool {
        self.freeform_surfaces + self.freeform_curves + self.broken_faces > 0
    }
}

pub struct CadModel<'a> {
    pub doc: &'a ObjDocument,
    pub options: Options,
    pub layers: Vec<Layer>,
    pub meshes: Vec<MeshEntity>,
    pub polylines: Vec<PolylineEntity>,
    pub points: Vec<PointEntity>,
    /// Free-form curves from the file.
    pub splines: Vec<SplineEntity>,
    /// Curved surfaces recognized in the mesh (empty unless requested).
    pub surfaces: Vec<SurfaceEntity>,
    /// Parser diagnostics plus everything conversion could not carry over.
    pub diagnostics: Vec<Diagnostic>,
    pub unreferenced_vertices: u64,
    pub omissions: Omissions,
    /// The file has no elements at all, so every vertex was written as a point.
    pub point_cloud: bool,
    /// Faces whose color was sampled from a texture (approximate colors).
    pub texture_colored_faces: u64,
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
        for c in &self.splines {
            c.control.iter().for_each(|&v| take(v));
        }
        for s in &self.surfaces {
            if let Some((a, b)) = s.body.bounds() {
                for axis in 0..3 {
                    lo[axis] = lo[axis].min(a[axis]);
                    hi[axis] = hi[axis].max(b[axis]);
                }
                any = true;
            }
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

/// Builds the layer table: layer 0 first (required by DXF, never used for geometry),
/// then layers in order of first use, with valid, case-insensitively unique names.
struct LayerTable {
    layers: Vec<Layer>,
    by_source: HashMap<String, u32>,
    taken: HashSet<String>,
    default: Option<u32>,
    default_name: String,
}

impl LayerTable {
    fn new(default_name: &str) -> Self {
        Self {
            layers: vec![Layer {
                name: "0".into(),
                source: String::new(),
                color: layer_color(0),
                file: None,
            }],
            by_source: HashMap::new(),
            taken: HashSet::from(["0".to_owned()]),
            default: None,
            default_name: default_name.to_owned(),
        }
    }

    fn unique(&mut self, base: &str) -> String {
        let base = dxf_safe_name(base);
        let mut name = base.clone();
        let mut n = 2;
        while self.taken.contains(&name.to_lowercase()) {
            name = format!("{base}~{n}");
            n += 1;
        }
        self.taken.insert(name.to_lowercase());
        name
    }

    fn named(&mut self, source: &str) -> u32 {
        if let Some(&id) = self.by_source.get(source) {
            return id;
        }
        let name = self.unique(source);
        let id = self.layers.len() as u32;
        self.layers.push(Layer {
            name,
            source: source.to_owned(),
            color: layer_color(id),
            file: None,
        });
        self.by_source.insert(source.to_owned(), id);
        id
    }

    fn default_layer(&mut self) -> u32 {
        if let Some(id) = self.default {
            return id;
        }
        let base = if self.default_name.trim().is_empty() {
            "OBJ".to_owned()
        } else {
            self.default_name.clone()
        };
        let name = self.unique(&base);
        let id = self.layers.len() as u32;
        self.layers.push(Layer {
            name,
            source: String::new(),
            color: layer_color(id),
            file: None,
        });
        self.default = Some(id);
        id
    }
}

pub fn convert<'a>(
    doc: &'a ObjDocument,
    palette: Option<&Palette>,
    options: Options,
) -> CadModel<'a> {
    convert_with(
        doc,
        &Materials {
            palette,
            textures: HashMap::new(),
        },
        options,
    )
}

/// Color of vertex `v` from the file (`v x y z r g b`, or a point cloud's RGB columns).
fn vertex_rgb(doc: &ObjDocument, v: u32) -> Option<[u8; 3]> {
    doc.colors
        .get(v as usize)
        .copied()
        .flatten()
        .map(|c| c.map(|x| (f64::from(x).clamp(0.0, 1.0) * 255.0).round() as u8))
}

/// Per-face colors sampled from textures, reduced per texture (see [`TEXTURE_COLORS`]).
/// `None` for faces that keep their material color.
fn texture_colors(doc: &ObjDocument, materials: &Materials) -> Option<Vec<Option<[u8; 3]>>> {
    if materials.textures.is_empty() || doc.face_uvs.is_empty() {
        return None;
    }
    let tex_of_attr: Vec<Option<(usize, &Texture)>> = doc
        .attrs
        .iter()
        .map(|a| {
            let m = a.material?;
            let name = doc.materials[m as usize].as_str();
            materials.textures.get(name).map(|t| (m as usize, t))
        })
        .collect();
    if tex_of_attr.iter().all(Option::is_none) {
        return None;
    }
    let mut out: Vec<Option<[u8; 3]>> = vec![None; doc.faces.len()];
    // Sampled colors per material, then reduced.
    let mut sampled: HashMap<usize, (Vec<usize>, Vec<[u8; 3]>)> = HashMap::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    for fi in 0..doc.faces.len() {
        let Some((m, tex)) = tex_of_attr[doc.faces.attr[fi] as usize] else {
            continue;
        };
        let range = doc.faces.offsets[fi] as usize..doc.faces.offsets[fi + 1] as usize;
        uvs.clear();
        for &t in &doc.face_uvs[range] {
            if t == NO_UV {
                break;
            }
            uvs.push(doc.texcoords[t as usize]);
        }
        if uvs.len() != doc.faces.get(fi).len() {
            continue;
        }
        let c = tex
            .face_color(&uvs)
            .map(|x| x.round().clamp(0.0, 255.0) as u8);
        let e = sampled.entry(m).or_default();
        e.0.push(fi);
        e.1.push(c);
    }
    let mut keys: Vec<usize> = sampled.keys().copied().collect();
    keys.sort_unstable();
    for m in keys {
        let (faces, colors) = &sampled[&m];
        let (palette, index) = texture::quantize(colors, TEXTURE_COLORS);
        for (&fi, &k) in faces.iter().zip(&index) {
            out[fi] = Some(palette[k as usize]);
        }
    }
    Some(out)
}

/// [`convert`] with textures: faces on a textured material that have texture coordinates
/// are colored from the image (one color per face, approximate). Shapes are unchanged.
pub fn convert_with<'a>(
    doc: &'a ObjDocument,
    materials: &Materials,
    options: Options,
) -> CadModel<'a> {
    let palette = materials.palette;
    let mut diags = Diagnostics::default();
    note_dropped(doc, &mut diags);
    if palette.is_none() && !doc.materials.is_empty() {
        diags.push(Severity::Info, Code::MaterialColorsUnavailable, 0, || {
            "materials are referenced but no MTL file was provided; entities use layer color".into()
        });
    }

    // ---- layers ----------------------------------------------------------------------
    let mut table = LayerTable::new(&options.default_layer);
    let has_objects = !doc.objects.is_empty();
    let has_groups = !doc.groups.is_empty();
    let layer_of_attr: Vec<u32> = doc
        .attrs
        .iter()
        .map(|a| {
            let obj = a.object.map(|i| doc.objects[i as usize].as_str());
            let grp = a.group.map(|i| doc.groups[i as usize].as_str());
            let mat = a.material.map(|i| doc.materials[i as usize].as_str());
            let source = match options.layer_mode {
                LayerMode::Objects if has_objects => obj,
                LayerMode::Objects => grp,
                LayerMode::Groups if has_groups => grp,
                LayerMode::Groups => obj,
                LayerMode::Materials => mat,
                LayerMode::Single => None,
            };
            match source {
                Some(s) if !s.trim().is_empty() => table.named(s),
                _ => table.default_layer(),
            }
        })
        .collect();
    // Which file each layer's geometry came from (bundles of several files).
    if !doc.attr_file.is_empty() {
        let mut seen: Vec<Option<Option<u32>>> = vec![None; table.layers.len()];
        for (a, &l) in layer_of_attr.iter().enumerate() {
            let f = doc.attr_file[a];
            let s = &mut seen[l as usize];
            *s = match *s {
                None => Some(Some(f)),
                Some(Some(g)) if g == f => Some(Some(f)),
                _ => Some(None),
            };
        }
        for (layer, s) in table.layers.iter_mut().zip(seen) {
            layer.file = s.flatten().map(|f| doc.files[f as usize].clone());
        }
    }
    if table
        .layers
        .iter()
        .any(|l| !l.source.is_empty() && l.name != l.source)
    {
        diags.push(Severity::Info, Code::LayerRenamed, 0, || {
            "some names were adjusted to be valid, unique layer names (see layer map)".into()
        });
    }

    // Layers left out on request.
    let excluded: HashSet<u32> = table
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            options
                .exclude_layers
                .iter()
                .any(|x| x.eq_ignore_ascii_case(&l.name))
        })
        .map(|(i, _)| i as u32)
        .collect();
    let mut omissions = Omissions {
        freeform_surfaces: doc.counts.freeform_surfaces,
        freeform_curves: doc.counts.freeform_curves,
        broken_faces: doc.counts.faces_skipped,
        excluded_layers: excluded
            .iter()
            .map(|&i| table.layers[i as usize].name.clone())
            .collect(),
        ..Default::default()
    };
    omissions.excluded_layers.sort();

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
    let face_colors = texture_colors(doc, materials);
    let texture_colored_faces = face_colors
        .as_ref()
        .map_or(0, |c| c.iter().filter(|x| x.is_some()).count() as u64);
    if texture_colored_faces > 0 {
        diags.push(Severity::Info, Code::TextureColors, 0, || {
            format!(
                "{texture_colored_faces} faces are colored from textures, at most {TEXTURE_COLORS} colors per texture (approximate)"
            )
        });
    }
    // Source vertex -> (mesh, local index) for the first mesh that used it; vertices shared
    // with other meshes fall back to a map (rare: only on layer/material borders).
    let mut slot: Vec<(u32, u32)> = vec![(u32::MAX, 0); doc.positions.len()];
    let mut shared: HashMap<(u32, u32), u32> = HashMap::new();
    let (mut last_key, mut last_mi) = (None, 0usize);
    for (fi, face) in doc.faces.iter().enumerate() {
        let attr = doc.faces.attr[fi];
        let mut key = keys[attr as usize];
        if let Some(c) = face_colors.as_ref().and_then(|c| c[fi]) {
            key.1 = Some(c);
        }
        if excluded.contains(&key.0) {
            omissions.excluded_faces += 1;
            face.iter().for_each(|&v| referenced[v as usize] = true);
            continue;
        }
        let full = meshes
            .get(last_mi)
            .is_some_and(|m| m.face_count() >= MAX_FACES_PER_MESH);
        if last_key != Some(key) || full {
            last_mi = match open.get(&key) {
                Some(&mi) if meshes[mi].face_count() < MAX_FACES_PER_MESH => mi,
                _ => {
                    meshes.push(MeshEntity {
                        layer: key.0,
                        color: key.1,
                        vertices: Vec::new(),
                        face_offsets: vec![0],
                        face_indices: Vec::new(),
                        source_faces: Vec::new(),
                    });
                    open.insert(key, meshes.len() - 1);
                    meshes.len() - 1
                }
            };
            last_key = Some(key);
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
        m.source_faces.push(fi as u32);
    }
    let face_vertices: usize = {
        let mut seen = vec![false; doc.positions.len()];
        let mut n = 0;
        for m in &meshes {
            for &v in &m.vertices {
                if !seen[v as usize] {
                    seen[v as usize] = true;
                    n += 1;
                }
            }
        }
        n
    };
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
        if excluded.contains(&layer_of_attr[attr as usize]) {
            omissions.excluded_lines += 1;
            continue;
        }
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
            if excluded.contains(&layer_of_attr[attr as usize]) {
                omissions.excluded_points += 1;
                continue;
            }
            points.push(PointEntity {
                layer: layer_of_attr[attr as usize],
                color: vertex_rgb(doc, v).or_else(|| color_of_attr(attr)),
                vertex: v,
            });
        }
    }

    let mut splines = Vec::new();
    for c in &doc.curves {
        c.control
            .iter()
            .for_each(|&v| referenced[v as usize] = true);
        let layer = layer_of_attr[c.attr as usize];
        if excluded.contains(&layer) {
            omissions.excluded_lines += 1;
            continue;
        }
        splines.push(SplineEntity {
            layer,
            color: color_of_attr(c.attr),
            degree: c.degree,
            knots: c.knots.clone(),
            control: c.control.clone(),
            weights: c.rational.then(|| {
                c.control
                    .iter()
                    .map(|&v| doc.weights.get(v as usize).copied().unwrap_or(1.0))
                    .collect()
            }),
        });
    }
    // Only the free-form curves that couldn't be written are left out.
    omissions.freeform_curves = doc
        .counts
        .freeform_curves
        .saturating_sub(doc.curves.len() as u64);

    // Loose vertices: a file of nothing but vertices is a point cloud, written as points.
    // In other files they are usually leftovers, kept only on request.
    let unreferenced = referenced.iter().filter(|r| !**r).count() as u64;
    let point_cloud = doc.faces.is_empty()
        && doc.lines.is_empty()
        && doc.points.is_empty()
        && doc.curves.is_empty()
        && !doc.positions.is_empty();
    if unreferenced > 0 && (point_cloud || options.keep_loose_points) {
        let layer = table.default_layer();
        if options
            .exclude_layers
            .iter()
            .any(|x| x.eq_ignore_ascii_case(&table.layers[layer as usize].name))
        {
            omissions.excluded_points += unreferenced;
        } else {
            for (v, used) in referenced.iter().enumerate() {
                if !used {
                    points.push(PointEntity {
                        layer,
                        color: vertex_rgb(doc, v as u32),
                        vertex: v as u32,
                    });
                }
            }
        }
        diags.push(Severity::Info, Code::LooseVerticesAsPoints, 0, || {
            format!("{unreferenced} vertices that no face or line uses were written as points")
        });
    } else if unreferenced > 0 {
        omissions.loose_points = unreferenced;
        diags.push(Severity::Warning, Code::UnreferencedVertices, 0, || {
            format!("{unreferenced} vertices are not used by any face, line or point and were not written")
        });
    }
    if !omissions.excluded_layers.is_empty() {
        let names = omissions.excluded_layers.join(", ");
        diags.push(Severity::Info, Code::LayersExcluded, 0, || {
            format!("layers left out on request: {names}")
        });
    }

    let mut diagnostics = doc.diagnostics.clone();
    diagnostics.extend(diags.into_vec());
    diagnostics.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));

    CadModel {
        doc,
        options,
        layers: table.layers,
        meshes,
        polylines,
        points,
        splines,
        surfaces: Vec::new(),
        diagnostics,
        unreferenced_vertices: unreferenced,
        omissions,
        point_cloud,
        texture_colored_faces,
    }
}

fn note_dropped(doc: &ObjDocument, d: &mut Diagnostics) {
    let c = &doc.counts;
    // Rational curves carry their control vertices' weights; other weights are lost.
    let weights_lost = if c.weighted_vertices == 0 {
        0
    } else {
        let mut carried = vec![false; doc.positions.len()];
        for curve in doc.curves.iter().filter(|x| x.rational) {
            curve
                .control
                .iter()
                .for_each(|&v| carried[v as usize] = true);
        }
        doc.weights
            .iter()
            .enumerate()
            .filter(|&(v, &w)| w != 1.0 && !carried[v])
            .count() as u64
    };
    // Points carry their vertex's color; faces and lines carry one color per entity.
    let colors_lost = if c.vertices_with_color == 0 {
        0
    } else {
        let mut seen = vec![false; doc.positions.len()];
        for &v in doc.faces.indices.iter().chain(&doc.lines.indices) {
            if doc.colors[v as usize].is_some() {
                seen[v as usize] = true;
            }
        }
        seen.iter().filter(|x| **x).count() as u64
    };
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
            weights_lost,
            Code::VertexWeightDropped,
            "homogeneous vertex weights (w ≠ 1) were not written",
        ),
        (
            colors_lost,
            Code::VertexColorsDropped,
            "per-vertex colors on faces and lines are not written",
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

    fn names(m: &CadModel) -> Vec<String> {
        m.layers.iter().map(|l| l.name.clone()).collect()
    }

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
        assert_eq!(names(&m), ["0", "a_b", "a_b~2", "A_B~3"]);
        assert!(m.diagnostics.iter().any(|d| d.code == Code::LayerRenamed));
    }

    #[test]
    fn unnamed_geometry_goes_on_the_default_layer_not_layer_0() {
        let doc = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\no Lid\nf 3 2 1\n").unwrap();
        let m = convert(
            &doc,
            None,
            Options {
                default_layer: "chair".into(),
                ..Default::default()
            },
        );
        assert_eq!(names(&m), ["0", "chair", "Lid"]);
        assert!(m.meshes.iter().all(|e| e.layer != 0));
        assert_eq!(m.layers[1].color, LAYER_COLORS[0]);
    }

    #[test]
    fn layer_modes() {
        let src = b"v 0 0 0\nv 1 0 0\nv 0 1 0\no Chair\ng seat\nusemtl wood\nf 1 2 3\ng back\nusemtl steel\nf 3 2 1\n";
        let doc = parse(src).unwrap();
        let by = |mode| {
            names(&convert(
                &doc,
                None,
                Options {
                    layer_mode: mode,
                    ..Default::default()
                },
            ))
        };
        assert_eq!(by(LayerMode::Objects), ["0", "Chair"]);
        assert_eq!(by(LayerMode::Groups), ["0", "seat", "back"]);
        assert_eq!(by(LayerMode::Materials), ["0", "wood", "steel"]);
        assert_eq!(by(LayerMode::Single), ["0", "OBJ"]);
    }

    #[test]
    fn materials_split_meshes_by_color() {
        let doc =
            parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl red\nf 1 2 3\nusemtl blue\nf 1 2 3\nusemtl red\nf 3 2 1\n").unwrap();
        let pal = crate::mtl::parse(b"newmtl red\nKd 1 0 0\nnewmtl blue\nKd 0 0 1\n");
        let m = convert(&doc, Some(&pal), Options::default());
        assert_eq!(m.meshes.len(), 2);
        assert_eq!(m.meshes[0].color, Some([255, 0, 0]));
        assert_eq!(m.meshes[0].face_count(), 2);
    }

    #[test]
    fn loose_vertices() {
        let doc = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nv 9 9 9\nf 1 2 3\n").unwrap();
        let m = convert(&doc, None, Options::default());
        assert_eq!(
            (
                m.unreferenced_vertices,
                m.points.len(),
                m.omissions.loose_points
            ),
            (1, 0, 1)
        );
        let m = convert(
            &doc,
            None,
            Options {
                keep_loose_points: true,
                ..Default::default()
            },
        );
        assert_eq!((m.points.len(), m.omissions.loose_points), (1, 0));
        assert_eq!(m.points[0].vertex, 3);
    }

    #[test]
    fn point_clouds_become_points() {
        let doc = parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\n").unwrap();
        let m = convert(
            &doc,
            None,
            Options {
                default_layer: "scan".into(),
                ..Default::default()
            },
        );
        assert!(m.point_cloud);
        assert_eq!(m.points.len(), 3);
        assert_eq!(m.layers[m.points[0].layer as usize].name, "scan");
    }

    #[test]
    fn excluded_layers_are_left_out_and_recorded() {
        let doc =
            parse(b"v 0 0 0\nv 1 0 0\nv 0 1 0\no Keep\nf 1 2 3\no Drop\nf 3 2 1\nl 1 2\n").unwrap();
        let m = convert(
            &doc,
            None,
            Options {
                exclude_layers: vec!["drop".into()],
                ..Default::default()
            },
        );
        assert_eq!(m.meshes.iter().map(|e| e.face_count()).sum::<usize>(), 1);
        assert!(m.polylines.is_empty());
        assert_eq!(m.omissions.excluded_layers, ["Drop"]);
        assert_eq!(
            (m.omissions.excluded_faces, m.omissions.excluded_lines),
            (1, 1)
        );
        assert!(
            !m.omissions.is_partial(),
            "leaving out what was asked isn't partial"
        );
        assert_eq!(
            m.unreferenced_vertices, 0,
            "vertices of excluded faces aren't 'loose'"
        );
    }

    #[test]
    fn omissions_track_what_is_missing() {
        let doc = parse(
            b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\nf 1 2\ncstype bspline\nsurf 0 1 0 1 1 2 3\n",
        )
        .unwrap();
        let m = convert(&doc, None, Options::default());
        assert_eq!(
            (m.omissions.broken_faces, m.omissions.freeform_surfaces),
            (1, 1)
        );
        assert!(m.omissions.is_partial());
    }
}
