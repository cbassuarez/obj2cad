//! Wavefront OBJ parser.
//!
//! Follows the OBJ spec (Wavefront Advanced Visualizer, Appendix B1) plus the common
//! `v x y z r g b` vertex-color extension. The parser describes the file faithfully;
//! deciding what a target format can or cannot carry is the converter's job, using
//! [`SourceCounts`] and the element tables.
//!
//! Anything that would make the geometry ambiguous is a hard error with a line number
//! and an [`ErrorKind`]. Parsing continues past an error to report up to
//! [`MAX_ISSUES`] problems at once, but a file with any error produces no document.

use crate::diag::{Code, Diagnostic, Diagnostics, Severity};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;

/// How many problems a failed parse reports before it stops looking.
pub const MAX_ISSUES: usize = 20;

/// What kind of problem made a file unreadable, so the UI can say what to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// A coordinate or number is not a number (`1.2.3`, `abc`).
    InvalidNumber,
    /// A number uses a decimal comma (`1,5`), typical of a localized exporter.
    CommaDecimal,
    /// `nan`, `inf`, or a value too large for a double.
    NonFinite,
    /// A `v`/`vt`/`vn`/`vp` line has the wrong number of values.
    WrongArity,
    /// An element refers to index 0 (OBJ indices start at 1).
    IndexZero,
    /// An element refers to a vertex, texture or normal that doesn't exist.
    IndexOutOfRange,
    /// An index is not an integer.
    InvalidIndex,
    /// A reference like `//3` or `1/2/3/4`.
    MalformedReference,
    /// A line element with one vertex, or a point element with none.
    ElementTooShort,
    /// Invisible characters (zero-width space, stray BOM, UTF-16 bytes) before a keyword.
    HiddenCharacters,
    /// The file is UTF-16; OBJ must be ASCII/UTF-8.
    Encoding,
    /// More than 4 billion references or 4 GB of coordinate text.
    TooLarge,
    /// A point cloud's extra columns could mean more than one thing.
    AmbiguousColumns,
}

/// One problem found while parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParseIssue {
    /// 1-based line number.
    pub line: u64,
    pub kind: ErrorKind,
    pub message: String,
}

/// Hard failure: the file cannot be interpreted without guessing. `line`, `kind` and
/// `message` describe the first problem; `more` lists the next ones in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: u64,
    pub kind: ErrorKind,
    pub message: String,
    pub more: Vec<ParseIssue>,
    /// Parsing stopped after [`MAX_ISSUES`] problems; there may be more.
    pub truncated: bool,
}

impl ParseError {
    /// Every reported problem, first one included.
    pub fn issues(&self) -> Vec<ParseIssue> {
        let mut all = vec![ParseIssue {
            line: self.line,
            kind: self.kind,
            message: self.message.clone(),
        }];
        all.extend(self.more.iter().cloned());
        all
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)?;
        if !self.more.is_empty() {
            let plus = if self.truncated { "+" } else { "" };
            write!(f, " (and {}{plus} more)", self.more.len())?;
        }
        Ok(())
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
        Self::empty()
    }

    pub(crate) fn empty() -> Self {
        Self {
            offsets: vec![0],
            ..Default::default()
        }
    }

    pub(crate) fn push_element(
        &mut self,
        indices: &[u32],
        attr: u32,
        line: u64,
    ) -> Result<(), ParseIssue> {
        self.push(indices, attr, line)
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

    fn push(&mut self, indices: &[u32], attr: u32, line: u64) -> Result<(), ParseIssue> {
        self.indices.extend_from_slice(indices);
        let end = u32::try_from(self.indices.len()).map_err(|_| ParseIssue {
            line,
            kind: ErrorKind::TooLarge,
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
    /// Free-form surfaces (`surf`) and curves (`curv`, `curv2`) in the file.
    pub freeform_surfaces: u64,
    pub freeform_curves: u64,
    pub render_statements: u64,
    pub unknown_statements: u64,
    pub comment_lines: u64,
    /// Faces skipped for having fewer than 3 vertices.
    pub faces_skipped: u64,
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
    /// Texture coordinates (`vt` u, v), kept for coloring faces from textures.
    pub texcoords: Vec<[f32; 2]>,
    /// Texture coordinate of each face corner, parallel to `faces.indices`
    /// ([`NO_UV`] where a corner has none). Empty when no face uses texture coordinates.
    pub face_uvs: Vec<u32>,
    pub(crate) coord_text: Vec<u8>,
    pub(crate) coord_offsets: Vec<u32>,
}

/// A face corner without a texture coordinate.
pub const NO_UV: u32 = u32::MAX;

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
const CORE: &[&str] = &[
    "v", "vt", "vn", "vp", "f", "fo", "l", "p", "o", "g", "s", "usemtl", "mtllib",
];

fn is_keyword(kw: &[u8]) -> bool {
    CORE.iter()
        .chain(FREEFORM)
        .chain(RENDER)
        .any(|k| k.as_bytes() == kw)
}

/// Deferred check for an index that pointed past the elements defined so far. The OBJ
/// spec numbers vertices by their order in the file, so a later definition is valid.
struct Forward {
    line: u64,
    index: u32,
    slot: usize,
}

/// Interned names (objects, groups, materials) with O(1) lookup.
#[derive(Default)]
struct Names {
    list: Vec<String>,
    index: HashMap<String, u32>,
}

impl Names {
    fn intern(&mut self, name: String) -> u32 {
        if let Some(&i) = self.index.get(&name) {
            return i;
        }
        let i = self.list.len() as u32;
        self.index.insert(name.clone(), i);
        self.list.push(name);
        i
    }
}

struct Parser {
    doc: ObjDocument,
    diags: Diagnostics,
    line: u64,
    attrs_lookup: HashMap<ElementAttrs, u32>,
    current: ElementAttrs,
    current_attr_id: Option<u32>,
    in_header: bool,
    scratch: Vec<u32>,
    scratch_uv: Vec<u32>,
    objects: Names,
    groups: Names,
    materials: Names,
    forward: Vec<Forward>,
}

/// Parse an OBJ file from raw bytes.
pub fn parse(src: &[u8]) -> Result<ObjDocument, ParseError> {
    parse_with_progress(src, |_, _| {})
}

/// [`parse`], calling `progress(bytes_done, bytes_total)` about every 8 MiB.
pub fn parse_with_progress(
    src: &[u8],
    mut progress: impl FnMut(usize, usize),
) -> Result<ObjDocument, ParseError> {
    if src.starts_with(b"\xFF\xFE") || src.starts_with(b"\xFE\xFF") {
        return Err(ParseError {
            line: 1,
            kind: ErrorKind::Encoding,
            message: "the file is UTF-16 encoded; OBJ must be ASCII or UTF-8".into(),
            more: Vec::new(),
            truncated: false,
        });
    }
    // A UTF-8 byte-order mark is an encoding marker, not content.
    let src = src.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(src);

    let mut p = Parser {
        doc: ObjDocument {
            faces: Elements::new(),
            lines: Elements::new(),
            points: Elements::new(),
            ..Default::default()
        },
        diags: Diagnostics::default(),
        line: 0,
        attrs_lookup: HashMap::new(),
        current: ElementAttrs {
            object: None,
            group: None,
            material: None,
        },
        current_attr_id: None,
        in_header: true,
        scratch: Vec::new(),
        scratch_uv: Vec::new(),
        objects: Names::default(),
        groups: Names::default(),
        materials: Names::default(),
        forward: Vec::new(),
    };
    p.doc.coord_offsets.push(0);

    let mut issues: Vec<ParseIssue> = Vec::new();
    let mut truncated = false;
    // Old Mac files end lines with CR alone.
    let sep = if memchr::memchr(b'\n', src).is_none() && memchr::memchr(b'\r', src).is_some() {
        b'\r'
    } else {
        b'\n'
    };
    const STEP: usize = 8 << 20;
    let mut next_report = STEP;
    let mut logical: Vec<u8> = Vec::new();
    let mut logical_start = 0u64;
    let mut start = 0usize;
    for end in memchr::memchr_iter(sep, src).chain(std::iter::once(src.len())) {
        let raw = &src[start..end];
        start = end + 1;
        if end >= next_report {
            progress(end, src.len());
            next_report = end + STEP;
        }
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
        if let Err(issue) = p.statement(text) {
            let fatal = issue.kind == ErrorKind::TooLarge;
            issues.push(issue);
            if fatal || issues.len() >= MAX_ISSUES {
                truncated = !fatal;
                break;
            }
        }
        p.line = line_no;
    }
    if !logical.is_empty() && issues.len() < MAX_ISSUES {
        let text = std::mem::take(&mut logical);
        p.line = logical_start;
        if let Err(issue) = p.statement(&text) {
            issues.push(issue);
        }
    }
    progress(src.len(), src.len());

    // Forward references must exist by the end of the file.
    let totals = [
        p.doc.positions.len() as u64,
        p.doc.counts.texcoords,
        p.doc.counts.normals,
    ];
    for f in &p.forward {
        if u64::from(f.index) >= totals[f.slot] && issues.len() < MAX_ISSUES {
            let what = ["vertex", "texture", "normal"][f.slot];
            issues.push(ParseIssue {
                line: f.line,
                kind: ErrorKind::IndexOutOfRange,
                message: format!(
                    "{what} index {} is out of range (the file defines {})",
                    f.index + 1,
                    totals[f.slot]
                ),
            });
        }
    }

    if !issues.is_empty() {
        issues.sort_by_key(|i| i.line);
        let first = issues.remove(0);
        return Err(ParseError {
            line: first.line,
            kind: first.kind,
            message: first.message,
            more: issues,
            truncated,
        });
    }

    let has_color = p.doc.counts.vertices_with_color;
    if has_color > 0 && has_color < p.doc.positions.len() as u64 {
        let total = p.doc.positions.len();
        p.diags
            .push(Severity::Warning, Code::PartialVertexColors, 0, || {
                format!("{has_color} of {total} vertices have colors")
            });
    }
    p.doc.objects = std::mem::take(&mut p.objects.list);
    p.doc.groups = std::mem::take(&mut p.groups.list);
    p.doc.materials = std::mem::take(&mut p.materials.list);
    p.doc.diagnostics = p.diags.into_vec();
    Ok(p.doc)
}

fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == 0x0b || b == 0x0c
}

fn tokens(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&b| is_space(b)).filter(|t| !t.is_empty())
}

impl Parser {
    fn issue<T>(&self, kind: ErrorKind, message: impl Into<String>) -> Result<T, ParseIssue> {
        Err(ParseIssue {
            line: self.line,
            kind,
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

    fn statement(&mut self, text: &[u8]) -> Result<(), ParseIssue> {
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
        let body =
            match memchr::memchr_iter(b'#', trimmed).find(|&i| i > 0 && is_space(trimmed[i - 1])) {
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
                let mut uv = [0f32; 2];
                let mut n = 0;
                for t in tokens(rest) {
                    let v = self.number(t)?;
                    if n < 2 {
                        uv[n] = v as f32;
                    }
                    n += 1;
                }
                // Keep indices aligned even when the line is rejected below.
                self.doc.texcoords.push(uv);
                if !(1..=3).contains(&n) {
                    return self.issue(
                        ErrorKind::WrongArity,
                        format!("`vt` needs 1..=3 numbers, found {n}"),
                    );
                }
                Ok(())
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
                self.current.object = Some(self.objects.intern(name));
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
                self.current.group = Some(self.groups.intern(name));
                self.current_attr_id = None;
                Ok(())
            }
            b"usemtl" => {
                let name = self.text(rest);
                self.current.material = Some(self.materials.intern(name));
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
                // A keyword behind invisible bytes (zero-width space, a stray BOM, UTF-16)
                // must not be skipped: dropping a `v` line would shift every later index.
                if !kw.iter().all(u8::is_ascii_graphic) {
                    let visible: Vec<u8> =
                        kw.iter().copied().filter(u8::is_ascii_graphic).collect();
                    if is_keyword(&visible) {
                        let k = String::from_utf8_lossy(&visible).into_owned();
                        return self.issue(
                            ErrorKind::HiddenCharacters,
                            format!("invisible characters before `{k}`"),
                        );
                    }
                }
                let line = self.line;
                let name = String::from_utf8_lossy(kw).into_owned();
                if FREEFORM.iter().any(|k| k.as_bytes() == kw) {
                    self.doc.counts.freeform_statements += 1;
                    match kw {
                        b"surf" => self.doc.counts.freeform_surfaces += 1,
                        b"curv" | b"curv2" => self.doc.counts.freeform_curves += 1,
                        _ => {}
                    }
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

    fn number(&self, tok: &[u8]) -> Result<f64, ParseIssue> {
        let s = match std::str::from_utf8(tok) {
            Ok(s) => s,
            Err(_) => {
                return self.issue(
                    ErrorKind::InvalidNumber,
                    "a number contains non-ASCII bytes",
                )
            }
        };
        match s.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(v),
            Ok(_) => self.issue(
                ErrorKind::NonFinite,
                format!("`{s}` is not a finite number"),
            ),
            Err(_) if s.contains(',') && s.replace(',', ".").parse::<f64>().is_ok() => self.issue(
                ErrorKind::CommaDecimal,
                format!("`{s}` uses a decimal comma"),
            ),
            Err(_) => self.issue(ErrorKind::InvalidNumber, format!("`{s}` is not a number")),
        }
    }

    fn numbers(&self, rest: &[u8], min: usize, max: usize, what: &str) -> Result<(), ParseIssue> {
        let mut n = 0;
        for t in tokens(rest) {
            self.number(t)?;
            n += 1;
        }
        if n < min || n > max {
            return self.issue(
                ErrorKind::WrongArity,
                format!("`{what}` needs {min}..={max} numbers, found {n}"),
            );
        }
        Ok(())
    }

    fn vertex(&mut self, rest: &[u8]) -> Result<(), ParseIssue> {
        match self.read_vertex(rest) {
            Ok((p, text, color, weighted)) => {
                for t in text {
                    self.doc.coord_text.extend_from_slice(t);
                    let end = u32::try_from(self.doc.coord_text.len()).map_err(|_| ParseIssue {
                        line: self.line,
                        kind: ErrorKind::TooLarge,
                        message: "coordinate text exceeds 4 GB".into(),
                    })?;
                    self.doc.coord_offsets.push(end);
                }
                if weighted {
                    self.doc.counts.weighted_vertices += 1;
                }
                if color.is_some() {
                    self.doc.counts.vertices_with_color += 1;
                }
                self.doc.positions.push(p);
                self.doc.colors.push(color);
                Ok(())
            }
            Err(e) => {
                // Keep indices aligned so one bad vertex doesn't cascade into bogus
                // index errors on every later face (the document is discarded anyway).
                let end = *self.doc.coord_offsets.last().unwrap_or(&0);
                self.doc.coord_offsets.extend_from_slice(&[end; 3]);
                self.doc.positions.push([0.0; 3]);
                self.doc.colors.push(None);
                Err(e)
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn read_vertex<'b>(
        &mut self,
        rest: &'b [u8],
    ) -> Result<([f64; 3], [&'b [u8]; 3], Option<[f32; 3]>, bool), ParseIssue> {
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
            return self.issue(
                ErrorKind::WrongArity,
                format!("vertex needs at least 3 coordinates, found {count}"),
            );
        }
        let mut p = [0.0f64; 3];
        for axis in 0..3 {
            p[axis] = self.number(toks[axis])?;
        }
        let (mut color, mut weighted) = (None, false);
        match count {
            3 => {}
            4 => weighted = self.number(toks[3])? != 1.0,
            6 => {
                let mut c = [0f32; 3];
                for i in 0..3 {
                    c[i] = self.number(toks[3 + i])? as f32;
                }
                color = Some(c);
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
        Ok((p, [toks[0], toks[1], toks[2]], color, weighted))
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

    fn element(&mut self, rest: &[u8], kind: Kind) -> Result<(), ParseIssue> {
        let counts = [
            self.doc.positions.len() as i64,
            self.doc.counts.texcoords as i64,
            self.doc.counts.normals as i64,
        ];
        let mut idx = std::mem::take(&mut self.scratch);
        idx.clear();
        let mut uvs = std::mem::take(&mut self.scratch_uv);
        uvs.clear();
        let result = self.references(rest, counts, &mut idx, &mut uvs);
        let line = self.line;
        let outcome = result.and_then(|()| {
            let attr = self.attr_id();
            match kind {
                Kind::Face => {
                    if idx.len() < 3 {
                        self.doc.counts.faces_skipped += 1;
                        self.diags
                            .push(Severity::Warning, Code::FaceTooSmall, line, || {
                                "face with fewer than 3 vertices was skipped".into()
                            });
                        Ok(())
                    } else {
                        if has_repeat(&idx) {
                            self.diags.push(
                                Severity::Warning,
                                Code::FaceRepeatedVertex,
                                line,
                                || "face uses the same vertex more than once (kept as-is)".into(),
                            );
                        }
                        let before = self.doc.faces.indices.len();
                        if uvs.iter().any(|&u| u != NO_UV) && self.doc.face_uvs.is_empty() {
                            self.doc.face_uvs.resize(before, NO_UV);
                        }
                        if !self.doc.face_uvs.is_empty() {
                            self.doc.face_uvs.extend_from_slice(&uvs);
                        }
                        self.doc.faces.push(&idx, attr, line)
                    }
                }
                Kind::Line if idx.len() < 2 => self.issue(
                    ErrorKind::ElementTooShort,
                    "line element needs at least 2 vertices",
                ),
                Kind::Line => self.doc.lines.push(&idx, attr, line),
                Kind::Point if idx.is_empty() => self.issue(
                    ErrorKind::ElementTooShort,
                    "point element needs at least 1 vertex",
                ),
                Kind::Point => self.doc.points.push(&idx, attr, line),
            }
        });
        self.scratch = idx;
        self.scratch_uv = uvs;
        outcome
    }

    fn references(
        &mut self,
        rest: &[u8],
        counts: [i64; 3],
        idx: &mut Vec<u32>,
        uvs: &mut Vec<u32>,
    ) -> Result<(), ParseIssue> {
        for tok in tokens(rest) {
            uvs.push(NO_UV);
            let mut parts = tok.split(|&b| b == b'/');
            for (slot, &count) in counts.iter().enumerate() {
                let Some(part) = parts.next() else { break };
                if part.is_empty() {
                    if slot == 0 {
                        let t = String::from_utf8_lossy(tok);
                        return self.issue(
                            ErrorKind::MalformedReference,
                            format!("`{t}` has no vertex index"),
                        );
                    }
                    continue;
                }
                let resolved = self.index(part, count, slot)?;
                match slot {
                    0 => idx.push(resolved),
                    1 => *uvs.last_mut().expect("pushed above") = resolved,
                    _ => {}
                }
            }
            if parts.next().is_some() {
                let t = String::from_utf8_lossy(tok);
                return self.issue(
                    ErrorKind::MalformedReference,
                    format!("`{t}` has too many `/` parts"),
                );
            }
        }
        Ok(())
    }

    /// Resolve a 1-based (positive) or relative (negative) index. Positive indices past
    /// the elements defined so far are checked at the end of the file.
    fn index(&mut self, tok: &[u8], count: i64, slot: usize) -> Result<u32, ParseIssue> {
        let what = ["vertex", "texture", "normal"][slot];
        let Some(i) = parse_index(tok) else {
            let t = String::from_utf8_lossy(tok);
            return self.issue(
                ErrorKind::InvalidIndex,
                format!("`{t}` is not a valid {what} index"),
            );
        };
        if i == 0 {
            return self.issue(
                ErrorKind::IndexZero,
                format!("{what} index 0 is invalid (OBJ indices start at 1)"),
            );
        }
        if i < 0 {
            let resolved = count + i;
            if resolved < 0 {
                return self.issue(
                    ErrorKind::IndexOutOfRange,
                    format!("{what} index {i} is out of range ({count} defined so far)"),
                );
            }
            return Ok(resolved as u32);
        }
        let Ok(resolved) = u32::try_from(i - 1) else {
            return self.issue(
                ErrorKind::IndexOutOfRange,
                format!("{what} index {i} is out of range"),
            );
        };
        if i > count {
            self.forward.push(Forward {
                line: self.line,
                index: resolved,
                slot,
            });
        }
        Ok(resolved)
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
        assert_eq!(
            parse(b"v 0 0 0\nf 1 2 3\n").unwrap_err().kind,
            ErrorKind::IndexOutOfRange
        );
        assert_eq!(
            parse(b"v 0 0 0\nf 0 1 1\n").unwrap_err().kind,
            ErrorKind::IndexZero
        );
        assert_eq!(
            parse(b"v nan 0 0\n").unwrap_err().kind,
            ErrorKind::NonFinite
        );
        assert_eq!(
            parse(b"v 1e999 0 0\n").unwrap_err().kind,
            ErrorKind::NonFinite
        );
        assert_eq!(parse(b"v 1 2\n").unwrap_err().kind, ErrorKind::WrongArity);
        assert_eq!(
            parse(b"v 1,5 0 0\n").unwrap_err().kind,
            ErrorKind::CommaDecimal
        );
        let e = parse(b"v 0 0 0\nv 0 0 0\n\nf 1 2 x\n").unwrap_err();
        assert_eq!((e.line, e.kind), (4, ErrorKind::InvalidIndex));
    }

    #[test]
    fn a_utf8_bom_is_not_content() {
        // The BOM used to hide the first `v`, shifting every face index by one.
        let d = ok("\u{feff}v 0 0 0\nv 10 0 0\nv 0 10 0\nv 0 0 10\nf 1 2 3\n");
        assert_eq!(d.positions.len(), 4);
        assert_eq!(d.positions[0], [0.0, 0.0, 0.0]);
        assert_eq!(d.faces.get(0), &[0, 1, 2]);
    }

    #[test]
    fn invisible_characters_before_a_keyword_are_errors() {
        // Zero-width space before `v` in the middle of a file.
        let e = parse("v 0 0 0\n\u{200b}v 1 0 0\nv 0 1 0\nf 1 2 3\n".as_bytes()).unwrap_err();
        assert_eq!((e.line, e.kind), (2, ErrorKind::HiddenCharacters));
        // A BOM that isn't at the very start is also hidden content.
        let e = parse("v 0 0 0\n\u{feff}v 1 0 0\n".as_bytes()).unwrap_err();
        assert_eq!(e.kind, ErrorKind::HiddenCharacters);
        // UTF-16 files are rejected up front.
        assert_eq!(
            parse(b"\xFF\xFEv\x00 \x000\x00").unwrap_err().kind,
            ErrorKind::Encoding
        );
        // Genuinely unknown keywords still only warn.
        assert!(parse(b"frobnicate 1 2\nv 0 0 0\n").is_ok());
    }

    #[test]
    fn reports_many_problems_at_once_without_cascades() {
        let e = parse(b"v 0 0 0\nv 1,5 0 0\nv 0 1 0\nf 1 2 3\nv x 0 0\nf 1 2 9\n").unwrap_err();
        let kinds: Vec<_> = e.issues().iter().map(|i| (i.line, i.kind)).collect();
        // The bad vertex on line 2 doesn't make `f 1 2 3` an index error.
        assert_eq!(
            kinds,
            [
                (2, ErrorKind::CommaDecimal),
                (5, ErrorKind::InvalidNumber),
                (6, ErrorKind::IndexOutOfRange)
            ]
        );
        let many: String = (0..40).map(|_| "v a 0 0\n").collect();
        let e = parse(many.as_bytes()).unwrap_err();
        assert_eq!(e.issues().len(), MAX_ISSUES);
        assert!(e.truncated);
    }

    #[test]
    fn forward_references_are_valid() {
        let d = ok("f 1 2 3\nv 0 0 0\nv 1 0 0\nv 0 1 0\n");
        assert_eq!(d.faces.get(0), &[0, 1, 2]);
        let e = parse(b"f 1 2 4\nv 0 0 0\nv 1 0 0\nv 0 1 0\n").unwrap_err();
        assert_eq!((e.line, e.kind), (1, ErrorKind::IndexOutOfRange));
    }

    #[test]
    fn continuation_and_line_endings() {
        let d = ok("v 0 0 0\r\nv 1 0 0\r\nv 0 1 0\r\nf 1 \\\r\n 2 3\r\n");
        assert_eq!(d.faces.len(), 1);
        // Classic Mac: CR only.
        let d = ok("v 0 0 0\rv 1 0 0\rv 0 1 0\rf 1 2 3\r");
        assert_eq!((d.positions.len(), d.faces.len()), (3, 1));
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
    fn many_objects_intern_quickly() {
        let src: String = (0..50_000)
            .map(|i| format!("o part{i}\nv 0 0 0\np -1\n"))
            .collect();
        let t = std::time::Instant::now();
        let d = ok(&src);
        assert_eq!(d.objects.len(), 50_000);
        assert!(
            t.elapsed().as_secs_f64() < 2.0,
            "interning is not quadratic"
        );
    }

    #[test]
    fn freeform_is_reported_not_dropped_silently() {
        let d = ok("v 0 0 0\ncstype bspline\ndeg 3\nsurf 0 1 0 1 1\ncurv 0 1 1\n");
        assert_eq!(d.counts.freeform_statements, 4);
        assert_eq!(
            (d.counts.freeform_surfaces, d.counts.freeform_curves),
            (1, 1)
        );
        let diag = d
            .diagnostics
            .iter()
            .find(|x| x.code == Code::FreeformNotConverted)
            .unwrap();
        assert_eq!((diag.line, diag.count), (2, 4));
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

    #[test]
    fn progress_is_reported() {
        // ~24 MB, so at least two progress steps (every 8 MiB) plus the final call.
        let src: String = (0..800_000)
            .map(|i| format!("v {i}.000000 0.000000 0.000000\n"))
            .collect();
        let mut calls = Vec::new();
        parse_with_progress(src.as_bytes(), |done, total| calls.push((done, total))).unwrap();
        assert!(calls.len() >= 2);
        assert_eq!(calls.last().unwrap().0, src.len());
    }
}
