//! A bundle: the files that make up one drawing. Models (`.obj`), point clouds (`.xyz`),
//! material libraries (`.mtl`) and texture images (`.jpg`, `.png`), as they come out of
//! a 3D app or a scanner, loose, in a folder or in a zip.
//!
//! Files are matched by name, ignoring folders and case: each model to the libraries its
//! `mtllib` names, each material to the image its `map_Kd` names. Every model and cloud
//! goes into one document (they share coordinates), so everything downstream (parity
//! hash, writers, preview) works on a bundle exactly as on a single file. Every file is
//! accounted for in the report: used, not needed, or missing.

use crate::diag::{Code, Diagnostic, Diagnostics, Severity};
use crate::hash::sha256_hex;
use crate::mtl::{self, Library, TextureRef};
use crate::obj::{self, ElementAttrs, ObjDocument, ParseError, NO_UV};
use crate::texture::{self, Image};
use crate::xyz;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// One file as given: its path (folders are kept for display, ignored for matching).
pub struct InputFile<'a> {
    pub path: String,
    pub bytes: &'a [u8],
    /// Precomputed SHA-256 (the browser hashes natively); computed when `None`.
    pub sha256: Option<String>,
    /// Modification time, Unix seconds (whole seconds, so every caller agrees).
    pub modified: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Model,
    PointCloud,
    Materials,
    Texture,
    /// In the bundle but not used by anything.
    NotUsed,
    /// Named by another file but not in the bundle.
    Missing,
    /// Used, but couldn't be read (a damaged image).
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    /// Position of this file in the list given to [`load`].
    #[serde(skip)]
    pub input: usize,
    pub role: Role,
    pub bytes: u64,
    pub sha256: Option<String>,
    /// For missing files: who named it. For unreadable ones: what went wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A material's texture: which decoded image, and how the MTL maps it.
#[derive(Debug, Clone)]
pub struct MaterialTexture {
    pub image: usize,
    pub map: TextureRef,
}

pub struct Bundle {
    pub doc: ObjDocument,
    /// Material colors (`Kd`) by (possibly renamed) material name; `None` when no model
    /// has a material library.
    pub palette: Option<mtl::Palette>,
    pub textures: HashMap<String, MaterialTexture>,
    pub images: Vec<Image>,
    pub files: Vec<FileEntry>,
    /// The drawing's name: the model's file name, or the bundle's name for several.
    pub name: String,
    pub stem: String,
    /// The model's SHA-256; for several models or clouds, a hash of every used file's name
    /// and SHA-256 (sorted), so the drawing still identifies exactly what made it.
    pub source_sha256: String,
    pub source_len: u64,
    /// The newest modification time among the files used: the drawing's date.
    pub modified: Option<f64>,
}

/// A model or cloud that couldn't be read, and which file it was.
#[derive(Debug, Clone)]
pub struct BundleError {
    pub file: String,
    pub error: ParseError,
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.error)
    }
}

impl std::error::Error for BundleError {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Obj,
    Xyz,
    Mtl,
    Image,
    Other,
}

fn kind(name: &str) -> Kind {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("obj") => Kind::Obj,
        Some("xyz") => Kind::Xyz,
        Some("mtl") => Kind::Mtl,
        Some("jpg" | "jpeg" | "png") => Kind::Image,
        _ => Kind::Other,
    }
}

/// The file name without folders.
pub fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// Is this a file a bundle can use?
pub fn is_supported(path: &str) -> bool {
    kind(path) != Kind::Other
}

/// Load a bundle. `name` names the drawing when it holds several models or clouds (a
/// zip's or folder's name). `progress(done, total)` reports parsing by bytes.
pub fn load(
    files: Vec<InputFile<'_>>,
    name: &str,
    mut progress: impl FnMut(usize, usize),
) -> Result<Bundle, BundleError> {
    // Display names: file names, made unique in path order ("a.obj", "a (2).obj").
    let mut files: Vec<(usize, InputFile<'_>)> = files.into_iter().enumerate().collect();
    files.sort_by(|a, b| {
        base_name(&a.1.path)
            .to_lowercase()
            .cmp(&base_name(&b.1.path).to_lowercase())
            .then(a.1.path.cmp(&b.1.path))
    });
    let (order, files): (Vec<usize>, Vec<InputFile<'_>>) = files.into_iter().unzip();
    let mut taken: HashMap<String, u32> = HashMap::new();
    let names: Vec<String> = files
        .iter()
        .map(|f| {
            let base = base_name(&f.path);
            let n = taken.entry(base.to_lowercase()).or_default();
            *n += 1;
            if *n == 1 {
                base.to_owned()
            } else {
                match base.rsplit_once('.') {
                    Some((s, e)) => format!("{s} ({n}).{e}"),
                    None => format!("{base} ({n})"),
                }
            }
        })
        .collect();
    let by_name = |wanted: &str, k: Kind| -> Option<usize> {
        let w = base_name(wanted).to_lowercase();
        (0..files.len()).find(|&i| kind(&names[i]) == k && names[i].to_lowercase() == w)
    };

    let mut entries: Vec<FileEntry> = files
        .iter()
        .zip(&names)
        .zip(&order)
        .map(|((f, n), &input)| FileEntry {
            name: n.clone(),
            input,
            role: Role::NotUsed,
            bytes: f.bytes.len() as u64,
            sha256: f.sha256.clone(),
            note: None,
        })
        .collect();
    let mut missing: Vec<FileEntry> = Vec::new();
    let mut notes = Diagnostics::default();

    // ---- models and clouds -------------------------------------------------------------
    let geometry: Vec<usize> = (0..files.len())
        .filter(|&i| matches!(kind(&names[i]), Kind::Obj | Kind::Xyz))
        .collect();
    let total: usize = geometry.iter().map(|&i| files[i].bytes.len()).sum();
    let mut done = 0usize;
    let mut parts: Vec<(usize, ObjDocument)> = Vec::new();
    for &i in &geometry {
        let bytes = files[i].bytes;
        let doc = if kind(&names[i]) == Kind::Obj {
            obj::parse_with_progress(bytes, |d, _| progress(done + d, total))
        } else {
            xyz::parse(bytes, &names[i])
        }
        .map_err(|error| BundleError {
            file: names[i].clone(),
            error,
        })?;
        done += bytes.len();
        progress(done, total);
        entries[i].role = if kind(&names[i]) == Kind::Obj {
            Role::Model
        } else {
            Role::PointCloud
        };
        parts.push((i, doc));
    }
    let objs = parts
        .iter()
        .filter(|(i, _)| kind(&names[*i]) == Kind::Obj)
        .count();
    let mtls: Vec<usize> = (0..files.len())
        .filter(|&i| kind(&names[i]) == Kind::Mtl)
        .collect();

    // ---- material libraries and textures ------------------------------------------------
    let mut images: Vec<Image> = Vec::new();
    let mut image_of: HashMap<usize, Option<usize>> = HashMap::new();
    let mut any_library = false;
    // Per part: material name → (Kd, texture).
    let mut definitions: Vec<Definitions> = Vec::new();
    for (pi, (i, doc)) in parts.iter().enumerate() {
        let mut defs = HashMap::new();
        if kind(&names[*i]) != Kind::Obj {
            definitions.push(defs);
            continue;
        }
        let mut libs: Vec<usize> = Vec::new();
        for lib in &doc.mtllibs {
            match by_name(lib, Kind::Mtl) {
                Some(m) => libs.push(m),
                None => {
                    let n = base_name(lib).to_owned();
                    if !missing.iter().any(|e| e.name.eq_ignore_ascii_case(&n)) {
                        missing.push(FileEntry {
                            name: n.clone(),
                            input: usize::MAX,
                            role: Role::Missing,
                            bytes: 0,
                            sha256: None,
                            note: Some(format!("named by {}", names[*i])),
                        });
                    }
                    let who = names[*i].clone();
                    notes.push(Severity::Warning, Code::MissingFile, 0, || {
                        format!("{who} uses {n}, which isn't in the bundle")
                    });
                }
            }
        }
        // One model and one library: they belong together, whatever the model calls it.
        if libs.is_empty()
            && objs == 1
            && mtls.len() == 1
            && (!doc.materials.is_empty() || doc.mtllibs.is_empty())
        {
            libs.push(mtls[0]);
        }
        let _ = pi;
        for m in libs {
            any_library = true;
            entries[m].role = Role::Materials;
            let lib: Library = mtl::parse_library(files[m].bytes);
            for (mat, kd) in &lib.colors {
                defs.entry(mat.clone())
                    .or_insert((None, None))
                    .0
                    .get_or_insert(*kd);
            }
            for (mat, t) in &lib.textures {
                let slot = defs.entry(mat.clone()).or_insert((None, None));
                if slot.1.is_some() || !doc.materials.contains(mat) {
                    continue;
                }
                let Some(img) = by_name(&t.file, Kind::Image) else {
                    let n = base_name(&t.file).to_owned();
                    if !missing.iter().any(|e| e.name.eq_ignore_ascii_case(&n)) {
                        missing.push(FileEntry {
                            name: n.clone(),
                            input: usize::MAX,
                            role: Role::Missing,
                            bytes: 0,
                            sha256: None,
                            note: Some(format!("named by {}", names[m])),
                        });
                        let who = names[m].clone();
                        notes.push(Severity::Warning, Code::MissingFile, 0, || {
                            format!("{who} uses {n}, which isn't in the bundle")
                        });
                    }
                    continue;
                };
                let decoded = *image_of.entry(img).or_insert_with(|| {
                    match texture::decode(files[img].bytes) {
                        Ok(image) => {
                            entries[img].role = Role::Texture;
                            images.push(image);
                            Some(images.len() - 1)
                        }
                        Err(e) => {
                            entries[img].role = Role::Unreadable;
                            entries[img].note = Some(e.clone());
                            let n = names[img].clone();
                            notes.push(Severity::Warning, Code::MissingFile, 0, || {
                                format!(
                                    "{n} couldn't be read ({e}); its materials keep their color"
                                )
                            });
                            None
                        }
                    }
                });
                if let Some(image) = decoded {
                    slot.1 = Some(MaterialTexture {
                        image,
                        map: t.clone(),
                    });
                }
            }
        }
        definitions.push(defs);
    }

    // ---- one document -------------------------------------------------------------------
    let several = parts.len() > 1;
    let (doc, renames) = merge(&names, parts, &definitions, several);
    let mut palette = mtl::Palette::new();
    let mut textures = HashMap::new();
    for (pi, defs) in definitions.into_iter().enumerate() {
        for (mat, (kd, tex)) in defs {
            let name = renames.get(&(pi, mat.clone())).cloned().unwrap_or(mat);
            if let Some(kd) = kd {
                palette.entry(name.clone()).or_insert(kd);
            }
            if let Some(t) = tex {
                textures.entry(name).or_insert(t);
            }
        }
    }
    let mut doc = doc;
    doc.diagnostics.extend(notes.into_vec());

    // ---- identity -----------------------------------------------------------------------
    for (e, f) in entries.iter_mut().zip(&files) {
        if e.sha256.is_none() && e.role != Role::NotUsed {
            e.sha256 = Some(sha256_hex(f.bytes));
        }
    }
    let (display, source_sha256, source_len) = if geometry.len() == 1 {
        let g = geometry[0];
        (
            names[g].clone(),
            entries[g].sha256.clone().unwrap_or_default(),
            files[g].bytes.len() as u64,
        )
    } else {
        let used: BTreeMap<&str, &str> = entries
            .iter()
            .filter(|e| !matches!(e.role, Role::NotUsed | Role::Missing))
            .map(|e| (e.name.as_str(), e.sha256.as_deref().unwrap_or("")))
            .collect();
        let manifest: String = used.iter().map(|(n, h)| format!("{h}  {n}\n")).collect();
        let len = entries
            .iter()
            .filter(|e| !matches!(e.role, Role::NotUsed | Role::Missing))
            .map(|e| e.bytes)
            .sum();
        let display = if !name.trim().is_empty() {
            name.to_owned()
        } else {
            geometry
                .first()
                .map_or("bundle", |&g| stem_of(&names[g]))
                .to_owned()
        };
        (display, sha256_hex(manifest.as_bytes()), len)
    };
    let modified = entries
        .iter()
        .zip(&files)
        .filter(|(e, _)| e.role != Role::NotUsed)
        .filter_map(|(_, f)| f.modified.map(f64::floor))
        .reduce(f64::max);
    entries.extend(missing);
    Ok(Bundle {
        modified,
        doc,
        palette: any_library.then_some(palette),
        textures,
        images,
        files: entries,
        stem: stem_of(&display).to_owned(),
        name: display,
        source_sha256,
        source_len,
    })
}

type Renames = HashMap<(usize, String), String>;

/// A model's materials: name → (diffuse color, texture).
type Definitions = HashMap<String, (Option<[f64; 3]>, Option<MaterialTexture>)>;

/// Concatenate documents. With several parts, unnamed geometry is named after its file
/// (so each file gets its own layer), object and group names another file already used
/// get the later file's name ("Chair (b)"), so two files never share a layer by accident,
/// and materials that two files define differently are renamed "material (file)".
fn merge(
    names: &[String],
    parts: Vec<(usize, ObjDocument)>,
    definitions: &[Definitions],
    several: bool,
) -> (ObjDocument, Renames) {
    let mut renames = Renames::new();
    if !several {
        let doc = parts.into_iter().next().map(|(_, d)| d).unwrap_or_else(|| {
            let mut d = ObjDocument {
                faces: obj::Elements::empty(),
                lines: obj::Elements::empty(),
                points: obj::Elements::empty(),
                ..Default::default()
            };
            d.coord_offsets.push(0);
            d
        });
        return (doc, renames);
    }

    // Material names that mean different things in different files.
    let key = |pi: usize, m: &str| {
        definitions[pi].get(m).map(|(kd, t)| {
            (
                kd.map(|c| c.map(f64::to_bits)),
                t.as_ref().map(|t| {
                    (
                        t.image,
                        t.map.file.clone(),
                        t.map.offset.map(f64::to_bits),
                        t.map.scale.map(f64::to_bits),
                    )
                }),
            )
        })
    };
    let mut first_meaning: HashMap<String, _> = HashMap::new();
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (pi, (_, d)) in parts.iter().enumerate() {
        for m in &d.materials {
            let k = key(pi, m);
            match first_meaning.get(m) {
                None => {
                    first_meaning.insert(m.clone(), k);
                    used_names.insert(m.clone());
                }
                Some(prev) if *prev == k => {}
                Some(_) => {
                    let stem = stem_of(&names[parts[pi].0]);
                    let mut n = format!("{m} ({stem})");
                    let mut i = 2;
                    while used_names.contains(&n) {
                        n = format!("{m} ({stem} {i})");
                        i += 1;
                    }
                    used_names.insert(n.clone());
                    renames.insert((pi, m.clone()), n);
                }
            }
        }
    }

    let mut out = ObjDocument {
        faces: obj::Elements::empty(),
        lines: obj::Elements::empty(),
        points: obj::Elements::empty(),
        ..Default::default()
    };
    out.coord_offsets.push(0);
    let any_uvs = parts.iter().any(|(_, d)| !d.face_uvs.is_empty());
    let mut names_index: [HashMap<String, u32>; 3] = Default::default();
    // Object and group names already used by an earlier file.
    let mut taken: [std::collections::HashSet<String>; 2] = Default::default();
    let mut header_taken = false;
    let mut attr_index: HashMap<ElementAttrs, u32> = HashMap::new();
    for (pi, (fi, d)) in parts.into_iter().enumerate() {
        let file = &names[fi];
        let stem = stem_of(file).to_owned();
        out.files.push(file.clone());
        // This file's object/group names as written in the drawing.
        let mut own: [HashMap<String, String>; 2] = Default::default();
        let mut claim = |which: usize, raw: &str| -> String {
            if let Some(n) = own[which].get(raw) {
                return n.clone();
            }
            let mut n = raw.to_owned();
            let mut i = 2;
            while taken[which].contains(&n) {
                n = if i == 2 {
                    format!("{raw} ({stem})")
                } else {
                    format!("{raw} ({stem} {i})")
                };
                i += 1;
            }
            taken[which].insert(n.clone());
            own[which].insert(raw.to_owned(), n.clone());
            n
        };
        let v0 = out.positions.len() as u32;
        let t0 = out.texcoords.len() as u32;
        // Coordinates and their text.
        let text0 = out.coord_text.len() as u32;
        out.coord_text.extend_from_slice(&d.coord_text);
        out.coord_offsets
            .extend(d.coord_offsets[1..].iter().map(|o| o + text0));
        out.positions.extend_from_slice(&d.positions);
        out.colors.extend_from_slice(&d.colors);
        out.texcoords.extend_from_slice(&d.texcoords);
        // Names.
        let mut intern = |which: usize, n: &str, list: &mut Vec<String>| -> u32 {
            *names_index[which].entry(n.to_owned()).or_insert_with(|| {
                list.push(n.to_owned());
                (list.len() - 1) as u32
            })
        };
        let attr_map: Vec<u32> = d
            .attrs
            .iter()
            .map(|a| {
                let object = claim(
                    0,
                    a.object
                        .map_or(stem.as_str(), |o| d.objects[o as usize].as_str()),
                );
                let a2 = ElementAttrs {
                    object: Some(intern(0, &object, &mut out.objects)),
                    group: a.group.map(|g| {
                        let group = claim(1, &d.groups[g as usize]);
                        intern(1, &group, &mut out.groups)
                    }),
                    material: a.material.map(|m| {
                        let raw = &d.materials[m as usize];
                        let n = renames.get(&(pi, raw.clone())).unwrap_or(raw);
                        intern(2, n, &mut out.materials)
                    }),
                };
                *attr_index.entry(a2).or_insert_with(|| {
                    out.attrs.push(a2);
                    out.attr_file.push(pi as u32);
                    (out.attrs.len() - 1) as u32
                })
            })
            .collect();
        // Elements.
        let add = |dst: &mut obj::Elements, src: &obj::Elements| {
            for (k, el) in src.iter().enumerate() {
                let shifted: Vec<u32> = el.iter().map(|&v| v + v0).collect();
                dst.push_element(&shifted, attr_map[src.attr[k] as usize], src.line[k])
                    .expect("sizes were checked when parsing");
            }
        };
        if any_uvs {
            if d.face_uvs.is_empty() {
                out.face_uvs
                    .resize(out.face_uvs.len() + d.faces.indices.len(), NO_UV);
            } else {
                out.face_uvs.extend(
                    d.face_uvs
                        .iter()
                        .map(|&t| if t == NO_UV { NO_UV } else { t + t0 }),
                );
            }
        }
        add(&mut out.faces, &d.faces);
        add(&mut out.lines, &d.lines);
        add(&mut out.points, &d.points);
        if !d.weights.is_empty() || !out.weights.is_empty() {
            out.weights.resize(v0 as usize, 1.0);
            if d.weights.is_empty() {
                out.weights.resize(v0 as usize + d.positions.len(), 1.0);
            } else {
                out.weights.extend_from_slice(&d.weights);
            }
        }
        out.curves
            .extend(d.curves.iter().map(|c| obj::FreeformCurve {
                control: c.control.iter().map(|&v| v + v0).collect(),
                attr: attr_map[c.attr as usize],
                ..c.clone()
            }));
        out.mtllibs.extend(d.mtllibs.iter().cloned());
        if !header_taken && !d.header_comments.is_empty() && kind(file) == Kind::Obj {
            out.header_comments = d.header_comments.clone();
            header_taken = true;
        }
        let c = &d.counts;
        let o = &mut out.counts;
        o.texcoords += c.texcoords;
        o.normals += c.normals;
        o.param_vertices += c.param_vertices;
        o.weighted_vertices += c.weighted_vertices;
        o.vertices_with_color += c.vertices_with_color;
        o.smoothing_statements += c.smoothing_statements;
        o.freeform_statements += c.freeform_statements;
        o.freeform_surfaces += c.freeform_surfaces;
        o.freeform_curves += c.freeform_curves;
        o.render_statements += c.render_statements;
        o.unknown_statements += c.unknown_statements;
        o.comment_lines += c.comment_lines;
        o.faces_skipped += c.faces_skipped;
        out.diagnostics
            .extend(d.diagnostics.into_iter().map(|x| Diagnostic {
                message: format!("{file}: {}", x.message),
                ..x
            }));
    }
    (out, renames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(path: &str, bytes: &'a [u8]) -> InputFile<'a> {
        InputFile {
            path: path.into(),
            bytes,
            sha256: None,
            modified: None,
        }
    }

    #[test]
    fn one_model_is_unchanged() {
        let src = b"mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl red\nf 1 2 3\n";
        let b = load(
            vec![
                file("x/Model.obj", src),
                file("M.MTL", b"newmtl red\nKd 1 0 0\n"),
                file("notes.txt", b"hi"),
            ],
            "",
            |_, _| {},
        )
        .unwrap();
        assert_eq!((b.name.as_str(), b.stem.as_str()), ("Model.obj", "Model"));
        assert_eq!(b.source_sha256, sha256_hex(src));
        assert_eq!(b.doc.objects.len(), 0, "a single file keeps its own names");
        assert_eq!(b.palette.unwrap()["red"], [1.0, 0.0, 0.0]);
        let roles: Vec<_> = b.files.iter().map(|f| (f.name.as_str(), f.role)).collect();
        assert_eq!(
            roles,
            [
                ("M.MTL", Role::Materials),
                ("Model.obj", Role::Model),
                ("notes.txt", Role::NotUsed)
            ]
        );
    }

    #[test]
    fn models_and_clouds_merge_with_exact_text() {
        let b = load(
            vec![
                file("scan.xyz", b"10.5 0 0 255 0 0\n"),
                file("part.obj", b"v 0 0 0\nv 1 0 0\nv 0 1 -0.000000\nf 1 2 3\n"),
            ],
            "site.zip",
            |_, _| {},
        )
        .unwrap();
        assert_eq!(b.name, "site.zip");
        assert_eq!(b.doc.positions.len(), 4);
        assert_eq!(b.doc.coord_text(2, 2), "-0.000000");
        assert_eq!(b.doc.coord_text(3, 0), "10.5");
        assert_eq!(b.doc.objects, ["part", "scan"]);
        assert_eq!(b.doc.faces.get(0), &[0, 1, 2]);
        assert_eq!(b.doc.points.get(0), &[3]);
        assert!(b.doc.colors[3].is_some());
    }

    #[test]
    fn conflicting_materials_are_renamed_and_missing_files_listed() {
        let a = b"mtllib a.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl paint\nf 1 2 3\n";
        let b2 =
            b"mtllib b.mtl\nmtllib gone.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl paint\nf 1 2 3\n";
        let b = load(
            vec![
                file("a.obj", a),
                file("b.obj", b2),
                file("a.mtl", b"newmtl paint\nKd 1 0 0\n"),
                file("b.mtl", b"newmtl paint\nKd 0 0 1\nmap_Kd wood.jpg\n"),
            ],
            "",
            |_, _| {},
        )
        .unwrap();
        assert_eq!(b.doc.materials, ["paint", "paint (b)"]);
        let pal = b.palette.unwrap();
        assert_eq!(
            (pal["paint"], pal["paint (b)"]),
            ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
        );
        let missing: Vec<_> = b
            .files
            .iter()
            .filter(|f| f.role == Role::Missing)
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(missing, ["gone.mtl", "wood.jpg"]);
        assert!(b
            .doc
            .diagnostics
            .iter()
            .any(|d| d.code == Code::MissingFile));
    }

    #[test]
    fn files_never_share_a_layer_by_accident() {
        let cube = b"o Cube\ng top\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let b = load(
            vec![
                file("a.obj", cube),
                file("b.obj", cube),
                file("scan.xyz", b"0 0 5\n1 1 5\n"),
            ],
            "site.zip",
            |_, _| {},
        )
        .unwrap();
        assert_eq!(b.doc.objects, ["Cube", "Cube (b)", "scan"]);
        assert_eq!(b.doc.groups, ["top", "top (b)"]);
        assert_eq!(b.doc.files, ["a.obj", "b.obj", "scan.xyz"]);
        let layers = |mode| {
            let options = crate::Options {
                layer_mode: mode,
                ..Default::default()
            };
            crate::convert(&b.doc, None, options)
                .layers
                .iter()
                .filter(|l| !l.source.is_empty())
                .map(|l| (l.name.clone(), l.file.clone()))
                .collect::<Vec<_>>()
        };
        let named = |n: &str, f: &str| (n.to_owned(), Some(f.to_owned()));
        assert_eq!(
            layers(crate::LayerMode::Objects),
            [
                named("Cube", "a.obj"),
                named("Cube (b)", "b.obj"),
                named("scan", "scan.xyz")
            ]
        );
        assert_eq!(
            layers(crate::LayerMode::Groups)[..2],
            [named("top", "a.obj"), named("top (b)", "b.obj")]
        );
    }

    #[test]
    fn errors_name_the_file() {
        let e = load(
            vec![file("ok.obj", b"v 0 0 0\n"), file("bad.xyz", b"1 2\n")],
            "",
            |_, _| {},
        )
        .err()
        .unwrap();
        assert_eq!(e.file, "bad.xyz");
    }
}
