//! obj2cad command-line tool. Same engine and same automatic decisions as the web app.

use obj2cad_core::hints::{self, Choices};
use obj2cad_core::Meta;
use obj2cad_core::{convert, mtl, parse, report, LayerMode, Options, Units, UpAxis};
use obj2cad_dxf::Format;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  obj2cad convert <input.obj> [options]
      -o <out>                   output file (default: input with .dxf; a .dwg name writes DWG)
      --format dxf|dxf-binary|dwg   ASCII DXF (default), binary DXF, or DWG (beta)
      --mtl <file.mtl>           materials (default: the mtllib next to the OBJ)
      --units auto|unitless|mm|cm|m|in|ft          (default: auto)
      --default-units mm|cm|m|in|ft   unit for files whose exporter doesn't state one
      --up auto|as-is|y-to-z                       (default: auto)
      --layers objects|groups|materials|single     (default: objects)
      --keep-loose-points        write unused vertices as points
      --exclude-layer <name>     leave a layer out (repeatable)
      --report <file.json>       report path (default: <out>.report.json)
      --quiet
  obj2cad inspect <input.obj>    detected units and up direction
  obj2cad dwg-dump <file.dwg>    a DWG's geometry as JSON (exact bit patterns; for tests)
  obj2cad bench [--synthetic N] [--runs R] [--json] [files.obj...]
  obj2cad synth <n> <out.obj>    write an n×n synthetic terrain (for tests)
  obj2cad --version";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("obj2cad: {e}");
            ExitCode::FAILURE
        }
    }
}

/// What `convert` writes.
#[derive(Clone, Copy, PartialEq)]
enum OutFormat {
    Dxf(Format),
    Dwg,
}

impl OutFormat {
    fn id(self) -> &'static str {
        match self {
            OutFormat::Dxf(f) => f.id(),
            OutFormat::Dwg => obj2cad_dwg::FORMAT_ID,
        }
    }

    fn extension(self) -> &'static str {
        if self == OutFormat::Dwg {
            "dwg"
        } else {
            "dxf"
        }
    }
}

fn parse_units(s: &str) -> Result<Units, String> {
    Ok(match s {
        "unitless" => Units::Unitless,
        "mm" => Units::Millimeters,
        "cm" => Units::Centimeters,
        "m" => Units::Meters,
        "in" => Units::Inches,
        "ft" => Units::Feet,
        u => return Err(format!("unknown unit `{u}`")),
    })
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut it = args.into_iter();
    match it.next().as_deref() {
        Some("convert") => convert_cmd(it.collect()),
        Some("bench") => bench(it.collect()),
        Some("inspect") => {
            let path = it.next().ok_or(USAGE)?;
            let src = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            let doc = parse(&src).map_err(|e| format!("{path}: {e}"))?;
            let h = hints::hints(&doc);
            println!("{}", serde_json::to_string_pretty(&h).expect("serializes"));
            Ok(())
        }
        Some("dwg-dump") => {
            let path = it.next().ok_or(USAGE)?;
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            println!("{}", dwg_dump(bytes).map_err(|e| format!("{path}: {e}"))?);
            Ok(())
        }
        Some("synth") => {
            let n: usize = it.next().and_then(|v| v.parse().ok()).ok_or(USAGE)?;
            let out = it.next().ok_or(USAGE)?;
            std::fs::write(&out, obj2cad_core::synth::terrain(n)).map_err(|e| format!("{out}: {e}"))
        }
        Some("--version") => {
            println!("obj2cad {}", obj2cad_core::VERSION);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn convert_cmd(args: Vec<String>) -> Result<(), String> {
    let (mut input, mut output, mut mtl_path, mut report_path) = (None, None, None, None);
    let mut choices = Choices::default();
    let (mut format, mut layer_mode, mut keep_loose, mut exclude, mut quiet) =
        (None, LayerMode::Objects, false, Vec::new(), false);
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "-o" => output = Some(PathBuf::from(val()?)),
            "--mtl" => mtl_path = Some(PathBuf::from(val()?)),
            "--report" => report_path = Some(PathBuf::from(val()?)),
            "--format" => {
                format = Some(match val()?.as_str() {
                    "dxf" => OutFormat::Dxf(Format::Ascii),
                    "dxf-binary" => OutFormat::Dxf(Format::Binary),
                    "dwg" => OutFormat::Dwg,
                    f => return Err(format!("unknown format `{f}`")),
                })
            }
            "--units" => {
                let v = val()?;
                choices.units = if v == "auto" {
                    None
                } else {
                    Some(parse_units(&v)?)
                };
            }
            "--default-units" => choices.default_units = Some(parse_units(&val()?)?),
            "--up" => {
                choices.up_axis = match val()?.as_str() {
                    "auto" => None,
                    "as-is" => Some(UpAxis::AsIs),
                    "y-to-z" => Some(UpAxis::YUpToZUp),
                    u => return Err(format!("unknown up direction `{u}`")),
                }
            }
            "--layers" => {
                layer_mode = match val()?.as_str() {
                    "objects" => LayerMode::Objects,
                    "groups" => LayerMode::Groups,
                    "materials" => LayerMode::Materials,
                    "single" => LayerMode::Single,
                    m => return Err(format!("unknown layer mode `{m}`")),
                }
            }
            "--keep-loose-points" => keep_loose = true,
            "--exclude-layer" => exclude.push(val()?),
            "--quiet" => quiet = true,
            _ if input.is_none() && !a.starts_with('-') => input = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument `{a}`\n{USAGE}")),
        }
    }
    let input = input.ok_or(USAGE)?;
    // Without --format, an output named *.dwg means DWG.
    let format = format.unwrap_or(match &output {
        Some(o) if o.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg")) => OutFormat::Dwg,
        _ => OutFormat::Dxf(Format::Ascii),
    });
    let output = output.unwrap_or_else(|| input.with_extension(format.extension()));

    let src = std::fs::read(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let doc = parse(&src).map_err(|e| {
        let mut msg = format!("{}: {e}", input.display());
        for issue in e.more.iter() {
            msg.push_str(&format!("\n  line {}: {}", issue.line, issue.message));
        }
        msg
    })?;

    // MTL: explicit, else the first `mtllib` found next to the OBJ.
    let mtl_path = mtl_path.or_else(|| {
        let dir = input.parent().unwrap_or(Path::new("."));
        doc.mtllibs
            .iter()
            .map(|m| dir.join(m))
            .find(|p| p.is_file())
    });
    let palette = match &mtl_path {
        Some(p) => Some(mtl::parse(
            &std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?,
        )),
        None => None,
    };

    let h = hints::hints(&doc);
    let (units, up_axis) = hints::resolve(&h, &choices);
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let options = Options {
        units,
        up_axis,
        layer_mode,
        default_layer: stem,
        keep_loose_points: keep_loose,
        exclude_layers: exclude,
    };
    let model = convert(&doc, palette.as_ref(), options);

    let source_sha = obj2cad_core::hash::sha256_hex(&src);
    let parity = obj2cad_core::hash::parity_hash(&model);
    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let props = [
        ("obj2cad.version", obj2cad_core::VERSION),
        ("obj2cad.source_sha256", source_sha.as_str()),
        ("obj2cad.parity_hash", parity.as_str()),
        (
            "obj2cad.parity",
            if model.omissions.is_partial() {
                "partial"
            } else {
                "exact"
            },
        ),
        ("obj2cad.source_name", name.as_str()),
    ];
    let created_unix = std::fs::metadata(&input)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as f64);
    let meta = Meta {
        properties: &props,
        fingerprint_seed: &source_sha,
        created_unix,
    };
    let bytes = match format {
        OutFormat::Dxf(f) => obj2cad_dxf::write(&model, &meta, f),
        OutFormat::Dwg => obj2cad_dwg::write(&model, &meta).map_err(|e| e.to_string())?,
    };
    std::fs::write(&output, &bytes).map_err(|e| format!("{}: {e}", output.display()))?;

    let rep = report::build(
        &model,
        &report::Source {
            name: &name,
            len: src.len() as u64,
            sha256: &source_sha,
        },
        &parity,
        report::Written {
            format: format.id(),
            bytes: bytes.len() as u64,
            sha256: Some(obj2cad_core::hash::sha256_hex(&bytes)),
        },
    );
    let report_path = report_path.unwrap_or_else(|| output.with_extension("report.json"));
    let json = serde_json::to_string_pretty(&rep).expect("report serializes");
    std::fs::write(&report_path, json).map_err(|e| format!("{}: {e}", report_path.display()))?;

    if !quiet {
        let auto = |explicit: bool| if explicit { "" } else { " (auto)" };
        let unit_name = if units == Units::Unitless {
            "none".to_owned()
        } else {
            units.symbol().to_owned()
        };
        let up = if up_axis == UpAxis::YUpToZUp {
            "stood upright (y-to-z)"
        } else {
            "as exported"
        };
        eprintln!(
            "{} → {}  ({} faces, {} entities, parity {})\n  units: {unit_name}{}\n  up: {up}{}",
            input.display(),
            output.display(),
            rep.output.faces,
            rep.output.mesh_entities + rep.output.polylines + rep.output.points,
            &parity[..12],
            auto(choices.units.is_some()),
            auto(choices.up_axis.is_some()),
        );
        for d in &rep.diagnostics {
            eprintln!("  {:?}: {} (x{})", d.severity, d.message, d.count);
        }
    }
    Ok(())
}

/// Everything the parity harness checks in a DWG, read back with acadrust. Coordinates
/// are the doubles' bit patterns in hex, so nothing is lost to JSON number formatting.
fn dwg_dump(bytes: Vec<u8>) -> Result<String, String> {
    use obj2cad_dwg::acadrust::{Color, DwgReader, EntityType, Vector3};
    use serde_json::{json, Value};
    let outcome = DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read_with_stats()
        .map_err(|e| e.to_string())?;
    let doc = &outcome.document;
    let color = |c: &Color| match c {
        Color::Rgb { r, g, b } => json!(format!("{r:02x}{g:02x}{b:02x}")),
        Color::ByLayer => Value::Null,
        other => json!(format!("{other:?}")),
    };
    let bits = |v: &Vector3| [v.x, v.y, v.z].map(|c| format!("{:016x}", c.to_bits()));
    let entities: Vec<Value> = doc
        .entities()
        .map(|e| match e {
            EntityType::Mesh(m) => json!({
                "t": "mesh", "layer": m.common.layer, "color": color(&m.common.color),
                "v": m.vertices.iter().map(bits).collect::<Vec<_>>(),
                "faces": m.faces.iter().map(|f| &f.vertices).collect::<Vec<_>>(),
            }),
            EntityType::Polyline3D(l) => json!({
                "t": "polyline", "layer": l.common.layer, "color": color(&l.common.color),
                "v": l.vertices.iter().map(|v| bits(&v.position)).collect::<Vec<_>>(),
            }),
            EntityType::Point(p) => json!({
                "t": "point", "layer": p.common.layer, "color": color(&p.common.color), "v": [bits(&p.location)],
            }),
            other => json!({ "t": format!("{:?}", std::mem::discriminant(other)) }),
        })
        .collect();
    let layers: serde_json::Map<String, Value> = doc
        .layers
        .iter()
        .map(|l| (l.name.clone(), color(&l.color)))
        .collect();
    let custom: serde_json::Map<String, Value> = doc
        .summary_info
        .custom_properties
        .iter()
        .map(|(k, v)| (k.clone(), json!(v)))
        .collect();
    let stats = &outcome.stats;
    Ok(json!({
        "version": doc.version.as_str(),
        "insunits": doc.header.insertion_units,
        "custom": custom,
        "layers": layers,
        "problems": stats.diagnostics.len() + stats.skipped_source_records + stats.recovered_errors,
        "entities": entities,
    })
    .to_string())
}

/// Time each stage (best of `runs`) on a synthetic grid and/or given files.
fn bench(args: Vec<String>) -> Result<(), String> {
    use std::time::Instant;
    let (mut runs, mut json, mut synthetic, mut files) = (5usize, false, None, Vec::new());
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--runs" => {
                runs = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--runs needs a number")?
            }
            "--synthetic" => {
                synthetic = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .ok_or("--synthetic needs a size")?,
                )
            }
            "--json" => json = true,
            _ => files.push(PathBuf::from(a)),
        }
    }
    let mut inputs: Vec<(String, Vec<u8>)> = Vec::new();
    if synthetic.is_none() && files.is_empty() {
        synthetic = Some(1000);
    }
    if let Some(n) = synthetic {
        inputs.push((format!("synthetic-{n}"), obj2cad_core::synth::terrain(n)));
    }
    for f in files {
        let bytes = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        inputs.push((f.display().to_string(), bytes));
    }
    let best = |f: &mut dyn FnMut()| {
        (0..runs)
            .map(|_| {
                let t = Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .fold(f64::INFINITY, f64::min)
    };
    let meta = Meta {
        properties: &[],
        fingerprint_seed: "bench",
        created_unix: None,
    };
    let mut rows = Vec::new();
    for (name, src) in &inputs {
        let doc = parse(src).map_err(|e| format!("{name}: {e}"))?;
        let model = convert(&doc, None, Options::default());
        let dxf = obj2cad_dxf::write(&model, &meta, Format::Ascii);
        let parse_ms = best(&mut || {
            std::hint::black_box(parse(src).unwrap());
        });
        let convert_ms = best(&mut || {
            std::hint::black_box(convert(&doc, None, Options::default()));
        });
        let write_ms = best(&mut || {
            std::hint::black_box(obj2cad_dxf::write(&model, &meta, Format::Ascii));
        });
        let binary_ms = best(&mut || {
            std::hint::black_box(obj2cad_dxf::write(&model, &meta, Format::Binary));
        });
        let dwg_ms = best(&mut || {
            std::hint::black_box(obj2cad_dwg::write(&model, &meta).unwrap());
        });
        let hints_ms = best(&mut || {
            std::hint::black_box(hints::hints(&doc));
        });
        let hash_ms = best(&mut || {
            std::hint::black_box(obj2cad_core::hash::parity_hash(&model));
        });
        let total = parse_ms + convert_ms + write_ms + hash_ms;
        rows.push(serde_json::json!({
            "input": name, "bytes": src.len(), "faces": doc.faces.len(), "dxf_bytes": dxf.len(),
            "parse_ms": parse_ms, "hints_ms": hints_ms, "convert_ms": convert_ms, "write_ms": write_ms,
            "binary_write_ms": binary_ms, "dwg_write_ms": dwg_ms, "hash_ms": hash_ms,
            "total_ms": total, "mb_per_s": src.len() as f64 / 1e6 / (total / 1e3),
        }));
        if !json {
            println!(
                "{name}\n  {:.1} MB, {} faces → {:.1} MB DXF\n  parse {parse_ms:.0} ms · detect {hints_ms:.0} ms · convert {convert_ms:.0} ms · write {write_ms:.0} ms (binary {binary_ms:.0} ms, DWG {dwg_ms:.0} ms) · hash {hash_ms:.0} ms\n  total {total:.0} ms  ({:.0} MB/s)",
                src.len() as f64 / 1e6,
                doc.faces.len(),
                dxf.len() as f64 / 1e6,
                src.len() as f64 / 1e6 / (total / 1e3)
            );
        }
    }
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).expect("serializes")
        );
    }
    Ok(())
}
