//! Wavefront OBJ parser.
//!
//! Follows the OBJ spec (Wavefront Advanced Visualizer, Appendix B1) plus the common
//! `v x y z r g b` vertex-color extension. The parser describes the file faithfully;
//! deciding what a target format can or cannot carry is the converter's job, using
//! [`SourceCounts`] and the element tables.

use crate::diag::{Code, Diagnostic, Diagnostics, Severity};
use serde::Serialize;
use std::fmt;

/// Hard failure: the file cannot be interpreted without guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number.
    pub line: u64,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// A list of variable-length elements (faces, polylines, point sets) in flat storage.
#[derive(Debug, Default, Clone)]
pub struct Elements {
    /// `offsets[i]..offsets[i + 1]` is the index range of element `i`.
    pub offsets: Vec<u32>,
    /// 0-based vertex indices.
    pub indices: Vec<u32>,
    /// Index into [`ObjDocument::attrs`] for each element.
    pub attr: Vec<u32>,
    /// 1-based source line of each element.
    pub line: Vec<u64>,
}

impl Elements {
    fn new() -> Self {
        Self {
            offsets: vec![0],
            ..Default::default()
        }
    }

    pub fn len(&self) -> usize {
        self.attr.len()
    }

    pub fn is_empty(&self) -> bool {
        self.attr.is_empty()
    }

    pub fn get(&self, i: usize) -> &[u32] {
        &self.indices[self.offsets[i] as usize..self.offsets[i + 1] as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = &[u32]> + '_ {
        (0..self.len()).map(|i| self.get(i))
    }

    fn push(&mut self, indices: &[u32], attr: u32, line: u64) -> Result<(), ParseError> {
        self.indices.extend_from_slice(indices);
        let end = u32::try_from(self.indices.len()).map_err(|_| ParseError {
            line,
            message: "more than 4 billion element references".into(),
        })?;
        self.offsets.push(end);
        self.attr.push(attr);
        self.line.push(line);
        Ok(())
    }
}

/// The object / group / material state an element was declared under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ElementAttrs {
    pub object: Option<u32>,
    pub group: Option<u32>,
    pub material: Option<u32>,
}

/// Counts of source data that has no element-level representation.
#[derive(Debug, Default, Clone, Serialize)]
pub struct SourceCounts {
    pub texcoords: u64,
    pub normals: u64,
    pub param_vertices: u64,
    /// Vertices with an explicit homogeneous weight other than 1.
    pub weighted_vertices: u64,
    pub vertices_with_color: u64,
    pub smoothing_statements: u64,
    pub freeform_statements: u64,
    pub render_statements: u64,
    pub unknown_statements: u64,
    pub comment_lines: u64,
}

#[derive(Debug, Default)]
pub struct ObjDocument {
    pub positions: Vec<[f64; 3]>,
    /// Per-vertex RGB colors (`v x y z r g b`). `None` for vertices without a color.
    pub colors: Vec<Option<[f32; 3]>>,
    pub faces: Elements,
    pub lines: Elements,
    pub points: Elements,
    pub attrs: Vec<ElementAttrs>,
    pub objects: Vec<String>,
    pub groups: Vec<String>,
    pub materials: Vec<String>,
    pub mtllibs: Vec<String>,
    /// Leading comment block (exporter name, units hints). At most 32 lines.
    pub header_comments: Vec<String>,
    pub counts: SourceCounts,
    pub diagnostics: Vec<Diagnostic>,
    coord_text: Vec<u8>,
    coord_offsets: Vec<u32>,
}

impl ObjDocument {
    /// The exact source text of coordinate `axis` (0..3) of vertex `v`.
    pub fn coord_text(&self, v: usize, axis: usize) -> &str {
        let k = v * 3 + axis;
        let s =
            &self.coord_text[self.coord_offsets[k] as usize..self.coord_offsets[k + 1] as usize];
        // Only ASCII number tokens that parsed successfully are stored.
        std::str::from_utf8(s).expect("coordinate text is ASCII")
    }

    pub fn has_vertex_colors(&self) -> bool {
        self.counts.vertices_with_color > 0
    }
}

const FREEFORM: &[&str] = &[
    "cstype", "deg", "bmat", "step", "curv", "curv2", "surf", "parm", "trim", "hole", "scrv", "sp",
    "end", "con", "mg",
];
const RENDER: &[&str] = &[
    "usemap",
    "maplib",
    "lod",
    "bevel",
    "c_interp",
    "d_interp",
    "shadow_obj",
    "trace_obj",
    "ctech",
    "stech",
];

struct Parser<'a> {
    doc: ObjDocument,
    diags: Diagnostics,
    line: u64,
    attrs_lookup: std::collections::HashMap<ElementAttrs, u32>,
    current: ElementAttrs,
    current_attr_id: Option<u32>,
    in_header: bool,
    scratch: Vec<u32>,
    _src: std::marker::PhantomData<&'a ()>,
}

/// Parse an OBJ file from raw bytes.
pub fn parse(src: &[u8]) -> Result<ObjDocument, ParseError> {
    let mut p = Parser {
        doc: ObjDocument {
            faces: Elements::new(),
            lines: Elements::new(),
            points: Elements::new(),
            ..Default::default()
        },
        diags: Diagnostics::default(),
        line: 0,
        attrs_lookup: Default::default(),
        current: ElementAttrs {
            object: None,
            group: None,
            material: None,
        },
        current_attr_id: None,
        in_header: true,
        scratch: Vec::new(),
        _src: std::marker::PhantomData,
    };
    p.doc.coord_offsets.push(0);

    let mut logical: Vec<u8> = Vec::new();
    let mut logical_start = 0u64;
    for raw in src.split(|&b| b == b'\n') {
        p.line += 1;
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        if logical.is_empty() {
            logical_start = p.line;
        }
        // A trailing backslash joins the next line (spec: line continuation).
        if let Some(body) = raw.strip_suffix(b"\\") {
            logical.extend_from_slice(body);
            logical.push(b' ');
            continue;
        }
        let whole_line;
        let text: &[u8] = if logical.is_empty() {
            raw
        } else {
            logical.extend_from_slice(raw);
            whole_line = std::mem::take(&mut logical);
            &whole_line[..]
        };
        let line_no = p.line;
        p.line = logical_start;
        p.statement(text)?;
        p.line = line_no;
    }
    if !logical.is_empty() {
        let text = std::mem::take(&mut logical);
        p.line = logical_start;
        p.statement(&text)?;
    }

    let has_color = p.doc.counts.vertices_with_color;
    if has_color > 0 && has_color < p.doc.positions.len() as u64 {
        p.diags
            .push(Severity::Warning, Code::PartialVertexColors, 0, || {
                format!(
                    "{} of {} vertices have colors",
                    has_color,
                    p.doc.positions.len()
                )
            });
    }
    p.doc.diagnostics = p.diags.into_vec();
    Ok(p.doc)
}

fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == 0x0b || b == 0x0c
}

fn tokens(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&b| is_space(b)).filter(|t| !t.is_empty())
}

impl Parser<'_> {
    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            line: self.line,
            message: message.into(),
        })
    }

    fn text(&mut self, bytes: &[u8]) -> String {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => {
                let line = self.line;
                self.diags
                    .push(Severity::Warning, Code::NonUtf8Text, line, || {
                        "names are not valid UTF-8; invalid bytes were replaced with U+FFFD".into()
                    });
                String::from_utf8_lossy(bytes).into_owned()
            }
        }
    }

    fn statement(&mut self, text: &[u8]) -> Result<(), ParseError> {
        let trimmed = trim(text);
        if trimmed.is_empty() {
            return Ok(());
        }
        if trimmed[0] == b'#' {
            self.doc.counts.comment_lines += 1;
            if self.in_header && self.doc.header_comments.len() < 32 {
                let c = self.text(trim(&trimmed[1..]));
                self.doc.header_comments.push(c);
            }
            return Ok(());
        }
        self.in_header = false;
        // Strip a trailing comment that starts at a token boundary.
        let body = match trimmed
            .windows(2)
            .position(|w| is_space(w[0]) && w[1] == b'#')
        {
            Some(i) => trim(&trimmed[..i]),
            None => trimmed,
        };
        let kw_end = body.iter().position(|&b| is_space(b)).unwrap_or(body.len());
        let kw = &body[..kw_end];
        let rest = trim(&body[kw_end..]);

        match kw {
            b"v" => self.vertex(rest),
            b"vt" => {
                self.doc.counts.texcoords += 1;
                self.numbers(rest, 1, 3, "vt")
            }
            b"vn" => {
                self.doc.counts.normals += 1;
                self.numbers(rest, 3, 3, "vn")
            }
            b"vp" => {
                self.doc.counts.param_vertices += 1;
                self.numbers(rest, 1, 3, "vp")
            }
            b"f" | b"fo" => self.element(rest, Kind::Face),
            b"l" => self.element(rest, Kind::Line),
            b"p" => self.element(rest, Kind::Point),
            b"o" => {
                let name = self.text(rest);
                self.current.object = Some(intern(&mut self.doc.objects, name));
                self.current.group = None;
                self.current_attr_id = None;
                Ok(())
            }
            b"g" => {
                let mut names = tokens(rest);
                let first = names.next().map(|n| n.to_vec());
                if names.next().is_some() {
                    let line = self.line;
                    self.diags
                        .push(Severity::Info, Code::MultipleGroups, line, || {
                            "elements belong to several groups; the first group names the layer"
                                .into()
                        });
                }
                let name = match first {
                    Some(n) => self.text(&n),
                    None => "default".to_owned(),
                };
                self.current.group = Some(intern(&mut self.doc.groups, name));
                self.current_attr_id = None;
                Ok(())
            }
            b"usemtl" => {
                let name = self.text(rest);
                self.current.material = Some(intern(&mut self.doc.materials, name));
                self.current_attr_id = None;
                Ok(())
            }
            b"mtllib" => {
                let all: Vec<&[u8]> = tokens(rest).collect();
                if !all.is_empty()
                    && all
                        .iter()
                        .all(|t| t.to_ascii_lowercase().ends_with(b".mtl"))
                {
                    for t in all {
                        let n = self.text(t);
                        self.doc.mtllibs.push(n);
                    }
                } else if !rest.is_empty() {
                    // A single file name containing spaces.
                    let n = self.text(rest);
                    self.doc.mtllibs.push(n);
                }
                Ok(())
            }
            b"s" => {
                self.doc.counts.smoothing_statements += 1;
                Ok(())
            }
            _ => {
                let line = self.line;
                let name = String::from_utf8_lossy(kw).into_owned();
                if FREEFORM.iter().any(|k| k.as_bytes() == kw) {
                    self.doc.counts.freeform_statements += 1;
                    self.diags.push(Severity::Warning, Code::FreeformNotConverted, line, || {
                        format!("free-form geometry (`{name}` and related statements) is not converted yet")
                    });
                } else if RENDER.iter().any(|k| k.as_bytes() == kw) {
                    self.doc.counts.render_statements += 1;
                    self.diags
                        .push(Severity::Info, Code::RenderAttributeIgnored, line, || {
                            format!("render-only statement `{name}` has no CAD equivalent")
                        });
                } else {
                    self.doc.counts.unknown_statements += 1;
                    self.diags
                        .push(Severity::Warning, Code::UnknownStatement, line, || {
                            format!("unknown statement `{name}` was skipped")
                        });
                }
                Ok(())
            }
        }
    }

    fn number(&self, tok: &[u8]) -> Result<f64, ParseError> {
        let s = match std::str::from_utf8(tok) {
            Ok(s) => s,
            Err(_) => return self.err("number is not ASCII"),
        };
        match s.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(v),
            Ok(_) => self.err(format!("`{s}` is not a finite number")),
            Err(_) => self.err(format!("`{s}` is not a number")),
        }
    }

    fn numbers(&self, rest: &[u8], min: usize, max: usize, what: &str) -> Result<(), ParseError> {
        let mut n = 0;
        for t in tokens(rest) {
            self.number(t)?;
            n += 1;
        }
        if n < min || n > max {
            return self.err(format!("`{what}` needs {min}..={max} numbers, found {n}"));
        }
        Ok(())
    }

    fn vertex(&mut self, rest: &[u8]) -> Result<(), ParseError> {
        // No allocation: keep the first 7 tokens, validate any beyond that immediately.
        let mut toks: [&[u8]; 7] = [&[]; 7];
        let mut count = 0usize;
        for t in tokens(rest) {
            if count < toks.len() {
                toks[count] = t;
            } else {
                self.number(t)?;
            }
            count += 1;
        }
        if count < 3 {
            return self.err(format!(
                "vertex needs at least 3 coordinates, found {count}"
            ));
        }
        let mut p = [0.0f64; 3];
        for axis in 0..3 {
            p[axis] = self.number(toks[axis])?;
            self.doc.coord_text.extend_from_slice(toks[axis]);
            let end = u32::try_from(self.doc.coord_text.len()).map_err(|_| ParseError {
                line: self.line,
                message: "coordinate text exceeds 4 GB".into(),
            })?;
            self.doc.coord_offsets.push(end);
        }
        let mut color = None;
        match count {
            3 => {}
            4 => {
                if self.number(toks[3])? != 1.0 {
                    self.doc.counts.weighted_vertices += 1;
                }
            }
            6 => {
                let mut c = [0f32; 3];
                for i in 0..3 {
                    c[i] = self.number(toks[3 + i])? as f32;
                }
                color = Some(c);
                self.doc.counts.vertices_with_color += 1;
            }
            n => {
                for t in &toks[3..n.min(toks.len())] {
                    self.number(t)?;
                }
                let line = self.line;
                self.diags
                    .push(Severity::Warning, Code::UnusualVertexArity, line, || {
                        format!("vertex has {n} components; only x y z were used")
                    });
            }
        }
        self.doc.positions.push(p);
        self.doc.colors.push(color);
        Ok(())
    }

    fn attr_id(&mut self) -> u32 {
        if let Some(id) = self.current_attr_id {
            return id;
        }
        let next = self.doc.attrs.len() as u32;
        let id = *self.attrs_lookup.entry(self.current).or_insert(next);
        if id == next {
            self.doc.attrs.push(self.current);
        }
        self.current_attr_id = Some(id);
        id
    }

    fn element(&mut self, rest: &[u8], kind: Kind) -> Result<(), ParseError> {
        let nv = self.doc.positions.len() as i64;
        let limits = [
            nv,
            self.doc.counts.texcoords as i64,
            self.doc.counts.normals as i64,
        ];
        let mut idx = std::mem::take(&mut self.scratch);
        idx.clear();
        for tok in tokens(rest) {
            let mut parts = tok.split(|&b| b == b'/');
            for (slot, limit) in limits.iter().enumerate() {
                let Some(part) = parts.next() else { break };
                if part.is_empty() {
                    if slot == 0 {
                        return self.err(format!(
                            "`{}` has no vertex index",
                            String::from_utf8_lossy(tok)
                        ));
                    }
                    continue;
                }
                let resolved = self.index(part, *limit, ["vertex", "texture", "normal"][slot])?;
                if slot == 0 {
                    idx.push(resolved);
                }
            }
            if parts.next().is_some() {
                return self.err(format!(
                    "`{}` has too many `/` parts",
                    String::from_utf8_lossy(tok)
                ));
            }
        }
        let line = self.line;
        let attr = self.attr_id();
        match kind {
            Kind::Face => {
                if idx.len() < 3 {
                    self.diags
                        .push(Severity::Warning, Code::FaceTooSmall, line, || {
                            "face with fewer than 3 vertices was skipped".into()
                        });
                } else {
                    if has_repeat(&idx) {
                        self.diags
                            .push(Severity::Warning, Code::FaceRepeatedVertex, line, || {
                                "face uses the same vertex more than once (kept as-is)".into()
                            });
                    }
                    self.doc.faces.push(&idx, attr, line)?;
                }
            }
            Kind::Line => {
                if idx.len() < 2 {
                    return self.err("line element needs at least 2 vertices");
                }
                self.doc.lines.push(&idx, attr, line)?;
            }
            Kind::Point => {
                if idx.is_empty() {
                    return self.err("point element needs at least 1 vertex");
                }
                self.doc.points.push(&idx, attr, line)?;
            }
        }
        self.scratch = idx;
        Ok(())
    }

    /// Resolve a 1-based (positive) or relative (negative) index against `count` items so far.
    fn index(&self, tok: &[u8], count: i64, what: &str) -> Result<u32, ParseError> {
        let i = match parse_index(tok) {
            Some(i) => i,
            None => {
                return self.err(format!(
                    "`{}` is not a valid {what} index",
                    String::from_utf8_lossy(tok)
                ))
            }
        };
        let resolved = if i > 0 {
            i - 1
        } else if i < 0 {
            count + i
        } else {
            return self.err(format!(
                "{what} index 0 is invalid (OBJ indices start at 1)"
            ));
        };
        if resolved < 0 || resolved >= count {
            return self.err(format!(
                "{what} index {i} is out of range ({count} defined so far)"
            ));
        }
        Ok(resolved as u32)
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Face,
    Line,
    Point,
}

/// `[+-]?digits` as i64 without UTF-8 validation or allocation; `None` on anything else
/// (including overflow), matching `str::parse::<i64>`.
fn parse_index(tok: &[u8]) -> Option<i64> {
    let (neg, digits) = match tok.first()? {
        b'-' => (true, &tok[1..]),
        b'+' => (false, &tok[1..]),
        _ => (false, tok),
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: i64 = 0;
    for &d in digits {
        if !d.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(i64::from(d - b'0'))?;
    }
    Some(if neg { -v } else { v })
}

/// Does a face reference the same vertex twice? Quadratic scan for typical small faces
/// (no allocation), sort for large n-gons.
fn has_repeat(idx: &[u32]) -> bool {
    if idx.len() <= 16 {
        (1..idx.len()).any(|i| idx[..i].contains(&idx[i]))
    } else {
        let mut sorted = idx.to_vec();
        sorted.sort_unstable();
        sorted.windows(2).any(|w| w[0] == w[1])
    }
}

fn trim(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|&c| !is_space(c)).unwrap_or(b.len());
    let end = b
        .iter()
        .rposition(|&c| !is_space(c))
        .map_or(start, |e| e + 1);
    &b[start..end]
}

fn intern(list: &mut Vec<String>, name: String) -> u32 {
    if let Some(i) = list.iter().position(|n| *n == name) {
        return i as u32;
    }
    list.push(name);
    (list.len() - 1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> ObjDocument {
        parse(src.as_bytes()).unwrap()
    }

    #[test]
    fn keeps_exact_text_and_bits() {
        let d = ok("v 0.1 -0.000000 1e-5\nv 1.000000 2 3\nf 1 2 1\n");
        assert_eq!(d.coord_text(0, 0), "0.1");
        assert_eq!(d.coord_text(0, 1), "-0.000000");
        assert!(
            d.positions[0][1].is_sign_negative(),
            "sign of zero is preserved"
        );
        assert_eq!(d.coord_text(1, 0), "1.000000");
        assert_eq!(d.positions[0][2].to_bits(), 1e-5f64.to_bits());
    }

    #[test]
    fn negative_and_slashed_indices() {
        let d = ok("v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\nf -3/1/1 -2//1 -1/1\n");
        assert_eq!(d.faces.get(0), &[0, 1, 2]);
    }

    #[test]
    fn ngons_are_not_triangulated() {
        let d = ok("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 2 0\nf 1 2 3 4 5\n");
        assert_eq!(d.faces.get(0).len(), 5);
    }

    #[test]
    fn rejects_ambiguity() {
        assert!(parse(b"v 0 0 0\nf 1 2 3\n").is_err(), "out of range");
        assert!(parse(b"v 0 0 0\nf 0 1 1\n").is_err(), "zero index");
        assert!(parse(b"v nan 0 0\n").is_err(), "nan");
        assert!(parse(b"v 1e999 0 0\n").is_err(), "overflow");
        assert!(parse(b"v 1 2\n").is_err(), "too few coords");
        let e = parse(b"v 0 0 0\nv 0 0 0\n\nf 1 2 x\n").unwrap_err();
        assert_eq!(e.line, 4);
    }

    #[test]
    fn continuation_and_crlf() {
        let d = ok("v 0 0 0\r\nv 1 0 0\r\nv 0 1 0\r\nf 1 \\\r\n 2 3\r\n");
        assert_eq!(d.faces.len(), 1);
    }

    #[test]
    fn structure_is_tracked() {
        let d = ok(
            "mtllib a.mtl\no Chair\nv 0 0 0\nv 1 0 0\nv 0 1 0\ng seat back\nusemtl wood\nf 1 2 3\n",
        );
        let a = d.attrs[d.faces.attr[0] as usize];
        assert_eq!(d.objects[a.object.unwrap() as usize], "Chair");
        assert_eq!(d.groups[a.group.unwrap() as usize], "seat");
        assert_eq!(d.materials[a.material.unwrap() as usize], "wood");
        assert!(d.diagnostics.iter().any(|x| x.code == Code::MultipleGroups));
    }

    #[test]
    fn freeform_is_reported_not_dropped_silently() {
        let d = ok("v 0 0 0\ncstype bspline\ndeg 3\n");
        assert_eq!(d.counts.freeform_statements, 2);
        let diag = d
            .diagnostics
            .iter()
            .find(|x| x.code == Code::FreeformNotConverted)
            .unwrap();
        assert_eq!((diag.line, diag.count), (2, 2));
    }

    #[test]
    fn vertex_colors() {
        let d = ok("v 0 0 0 1 0 0\nv 0 0 0\n");
        assert_eq!(d.colors[0], Some([1.0, 0.0, 0.0]));
        assert!(d
            .diagnostics
            .iter()
            .any(|x| x.code == Code::PartialVertexColors));
    }
}
