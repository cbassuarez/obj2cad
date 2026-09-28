//! Exact DXF (R2018 / AC1032) writer, ASCII or binary.
//!
//! **ASCII:** every coordinate is written so a correctly-rounding reader recovers the
//! identical IEEE-754 double: either the OBJ's own token (a plain decimal of at most 17
//! significant digits, which parses to the same bits by construction) or the shortest
//! round-trip form.
//!
//! **Binary:** coordinates are the raw 8-byte doubles, exact without any parsing, and
//! the file is smaller and faster for CAD programs to open.
//!
//! Output is streamed to any [`std::io::Write`] in chunks, so the whole file never has
//! to exist in memory twice. Document boilerplate comes from `templates/r2018.dxf`
//! (see `tools/dxf-template`).

mod aci;

use obj2cad_core::convert::CadModel;
use obj2cad_core::output::{fitted_view, VIEW_DIRECTION};
use std::borrow::Cow;
use std::io::{self, Write};

const TEMPLATE: &str = include_str!("../templates/r2018.dxf");
/// Handles used by the template that new records refer to.
const MODEL_SPACE_RECORD: &str = "17";
const LAYER_TABLE: &str = "1";
const PLOTSTYLE_PLACEHOLDER: &str = "13";
const GLOBAL_MATERIAL: &str = "21";
/// First handle for records we add; everything below is reserved for the template.
const FIRST_HANDLE: u64 = 0x100;
/// Bytes buffered before a chunk is handed to the sink.
const CHUNK: usize = 1 << 20;

/// DXF encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Text DXF: readable, diffable; coordinates keep their source text.
    #[default]
    Ascii,
    /// Binary DXF: raw doubles, smaller and faster to open.
    Binary,
}

impl Format {
    pub fn id(self) -> &'static str {
        match self {
            Format::Ascii => "dxf-r2018",
            Format::Binary => "dxf-r2018-binary",
        }
    }
}

pub use obj2cad_core::output::Meta;

/// Is `text` a plain decimal DXF readers parse unambiguously: `-?\d+(\.\d+)?([eE][+-]?\d+)?`
/// with at most 17 significant digits (longer inputs risk mis-rounding in some readers)?
fn is_plain_decimal(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    // Significant digits: from the first non-zero mantissa digit to the last non-zero one.
    let (mut seen, mut first_nz, mut last_nz) = (0usize, usize::MAX, 0usize);
    let mut mantissa = |i: &mut usize| {
        let s = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            if b[*i] != b'0' {
                first_nz = first_nz.min(seen);
                last_nz = seen;
            }
            seen += 1;
            *i += 1;
        }
        *i - s
    };
    if mantissa(&mut i) == 0 {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        if mantissa(&mut i) == 0 {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let s = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == s {
            return false;
        }
    }
    i == b.len() && (first_nz == usize::MAX || last_nz - first_nz < 17)
}

/// Text for a real value that round-trips to exactly `value`.
pub fn exact_real<'a>(value: f64, source: Option<&'a str>) -> Cow<'a, str> {
    if let Some(t) = source {
        if is_plain_decimal(t) && t.parse::<f64>().map(f64::to_bits) == Ok(value.to_bits()) {
            return Cow::Borrowed(t);
        }
    }
    assert!(value.is_finite(), "non-finite values are rejected by the parser");
    let mut buf = ryu::Buffer::new();
    Cow::Owned(buf.format_finite(value).to_owned())
}

/// Value type of a group code in binary DXF (the table ezdxf and AutoCAD use).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Str,
    Double,
    Bool,
    I16,
    I32,
    I64,
    Bytes,
}

fn kind(code: i32) -> Kind {
    match code {
        10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059 => Kind::Double,
        290..=299 => Kind::Bool,
        60..=79 | 170..=179 | 270..=289 | 370..=389 | 400..=409 | 1060..=1070 => Kind::I16,
        90..=99 | 420..=429 | 440..=459 | 1071 => Kind::I32,
        160..=169 => Kind::I64,
        310..=319 | 1004 => Kind::Bytes,
        _ => Kind::Str,
    }
}

struct Out<'w> {
    format: Format,
    buf: Vec<u8>,
    sink: &'w mut dyn Write,
    written: u64,
    next_handle: u64,
    ryu: ryu::Buffer,
    itoa: itoa::Buffer,
}

impl Out<'_> {
    fn maybe_flush(&mut self) -> io::Result<()> {
        if self.buf.len() >= CHUNK {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sink.write_all(&self.buf)?;
        self.written += self.buf.len() as u64;
        self.buf.clear();
        Ok(())
    }

    fn code(&mut self, code: i32) {
        match self.format {
            Format::Ascii => {
                // Right-aligned to three characters like AutoCAD writes it.
                let t = self.itoa.format(code);
                for _ in t.len()..3 {
                    self.buf.push(b' ');
                }
                self.buf.extend_from_slice(t.as_bytes());
                self.buf.extend_from_slice(b"\r\n");
            }
            Format::Binary => self.buf.extend_from_slice(&(code as u16).to_le_bytes()),
        }
    }

    fn str(&mut self, code: i32, value: &str) {
        if code == 999 && self.format == Format::Binary {
            return; // binary DXF has no comments
        }
        self.code(code);
        self.buf.extend_from_slice(value.as_bytes());
        match self.format {
            Format::Ascii => self.buf.extend_from_slice(b"\r\n"),
            Format::Binary => self.buf.push(0),
        }
    }

    fn int(&mut self, code: i32, value: i64) {
        self.code(code);
        match self.format {
            Format::Ascii => {
                let t = self.itoa.format(value);
                self.buf.extend_from_slice(t.as_bytes());
                self.buf.extend_from_slice(b"\r\n");
            }
            Format::Binary => match kind(code) {
                Kind::Bool => self.buf.push(value as u8),
                Kind::I16 => self.buf.extend_from_slice(&(value as i16).to_le_bytes()),
                Kind::I32 => self.buf.extend_from_slice(&(value as i32).to_le_bytes()),
                Kind::I64 => self.buf.extend_from_slice(&value.to_le_bytes()),
                k => unreachable!("group code {code} is {k:?}, not an integer"),
            },
        }
    }

    fn real(&mut self, code: i32, value: f64) {
        self.code(code);
        match self.format {
            Format::Ascii => {
                let t = self.ryu.format_finite(value);
                self.buf.extend_from_slice(t.as_bytes());
                self.buf.extend_from_slice(b"\r\n");
            }
            Format::Binary => self.buf.extend_from_slice(&value.to_le_bytes()),
        }
    }

    /// A pair from the template, whose values are text.
    fn template_pair(&mut self, code: i32, value: &str) {
        match (self.format, kind(code)) {
            (Format::Ascii, _) | (_, Kind::Str) => self.str(code, value),
            (Format::Binary, Kind::Double) => {
                let v: f64 = value.trim().parse().unwrap_or_else(|_| panic!("template: bad real {value:?} for {code}"));
                self.real(code, v);
            }
            (Format::Binary, Kind::Bytes) => {
                let bytes: Vec<u8> = (0..value.len() / 2)
                    .map(|i| u8::from_str_radix(&value[2 * i..2 * i + 2], 16).expect("template: bad hex"))
                    .collect();
                for chunk in bytes.chunks(127) {
                    self.code(code);
                    self.buf.push(chunk.len() as u8);
                    self.buf.extend_from_slice(chunk);
                }
            }
            (Format::Binary, _) => {
                let v: i64 = value.trim().parse().unwrap_or_else(|_| panic!("template: bad int {value:?} for {code}"));
                self.int(code, v);
            }
        }
    }

    fn template(&mut self, text: &str) {
        let mut lines = text.lines();
        while let Some(code) = lines.next() {
            let value = lines.next().expect("template pairs");
            match self.format {
                Format::Ascii => {
                    // Verbatim, as generated from ezdxf.
                    self.buf.extend_from_slice(code.as_bytes());
                    self.buf.extend_from_slice(b"\r\n");
                    self.buf.extend_from_slice(value.as_bytes());
                    self.buf.extend_from_slice(b"\r\n");
                }
                Format::Binary => self.template_pair(code.trim().parse().expect("template code"), value),
            }
        }
    }

    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next_handle);
        self.next_handle += 1;
        h
    }

    fn entity_head(&mut self, kind: &str, owner: &str, layer: &str, color: Option<[u8; 3]>) -> String {
        let h = self.handle();
        self.str(0, kind);
        self.str(5, &h);
        self.str(330, owner);
        self.str(100, "AcDbEntity");
        self.str(8, layer);
        if let Some(rgb) = color {
            self.int(420, rgb_int(rgb));
        }
        h
    }

    /// Write a vertex. ASCII copies the source token when it is a plain decimal (it is
    /// the exact text the value was parsed from, correctly rounded, and parsing is
    /// symmetric under negation, so it round-trips by construction); otherwise the
    /// shortest form. Binary writes the double's bytes.
    fn xyz(&mut self, model: &CadModel, v: u32) {
        let p = model.position(v);
        for (axis, code) in [10, 20, 30].into_iter().enumerate() {
            if self.format == Format::Binary {
                self.real(code, p[axis]);
                continue;
            }
            self.code(code);
            let (t, negate) = model.coord_source(v, axis);
            if is_plain_decimal(t) {
                debug_assert_eq!(t.parse::<f64>().map(|x| if negate { -x } else { x }.to_bits()), Ok(p[axis].to_bits()));
                match (negate, t.strip_prefix('-')) {
                    (false, _) => self.buf.extend_from_slice(t.as_bytes()),
                    (true, Some(rest)) => self.buf.extend_from_slice(rest.as_bytes()),
                    (true, None) => {
                        self.buf.push(b'-');
                        self.buf.extend_from_slice(t.as_bytes());
                    }
                }
            } else {
                let r = self.ryu.format_finite(p[axis]);
                self.buf.extend_from_slice(r.as_bytes());
            }
            self.buf.extend_from_slice(b"\r\n");
        }
    }
}

fn rgb_int([r, g, b]: [u8; 3]) -> i64 {
    (i64::from(r) << 16) | (i64::from(g) << 8) | i64::from(b)
}

/// Number of handles the entities and layers will use (known before writing, so the
/// header's `$HANDSEED` can be written first and the rest streamed).
fn handles_needed(model: &CadModel) -> u64 {
    (model.layers.len() as u64 - 1)
        + model.meshes.len() as u64
        + model.polylines.iter().map(|l| 2 + l.vertices.len() as u64).sum::<u64>()
        + model.points.len() as u64
}

/// Write `model` as DXF R2018 to `sink`. Returns the number of bytes written.
pub fn write_to(model: &CadModel, meta: &Meta, format: Format, sink: &mut dyn Write) -> io::Result<u64> {
    let mut out = Out { format, buf: Vec::with_capacity(CHUNK + 4096), sink, written: 0, next_handle: FIRST_HANDLE, ryu: ryu::Buffer::new(), itoa: itoa::Buffer::new() };
    if format == Format::Binary {
        out.buf.extend_from_slice(b"AutoCAD Binary DXF\r\n\x1a\x00");
    }
    out.str(999, &format!("obj2cad {}", obj2cad_core::VERSION));

    let handseed = format!("{:X}", FIRST_HANDLE + handles_needed(model));
    let julian = meta.julian_date();
    let view = fitted_view(model);

    for chunk in split_markers(TEMPLATE) {
        match chunk {
            Chunk::Text(t) => out.template(t),
            Chunk::Marker("INSUNITS") => {
                out.str(9, "$INSUNITS");
                out.int(70, i64::from(model.options.units.insunits()));
            }
            Chunk::Marker("MEASUREMENT") => {
                out.str(9, "$MEASUREMENT");
                out.int(70, i64::from(model.options.units.is_metric()));
            }
            Chunk::Marker("HANDSEED") => {
                out.str(9, "$HANDSEED");
                out.str(5, &handseed);
            }
            Chunk::Marker(m @ ("EXTMIN" | "EXTMAX")) => {
                out.str(9, &format!("${m}"));
                let (lo, hi) = model.bounds().unwrap_or(([1e20; 3], [-1e20; 3]));
                let p = if m == "EXTMIN" { lo } else { hi };
                out.real(10, p[0]);
                out.real(20, p[1]);
                out.real(30, p[2]);
            }
            Chunk::Marker(m @ ("TDCREATE" | "TDUCREATE" | "TDUPDATE" | "TDUUPDATE")) => {
                out.str(9, &format!("${m}"));
                out.real(40, julian);
            }
            Chunk::Marker("FINGERPRINTGUID") => {
                out.str(9, "$FINGERPRINTGUID");
                out.str(2, &meta.fingerprint_guid());
            }
            Chunk::Marker("VERSIONGUID") => {
                out.str(9, "$VERSIONGUID");
                out.str(2, &meta.version_guid());
            }
            Chunk::Marker("LASTSAVEDBY") => {
                out.str(9, "$LASTSAVEDBY");
                out.str(1, "obj2cad");
            }
            Chunk::Marker("CUSTOMPROPERTIES") => {
                for (k, v) in meta.properties {
                    debug_assert!(!k.contains('\n') && !v.contains('\n'));
                    out.str(9, "$CUSTOMPROPERTYTAG");
                    out.str(1, k);
                    out.str(9, "$CUSTOMPROPERTY");
                    out.str(1, v);
                }
            }
            Chunk::Marker("VPORT_CENTER") => {
                out.real(12, 0.0);
                out.real(22, 0.0);
            }
            Chunk::Marker("VPORT_DIRECTION") => {
                let d = if view.is_some() { VIEW_DIRECTION } else { [0.0, 0.0, 1.0] };
                out.real(16, d[0]);
                out.real(26, d[1]);
                out.real(36, d[2]);
            }
            Chunk::Marker("VPORT_TARGET") => {
                let t = view.as_ref().map_or([0.0; 3], |v| v.target);
                out.real(17, t[0]);
                out.real(27, t[1]);
                out.real(37, t[2]);
            }
            Chunk::Marker("VPORT_HEIGHT") => out.real(40, view.as_ref().map_or(1000.0, |v| v.height)),
            Chunk::Marker("LAYERCOUNT") => out.int(70, model.layers.len() as i64 + 1),
            Chunk::Marker("LAYERS") => {
                for layer in model.layers.iter().skip(1) {
                    let h = out.handle();
                    out.str(0, "LAYER");
                    out.str(5, &h);
                    out.str(330, LAYER_TABLE);
                    out.str(100, "AcDbSymbolTableRecord");
                    out.str(100, "AcDbLayerTableRecord");
                    out.str(2, &layer.name);
                    out.int(70, 0);
                    out.int(62, i64::from(aci::nearest(layer.color)));
                    out.int(420, rgb_int(layer.color));
                    out.str(6, "Continuous");
                    out.int(370, -3);
                    out.str(390, PLOTSTYLE_PLACEHOLDER);
                    out.str(347, GLOBAL_MATERIAL);
                }
            }
            Chunk::Marker("ENTITIES") => entities(&mut out, model)?,
            Chunk::Marker(other) => unreachable!("unknown template marker {other}"),
        }
        out.maybe_flush()?;
    }
    out.flush()?;
    debug_assert_eq!(out.next_handle, FIRST_HANDLE + handles_needed(model), "handle count");
    Ok(out.written)
}

fn entities(out: &mut Out, model: &CadModel) -> io::Result<()> {
    for m in &model.meshes {
        let layer = &model.layers[m.layer as usize].name;
        out.entity_head("MESH", MODEL_SPACE_RECORD, layer, m.color);
        out.str(100, "AcDbSubDMesh");
        out.int(71, 2);
        out.int(72, 0);
        out.int(91, 0);
        out.int(92, m.vertices.len() as i64);
        for &v in &m.vertices {
            out.xyz(model, v);
            out.maybe_flush()?;
        }
        let list_len: usize = m.faces().map(|f| f.len() + 1).sum();
        out.int(93, list_len as i64);
        for f in m.faces() {
            out.int(90, f.len() as i64);
            for &i in f {
                out.int(90, i64::from(i));
            }
            out.maybe_flush()?;
        }
        out.int(94, 0);
        out.int(95, 0);
        out.int(90, 0);
    }
    for l in &model.polylines {
        let layer = &model.layers[l.layer as usize].name;
        let h = out.entity_head("POLYLINE", MODEL_SPACE_RECORD, layer, l.color);
        out.str(100, "AcDb3dPolyline");
        out.int(66, 1);
        for code in [10, 20, 30] {
            out.real(code, 0.0);
        }
        out.int(70, 8);
        for &v in &l.vertices {
            out.entity_head("VERTEX", &h, layer, l.color);
            out.str(100, "AcDbVertex");
            out.str(100, "AcDb3dPolylineVertex");
            out.xyz(model, v);
            out.int(70, 32);
        }
        out.entity_head("SEQEND", &h, layer, None);
        out.maybe_flush()?;
    }
    for p in &model.points {
        let layer = &model.layers[p.layer as usize].name;
        out.entity_head("POINT", MODEL_SPACE_RECORD, layer, p.color);
        out.str(100, "AcDbPoint");
        out.xyz(model, p.vertex);
        out.maybe_flush()?;
    }
    Ok(())
}

/// Write `model` as DXF R2018 into memory.
pub fn write(model: &CadModel, meta: &Meta, format: Format) -> Vec<u8> {
    let mut v = Vec::new();
    write_to(model, meta, format, &mut v).expect("writing to memory cannot fail");
    v
}

enum Chunk<'a> {
    Text(&'a str),
    Marker(&'a str),
}

fn split_markers(t: &str) -> Vec<Chunk<'_>> {
    let mut out = Vec::new();
    let mut rest = t;
    while let Some(start) = rest.find("@@") {
        let end = start + 2 + rest[start + 2..].find("@@").expect("unterminated marker");
        out.push(Chunk::Text(&rest[..start]));
        out.push(Chunk::Marker(&rest[start + 2..end]));
        // skip marker and its line break
        rest = rest[end + 2..].strip_prefix('\n').unwrap_or(&rest[end + 2..]);
    }
    out.push(Chunk::Text(rest));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use obj2cad_core::{convert, parse, Options};

    const META: Meta = Meta { properties: &[("obj2cad.version", "test")], fingerprint_seed: "seed", created_unix: None };

    #[test]
    fn exact_real_keeps_plain_tokens_and_round_trips_everything() {
        assert_eq!(exact_real(1.0, Some("1.000000")), "1.000000");
        assert_eq!(exact_real(-0.0, Some("-0.000000")), "-0.000000");
        // too many digits → shortest form instead
        let long = "0.12345678901234567890";
        let v: f64 = long.parse().unwrap();
        assert_eq!(exact_real(v, Some(long)).parse::<f64>().unwrap().to_bits(), v.to_bits());
        assert_ne!(exact_real(v, Some(long)), long);
        // non-plain syntaxes are never copied
        assert_ne!(exact_real(0.5, Some(".5")), ".5");
        assert_ne!(exact_real(5.0, Some("+5")), "+5");
        // pseudo-random sweep over the whole bit space
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let v = f64::from_bits(x);
            if v.is_finite() {
                assert_eq!(exact_real(v, None).parse::<f64>().unwrap().to_bits(), v.to_bits());
            }
        }
    }

    #[test]
    fn plain_decimal_grammar() {
        for ok in ["0", "-1", "1.5", "1e-5", "1.25E+03", "00012.5000"] {
            assert!(is_plain_decimal(ok), "{ok}");
        }
        for bad in ["", "-", ".5", "5.", "+5", "1e", "nan", "inf", "1.2.3", "0x10", "1_000", "123456789012345678"] {
            assert!(!is_plain_decimal(bad), "{bad}");
        }
    }

    fn sample() -> obj2cad_core::ObjDocument {
        parse(b"o Part A\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\nl 1 2\np 3\n").unwrap()
    }

    #[test]
    fn writes_all_markers_and_is_deterministic() {
        let doc = sample();
        let model = convert(&doc, None, Options::default());
        let a = write(&model, &META, Format::Ascii);
        assert_eq!(a, write(&model, &META, Format::Ascii));
        let s = String::from_utf8(a).unwrap();
        assert!(!s.contains("@@"));
        assert!(s.contains("AcDbSubDMesh") && s.contains("AcDb3dPolyline") && s.contains("AcDbPoint"));
        assert!(s.contains("\r\nPart A\r\n"));
        assert!(s.ends_with("EOF\r\n"));
    }

    #[test]
    fn layers_carry_their_colors() {
        let doc = sample();
        let model = convert(&doc, None, Options::default());
        let s = String::from_utf8(write(&model, &META, Format::Ascii)).unwrap();
        let [r, g, b] = model.layers[1].color;
        let rgb = format!("{}", (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
        let at = s.find("\r\nPart A\r\n").unwrap();
        assert!(s[at..at + 200].contains(&format!("420\r\n{rgb}\r\n")), "layer true color");
        assert!(!s[at..at + 200].contains(" 62\r\n7\r\n"), "not the default white");
    }

    #[test]
    fn opening_view_fits_an_off_origin_model() {
        let doc = parse(b"v 500123.456 4649876.543 1234.5\nv 500124.456 4649876.543 1234.5\nv 500124.456 4649877.543 1234.5\nf 1 2 3\n").unwrap();
        let model = convert(&doc, None, Options::default());
        let s = String::from_utf8(write(&model, &META, Format::Ascii)).unwrap();
        let at = s.find("*Active").unwrap();
        let vport = &s[at..at + 900];
        let value = |code: &str| -> f64 { vport.split(&format!("{code}\r\n")).nth(1).unwrap().split("\r\n").next().unwrap().parse().unwrap() };
        assert!((value(" 17") - 500_123.956).abs() < 1e-6, "target at the model center: {vport}");
        assert!((value(" 27") - 4_649_877.043).abs() < 1e-6);
        assert!(vport.contains(" 16\r\n1.0\r\n 26\r\n-1.0\r\n 36\r\n1.0\r\n"), "SE isometric");
        let h = value(" 40");
        assert!(h > 1.0 && h < 3.0, "height fits a ~1.4-unit model, got {h}");
    }

    #[test]
    fn dates_come_from_the_source() {
        let doc = sample();
        let model = convert(&doc, None, Options::default());
        let meta = Meta { created_unix: Some(1_790_000_000.0), ..META };
        let s = String::from_utf8(write(&model, &meta, Format::Ascii)).unwrap();
        let jd = 1_790_000_000.0 / 86_400.0 + 2_440_587.5;
        assert!(s.contains(&format!("$TDCREATE\r\n 40\r\n{}\r\n", ryu::Buffer::new().format(jd))));
    }

    #[test]
    fn streaming_matches_in_memory() {
        struct Chunks(Vec<Vec<u8>>);
        impl Write for Chunks {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.push(b.to_vec());
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let src = obj2cad_core::synth::terrain(300);
        let doc = parse(&src).unwrap();
        let model = convert(&doc, None, Options::default());
        let whole = write(&model, &META, Format::Ascii);
        let mut sink = Chunks(Vec::new());
        let n = write_to(&model, &META, Format::Ascii, &mut sink).unwrap();
        assert!(sink.0.len() > 2, "written in several chunks");
        assert_eq!(n as usize, whole.len());
        assert_eq!(sink.0.concat(), whole);
    }

    /// Minimal binary DXF reader, independent of the writer, for the tests below.
    fn read_binary(b: &[u8]) -> Vec<(i32, String)> {
        assert!(b.starts_with(b"AutoCAD Binary DXF\r\n\x1a\x00"));
        let mut i = 22;
        let mut out = Vec::new();
        while i < b.len() {
            let code = i32::from(u16::from_le_bytes([b[i], b[i + 1]]));
            i += 2;
            let take = |i: &mut usize, n: usize| {
                let s = &b[*i..*i + n];
                *i += n;
                s
            };
            let value = match kind(code) {
                Kind::Double => f64::from_le_bytes(take(&mut i, 8).try_into().unwrap()).to_bits().to_string(),
                Kind::Bool => take(&mut i, 1)[0].to_string(),
                Kind::I16 => i16::from_le_bytes(take(&mut i, 2).try_into().unwrap()).to_string(),
                Kind::I32 => i32::from_le_bytes(take(&mut i, 4).try_into().unwrap()).to_string(),
                Kind::I64 => i64::from_le_bytes(take(&mut i, 8).try_into().unwrap()).to_string(),
                Kind::Bytes => {
                    let n = take(&mut i, 1)[0] as usize;
                    format!("{:?}", take(&mut i, n))
                }
                Kind::Str => {
                    let end = b[i..].iter().position(|&c| c == 0).unwrap();
                    let s = String::from_utf8(b[i..i + end].to_vec()).unwrap();
                    i += end + 1;
                    s
                }
            };
            out.push((code, value));
        }
        out
    }

    #[test]
    fn binary_dxf_is_well_formed_and_exact() {
        let doc = parse(b"o Part A\nv 0.1 -0.000000 1.2345678901234567e-5\nv 5e-324 1 0\nv 0 1 1e300\nf 1 2 3\nl 1 2\np 3\n").unwrap();
        let model = convert(&doc, None, Options::default());
        let bin = write(&model, &META, Format::Binary);
        let pairs = read_binary(&bin);
        assert_eq!(pairs.last().unwrap(), &(0, "EOF".to_owned()));
        assert!(!pairs.iter().any(|(c, _)| *c == 999), "no comments in binary DXF");
        // Same tags as the ASCII file, minus the comment.
        let ascii = String::from_utf8(write(&model, &META, Format::Ascii)).unwrap();
        let ascii_codes: Vec<i32> = ascii.split("\r\n").step_by(2).filter(|l| !l.is_empty()).map(|c| c.trim().parse().unwrap()).filter(|&c| c != 999).collect();
        let bin_codes: Vec<i32> = pairs.iter().map(|(c, _)| *c).collect();
        assert_eq!(bin_codes, ascii_codes);
        // The MESH vertices are the exact doubles.
        let mesh = pairs.iter().position(|(c, v)| *c == 0 && v == "MESH").unwrap();
        let xs: Vec<u64> = pairs[mesh..].iter().filter(|(c, _)| *c == 10).take(3).map(|(_, v)| v.parse().unwrap()).collect();
        assert_eq!(xs, [0.1f64.to_bits(), 5e-324f64.to_bits(), 0.0f64.to_bits()]);
        let ys: Vec<u64> = pairs[mesh..].iter().filter(|(c, _)| *c == 20).take(1).map(|(_, v)| v.parse().unwrap()).collect();
        assert_eq!(ys, [(-0.0f64).to_bits()], "signed zero survives");
    }

    #[test]
    fn template_references_exist() {
        for h in [MODEL_SPACE_RECORD, LAYER_TABLE, PLOTSTYLE_PLACEHOLDER, GLOBAL_MATERIAL] {
            assert!(TEMPLATE.contains(&format!("  5\n{h}\n")), "handle {h} missing from template");
        }
        let mut max = 0;
        for chunk in split_markers(TEMPLATE) {
            let Chunk::Text(t) = chunk else { continue };
            let lines: Vec<_> = t.lines().collect();
            assert!(lines.len() % 2 == 0, "template chunk is not made of code/value pairs");
            for pair in lines.chunks(2) {
                if matches!(pair[0].trim(), "5" | "105") {
                    max = max.max(u64::from_str_radix(pair[1].trim(), 16).unwrap());
                }
            }
        }
        assert!(max < FIRST_HANDLE, "template handle {max:X} collides with writer handles");
    }

    #[test]
    fn aci_nearest() {
        assert_eq!(aci::nearest([255, 0, 0]), 1);
        assert_eq!(aci::nearest([255, 255, 255]), 7);
        assert!((1..=255).contains(&aci::nearest([124, 158, 201])));
    }
}
