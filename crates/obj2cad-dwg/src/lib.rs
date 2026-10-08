//! Exact DWG (R2018 / AC1032) writer, built on [acadrust].
//!
//! DWG stores coordinates as raw IEEE-754 doubles, so every coordinate is exact by
//! construction. The one trap is negative zero: acadrust 0.5.5 folds `-0.0` into the
//! short code for `+0.0`, which obj2cad's fork fixes (`vendor/acadrust/OBJ2CAD.md`).
//! The tests read every file back and compare every coordinate's bits.
//!
//! The drawing mirrors the DXF writer: the same layers and colors, the same entities
//! (MESH, 3D POLYLINE, POINT), units, dates, custom properties and opening view.
//!
//! A drawing with many points (a scan) streams them: they are encoded one at a time as
//! the file is written, never held as acadrust entities, and the file goes out as it is
//! made (see obj2cad's fork of acadrust, `DwgWriter::write_streaming`).

use acadrust::entities::solid3d::AcisData;
use acadrust::entities::surface::SurfaceKind;
use acadrust::entities::{
    Mesh, MeshFace, Point, Polyline3D, Solid3D, Spline, Surface, Vertex3DPolyline,
};
use acadrust::tables::TableEntry;
use acadrust::{
    CadDocument, Color, DwgWriter, DxfVersion, EntityType, Layer, PointStream, Vector2, Vector3,
};
use obj2cad_core::convert::CadModel;
use obj2cad_core::output::{fitted_view, Meta, VIEW_DIRECTION};
use std::io::{Seek, Write};

/// The acadrust in use (patched), for tools that read DWG back.
pub use acadrust;

/// Report and file-property id of this format.
pub const FORMAT_ID: &str = "dwg-r2018";

/// Writing failed inside acadrust. Not expected for any model obj2cad produces.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DWG writer: {}", self.0)
    }
}

impl std::error::Error for Error {}

fn fail(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}

fn v3(p: [f64; 3]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::Rgb { r, g, b }
}

fn color(c: Option<[u8; 3]>) -> Color {
    c.map_or(Color::ByLayer, rgb)
}

/// Drawings with more points than this stream them (see the module docs). Below it the
/// file is laid out as acadrust always has, the layout checked in AutoCAD.
pub const STREAM_POINTS: usize = 1 << 20;

/// Build the drawing as an acadrust document.
pub fn document(model: &CadModel, meta: &Meta) -> Result<CadDocument, Error> {
    document_with(model, meta, true)
}

/// [`document`], with or without the points (streamed instead, see [`write_to`]).
fn document_with(model: &CadModel, meta: &Meta, points: bool) -> Result<CadDocument, Error> {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);

    let h = &mut doc.header;
    h.insertion_units = model.options.units.insunits() as i16;
    h.measurement = i16::from(model.options.units.is_metric());
    h.create_date_julian = meta.julian_date();
    h.update_date_julian = meta.julian_date();
    h.fingerprint_guid = meta.fingerprint_guid();
    h.version_guid = meta.version_guid();
    h.last_saved_by = "obj2cad".into();
    if let Some((lo, hi)) = model.bounds() {
        h.model_space_extents_min = v3(lo);
        h.model_space_extents_max = v3(hi);
    }
    doc.summary_info.last_saved_by = "obj2cad".into();
    doc.summary_info.custom_properties = meta
        .properties
        .iter()
        .map(|&(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

    if let Some(view) = fitted_view(model) {
        if let Some(vp) = doc.vports.get_mut("*Active") {
            vp.view_center = Vector2::new(0.0, 0.0);
            vp.view_direction = v3(VIEW_DIRECTION);
            vp.view_target = v3(view.target);
            vp.view_height = view.height;
        }
    }

    // Layer "0" (index 0) comes with every drawing and is never used for geometry.
    for layer in model.layers.iter().skip(1) {
        let mut l = Layer::with_color(layer.name.as_str(), rgb(layer.color));
        l.set_handle(doc.allocate_handle());
        doc.layers.add(l).map_err(fail)?;
    }

    for m in &model.meshes {
        let mut mesh = Mesh::new();
        mesh.common.layer = model.layers[m.layer as usize].name.clone();
        mesh.common.color = color(m.color);
        mesh.blend_crease = false;
        mesh.vertices = m.vertices.iter().map(|&v| v3(model.position(v))).collect();
        mesh.faces = m
            .faces()
            .map(|f| MeshFace::new(f.iter().map(|&i| i as usize).collect()))
            .collect();
        doc.add_entity(EntityType::Mesh(mesh)).map_err(fail)?;
    }
    for l in &model.polylines {
        let layer = &model.layers[l.layer as usize].name;
        let mut polyline = Polyline3D::new();
        polyline.common.layer = layer.clone();
        polyline.common.color = color(l.color);
        polyline.vertices = l
            .vertices
            .iter()
            .map(|&v| Vertex3DPolyline {
                layer: layer.clone(),
                ..Vertex3DPolyline::new(v3(model.position(v)))
            })
            .collect();
        doc.add_entity(EntityType::Polyline3D(polyline))
            .map_err(fail)?;
    }
    for c in &model.splines {
        let mut spline = Spline::new();
        spline.common.layer = model.layers[c.layer as usize].name.clone();
        spline.common.color = color(c.color);
        spline.degree = c.degree as i32;
        spline.flags.rational = c.weights.is_some();
        spline.knots = c.knots.clone();
        spline.control_points = c.control.iter().map(|&v| v3(model.position(v))).collect();
        spline.weights = c.weights.clone().unwrap_or_default();
        doc.add_entity(EntityType::Spline(spline)).map_err(fail)?;
    }
    for p in model.points.iter().filter(|_| points) {
        let mut point = Point::new();
        point.common.layer = model.layers[p.layer as usize].name.clone();
        point.common.color = color(p.color);
        point.location = v3(model.position(p.vertex));
        doc.add_entity(EntityType::Point(point)).map_err(fail)?;
    }
    // Curved surfaces: SAB, which acadrust stores in the AcDs section (R2013+).
    let product = format!("obj2cad {}", obj2cad_core::VERSION);
    for s in &model.surfaces {
        let sab = obj2cad_acis::sab::write(&s.body, &product, meta.created_unix.unwrap_or(0.0));
        let layer = model.layers[s.layer as usize].name.clone();
        let entity = if s.body.solid {
            let mut e = Solid3D::new();
            e.common.layer = layer;
            e.common.color = color(s.color);
            e.acis_data = AcisData::from_sab(sab);
            EntityType::Solid3D(e)
        } else {
            let mut e = Surface::new(SurfaceKind::Generic);
            e.common.layer = layer;
            e.common.color = color(s.color);
            e.acis_data = AcisData::from_sab(sab);
            e.u_isolines = 6;
            e.v_isolines = 6;
            EntityType::Surface(e)
        };
        doc.add_entity(entity).map_err(fail)?;
    }
    Ok(doc)
}

/// Write `model` as DWG R2018.
pub fn write(model: &CadModel, meta: &Meta) -> Result<Vec<u8>, Error> {
    let mut out = std::io::Cursor::new(Vec::new());
    write_to(model, meta, &mut out)?;
    Ok(out.into_inner())
}

/// Write `model` as DWG R2018 to `out`, in order but for its first 256 bytes, which are
/// written again at the end (a seek to the start and back to the end; see [`HeldHead`]
/// for outputs that can't seek).
pub fn write_to<W: Write + Seek>(model: &CadModel, meta: &Meta, out: W) -> Result<(), Error> {
    write_streaming_over(model, meta, out, STREAM_POINTS)
}

/// [`write_to`], streaming the points when there are more than `over`.
fn write_streaming_over<W: Write + Seek>(
    model: &CadModel,
    meta: &Meta,
    out: W,
    over: usize,
) -> Result<(), Error> {
    if model.points.len() <= over {
        return DwgWriter::write_to_writer(out, &document(model, meta)?).map_err(fail);
    }
    let doc = document_with(model, meta, false)?;
    DwgWriter::write_streaming(out, &doc, &ModelPoints::new(model)).map_err(fail)
}

/// A model's points, given to the DWG writer one at a time.
struct ModelPoints<'a> {
    model: &'a CadModel<'a>,
    bounds: Option<(Vector3, Vector3)>,
}

impl<'a> ModelPoints<'a> {
    fn new(model: &'a CadModel<'a>) -> Self {
        let bounds = model
            .points
            .iter()
            .map(|p| model.position(p.vertex))
            .fold(None, |b: Option<([f64; 3], [f64; 3])>, p| {
                Some(match b {
                    None => (p, p),
                    Some((lo, hi)) => (
                        [0, 1, 2].map(|a| lo[a].min(p[a])),
                        [0, 1, 2].map(|a| hi[a].max(p[a])),
                    ),
                })
            })
            .map(|(lo, hi)| (v3(lo), v3(hi)));
        Self { model, bounds }
    }
}

impl PointStream for ModelPoints<'_> {
    fn len(&self) -> usize {
        self.model.points.len()
    }

    fn point(&self, i: usize, point: &mut Point) {
        let p = &self.model.points[i];
        let layer = &self.model.layers[p.layer as usize].name;
        if point.common.layer != *layer {
            point.common.layer = layer.clone();
        }
        point.common.color = color(p.color);
        point.location = v3(self.model.position(p.vertex));
    }

    fn bounds(&self) -> Option<(Vector3, Vector3)> {
        self.bounds
    }
}

/// An output that can't seek, for [`write_to`]: everything goes to `out` in order, but
/// the file's first 256 bytes, written last, go to `head` (the caller puts them first).
pub struct HeldHead<F: FnMut(&[u8]) -> std::io::Result<()>> {
    out: F,
    /// The first 256 bytes, as written so far.
    pub head: [u8; HEAD],
    at: u64,
    len: u64,
}

/// The bytes of a DWG written last, at the start of the file.
pub const HEAD: usize = 0x100;

impl<F: FnMut(&[u8]) -> std::io::Result<()>> HeldHead<F> {
    pub fn new(out: F) -> Self {
        Self {
            out,
            head: [0; HEAD],
            at: 0,
            len: 0,
        }
    }
}

impl<F: FnMut(&[u8]) -> std::io::Result<()>> Write for HeldHead<F> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut buf = buf;
        let n = buf.len();
        // The start of the file: kept.
        if self.at < HEAD as u64 {
            let k = (HEAD - self.at as usize).min(buf.len());
            self.head[self.at as usize..self.at as usize + k].copy_from_slice(&buf[..k]);
            self.at += k as u64;
            buf = &buf[k..];
        }
        if !buf.is_empty() {
            if self.at != self.len {
                return Err(std::io::Error::other(
                    "a DWG may only rewrite its first 256 bytes",
                ));
            }
            (self.out)(buf)?;
            self.at += buf.len() as u64;
        }
        self.len = self.len.max(self.at);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<F: FnMut(&[u8]) -> std::io::Result<()>> Seek for HeldHead<F> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.at = match pos {
            std::io::SeekFrom::Start(p) => p,
            std::io::SeekFrom::End(d) => self.len.saturating_add_signed(d),
            std::io::SeekFrom::Current(d) => self.at.saturating_add_signed(d),
        };
        Ok(self.at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::DwgReader;
    use obj2cad_core::{convert, parse, Options, UpAxis};

    /// acadrust's own SAB reader accepts every sample body, and finds every pointer valid.
    #[test]
    fn acis_samples_read_back_with_acadrust() {
        use acadrust::entities::acis::SabReader;
        for (name, body) in obj2cad_acis::samples::all() {
            let sab = obj2cad_acis::sab::write(&body, "obj2cad", 0.0);
            let sat = SabReader::read(&sab).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let errors = sat.validate();
            assert!(errors.is_empty(), "{name}: {errors:?}");
            assert_eq!(sat.faces().len(), body.faces.len(), "{name}");
            assert_eq!(sat.edges().len(), body.edges.len(), "{name}");
        }
    }

    const META: Meta = Meta {
        properties: &[("obj2cad.parity", "exact")],
        fingerprint_seed: "seed",
        created_unix: Some(1_700_000_000.0),
    };

    /// Write `model` the way every test checks: streamed when `stream` (whatever its size),
    /// through [`HeldHead`] (a sink that can't seek), and checked to be the same bytes as
    /// writing to memory.
    fn written(model: &CadModel, stream: bool) -> Vec<u8> {
        let over = if stream { 0 } else { usize::MAX };
        let mut direct = std::io::Cursor::new(Vec::new());
        write_streaming_over(model, &META, &mut direct, over).unwrap();
        let mut body = Vec::new();
        let mut held = HeldHead::new(|b: &[u8]| {
            body.extend_from_slice(b);
            Ok(())
        });
        write_streaming_over(model, &META, &mut held, over).unwrap();
        let head = held.head;
        let streamed: Vec<u8> = head.iter().copied().chain(body).collect();
        let direct = direct.into_inner();
        assert!(
            streamed == direct,
            "held head differs from writing to memory"
        );
        direct
    }

    fn read(bytes: &[u8]) -> CadDocument {
        DwgReader::from_stream(std::io::Cursor::new(bytes.to_vec()))
            .read()
            .expect("obj2cad's DWG reads back")
    }

    fn bits(v: &Vector3) -> [u64; 3] {
        [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
    }

    /// Every coordinate, face, line, point, layer and color comes back exactly, whether
    /// the points are streamed or not.
    fn assert_round_trip(src: &str, up: UpAxis) {
        assert_round_trip_as(src, up, false);
        assert_round_trip_as(src, up, true);
    }

    fn assert_round_trip_as(src: &str, up: UpAxis, stream: bool) {
        let doc = parse(src.as_bytes()).unwrap();
        let model = convert(
            &doc,
            None,
            Options {
                up_axis: up,
                ..Options::default()
            },
        );
        let back = read(&written(&model, stream));

        let meshes: Vec<&Mesh> = back
            .entities()
            .filter_map(|e| {
                if let EntityType::Mesh(m) = e {
                    Some(m)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(meshes.len(), model.meshes.len());
        for (m, got) in model.meshes.iter().zip(&meshes) {
            assert_eq!(got.common.layer, model.layers[m.layer as usize].name);
            assert_eq!(got.common.color, color(m.color));
            let want: Vec<[u64; 3]> = m
                .vertices
                .iter()
                .map(|&v| model.position(v).map(f64::to_bits))
                .collect();
            assert_eq!(got.vertices.iter().map(bits).collect::<Vec<_>>(), want);
            let faces: Vec<Vec<usize>> = m
                .faces()
                .map(|f| f.iter().map(|&i| i as usize).collect())
                .collect();
            assert_eq!(
                got.faces
                    .iter()
                    .map(|f| f.vertices.clone())
                    .collect::<Vec<_>>(),
                faces
            );
        }

        let lines: Vec<&Polyline3D> = back
            .entities()
            .filter_map(|e| {
                if let EntityType::Polyline3D(l) = e {
                    Some(l)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(lines.len(), model.polylines.len());
        for (l, got) in model.polylines.iter().zip(&lines) {
            let want: Vec<[u64; 3]> = l
                .vertices
                .iter()
                .map(|&v| model.position(v).map(f64::to_bits))
                .collect();
            assert_eq!(
                got.vertices
                    .iter()
                    .map(|v| bits(&v.position))
                    .collect::<Vec<_>>(),
                want
            );
        }

        let points: Vec<&Point> = back
            .entities()
            .filter_map(|e| {
                if let EntityType::Point(p) = e {
                    Some(p)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(points.len(), model.points.len());
        for (p, got) in model.points.iter().zip(&points) {
            assert_eq!(
                bits(&got.location),
                model.position(p.vertex).map(f64::to_bits)
            );
            assert_eq!(got.common.layer, model.layers[p.layer as usize].name);
            assert_eq!(got.common.color, color(p.color));
        }
        // Model space owns every entity, and every handle is distinct.
        let mut handles: Vec<u64> = back.entities().map(|e| e.common().handle.value()).collect();
        let n = handles.len();
        handles.sort_unstable();
        handles.dedup();
        assert_eq!(handles.len(), n, "duplicate handles");
        let ms = back.block_records.get("*Model_Space").expect("model space");
        assert_eq!(
            ms.entity_handles.len(),
            model.meshes.len()
                + model.polylines.len()
                + model.points.len()
                + model.splines.len()
                + model.surfaces.len()
        );

        for layer in model.layers.iter().skip(1) {
            let got = back
                .layers
                .get(&layer.name)
                .unwrap_or_else(|| panic!("layer {}", layer.name));
            assert_eq!(got.color, rgb(layer.color), "layer {}", layer.name);
        }
        assert_eq!(
            back.header.insertion_units,
            model.options.units.insunits() as i16
        );
    }

    const SIGNED_ZEROS: &str = "o part\nv -0.000000 0.0 -0\nv 1 -0.0 0\nv 0.1 0.2 0.30000000000000004\nv 1e-320 -1.7976931348623157e308 2.2250738585072014e-308\nf 1 2 3\nf 2 3 4\nl 1 3 4\np 2\n";

    #[test]
    fn signed_zeros_and_extremes_are_exact() {
        assert_round_trip(SIGNED_ZEROS, UpAxis::AsIs);
        assert_round_trip(SIGNED_ZEROS, UpAxis::YUpToZUp);
    }

    #[test]
    fn layers_colors_and_ngons() {
        let src = "mtllib m.mtl\no a\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0.5 1.5 0\nf 1 2 3 5 4\no b\nv 0 0 1\nv 1 0 1\nv 1 1 1\nf 6 7 8\no c\nl 6 7 8 6\n";
        assert_round_trip(src, UpAxis::AsIs);
    }

    #[test]
    fn point_clouds() {
        assert_round_trip("v 1 2 3\nv -0.0 5 6\nv 7 8 -9.5\n", UpAxis::AsIs);
    }

    /// A cloud spread over many pages of the objects section and many handle map chunks,
    /// with colors and two layers, streamed: every point comes back exactly.
    #[test]
    fn streamed_clouds_read_back_exactly() {
        let mut src = String::from("o scan\n");
        for i in 0..70_000u32 {
            let h = i.wrapping_mul(2_654_435_761);
            src.push_str(&format!(
                "v {}.{:03} -{}.5 {} {} {} {}\n",
                h % 1000,
                i % 1000,
                h % 77,
                if i % 3 == 0 {
                    "-0.0".into()
                } else {
                    format!("{}e-3", h % 9)
                },
                f64::from(h % 256) / 255.0,
                f64::from((h >> 8) % 256) / 255.0,
                f64::from((h >> 16) % 256) / 255.0,
            ));
            if i == 35_000 {
                src.push_str("o rest\n");
            }
        }
        src.push_str("o part\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 70001 70002 70003\n");
        let doc = parse(src.as_bytes()).unwrap();
        let model = convert(
            &doc,
            None,
            Options {
                keep_loose_points: true,
                ..Options::default()
            },
        );
        assert!(model.points.len() >= 70_000);
        assert_round_trip_as(&src, UpAxis::AsIs, true);
        assert_round_trip_as(&src, UpAxis::YUpToZUp, true);
        // Handles pass 0xFFFF, so model space's references to them grow a byte.
        let back = read(&written(&model, true));
        assert_eq!(
            back.entities().count(),
            model.points.len() + model.meshes.len()
        );
    }

    /// The handle map of a streamed cloud reaches past 2 GB of objects: those offsets
    /// read back exactly (acadrust's own reader decodes them as 64-bit).
    #[test]
    fn handle_offsets_past_two_gigabytes_round_trip() {
        use acadrust::io::dwg::dwg_stream_readers::handle_reader::read_handles;
        use acadrust::io::dwg::dwg_stream_writers::handle_writer::write_sorted_handles;
        let entries: Vec<(u64, i64)> = (0..5000u64)
            .map(|i| {
                (
                    0x20 + i,
                    i as i64 * 1_000_003 + if i > 2500 { 3_000_000_000 } else { 0 },
                )
            })
            .collect();
        let back = read_handles(&write_sorted_handles(entries.iter().copied(), 0)).unwrap();
        assert_eq!(back.len(), entries.len());
        for (h, o) in entries {
            assert_eq!(back[&h], o, "handle {h:#x}");
        }
    }

    #[test]
    fn deterministic() {
        let doc = parse(SIGNED_ZEROS.as_bytes()).unwrap();
        let model = convert(&doc, None, Options::default());
        assert_eq!(write(&model, &META).unwrap(), write(&model, &META).unwrap());
    }

    #[test]
    fn header_carries_units_dates_and_properties() {
        let doc = parse(SIGNED_ZEROS.as_bytes()).unwrap();
        let model = convert(
            &doc,
            None,
            Options {
                units: obj2cad_core::Units::Millimeters,
                ..Options::default()
            },
        );
        let back = read(&write(&model, &META).unwrap());
        assert_eq!(back.header.insertion_units, 4);
        assert!((back.header.create_date_julian - META.julian_date()).abs() < 1e-6);
        assert!(back
            .summary_info
            .custom_properties
            .iter()
            .any(|(k, v)| k == "obj2cad.parity" && v == "exact"));
    }
}
