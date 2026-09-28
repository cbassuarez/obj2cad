//! `obj2cad convert input.obj [-o out.dxf] [--mtl file.mtl] [--units mm] [--up as-is|y-to-z] [--report r.json]`

use obj2cad_core::{convert, mtl, parse, report, Options, Units, UpAxis};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: obj2cad convert <input.obj> [-o out.dxf] [--mtl file.mtl] \
[--units unitless|mm|cm|m|in|ft] [--up as-is|y-to-z] [--report report.json]";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("obj2cad: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut it = args.into_iter();
    match it.next().as_deref() {
        Some("convert") => {}
        Some("bench") => return bench(it.collect()),
        Some("--version") => {
            println!("obj2cad {}", obj2cad_core::VERSION);
            return Ok(());
        }
        _ => return Err(USAGE.into()),
    }
    let (mut input, mut output, mut mtl_path, mut report_path) = (None, None, None, None);
    let mut options = Options::default();
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "-o" => output = Some(PathBuf::from(val()?)),
            "--mtl" => mtl_path = Some(PathBuf::from(val()?)),
            "--report" => report_path = Some(PathBuf::from(val()?)),
            "--units" => {
                options.units = match val()?.as_str() {
                    "unitless" => Units::Unitless,
                    "mm" => Units::Millimeters,
                    "cm" => Units::Centimeters,
                    "m" => Units::Meters,
                    "in" => Units::Inches,
                    "ft" => Units::Feet,
                    u => return Err(format!("unknown unit `{u}`")),
                }
            }
            "--up" => {
                options.up_axis = match val()?.as_str() {
                    "as-is" => UpAxis::AsIs,
                    "y-to-z" => UpAxis::YUpToZUp,
                    u => return Err(format!("unknown axis mode `{u}`")),
                }
            }
            _ if input.is_none() && !a.starts_with('-') => input = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument `{a}`\n{USAGE}")),
        }
    }
    let input = input.ok_or(USAGE)?;
    let output = output.unwrap_or_else(|| input.with_extension("dxf"));

    let src = std::fs::read(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let doc = parse(&src).map_err(|e| format!("{}: {e}", input.display()))?;

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

    let model = convert(&doc, palette.as_ref(), options);
    let source_sha = obj2cad_core::hash::sha256_hex(&src);
    let parity = obj2cad_core::hash::parity_hash(&model);
    let props = [
        ("obj2cad.version", obj2cad_core::VERSION),
        ("obj2cad.source_sha256", source_sha.as_str()),
        ("obj2cad.parity_hash", parity.as_str()),
        ("obj2cad.parity", "exact"),
    ];
    let bytes = obj2cad_dxf::write(&model, &props, &source_sha);
    std::fs::write(&output, &bytes).map_err(|e| format!("{}: {e}", output.display()))?;

    let name = input
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let rep = report::build(
        &model,
        &report::Source {
            name: &name,
            len: src.len() as u64,
            sha256: &source_sha,
        },
        &parity,
        "dxf-r2018",
        &bytes,
        None,
    );
    let report_path = report_path.unwrap_or_else(|| output.with_extension("report.json"));
    let json = serde_json::to_string_pretty(&rep).expect("report serializes");
    std::fs::write(&report_path, json).map_err(|e| format!("{}: {e}", report_path.display()))?;

    eprintln!(
        "{} → {}  ({} faces, {} entities, parity {})",
        input.display(),
        output.display(),
        rep.output.faces,
        rep.output.mesh_entities + rep.output.polylines + rep.output.points,
        &parity[..12]
    );
    for d in &rep.diagnostics {
        eprintln!("  {:?}: {} (x{})", d.severity, d.message, d.count);
    }
    Ok(())
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
    let mut rows = Vec::new();
    for (name, src) in &inputs {
        let doc = parse(src).map_err(|e| format!("{name}: {e}"))?;
        let model = convert(&doc, None, Options::default());
        let dxf = obj2cad_dxf::write(&model, &[], "bench");
        let parse_ms = best(&mut || {
            std::hint::black_box(parse(src).unwrap());
        });
        let convert_ms = best(&mut || {
            std::hint::black_box(convert(&doc, None, Options::default()));
        });
        let write_ms = best(&mut || {
            std::hint::black_box(obj2cad_dxf::write(&model, &[], "bench"));
        });
        let hash_ms = best(&mut || {
            std::hint::black_box(obj2cad_core::hash::parity_hash(&model));
        });
        let total = parse_ms + convert_ms + write_ms + hash_ms;
        rows.push(serde_json::json!({
            "input": name, "bytes": src.len(), "faces": doc.faces.len(), "dxf_bytes": dxf.len(),
            "parse_ms": parse_ms, "convert_ms": convert_ms, "write_ms": write_ms, "hash_ms": hash_ms,
            "total_ms": total, "mb_per_s": src.len() as f64 / 1e6 / (total / 1e3),
        }));
        if !json {
            println!(
                "{name}\n  {:.1} MB, {} faces → {:.1} MB DXF\n  parse {parse_ms:.0} ms · convert {convert_ms:.0} ms · write {write_ms:.0} ms · hash {hash_ms:.0} ms\n  total {total:.0} ms  ({:.0} MB/s)",
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
