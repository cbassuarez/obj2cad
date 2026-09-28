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
    let fail = |issues: Vec<ParseIssue>, truncated: bool| {
        let mut issues = issues;
        let first = issues.remove(0);
        ParseError {
            line: first.line,
            kind: first.kind,
            message: first.message,
            more: issues,
            truncated,
        }
    };
    if src.starts_with(b"\xFF\xFE") || src.starts_with(b"\xFE\xFF") {
        let issue = ParseIssue {
            line: 1,
            kind: ErrorKind::Encoding,
            message: "the file is UTF-16 encoded; it must be ASCII or UTF-8".into(),
        };
        return Err(fail(vec![issue], false));
    }
    let src = src.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(src);
    let sep_byte = if memchr::memchr(b'\n', src).is_none() && memchr::memchr(b'\r', src).is_some() {
        b'\r'
    } else {
        b'\n'
    };

    // Data lines: (line number, bytes).
    let mut data: Vec<(u64, &[u8])> = Vec::new();
    let mut declared: Option<(u64, u64)> = None;
    let mut seen_content = false;
    for (i, raw) in src.split(|&b| b == sep_byte).enumerate() {
        let line = i as u64 + 1;
        let t = trim(raw.strip_suffix(b"\r").unwrap_or(raw));
        if t.is_empty() || t[0] == b'#' || t.starts_with(b"//") {
            continue;
        }
        if !seen_content {
            seen_content = true;
            // A point count (PTS) or a row of column names.
            if t.iter().all(u8::is_ascii_digit) {
                declared = std::str::from_utf8(t)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .map(|n| (line, n));
                continue;
            }
            if t.iter()
                .any(|b| b.is_ascii_alphabetic() && !matches!(b, b'e' | b'E'))
                && !t
                    .windows(3)
                    .any(|w| w.eq_ignore_ascii_case(b"nan") || w.eq_ignore_ascii_case(b"inf"))
            {
                continue;
            }
        }
        data.push((line, t));
    }

    let mut issues: Vec<ParseIssue> = Vec::new();
    let Some(&(first_line, first)) = data.first() else {
        let mut doc = empty_doc();
        doc.diagnostics = Vec::new();
        return Ok(doc);
    };
    let sep = if first.contains(&b';') {
        Sep::Semicolon
    } else if first.contains(&b',') && split(first, Sep::Space).len() < 3 {
        Sep::Comma
    } else {
        Sep::Space
    };
    let columns = split(first, sep).len();
    if ![3, 4, 6, 7, 9].contains(&columns) {
        issues.push(ParseIssue {
            line: first_line,
            kind: ErrorKind::AmbiguousColumns,
            message: format!(
                "{columns} columns per point; expected x y z plus 0, 1, 3, 4 or 6 more"
            ),
        });
        return Err(fail(issues, false));
    }

    // Pass 1: validate every number, and learn what the extra columns look like.
    let mut facts = vec![
        Column {
            byte: true,
            above_one: false
        };
        columns
    ];
    let mut unit_rows = [true, true]; // columns 3..6 and 6..9 look like unit normals
    let mut truncated = false;
    for &(line, text) in &data {
        let toks = split(text, sep);
        let issue = if toks.len() != columns {
            Some((
                ErrorKind::WrongArity,
                format!(
                    "{} columns; the file's first point has {columns}",
                    toks.len()
                ),
            ))
        } else {
            let mut bad = None;
            let mut vals = [0f64; 9];
            for (c, t) in toks.iter().enumerate() {
                match number(t) {
                    Ok(v) => {
                        vals[c] = v;
                        if c >= 3 {
                            let f = &mut facts[c];
                            f.byte &= t.iter().all(u8::is_ascii_digit) && v <= 255.0;
                            f.above_one |= v > 1.0;
                        }
                    }
                    Err(e) => {
                        bad = Some(e);
                        break;
                    }
                }
            }
            if bad.is_none() && columns >= 6 {
                let unit = |a: usize| {
                    let n =
                        vals[a] * vals[a] + vals[a + 1] * vals[a + 1] + vals[a + 2] * vals[a + 2];
                    (n - 1.0).abs() < 1e-2
                };
                unit_rows[0] &= unit(3);
                if columns == 9 {
                    unit_rows[1] &= unit(6);
                }
            }
            bad
        };
        if let Some((kind, message)) = issue {
            issues.push(ParseIssue {
                line,
                kind,
                message,
            });
            if issues.len() >= MAX_ISSUES {
                truncated = true;
                break;
            }
        }
    }
    if !issues.is_empty() {
        return Err(fail(issues, truncated));
    }

    // Colors are integers 0..=255 with at least one value above 1; normals are unit
    // vectors that aren't all 0/1 integers (those could be near-black colors too).
    let bytes = |a: usize| (a..a + 3).all(|c| facts[c].byte);
    let rgb = |a: usize| bytes(a) && (a..a + 3).any(|c| facts[c].above_one);
    let normals = |a: usize, k: usize| unit_rows[k] && !bytes(a);
    let ambiguous = |what: &str| ParseIssue {
        line: first_line,
        kind: ErrorKind::AmbiguousColumns,
        message: format!("the extra columns could be {what}; export with a clear column layout"),
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

    // Pass 2: build the document.
    let mut doc = empty_doc();
    let n = data.len();
    doc.positions.reserve(n);
    doc.colors.reserve(n);
    doc.coord_offsets.reserve(n * 3);
    for &(_, text) in &data {
        let toks = split(text, sep);
        let mut p = [0f64; 3];
        for (a, t) in toks[..3].iter().enumerate() {
            p[a] = number(t).expect("validated");
            doc.coord_text.extend_from_slice(t);
            let end = u32::try_from(doc.coord_text.len()).map_err(|_| {
                fail(
                    vec![ParseIssue {
                        line: 0,
                        kind: ErrorKind::TooLarge,
                        message: "coordinate text exceeds 4 GB".into(),
                    }],
                    false,
                )
            })?;
            doc.coord_offsets.push(end);
        }
        doc.positions.push(p);
        doc.colors.push(
            layout
                .rgb_at()
                .map(|c| [0, 1, 2].map(|k| number(toks[c + k]).expect("validated") as f32 / 255.0)),
        );
    }
    if layout.rgb_at().is_some() {
        doc.counts.vertices_with_color = n as u64;
    }

    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let stem = stem.rsplit(['/', '\\']).next().unwrap_or(stem);
    doc.objects.push(stem.to_owned());
    doc.attrs.push(ElementAttrs {
        object: Some(0),
        group: None,
        material: None,
    });
    let all: Vec<u32> = (0..n as u32).collect();
    doc.points
        .push_element(&all, 0, first_line)
        .map_err(|issue| fail(vec![issue], false))?;

    let mut diags = Diagnostics::default();
    if let Some(what) = layout.dropped() {
        diags.push(Severity::Info, Code::PointAttributesDropped, 0, || {
            format!(
                "point {what} {} not carried",
                if what == "normals" { "are" } else { "is" }
            )
        });
    }
    if let Some((line, count)) = declared {
        if count != n as u64 {
            diags.push(Severity::Warning, Code::PointCountMismatch, line, || {
                format!("the file declares {count} points but has {n}")
            });
        }
    }
    doc.diagnostics = diags.into_vec();
    Ok(doc)
}

fn empty_doc() -> ObjDocument {
    let mut doc = ObjDocument {
        faces: Elements::empty(),
        lines: Elements::empty(),
        points: Elements::empty(),
        ..Default::default()
    };
    doc.coord_offsets.push(0);
    doc
}

fn number(t: &[u8]) -> Result<f64, (ErrorKind, String)> {
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
        assert_eq!(d.colors[0], Some([1.0, 0.0, 12.0 / 255.0]));
        let d = ok("0 0 0 0 0 -1\n1 1 1 0.6 0.8 0\n");
        assert_eq!(d.colors[0], None);
        assert!(d
            .diagnostics
            .iter()
            .any(|x| x.code == Code::PointAttributesDropped));
        let d = ok("1,2,3,10,20,30\n4,5,6,40,50,60\n");
        assert_eq!(d.coord_text(1, 2), "6");
        assert!(d.colors[1].is_some());
        let d = ok("5\n1 2 3 -120 10 20 30\n");
        assert!(d.colors[0].is_some());
        assert!(d
            .diagnostics
            .iter()
            .any(|x| x.code == Code::PointCountMismatch));
    }

    #[test]
    fn headers_and_comments_are_not_points() {
        let d = ok("# scanner\nX Y Z R G B\n// note\n1 2 3 4 5 200\n");
        assert_eq!(d.positions.len(), 1);
        assert_eq!(d.colors[0], Some([4.0 / 255.0, 5.0 / 255.0, 200.0 / 255.0]));
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
}
