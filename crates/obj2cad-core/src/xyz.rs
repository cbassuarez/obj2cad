//! ASCII point clouds (`.xyz`): one point per line, `x y z` plus optional columns.
//!
//! Coordinates keep their exact text, like OBJ vertices. Extra columns are only used
//! when their meaning is unambiguous across the whole file:
//!
//! | Columns | Layout |
//! |---|---|
//! | 3 | `x y z` |
//! | 4 | `x y z intensity` (intensity is not carried) |
//! | 6 | `x y z r g b` (integers 0–255) or `x y z nx ny nz` (unit normals, not carried) |
//! | 7 | `x y z intensity r g b` (the PTS layout) |
//! | 9 | `x y z r g b nx ny nz` or `x y z nx ny nz r g b` |
//!
//! Anything else, or columns that could mean two things, is an error rather than a guess.
//! Columns are separated by spaces/tabs, commas or semicolons, the same throughout.
//! `#` and `//` lines are comments; a first line of column names or a point count is
//! allowed.

use crate::diag::{Code, Diagnostics, Severity};
use crate::obj::{
    ElementAttrs, Elements, ErrorKind, ObjDocument, ParseError, ParseIssue, MAX_ISSUES,
};
use crate::partial::{Colors, Partial, STEP};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sep {
    Space,
    Comma,
    Semicolon,
}

fn split(line: &[u8], sep: Sep) -> Vec<&[u8]> {
    let is_sep = |b: u8| match sep {
        Sep::Space => b == b' ' || b == b'\t',
        Sep::Comma => b == b',',
        Sep::Semicolon => b == b';',
    };
    line.split(|&b| is_sep(b))
        .map(trim)
        .filter(|t| sep != Sep::Space || !t.is_empty())
        .collect()
}

/// The tokens of a row, as [`split`] finds them, without collecting them.
fn tokens(line: &[u8], sep: Sep) -> impl Iterator<Item = &[u8]> {
    let is_sep = move |b: &u8| match sep {
        Sep::Space => *b == b' ' || *b == b'\t',
        Sep::Comma => *b == b',',
        Sep::Semicolon => *b == b';',
    };
    line.split(is_sep)
        .map(trim)
        .filter(move |t| sep != Sep::Space || !t.is_empty())
}

fn trim(t: &[u8]) -> &[u8] {
    let start = t
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(t.len());
    let end = t
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |e| e + 1);
    &t[start..end]
}

/// Which extra columns hold what.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layout {
    Xyz,
    Intensity,
    Rgb,
    Normals,
    IntensityRgb,
    RgbNormals,
    NormalsRgb,
}

impl Layout {
    fn rgb_at(self) -> Option<usize> {
        match self {
            Layout::Rgb | Layout::RgbNormals => Some(3),
            Layout::IntensityRgb => Some(4),
            Layout::NormalsRgb => Some(6),
            _ => None,
        }
    }

    fn dropped(self) -> Option<&'static str> {
        match self {
            Layout::Xyz | Layout::Rgb => None,
            Layout::Intensity | Layout::IntensityRgb => Some("intensity"),
            Layout::Normals | Layout::RgbNormals | Layout::NormalsRgb => Some("normals"),
        }
    }
}

/// Facts about one extra column, gathered over the whole file.
#[derive(Clone, Copy, Default)]
struct Column {
    /// Every value is an integer 0..=255 written without a decimal point.
    byte: bool,
    /// Some value is above 1 (so 0..=255 integers can't be normals).
    above_one: bool,
}

pub fn parse(src: &[u8], name: &str) -> Result<ObjDocument, ParseError> {
    parse_with_progress(src, name, |_| {})
}

/// [`parse`], calling `progress` with what has been read about every
/// [`crate::partial::STEP`] bytes, and once at the end.
pub fn parse_with_progress(
    src: &[u8],
    name: &str,
    mut progress: impl FnMut(&Partial),
) -> Result<ObjDocument, ParseError> {
    let mut r = Reader::new(name, src.len());
    r.feed(src, &mut progress);
    r.finish(&mut progress)
}

/// What the reader is waiting for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// The first line that isn't a comment: a point count, column names, or a point.
    Start,
    /// The first point (after a point count or column names).
    First,
    Points,
}

/// A point cloud read in pieces as they arrive, so the file never has to be in memory
/// whole: a large scan's text is far larger than what is kept of it. Feeding the whole
/// file at once is [`parse_with_progress`]; any split gives the same document.
pub struct Reader {
    name: String,
    /// The file's size (for progress, and to reserve memory once the line length is known).
    total: usize,
    /// Bytes fed so far.
    fed: usize,
    /// A line not finished at the end of the last piece.
    carry: Vec<u8>,
    /// Lines so far, counted from 1, empty and comment lines included.
    line: u64,
    /// Line ending: `\r` alone only in a file without any `\n` (old Mac files), decided
    /// from the first piece that has either.
    sep_byte: Option<u8>,
    started: bool,
    stage: Stage,
    declared: Option<(u64, u64)>,
    first_line: u64,
    sep: Sep,
    columns: usize,
    facts: Vec<Column>,
    /// Columns 3..6 and 6..9 look like unit normals.
    unit_rows: [bool; 2],
    issues: Vec<ParseIssue>,
    truncated: bool,
    /// A problem that ends reading at once (the error returned, whatever else was found).
    fatal: Option<ParseIssue>,
    doc: ObjDocument,
    /// The extra columns of every point, as bytes: only colors (integers 0..=255) are
    /// carried, so a value that couldn't be one is kept as 0 (its column can't be colors).
    extra: Vec<u8>,
    next_report: usize,
    reserved: bool,
}

impl Reader {
    /// `total`: the file's size in bytes.
    pub fn new(name: &str, total: usize) -> Self {
        Reader {
            name: name.to_owned(),
            total,
            fed: 0,
            carry: Vec::new(),
            line: 0,
            sep_byte: None,
            started: false,
            stage: Stage::Start,
            declared: None,
            first_line: 0,
            sep: Sep::Space,
            columns: 0,
            facts: Vec::new(),
            unit_rows: [true, true],
            issues: Vec::new(),
            truncated: false,
            fatal: None,
            doc: empty_doc(),
            extra: Vec::new(),
            next_report: STEP,
            reserved: false,
        }
    }

    fn stopped(&self) -> bool {
        self.fatal.is_some() || self.truncated
    }

    /// Read the next piece of the file. The first must hold the file's first 3 bytes (or
    /// all of it), so a byte order mark is seen whole.
    pub fn feed(&mut self, chunk: &[u8], progress: &mut impl FnMut(&Partial)) {
        let mut base = self.fed;
        self.fed += chunk.len();
        if self.stopped() {
            return;
        }
        let mut chunk = chunk;
        if !self.started {
            self.started = true;
            if chunk.starts_with(b"\xFF\xFE") || chunk.starts_with(b"\xFE\xFF") {
                self.fatal = Some(ParseIssue {
                    line: 1,
                    kind: ErrorKind::Encoding,
                    message: "the file is UTF-16 encoded; it must be ASCII or UTF-8".into(),
                });
                return;
            }
            if let Some(rest) = chunk.strip_prefix(b"\xEF\xBB\xBF") {
                chunk = rest;
                base += 3;
            }
        }
        if self.sep_byte.is_none() {
            if memchr::memchr(b'\n', chunk).is_some() {
                self.sep_byte = Some(b'\n');
            } else if memchr::memchr(b'\r', chunk).is_some() {
                self.sep_byte = Some(b'\r');
            } else {
                self.carry.extend_from_slice(chunk);
                return;
            }
        }
        let sep = self.sep_byte.unwrap_or(b'\n');
        let mut start = 0;
        // The line the last piece ended in.
        if !self.carry.is_empty() {
            let Some(end) = memchr::memchr(sep, chunk) else {
                self.carry.extend_from_slice(chunk);
                return;
            };
            let mut line = std::mem::take(&mut self.carry);
            let at = base - line.len();
            line.extend_from_slice(&chunk[..end]);
            self.line_of(at, &line, progress);
            line.clear();
            self.carry = line;
            start = end + 1;
        }
        let from = start;
        for end in memchr::memchr_iter(sep, &chunk[from..]) {
            if self.stopped() {
                return;
            }
            let end = from + end;
            self.line_of(base + start, &chunk[start..end], progress);
            start = end + 1;
        }
        if !self.stopped() {
            self.carry.extend_from_slice(&chunk[start..]);
        }
    }

    /// One line, starting at byte `at` of the file, without its line ending.
    fn line_of(&mut self, at: usize, raw: &[u8], progress: &mut impl FnMut(&Partial)) {
        self.line += 1;
        let line = self.line;
        let text = trim(raw.strip_suffix(b"\r").unwrap_or(raw));
        if text.is_empty() || text[0] == b'#' || text.starts_with(b"//") {
            return;
        }
        if self.stage == Stage::Start {
            self.stage = Stage::First;
            if text.iter().all(u8::is_ascii_digit) {
                self.declared = std::str::from_utf8(text)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .map(|n| (line, n));
                return;
            }
            if text
                .iter()
                .any(|b| b.is_ascii_alphabetic() && !matches!(b, b'e' | b'E'))
                && !text
                    .windows(3)
                    .any(|w| w.eq_ignore_ascii_case(b"nan") || w.eq_ignore_ascii_case(b"inf"))
            {
                return;
            }
        }
        if self.stage == Stage::First {
            self.stage = Stage::Points;
            self.first(line, text);
            if self.stopped() {
                return;
            }
        }
        if at >= self.next_report {
            self.next_report = at + STEP;
            self.reserve(at);
            progress(&self.partial(at));
        }
        self.point(line, text);
    }

    /// The first point: how its columns are separated, and how many there are.
    fn first(&mut self, line: u64, first: &[u8]) {
        self.first_line = line;
        self.sep = if first.contains(&b';') {
            Sep::Semicolon
        } else if first.contains(&b',') && split(first, Sep::Space).len() < 3 {
            Sep::Comma
        } else {
            Sep::Space
        };
        self.columns = split(first, self.sep).len();
        if ![3, 4, 6, 7, 9].contains(&self.columns) {
            self.fatal = Some(ParseIssue {
                line,
                kind: ErrorKind::AmbiguousColumns,
                message: format!(
                    "{} columns per point; expected x y z plus 0, 1, 3, 4 or 6 more",
                    self.columns
                ),
            });
            return;
        }
        self.facts = vec![
            Column {
                byte: true,
                above_one: false
            };
            self.columns
        ];
    }

    /// Room for every point, estimated from how many the first part of the file held
    /// (so the arrays aren't grown by doubling, which would need twice their size).
    fn reserve(&mut self, at: usize) {
        if self.reserved || at == 0 {
            return;
        }
        self.reserved = true;
        let n = self.doc.positions.len();
        let estimate = (n as f64 * self.total as f64 / at as f64 * 1.03) as usize + 1024;
        let more = estimate.saturating_sub(n);
        self.doc.positions.reserve_exact(more);
        self.doc.coords.reserve_exact(more * 3);
        self.extra.reserve_exact(more * (self.columns - 3));
    }

    fn partial(&self, at: usize) -> Partial<'_> {
        // The columns that look like colors so far (the same test as at the end).
        let facts = &self.facts;
        let rgb =
            |a: usize| (a..a + 3).all(|c| facts[c].byte) && (a..a + 3).any(|c| facts[c].above_one);
        let rgb_at = match self.columns {
            6 if rgb(3) => Some(3),
            7 if rgb(4) && !facts[3].byte => Some(4),
            9 if rgb(3) => Some(3),
            9 if rgb(6) => Some(6),
            _ => None,
        };
        Partial {
            done: at,
            total: self.total,
            positions: &self.doc.positions,
            colors: match rgb_at {
                Some(c) => Colors::Columns {
                    extra: &self.extra,
                    width: self.columns - 3,
                    at: c - 3,
                },
                None => Colors::None,
            },
        }
    }

    fn point(&mut self, line: u64, text: &[u8]) {
        let columns = self.columns;
        let mut toks: [&[u8]; 9] = [&[]; 9];
        let mut count = 0;
        for t in tokens(text, self.sep) {
            if count < 9 {
                toks[count] = t;
            }
            count += 1;
        }
        let issue = if count != columns {
            Some((
                ErrorKind::WrongArity,
                format!("{count} columns; the file's first point has {columns}"),
            ))
        } else {
            let mut bad = None;
            let mut vals = [0f64; 9];
            let mut bytes = [0u8; 9];
            for (c, t) in toks[..columns].iter().enumerate() {
                match number(t) {
                    Ok(v) => {
                        vals[c] = v;
                        if c >= 3 {
                            let byte = v <= 255.0 && t.iter().all(u8::is_ascii_digit);
                            if byte {
                                bytes[c] = v as u8;
                            }
                            let f = &mut self.facts[c];
                            f.byte &= byte;
                            f.above_one |= v > 1.0;
                        }
                    }
                    Err(e) => {
                        bad = Some(e);
                        break;
                    }
                }
            }
            if bad.is_none() {
                if columns >= 6 {
                    let unit = |a: usize| {
                        let n = vals[a] * vals[a]
                            + vals[a + 1] * vals[a + 1]
                            + vals[a + 2] * vals[a + 2];
                        (n - 1.0).abs() < 1e-2
                    };
                    self.unit_rows[0] &= unit(3);
                    if columns == 9 {
                        self.unit_rows[1] &= unit(6);
                    }
                }
                // Once a row is wrong nothing more is built; only problems are gathered.
                if self.issues.is_empty() {
                    for (t, &v) in toks[..3].iter().zip(&vals) {
                        if self.doc.coords.push(t, v).is_err() {
                            self.fatal = Some(ParseIssue {
                                line: 0,
                                kind: ErrorKind::TooLarge,
                                message: "coordinate text exceeds 4 GB".into(),
                            });
                            return;
                        }
                    }
                    self.doc.positions.push([vals[0], vals[1], vals[2]]);
                    self.extra.extend_from_slice(&bytes[3..columns]);
                }
            }
            bad
        };
        if let Some((kind, message)) = issue {
            self.issues.push(ParseIssue {
                line,
                kind,
                message,
            });
            if self.issues.len() >= MAX_ISSUES {
                self.truncated = true;
            }
        }
    }

    /// The end of the file: the document, or what is wrong with it.
    pub fn finish(
        mut self,
        progress: &mut impl FnMut(&Partial),
    ) -> Result<ObjDocument, ParseError> {
        let fail = |mut issues: Vec<ParseIssue>, truncated: bool| {
            let first = issues.remove(0);
            ParseError {
                line: first.line,
                kind: first.kind,
                message: first.message,
                more: issues,
                truncated,
            }
        };
        // The last line, when the file doesn't end with a line ending.
        if !self.stopped() && !self.carry.is_empty() {
            let line = std::mem::take(&mut self.carry);
            let at = self.fed - line.len();
            self.line_of(at, &line, progress);
        }
        if let Some(issue) = self.fatal.take() {
            return Err(fail(vec![issue], false));
        }
        if !self.issues.is_empty() {
            return Err(fail(self.issues, self.truncated));
        }
        let mut doc = self.doc;
        if self.stage != Stage::Points {
            doc.diagnostics = Vec::new();
            return Ok(doc);
        }
        let (columns, facts, first_line) = (self.columns, &self.facts, self.first_line);

        // Colors are integers 0..=255 with at least one value above 1; normals are unit
        // vectors that aren't all 0/1 integers (those could be near-black colors too).
        let bytes = |a: usize| (a..a + 3).all(|c| facts[c].byte);
        let rgb = |a: usize| bytes(a) && (a..a + 3).any(|c| facts[c].above_one);
        let unit_rows = self.unit_rows;
        let normals = |a: usize, k: usize| unit_rows[k] && !bytes(a);
        let ambiguous = |what: &str| ParseIssue {
            line: first_line,
            kind: ErrorKind::AmbiguousColumns,
            message: format!(
                "the extra columns could be {what}; export with a clear column layout"
            ),
        };
        let layout = match columns {
            3 => Layout::Xyz,
            4 => Layout::Intensity,
            6 => match (rgb(3), normals(3, 0)) {
                (true, false) => Layout::Rgb,
                (false, true) => Layout::Normals,
                _ => return Err(fail(vec![ambiguous("colors or normals")], false)),
            },
            7 => {
                if rgb(4) && !facts[3].byte {
                    Layout::IntensityRgb
                } else {
                    return Err(fail(
                        vec![ambiguous("intensity and colors, or colors and alpha")],
                        false,
                    ));
                }
            }
            _ => match (rgb(3) && normals(6, 1), normals(3, 0) && rgb(6)) {
                (true, false) => Layout::RgbNormals,
                (false, true) => Layout::NormalsRgb,
                _ => {
                    return Err(fail(
                        vec![ambiguous("colors and normals in either order")],
                        false,
                    ))
                }
            },
        };

        let n = doc.positions.len();
        let width = columns - 3;
        let mut extra = self.extra;
        match layout.rgb_at() {
            Some(c) => {
                // Each point's red, green and blue to the front, in place: point i's come
                // from at least 3i, so nothing is overwritten before it is read.
                let at = c - 3;
                for i in 0..n {
                    let from = i * width + at;
                    extra.copy_within(from..from + 3, i * 3);
                }
                extra.truncate(n * 3);
                extra.shrink_to_fit();
                doc.colors.push_bytes(extra);
                doc.counts.vertices_with_color = n as u64;
            }
            None => {
                drop(extra);
                doc.colors.push_none(n);
            }
        }
        // Keep a little room for the files a bundle puts before this one (see
        // `bundle::merge`), not the whole estimate's.
        doc.positions.shrink_to(n + n / 32);
        doc.coords.shrink_to(3 * (n + n / 32));
        progress(&Partial {
            done: self.fed,
            total: self.total,
            positions: &doc.positions,
            colors: Colors::Vertex(&doc.colors),
        });

        let name = self.name.as_str();
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        let stem = stem.rsplit(['/', '\\']).next().unwrap_or(stem);
        doc.objects.push(stem.to_owned());
        doc.attrs.push(ElementAttrs {
            object: Some(0),
            group: None,
            material: None,
        });
        let Ok(count) = u32::try_from(n) else {
            return Err(fail(
                vec![ParseIssue {
                    line: 0,
                    kind: ErrorKind::TooLarge,
                    message: "more than 4 billion points".into(),
                }],
                false,
            ));
        };
        doc.points = Elements {
            offsets: vec![0, count],
            indices: (0..count).collect(),
            attr: vec![0],
            line: vec![first_line],
        };

        let mut diags = Diagnostics::default();
        if let Some(what) = layout.dropped() {
            diags.push(Severity::Info, Code::PointAttributesDropped, 0, || {
                format!(
                    "point {what} {} not carried",
                    if what == "normals" { "are" } else { "is" }
                )
            });
        }
        if let Some((line, count)) = self.declared {
            if count != n as u64 {
                diags.push(Severity::Warning, Code::PointCountMismatch, line, || {
                    format!("the file declares {count} points but has {n}")
                });
            }
        }
        doc.diagnostics = diags.into_vec();
        Ok(doc)
    }
}

fn empty_doc() -> ObjDocument {
    ObjDocument {
        faces: Elements::empty(),
        lines: Elements::empty(),
        points: Elements::empty(),
        ..Default::default()
    }
}

fn number(t: &[u8]) -> Result<f64, (ErrorKind, String)> {
    if let Some(v) = crate::obj::plain_number(t) {
        return Ok(v);
    }
    let s = std::str::from_utf8(t).map_err(|_| {
        (
            ErrorKind::InvalidNumber,
            "a number contains non-ASCII bytes".to_owned(),
        )
    })?;
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(v),
        Ok(_) => Err((
            ErrorKind::NonFinite,
            format!("`{s}` is not a finite number"),
        )),
        Err(_) if s.contains(',') && s.replace(',', ".").parse::<f64>().is_ok() => Err((
            ErrorKind::CommaDecimal,
            format!("`{s}` uses a decimal comma"),
        )),
        Err(_) => Err((ErrorKind::InvalidNumber, format!("`{s}` is not a number"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> ObjDocument {
        parse(s.as_bytes(), "scan.xyz").unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn keeps_exact_text_and_puts_points_on_their_own_layer() {
        let d = ok("0.100000 -0.000000 1e3\n2 3 4\n");
        assert_eq!(d.positions.len(), 2);
        assert_eq!(d.coord_text(0, 1), "-0.000000");
        assert!(d.positions[0][1].is_sign_negative());
        assert_eq!(d.coord_text(0, 2), "1e3");
        assert_eq!(d.objects, ["scan"]);
        assert_eq!(d.points.get(0), &[0, 1]);
    }

    #[test]
    fn reads_colors_and_drops_normals() {
        let d = ok("0 0 0 255 0 12\n1 1 1 0 128 0\n");
        assert_eq!(d.colors.get(0), Some([1.0, 0.0, 12.0 / 255.0]));
        let d = ok("0 0 0 0 0 -1\n1 1 1 0.6 0.8 0\n");
        assert_eq!(d.colors.get(0), None);
        assert!(d
            .diagnostics
            .iter()
            .any(|x| x.code == Code::PointAttributesDropped));
        let d = ok("1,2,3,10,20,30\n4,5,6,40,50,60\n");
        assert_eq!(d.coord_text(1, 2), "6");
        assert!(d.colors.get(1).is_some());
        let d = ok("5\n1 2 3 -120 10 20 30\n");
        assert!(d.colors.get(0).is_some());
        assert!(d
            .diagnostics
            .iter()
            .any(|x| x.code == Code::PointCountMismatch));
    }

    #[test]
    fn headers_and_comments_are_not_points() {
        let d = ok("# scanner\nX Y Z R G B\n// note\n1 2 3 4 5 200\n");
        assert_eq!(d.positions.len(), 1);
        assert_eq!(
            d.colors.get(0),
            Some([4.0 / 255.0, 5.0 / 255.0, 200.0 / 255.0])
        );
    }

    #[test]
    fn ambiguity_is_an_error() {
        // Integers that are 0 or 1 could be colors (nearly black) or axis-aligned normals.
        let e = parse(b"0 0 0 1 0 0\n1 1 1 0 1 0\n", "a.xyz").unwrap_err();
        assert_eq!(e.kind, ErrorKind::AmbiguousColumns);
        let e = parse(b"0 0 0 1 2 3 4\n", "a.xyz").unwrap_err();
        assert_eq!(e.kind, ErrorKind::AmbiguousColumns);
        let e = parse(b"0 0\n", "a.xyz").unwrap_err();
        assert_eq!(e.kind, ErrorKind::AmbiguousColumns);
    }

    #[test]
    fn bad_rows_are_errors_with_line_numbers() {
        let e = parse(b"0 0 0\n1 1\n2 x 2\n3;3;3\n", "a.xyz").unwrap_err();
        assert_eq!((e.line, e.kind), (2, ErrorKind::WrongArity));
        assert_eq!(e.more[0].line, 3);
        let e = parse(b"1,5;2;3\n", "a.xyz").unwrap_err();
        assert_eq!(e.kind, ErrorKind::CommaDecimal);
        let e = parse(b"1 2 nan\n", "a.xyz").unwrap_err();
        assert_eq!(e.kind, ErrorKind::NonFinite);
    }

    /// The document, or the error, as text: everything a reader produces.
    fn outcome(r: Result<ObjDocument, ParseError>) -> String {
        match r {
            Err(e) => format!("{e:?}"),
            Ok(d) => {
                let texts: Vec<String> = (0..d.positions.len())
                    .flat_map(|v| (0..3).map(move |a| (v, a)))
                    .map(|(v, a)| d.coord_text(v, a).to_string())
                    .collect();
                let colors: Vec<_> = (0..d.positions.len()).map(|v| d.colors.get(v)).collect();
                let bits: Vec<_> = d.positions.iter().map(|p| p.map(f64::to_bits)).collect();
                format!(
                    "{bits:?} {texts:?} {colors:?} {:?} {:?} {:?} {:?}",
                    d.points, d.objects, d.diagnostics, d.counts.vertices_with_color
                )
            }
        }
    }

    #[test]
    fn any_split_reads_the_same_as_the_whole_file() {
        let files: [&[u8]; 9] = [
            b"\xEF\xBB\xBF# scan\nX Y Z R G B\n1.5 -0.000000 1e3 10 20 30\n\n2 3 4 0 128 255",
            b"3\r1 2 3\r4 5 6\r7 8 9\r",
            b"1;2;3\r\n4;5;6\r\n",
            b"0 0 0 0.6 0.8 0\n1 1 1 0 0 -1\n",
            b"0 0 0\n1 1\n2 x 2\n",
            b"1 2\n",
            b"\xFF\xFEx",
            b"",
            b"# only a comment",
        ];
        for f in files {
            let whole = outcome(parse(f, "scan.xyz"));
            for size in 3..=f.len().max(3) {
                let mut r = Reader::new("scan.xyz", f.len());
                for piece in f.chunks(size) {
                    r.feed(piece, &mut |_| {});
                }
                assert_eq!(outcome(r.finish(&mut |_| {})), whole, "pieces of {size}");
            }
        }
    }

    #[test]
    fn progress_shows_the_points_read_so_far() {
        // ~12 MB of colored points: a few steps, then the final call.
        let src: String = (0..400_000)
            .map(|i| format!("{i}.125 {}.5 0.75 {} 120 7\n", i % 97, i % 256))
            .collect();
        let mut calls = Vec::new();
        let d = parse_with_progress(src.as_bytes(), "scan.xyz", |p| {
            let last = p.positions.len().saturating_sub(1);
            calls.push((p.done, p.positions.len(), p.color(last)));
        })
        .unwrap();
        assert!(calls.len() >= 3);
        assert!(calls
            .windows(2)
            .all(|w| w[0].0 < w[1].0 && w[0].1 <= w[1].1));
        // Before the end the columns already look like colors, and are shown as such.
        let (done, n, color) = calls[0];
        assert!(done >= STEP && n > 0 && n < 400_000);
        let i = n - 1;
        assert_eq!(
            color,
            Some([(i % 256) as f32 / 255.0, 120.0 / 255.0, 7.0 / 255.0])
        );
        assert_eq!(calls.last().unwrap().0, src.len());
        assert_eq!(calls.last().unwrap().1, d.positions.len());
    }
}
