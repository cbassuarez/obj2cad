//! Property tests for the parser and the whole pipeline (proptest; stable Rust).
//!
//! These complement the hand-written fixtures by exploring inputs nobody thought of.
//! The nightly `cargo-fuzz` job (fuzz/) runs the same entry points coverage-guided.

use obj2cad_core::{convert, parse, ErrorKind, LayerMode, Options, UpAxis};
use proptest::prelude::*;

/// A finite double and one of the ways an exporter might write it.
fn coordinate() -> impl Strategy<Value = (f64, String)> {
    prop_oneof![
        // Shortest round-trip form of an arbitrary finite double.
        any::<u64>()
            .prop_map(f64::from_bits)
            .prop_filter("finite", |v| v.is_finite())
            .prop_map(|v| (v, format!("{v:?}"))),
        // Fixed 6 decimals, like most exporters.
        (-1e6f64..1e6).prop_map(|v| {
            let t = format!("{v:.6}");
            (t.parse().unwrap(), t)
        }),
        // Exponent notation and signed zero.
        (-1e300f64..1e300).prop_map(|v| {
            let t = format!("{v:e}");
            (t.parse().unwrap(), t)
        }),
        Just((-0.0, "-0.000000".to_owned())),
    ]
}

#[derive(Debug, Clone)]
struct Model {
    vertices: Vec<[(f64, String); 3]>,
    faces: Vec<Vec<usize>>,
}

fn model() -> impl Strategy<Value = Model> {
    prop::collection::vec([coordinate(), coordinate(), coordinate()], 3..40).prop_flat_map(
        |vertices| {
            let n = vertices.len();
            let faces = prop::collection::vec(prop::collection::vec(0..n, 3..7), 0..30);
            (Just(vertices), faces).prop_map(|(vertices, faces)| Model { vertices, faces })
        },
    )
}

/// Ways to write the same file that must not change what it means.
#[derive(Debug, Clone, Copy)]
struct Style {
    crlf: bool,
    bom: bool,
    negative_indices: bool,
    faces_first: bool,
    comments: bool,
    continuation: bool,
}

fn style() -> impl Strategy<Value = Style> {
    (
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(crlf, bom, negative_indices, faces_first, comments, continuation)| Style {
                crlf,
                bom,
                negative_indices: negative_indices && !faces_first,
                faces_first,
                comments,
                continuation,
            },
        )
}

fn render(m: &Model, s: Style) -> Vec<u8> {
    let nl = if s.crlf { "\r\n" } else { "\n" };
    let mut v = String::new();
    for (i, [x, y, z]) in m.vertices.iter().enumerate() {
        if s.comments && i % 5 == 0 {
            v += &format!("# vertex {i}{nl}");
        }
        v += &format!("v {} {} {}{nl}", x.1, y.1, z.1);
    }
    let mut f = String::new();
    for face in &m.faces {
        let refs: Vec<String> = face
            .iter()
            .map(|&i| {
                if s.negative_indices {
                    format!("-{}", m.vertices.len() - i)
                } else {
                    (i + 1).to_string()
                }
            })
            .collect();
        if s.continuation && refs.len() > 3 {
            f += &format!(
                "f {} \\{nl} {}{nl}",
                refs[..2].join(" "),
                refs[2..].join(" ")
            );
        } else {
            f += &format!("f {}{nl}", refs.join(" "));
        }
    }
    let body = if s.faces_first { f + &v } else { v + &f };
    let mut out = Vec::new();
    if s.bom {
        out.extend_from_slice(b"\xEF\xBB\xBF");
    }
    out.extend_from_slice(body.as_bytes());
    out
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Any bytes: an error or a document, never a panic.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = parse(&bytes);
    }

    /// Mostly-valid text with random damage: never a panic.
    #[test]
    fn damaged_files_never_panic(m in model(), s in style(), cut in any::<prop::sample::Index>(), junk in "[ -~\\t\u{feff}\u{200b},./#-]{0,8}") {
        let mut bytes = render(&m, s);
        let at = cut.index(bytes.len().max(1)).min(bytes.len());
        bytes.splice(at..at, junk.bytes());
        if let Ok(doc) = parse(&bytes) {
            let model = convert(&doc, None, Options::default());
            let _ = obj2cad_core::hash::parity_hash(&model);
        }
    }

    /// Whatever the style, the parsed geometry is exactly the generated geometry.
    #[test]
    fn geometry_round_trips_exactly(m in model(), s in style()) {
        let doc = parse(&render(&m, s)).expect("valid file");
        prop_assert_eq!(doc.positions.len(), m.vertices.len());
        for (i, v) in m.vertices.iter().enumerate() {
            for (a, (value, text)) in v.iter().enumerate() {
                prop_assert_eq!(doc.positions[i][a].to_bits(), value.to_bits(), "vertex {} axis {}", i, a);
                prop_assert_eq!(doc.coord_text(i, a), text.as_str());
            }
        }
        let faces: Vec<Vec<usize>> = doc.faces.iter().map(|f| f.iter().map(|&i| i as usize).collect()).collect();
        prop_assert_eq!(faces, m.faces.clone());
    }

    /// An invisible character before any keyword is an error, never a skipped line.
    #[test]
    fn hidden_characters_are_never_skipped(m in model(), line in any::<prop::sample::Index>(), hidden in prop::sample::select(vec!["\u{200b}", "\u{feff}", "\u{00a0}", "\u{2060}"])) {
        let text = String::from_utf8(render(&m, Style { crlf: false, bom: false, negative_indices: false, faces_first: false, comments: false, continuation: false })).unwrap();
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let i = line.index(lines.len());
        // A BOM at the very start of the file is an encoding marker, not content.
        prop_assume!(!(i == 0 && hidden == "\u{feff}"));
        lines[i] = format!("{hidden}{}", lines[i]);
        let err = parse(lines.join("\n").as_bytes()).expect_err("hidden characters must not parse");
        prop_assert_eq!(err.kind, ErrorKind::HiddenCharacters);
    }

    /// Everything that parses converts and writes, in every mode, without panicking,
    /// and both DXF encodings describe the same entities.
    #[test]
    fn pipeline_never_panics(m in model(), s in style(), upright in any::<bool>(), mode in 0usize..4, keep in any::<bool>()) {
        let doc = parse(&render(&m, s)).expect("valid file");
        let options = Options {
            up_axis: if upright { UpAxis::YUpToZUp } else { UpAxis::AsIs },
            layer_mode: [LayerMode::Objects, LayerMode::Groups, LayerMode::Materials, LayerMode::Single][mode],
            keep_loose_points: keep,
            ..Options::default()
        };
        let model = convert(&doc, None, options);
        let meta = obj2cad_dxf::Meta { properties: &[], fingerprint_seed: "p", created_unix: None };
        let ascii = obj2cad_dxf::write(&model, &meta, obj2cad_dxf::Format::Ascii);
        let binary = obj2cad_dxf::write(&model, &meta, obj2cad_dxf::Format::Binary);
        prop_assert!(ascii.ends_with(b"EOF\r\n"));
        prop_assert!(binary.ends_with(b"EOF\0"));
    }
}
