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

    // The lines, numbered from 1, without their line ending, trimmed.
    let lines = memchr::memchr_iter(sep_byte, src)
        .chain(std::iter::once(src.len()))
        .scan(0usize, |start, end| {
            let raw = &src[*start..end];
            *start = end + 1;
            Some(trim(raw.strip_suffix(b"\r").unwrap_or(raw)))
        })
        .enumerate()
        .map(|(i, t)| (i as u64 + 1, t))
        .filter(|(_, t)| !(t.is_empty() || t[0] == b'#' || t.starts_with(b"//")));

    // The first line may be a point count (PTS) or a row of column names.
    let mut lines = lines.peekable();
    let mut declared: Option<(u64, u64)> = None;
    if let Some(&(line, t)) = lines.peek() {
        if t.iter().all(u8::is_ascii_digit) {
            declared = std::str::from_utf8(t)
                .ok()
                .and_then(|s| s.parse().ok())
                .map(|n| (line, n));
            lines.next();
        } else if t
            .iter()
            .any(|b| b.is_ascii_alphabetic() && !matches!(b, b'e' | b'E'))
            && !t
                .windows(3)
                .any(|w| w.eq_ignore_ascii_case(b"nan") || w.eq_ignore_ascii_case(b"inf"))
        {
            lines.next();
        }
    }

    let mut issues: Vec<ParseIssue> = Vec::new();
    let Some(&(first_line, first)) = lines.peek() else {
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

    // One pass: every number is checked and read once; what the extra columns mean is
    // decided once the whole file is seen, so they are kept (as f32: colors are small
    // integers, and normals and intensity aren't carried).
    let estimate = src.len() / (columns * 6);
    let mut doc = empty_doc();
    doc.positions.reserve(estimate);
    doc.coord_offsets.reserve(estimate * 3);
    doc.coord_text.reserve(src.len() / 2);
    let mut extra: Vec<f32> = Vec::with_capacity(estimate * (columns - 3));
    let mut facts = vec![
        Column {
            byte: true,
            above_one: false
        };
        columns
    ];
    let mut unit_rows = [true, true]; // columns 3..6 and 6..9 look like unit normals
    let mut truncated = false;
    let width = columns - 3;
    let mut next_report = STEP;
    for (line, text) in lines {
        // Where this line starts in the file (every line is a slice of it).
        let at = text.as_ptr() as usize - src.as_ptr() as usize;
        if at >= next_report {
            next_report = at + STEP;
            // The columns that look like colors so far (the same test as at the end).
            let rgb = |a: usize| {
                (a..a + 3).all(|c| facts[c].byte) && (a..a + 3).any(|c| facts[c].above_one)
            };
            let rgb_at = match columns {
                6 if rgb(3) => Some(3),
                7 if rgb(4) && !facts[3].byte => Some(4),
                9 if rgb(3) => Some(3),
                9 if rgb(6) => Some(6),
                _ => None,
            };
            progress(&Partial {
                done: at,
                total: src.len(),
                positions: &doc.positions,
                colors: match rgb_at {
                    Some(c) => Colors::Columns {
                        extra: &extra,
                        width,
                        at: c - 3,
                    },
                    None => Colors::None,
                },
            });
        }
        let mut toks: [&[u8]; 9] = [&[]; 9];
        let mut count = 0;
        for t in tokens(text, sep) {
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
            for (c, t) in toks[..columns].iter().enumerate() {
                match number(t) {
                    Ok(v) => {
                        vals[c] = v;
                        if c >= 3 {
                            let f = &mut facts[c];
                            f.byte &= v <= 255.0 && t.iter().all(u8::is_ascii_digit);
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
                    unit_rows[0] &= unit(3);
                    if columns == 9 {
                        unit_rows[1] &= unit(6);
                    }
                }
                // Once a row is wrong nothing more is built; only problems are gathered.
                if issues.is_empty() {
                    for t in &toks[..3] {
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
                    doc.positions.push([vals[0], vals[1], vals[2]]);
                    extra.extend(vals[3..columns].iter().map(|&v| v as f32));
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

    let n = doc.positions.len();
    doc.colors = match layout.rgb_at() {
        Some(c) => extra
            .chunks_exact(width)
            .map(|e| Some([0, 1, 2].map(|k| e[c - 3 + k] / 255.0)))
            .collect(),
        None => vec![None; n],
    };
    drop(extra);
    if layout.rgb_at().is_some() {
        doc.counts.vertices_with_color = n as u64;
    }
    progress(&Partial {
        done: src.len(),
        total: src.len(),
        positions: &doc.positions,
        colors: Colors::Vertex(&doc.colors),
    });

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
