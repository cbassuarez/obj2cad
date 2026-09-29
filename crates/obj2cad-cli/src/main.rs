//! obj2cad command-line tool. Same engine and same automatic decisions as the web app.

use obj2cad_core::hints::{self, Choices};
use obj2cad_core::Meta;
use obj2cad_core::{convert, parse, report, LayerMode, Options, Units, UpAxis};
use obj2cad_dxf::Format;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  obj2cad convert <inputs...> [options]
      inputs: .obj, .xyz, .mtl, .jpg/.png files, folders or .zip files; together they
      make one drawing (a lone .obj brings the .mtl and textures it names)
      -o <out>                   output file (default: input with .dxf; a .dwg name writes DWG)
      --format dxf|dxf-binary|dwg   ASCII DXF (default), binary DXF, or DWG (beta)
      --mtl <file.mtl>           materials (default: the mtllib next to the OBJ)
      --units auto|unitless|mm|cm|m|in|ft          (default: auto)
      --default-units mm|cm|m|in|ft   unit for files whose exporter doesn't state one
      --up auto|as-is|y-to-z                       (default: auto)
      --layers objects|groups|materials|single     (default: objects)
      --keep-loose-points        write unused vertices as points
      --curves                   also write curved surfaces found in the mesh (cylinders,
                                 cones, spheres, tori; every vertex within its precision)
      --exclude-layer <name>     leave a layer out (repeatable)
      --report <file.json>       report path (default: <out>.report.json)
      --quiet
  obj2cad inspect <inputs...>    detected units and up direction
  obj2cad dwg-dump <file.dwg>    a DWG's geometry as JSON (exact bit patterns; for tests)
  obj2cad bench [--synthetic N] [--runs R] [--json] [files.obj...]
  obj2cad synth <n> <out.obj>    write an n×n synthetic terrain (for tests)
  obj2cad acis-samples <dir>     ACIS test bodies as DXF, binary DXF and DWG (AutoCAD acceptance)
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
            let inputs: Vec<PathBuf> = it.map(PathBuf::from).collect();
            if inputs.is_empty() {
                return Err(USAGE.into());
            }
            let (bundle, _) = load_inputs(&inputs, None, "dxf")?;
            let h = hints::hints(&bundle.doc);
            println!("{}", serde_json::to_string_pretty(&h).expect("serializes"));
            Ok(())
        }
        Some("dwg-dump") => {
            let path = it.next().ok_or(USAGE)?;
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            println!("{}", dwg_dump(bytes).map_err(|e| format!("{path}: {e}"))?);
            Ok(())
        }
        Some("acis-samples") => {
            let dir = PathBuf::from(it.next().ok_or(USAGE)?);
            acis_samples(&dir)
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

/// A file for the bundle, with its modification time (Unix seconds).
struct Gathered {
    path: String,
    bytes: Vec<u8>,
    modified: Option<f64>,
}

fn modified(meta: &std::fs::Metadata) -> Option<f64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as f64)
}

/// Hidden files and macOS archive metadata are never part of a bundle.
fn hidden(path: &str) -> bool {
    path.split(['/', '\\'])
        .any(|p| p.starts_with('.') || p == "__MACOSX")
}

fn read_file(path: &Path, as_name: String, out: &mut Vec<Gathered>) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    out.push(Gathered {
        path: as_name,
        bytes,
        modified: modified(&meta),
    });
    Ok(())
}

fn read_dir(root: &Path, dir: &Path, out: &mut Vec<Gathered>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    for p in entries {
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        if hidden(&rel) {
            continue;
        }
        if p.is_dir() {
            read_dir(root, &p, out)?;
        } else {
            read_file(&p, rel, out)?;
        }
    }
    Ok(())
}

fn read_zip(path: &Path, out: &mut Vec<Gathered>) -> Result<(), String> {
    use std::io::Read;
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| err(&e))?;
    // Entries take the archive's date, as in the web app.
    let when = file.metadata().ok().as_ref().and_then(modified);
    let mut zip = zip::ZipArchive::new(file).map_err(|e| err(&e))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| err(&e))?;
        let name = entry.name().to_owned();
        if entry.is_dir() || hidden(&name) || !obj2cad_core::bundle::is_supported(&name) {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{}: {name}: {e}", path.display()))?;
        out.push(Gathered {
            path: name,
            bytes,
            modified: when,
        });
    }
    Ok(())
}

/// A model on its own brings the files it names from next to it: its material
/// libraries, and their texture images.
fn read_companions(
    model: &Path,
    explicit_mtl: Option<&Path>,
    out: &mut Vec<Gathered>,
) -> Result<(), String> {
    let have = |out: &Vec<Gathered>, p: &str| {
        let b = obj2cad_core::bundle::base_name(p).to_lowercase();
        out.iter()
            .any(|g| obj2cad_core::bundle::base_name(&g.path).to_lowercase() == b)
    };
    let src = &out.last().expect("the model was just read").bytes;
    let dir = model.parent().unwrap_or(Path::new("."));
    let mut libs: Vec<PathBuf> = explicit_mtl.map(Path::to_path_buf).into_iter().collect();
    if libs.is_empty() {
        for line in String::from_utf8_lossy(src).lines() {
            if let Some(rest) = line.trim().strip_prefix("mtllib") {
                if rest.starts_with(char::is_whitespace) {
                    libs.push(dir.join(rest.trim()));
                    libs.extend(rest.split_whitespace().map(|t| dir.join(t)));
                }
            }
        }
    }
    for lib in libs {
        let name = lib
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !lib.is_file() || have(out, &name) {
            continue;
        }
        read_file(&lib, name, out)?;
        let text = String::from_utf8_lossy(&out.last().expect("just read").bytes).into_owned();
        let lib_dir = lib.parent().unwrap_or(Path::new("."));
        for t in obj2cad_core::mtl::parse_library(text.as_bytes())
            .textures
            .values()
        {
            let img = lib_dir.join(t.file.replace('\\', "/"));
            let name = obj2cad_core::bundle::base_name(&t.file).to_owned();
            if img.is_file() && !have(out, &name) {
                read_file(&img, name, out)?;
            }
        }
    }
    Ok(())
}

/// Read every input (files, folders, zips) into one bundle, and the default
/// output path for a single folder or zip (empty otherwise).
fn load_inputs(
    inputs: &[PathBuf],
    mtl_path: Option<&Path>,
    extension: &str,
) -> Result<(obj2cad_core::bundle::Bundle, PathBuf), String> {
    let mut gathered: Vec<Gathered> = Vec::new();
    for input in inputs {
        let is_zip = input
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        if input.is_dir() {
            read_dir(input, input, &mut gathered)?;
        } else if is_zip {
            read_zip(input, &mut gathered)?;
        } else {
            let name = input
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            read_file(input, name, &mut gathered)?;
            if inputs.len() == 1
                && input
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("obj"))
            {
                read_companions(input, mtl_path, &mut gathered)?;
            }
        }
    }
    if let (Some(m), true) = (mtl_path, inputs.len() > 1) {
        let name = m
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        read_file(m, name, &mut gathered)?;
    }
    // A folder or zip names the drawing; loose files are named after their first model.
    let (bundle_name, default_out) = match inputs {
        [one]
            if one.is_dir()
                || one
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("zip")) =>
        {
            let n = one
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (n, one.with_extension(extension))
        }
        _ => (String::new(), PathBuf::new()),
    };
    let files = gathered
        .iter()
        .map(|g| obj2cad_core::bundle::InputFile {
            path: g.path.clone(),
            bytes: &g.bytes,
            sha256: None,
            modified: g.modified,
        })
        .collect();
    let bundle = obj2cad_core::bundle::load(files, &bundle_name, |_, _| {}).map_err(|e| {
        let mut msg = e.to_string();
        for issue in e.error.more.iter() {
            msg.push_str(&format!("\n  line {}: {}", issue.line, issue.message));
        }
        msg
    })?;
    Ok((bundle, default_out))
}

fn convert_cmd(args: Vec<String>) -> Result<(), String> {
    let (mut inputs, mut output, mut mtl_path, mut report_path) = (Vec::new(), None, None, None);
    let mut choices = Choices::default();
    let (mut format, mut layer_mode, mut keep_loose, mut exclude, mut quiet) =
        (None, LayerMode::Objects, false, Vec::new(), false);
    let mut curves = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "-o" => output = Some(PathBuf::from(val()?)),
            "--curves" => curves = true,
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
            _ if !a.starts_with('-') => inputs.push(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument `{a}`\n{USAGE}")),
        }
    }
    if inputs.is_empty() {
        return Err(USAGE.into());
    }
    // Without --format, an output named *.dwg means DWG.
    let format = format.unwrap_or(match &output {
        Some(o) if o.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg")) => OutFormat::Dwg,
        _ => OutFormat::Dxf(Format::Ascii),
    });

    let (bundle, default_out) = load_inputs(&inputs, mtl_path.as_deref(), format.extension())?;
    let output = output.unwrap_or_else(|| {
        if default_out.as_os_str().is_empty() {
            let first = &inputs[0];
            first.with_file_name(format!("{}.{}", bundle.stem, format.extension()))
        } else {
            default_out
        }
    });
    let doc = &bundle.doc;
    let images_by_material: std::collections::HashMap<&str, obj2cad_core::texture::Texture> =
        bundle
            .textures
            .iter()
            .map(|(m, t)| {
                (
                    m.as_str(),
                    obj2cad_core::texture::Texture {
                        image: &bundle.images[t.image],
                        map: &t.map,
                    },
                )
            })
            .collect();
    let materials = obj2cad_core::Materials {
        palette: bundle.palette.as_ref(),
        textures: images_by_material,
    };

    let h = hints::hints(doc);
    let (units, up_axis) = hints::resolve(&h, &choices);
    let options = Options {
        units,
        up_axis,
        layer_mode,
        default_layer: bundle.stem.clone(),
        keep_loose_points: keep_loose,
        exclude_layers: exclude,
        curves,
    };
    let mut model = obj2cad_core::convert_with(doc, &materials, options);
    let curves_label = model
        .options
        .curves
        .then(|| obj2cad_curves::label(&obj2cad_curves::add_to(&mut model).to_string()));

    let source_sha = bundle.source_sha256.clone();
    let parity = obj2cad_core::hash::parity_hash(&model);
    let name = bundle.name.clone();
    let mut props = vec![
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
    if let Some(c) = &curves_label {
        props.push(("obj2cad.curves", c.as_str()));
    }
    // The newest file that went into the drawing dates it.
    let created_unix = bundle.modified;
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
            len: bundle.source_len,
            sha256: &source_sha,
            files: &bundle.files,
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
            name,
            output.display(),
            rep.output.faces,
            rep.output.mesh_entities
                + rep.output.polylines
                + rep.output.points
                + rep.output.splines
                + model.surfaces.len() as u64,
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
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
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
            EntityType::Spline(x) => json!({
                "t": "spline", "layer": x.common.layer, "color": color(&x.common.color),
                "degree": x.degree, "rational": x.flags.rational,
                "knots": x.knots.iter().map(|k| format!("{:016x}", k.to_bits())).collect::<Vec<_>>(),
                "v": x.control_points.iter().map(bits).collect::<Vec<_>>(),
                "weights": x.weights.iter().map(|w| format!("{:016x}", w.to_bits())).collect::<Vec<_>>(),
            }),
            EntityType::Surface(x) => json!({
                "t": "surface", "layer": x.common.layer, "color": color(&x.common.color),
                "sab": hex(&x.acis_data.sab_data), "sat": x.acis_data.sat_data,
            }),
            EntityType::Solid3D(x) => json!({
                "t": "solid", "layer": x.common.layer, "color": color(&x.common.color),
                "sab": hex(&x.acis_data.sab_data), "sat": x.acis_data.sat_data,
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

/// Small bodies with known answers (plane, box, cylinder, cone, sphere, torus), written
/// as they would be for a recognized surface, for checking in AutoCAD.
fn acis_samples(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let doc = parse(b"").map_err(|e| e.to_string())?;
    for (name, body) in obj2cad_acis::samples::all() {
        body.validate(1e-9).map_err(|e| format!("{name}: {e}"))?;
        let mut model = convert(&doc, None, Options::default());
        model.layers.push(obj2cad_core::convert::Layer {
            name: "Curves".into(),
            source: String::new(),
            color: obj2cad_core::convert::layer_color(1),
            file: None,
        });
        model.surfaces.push(obj2cad_core::convert::SurfaceEntity {
            layer: 1,
            color: None,
            body,
            region: None,
        });
        let props = [
            ("obj2cad.version", obj2cad_core::VERSION),
            ("obj2cad.sample", name),
        ];
        let meta = Meta {
            properties: &props,
            fingerprint_seed: name,
            created_unix: Some(1_790_000_000.0),
        };
        let write = |ext: &str, bytes: Vec<u8>| {
            let p = dir.join(format!("{name}{ext}"));
            std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))
        };
        write(".dxf", obj2cad_dxf::write(&model, &meta, Format::Ascii))?;
        write(
            ".binary.dxf",
            obj2cad_dxf::write(&model, &meta, Format::Binary),
        )?;
        write(
            ".dwg",
            obj2cad_dwg::write(&model, &meta).map_err(|e| e.to_string())?,
        )?;
    }
    Ok(())
}
