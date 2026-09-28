//! Exact DXF (R2018 / AC1032) writer.
//!
//! Every coordinate is written so a correctly-rounding reader recovers the identical
//! IEEE-754 double: either the OBJ's own token (when it is a plain decimal of at most 17
//! significant digits that parses to the same bits) or the shortest round-trip form.
//! Document boilerplate comes from `templates/r2018.dxf` (see `tools/dxf-template`).

use obj2cad_core::convert::CadModel;
use obj2cad_core::hash::sha256_hex;
use std::borrow::Cow;

const TEMPLATE: &str = include_str!("../templates/r2018.dxf");
/// Handles used by the template that new records refer to.
const MODEL_SPACE_RECORD: &str = "17";
const LAYER_TABLE: &str = "1";
const PLOTSTYLE_PLACEHOLDER: &str = "13";
const GLOBAL_MATERIAL: &str = "21";
/// First handle for records we add; everything below is reserved for the template.
const FIRST_HANDLE: u64 = 0x100;

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
    assert!(
        value.is_finite(),
        "non-finite values are rejected by the parser"
    );
    let mut buf = ryu::Buffer::new();
    Cow::Owned(buf.format_finite(value).to_owned())
}

struct Out {
    buf: Vec<u8>,
    next_handle: u64,
    ryu: ryu::Buffer,
    itoa: itoa::Buffer,
}

impl Out {
    fn new(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
            next_handle: FIRST_HANDLE,
            ryu: ryu::Buffer::new(),
            itoa: itoa::Buffer::new(),
        }
    }

    /// Group code, right-aligned to three characters like AutoCAD writes it.
    fn code(&mut self, code: i32) {
        let t = self.itoa.format(code);
        for _ in t.len()..3 {
            self.buf.push(b' ');
        }
        self.buf.extend_from_slice(t.as_bytes());
        self.buf.extend_from_slice(b"\r\n");
    }

    fn pair(&mut self, code: i32, value: &str) {
        self.code(code);
        self.buf.extend_from_slice(value.as_bytes());
        self.buf.extend_from_slice(b"\r\n");
    }

    fn int(&mut self, code: i32, value: i64) {
        self.code(code);
        let t = self.itoa.format(value);
        self.buf.extend_from_slice(t.as_bytes());
        self.buf.extend_from_slice(b"\r\n");
    }

    fn real(&mut self, code: i32, value: f64) {
        self.code(code);
        let t = self.ryu.format_finite(value);
        self.buf.extend_from_slice(t.as_bytes());
        self.buf.extend_from_slice(b"\r\n");
    }

    fn handle(&mut self) -> String {
        let h = format!("{:X}", self.next_handle);
        self.next_handle += 1;
        h
    }

    fn template(&mut self, text: &str) {
        for line in text.lines() {
            self.buf.extend_from_slice(line.as_bytes());
            self.buf.extend_from_slice(b"\r\n");
        }
    }

    fn entity_head(
        &mut self,
        kind: &str,
        owner: &str,
        layer: &str,
        color: Option<[u8; 3]>,
    ) -> String {
        let h = self.handle();
        self.pair(0, kind);
        self.pair(5, &h);
        self.pair(330, owner);
        self.pair(100, "AcDbEntity");
        self.pair(8, layer);
        if let Some([r, g, b]) = color {
            self.int(
                420,
                (i64::from(r) << 16) | (i64::from(g) << 8) | i64::from(b),
            );
        }
        h
    }

    /// Write a vertex. The source token is copied when it is a plain decimal: it is the
    /// exact text the value was parsed from (correctly rounded, and parsing is symmetric
    /// under negation), so it round-trips by construction. Otherwise the shortest form.
    fn xyz(&mut self, model: &CadModel, v: u32) {
        let p = model.position(v);
        for (axis, code) in [10, 20, 30].into_iter().enumerate() {
            self.code(code);
            let (t, negate) = model.coord_source(v, axis);
            if is_plain_decimal(t) {
                debug_assert_eq!(
                    t.parse::<f64>()
                        .map(|x| if negate { -x } else { x }.to_bits()),
                    Ok(p[axis].to_bits())
                );
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

/// Deterministic GUID-shaped string derived from a hash.
fn guid(seed: &str) -> String {
    let h = sha256_hex(seed.as_bytes()).to_uppercase();
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Write `model` as DXF R2018. `properties` become drawing custom properties
/// (visible in AutoCAD's DWGPROPS); keys and values must be single-line.
pub fn write(model: &CadModel, properties: &[(&str, &str)], fingerprint_seed: &str) -> Vec<u8> {
    let mut out = Out::new(estimate(model));
    out.pair(999, &format!("obj2cad {}", obj2cad_core::VERSION));

    // Entities first (into a side buffer) so $HANDSEED is known when the header is written.
    let mut ents = Out::new(estimate(model));
    let layer_handles: Vec<String> = model.layers.iter().skip(1).map(|_| ents.handle()).collect();
    for m in &model.meshes {
        let layer = &model.layers[m.layer as usize].name;
        ents.entity_head("MESH", MODEL_SPACE_RECORD, layer, m.color);
        ents.pair(100, "AcDbSubDMesh");
        ents.int(71, 2);
        ents.int(72, 0);
        ents.int(91, 0);
        ents.int(92, m.vertices.len() as i64);
        for &v in &m.vertices {
            ents.xyz(model, v);
        }
        let list_len: usize = m.faces().map(|f| f.len() + 1).sum();
        ents.int(93, list_len as i64);
        for f in m.faces() {
            ents.int(90, f.len() as i64);
            for &i in f {
                ents.int(90, i64::from(i));
            }
        }
        ents.int(94, 0);
        ents.int(95, 0);
        ents.int(90, 0);
    }
    for l in &model.polylines {
        let layer = &model.layers[l.layer as usize].name;
        let h = ents.entity_head("POLYLINE", MODEL_SPACE_RECORD, layer, l.color);
        ents.pair(100, "AcDb3dPolyline");
        ents.int(66, 1);
        for code in [10, 20, 30] {
            ents.pair(code, "0.0");
        }
        ents.int(70, 8);
        for &v in &l.vertices {
            ents.entity_head("VERTEX", &h, layer, l.color);
            ents.pair(100, "AcDbVertex");
            ents.pair(100, "AcDb3dPolylineVertex");
            ents.xyz(model, v);
            ents.int(70, 32);
        }
        ents.entity_head("SEQEND", &h, layer, None);
    }
    for p in &model.points {
        let layer = &model.layers[p.layer as usize].name;
        ents.entity_head("POINT", MODEL_SPACE_RECORD, layer, p.color);
        ents.pair(100, "AcDbPoint");
        ents.xyz(model, p.vertex);
    }
    let handseed = format!("{:X}", ents.next_handle);

    for chunk in split_markers(TEMPLATE) {
        match chunk {
            Chunk::Text(t) => out.template(t),
            Chunk::Marker("INSUNITS") => {
                out.pair(9, "$INSUNITS");
                out.int(70, i64::from(model.options.units.insunits()));
            }
            Chunk::Marker("MEASUREMENT") => {
                out.pair(9, "$MEASUREMENT");
                out.int(70, i64::from(model.options.units.is_metric()));
            }
            Chunk::Marker("HANDSEED") => {
                out.pair(9, "$HANDSEED");
                out.pair(5, &handseed);
            }
            Chunk::Marker(m @ ("EXTMIN" | "EXTMAX")) => {
                out.pair(9, &format!("${m}"));
                let (lo, hi) = model.bounds().unwrap_or(([1e20; 3], [-1e20; 3]));
                let p = if m == "EXTMIN" { lo } else { hi };
                out.real(10, p[0]);
                out.real(20, p[1]);
                out.real(30, p[2]);
            }
            Chunk::Marker("FINGERPRINTGUID") => {
                out.pair(9, "$FINGERPRINTGUID");
                out.pair(2, &guid(&format!("fingerprint:{fingerprint_seed}")));
            }
            Chunk::Marker("VERSIONGUID") => {
                out.pair(9, "$VERSIONGUID");
                out.pair(
                    2,
                    &guid(&format!(
                        "version:{fingerprint_seed}:{}",
                        obj2cad_core::VERSION
                    )),
                );
            }
            Chunk::Marker("LASTSAVEDBY") => {
                out.pair(9, "$LASTSAVEDBY");
                out.pair(1, "obj2cad");
            }
            Chunk::Marker("CUSTOMPROPERTIES") => {
                for (k, v) in properties {
                    debug_assert!(!k.contains('\n') && !v.contains('\n'));
                    out.pair(9, "$CUSTOMPROPERTYTAG");
                    out.pair(1, k);
                    out.pair(9, "$CUSTOMPROPERTY");
                    out.pair(1, v);
                }
            }
            Chunk::Marker("LAYERCOUNT") => out.int(70, model.layers.len() as i64 + 1),
            Chunk::Marker("LAYERS") => {
                for (layer, h) in model.layers.iter().skip(1).zip(&layer_handles) {
                    out.pair(0, "LAYER");
                    out.pair(5, h);
                    out.pair(330, LAYER_TABLE);
                    out.pair(100, "AcDbSymbolTableRecord");
                    out.pair(100, "AcDbLayerTableRecord");
                    out.pair(2, &layer.name);
                    out.int(70, 0);
                    out.int(62, 7);
                    out.pair(6, "Continuous");
                    out.int(370, -3);
                    out.pair(390, PLOTSTYLE_PLACEHOLDER);
                    out.pair(347, GLOBAL_MATERIAL);
                }
            }
            Chunk::Marker("ENTITIES") => out.buf.extend_from_slice(&ents.buf),
            Chunk::Marker(other) => unreachable!("unknown template marker {other}"),
        }
    }
    out.buf
}

fn estimate(model: &CadModel) -> usize {
    let verts: usize = model.meshes.iter().map(|m| m.vertices.len()).sum();
    let refs: usize = model.meshes.iter().map(|m| m.face_indices.len()).sum();
    verts * 3 * 20 + refs * 12 + 64 * 1024
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
        rest = rest[end + 2..]
            .strip_prefix('\n')
            .unwrap_or(&rest[end + 2..]);
    }
    out.push(Chunk::Text(rest));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use obj2cad_core::{convert, parse, Options};

    #[test]
    fn exact_real_keeps_plain_tokens_and_round_trips_everything() {
        assert_eq!(exact_real(1.0, Some("1.000000")), "1.000000");
        assert_eq!(exact_real(-0.0, Some("-0.000000")), "-0.000000");
        // too many digits → shortest form instead
        let long = "0.12345678901234567890";
        let v: f64 = long.parse().unwrap();
        assert_eq!(
            exact_real(v, Some(long)).parse::<f64>().unwrap().to_bits(),
            v.to_bits()
        );
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
                assert_eq!(
                    exact_real(v, None).parse::<f64>().unwrap().to_bits(),
                    v.to_bits()
                );
            }
        }
    }

    #[test]
    fn plain_decimal_grammar() {
        for ok in ["0", "-1", "1.5", "1e-5", "1.25E+03", "00012.5000"] {
            assert!(is_plain_decimal(ok), "{ok}");
        }
        for bad in [
            "",
            "-",
            ".5",
            "5.",
            "+5",
            "1e",
            "nan",
            "inf",
            "1.2.3",
            "0x10",
            "1_000",
            "123456789012345678",
        ] {
            assert!(!is_plain_decimal(bad), "{bad}");
        }
    }

    #[test]
    fn writes_all_markers_and_is_deterministic() {
        let doc = parse(b"o Part A\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\nl 1 2\np 3\n").unwrap();
        let model = convert(&doc, None, Options::default());
        let a = write(&model, &[("obj2cad.version", "test")], "seed");
        let b = write(&model, &[("obj2cad.version", "test")], "seed");
        assert_eq!(a, b);
        let s = String::from_utf8(a).unwrap();
        assert!(!s.contains("@@"));
        assert!(
            s.contains("AcDbSubDMesh") && s.contains("AcDb3dPolyline") && s.contains("AcDbPoint")
        );
        assert!(s.contains("\r\nPart A\r\n"));
        assert!(s.ends_with("EOF\r\n"));
    }

    #[test]
    fn template_references_exist() {
        for (code, h) in [
            ("5", MODEL_SPACE_RECORD),
            ("5", LAYER_TABLE),
            ("5", PLOTSTYLE_PLACEHOLDER),
            ("5", GLOBAL_MATERIAL),
        ] {
            assert!(
                TEMPLATE.contains(&format!("  {code}\n{h}\n")),
                "handle {h} missing from template"
            );
        }
        let mut max = 0;
        for chunk in split_markers(TEMPLATE) {
            let Chunk::Text(t) = chunk else { continue };
            let lines: Vec<_> = t.lines().collect();
            assert!(
                lines.len() % 2 == 0,
                "template chunk is not made of code/value pairs"
            );
            for pair in lines.chunks(2) {
                if matches!(pair[0].trim(), "5" | "105") {
                    max = max.max(u64::from_str_radix(pair[1].trim(), 16).unwrap());
                }
            }
        }
        assert!(
            max < FIRST_HANDLE,
            "template handle {max:X} collides with writer handles"
        );
    }
}
