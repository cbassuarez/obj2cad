//! A bundle: the files that make up one drawing. Models (`.obj`), point clouds (`.xyz`),
//! material libraries (`.mtl`) and texture images (`.jpg`, `.png`), as they come out of
//! a 3D app or a scanner, loose, in a folder or in a zip.
//!
//! Files are matched by name, ignoring folders and case: each model to the libraries its
//! `mtllib` names, each material to the image its `map_Kd` names. Every model and cloud
//! goes into one document (they share coordinates), so everything downstream (parity
//! hash, writers, preview) works on a bundle exactly as on a single file. Every file is
//! accounted for in the report: used, not needed, or missing.

use crate::coords::{concat, Coords};
use crate::diag::{Code, Diagnostic, Diagnostics, Severity};
use crate::hash::sha256_hex;
use crate::mtl::{self, Library, TextureRef};
use crate::obj::{self, ElementAttrs, ObjDocument, ParseError, NO_UV};
use crate::partial::Partial;
use crate::texture::{self, Image, Texture};
use crate::xyz;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// A bundle part-way through loading (see [`load`]).
pub struct Loading<'a> {
    /// Bytes of the models and clouds read so far, and their total.
    pub done: usize,
    pub total: usize,
    /// The file being read (its name in the drawing), and whether it is a point cloud.
    pub file: &'a str,
    pub cloud: bool,
    /// What has been read of that file so far.
    pub partial: &'a Partial<'a>,
}

/// One file as given: its path (folders are kept for display, ignored for matching).
pub struct InputFile<'a> {
    pub path: String,
    pub content: Content<'a>,
    /// Precomputed SHA-256 (the browser hashes natively); computed when `None`. Empty
    /// when the caller supplies it after loading, with [`Bundle::set_hashes`].
    pub sha256: Option<String>,
    /// Modification time, Unix seconds (whole seconds, so every caller agrees).
    pub modified: Option<f64>,
}

/// A file's bytes: in memory, or read in pieces from somewhere else (a large file the
/// browser holds, outside the engine's memory). A point cloud read in pieces is never
/// in memory whole; anything else is read whole when it is needed.
#[derive(Clone, Copy)]
pub enum Content<'a> {
    Bytes(&'a [u8]),
    Pieces(&'a dyn Pieces),
}

/// A file read in pieces (see [`Content`]).
pub trait Pieces {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Copy the file's bytes `at..at + buf.len()` into `buf`.
    fn read(&self, at: usize, buf: &mut [u8]);
}

/// How much of a file is read at a time.
const PIECE: usize = 4 << 20;

impl Content<'_> {
    pub fn len(&self) -> usize {
        match self {
            Content::Bytes(b) => b.len(),
            Content::Pieces(p) => p.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every byte (copied into memory when read in pieces).
    pub fn whole(&self) -> std::borrow::Cow<'_, [u8]> {
        match self {
            Content::Bytes(b) => std::borrow::Cow::Borrowed(b),
            Content::Pieces(p) => {
                let mut all = vec![0; p.len()];
                for (i, piece) in all.chunks_mut(PIECE).enumerate() {
                    p.read(i * PIECE, piece);
                }
                std::borrow::Cow::Owned(all)
            }
        }
    }

    /// Every piece in order (the whole file at once when it is in memory).
    fn each(&self, mut f: impl FnMut(&[u8])) {
        match self {
            Content::Bytes(b) => f(b),
            Content::Pieces(p) => {
                let mut buf = vec![0; PIECE.min(p.len())];
                let mut at = 0;
                while at < p.len() {
                    let n = PIECE.min(p.len() - at);
                    p.read(at, &mut buf[..n]);
                    f(&buf[..n]);
                    at += n;
                }
            }
        }
    }

    fn sha256(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        self.each(|piece| h.update(piece));
        crate::hash::hex(&h.finalize())
    }
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

/// A material's texture: which image file (by position in the bundle), and how the MTL
/// maps it.
#[derive(Debug, Clone)]
struct MaterialTexture {
    image: usize,
    map: TextureRef,
}

pub struct Bundle {
    pub doc: ObjDocument,
    /// Material colors (`Kd`) by (possibly renamed) material name; `None` when no model
    /// has a material library.
    pub palette: Option<mtl::Palette>,
    /// Each face's color from its material's texture (see [`crate::Materials::sampled`]),
    /// sampled once while loading so the images needn't be kept: decoded, a model's
    /// textures can take gigabytes. Empty when no face has a texture.
    pub face_textures: Vec<Option<[u8; 3]>>,
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
    /// Entries of the models and clouds (for the identity).
    geometry: Vec<usize>,
}

impl Bundle {
    /// Set each input file's SHA-256 (by its position in the list given to [`load`]) and
    /// the identity that follows from them. For callers that hash while the files are
    /// parsed: they load with empty hashes, then set them here.
    pub fn set_hashes(&mut self, sha256: &[String]) {
        for e in &mut self.files {
            if let Some(h) = sha256.get(e.input) {
                e.sha256 = Some(h.clone());
            }
        }
        self.source_sha256 = source_sha256(&self.files, &self.geometry);
    }
}

/// The model's SHA-256; for several models or clouds, a hash of every used file's name and
/// SHA-256 (sorted).
fn source_sha256(entries: &[FileEntry], geometry: &[usize]) -> String {
    if let [g] = geometry {
        return entries[*g].sha256.clone().unwrap_or_default();
    }
    let used: BTreeMap<&str, &str> = entries
        .iter()
        .filter(|e| !matches!(e.role, Role::NotUsed | Role::Missing))
        .map(|e| (e.name.as_str(), e.sha256.as_deref().unwrap_or("")))
        .collect();
    let manifest: String = used.iter().map(|(n, h)| format!("{h}  {n}\n")).collect();
    sha256_hex(manifest.as_bytes())
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
/// zip's or folder's name). `progress` reports parsing: bytes, and what has been read.
pub fn load(
    files: Vec<InputFile<'_>>,
    name: &str,
    mut progress: impl FnMut(&Loading),
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
            bytes: f.content.len() as u64,
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
    let total: usize = geometry.iter().map(|&i| files[i].content.len()).sum();
    let mut done = 0usize;
    let mut parts: Vec<(usize, ObjDocument)> = Vec::new();
    for &i in &geometry {
        let content = files[i].content;
        let cloud = kind(&names[i]) == Kind::Xyz;
        let mut report = |partial: &Partial| {
            progress(&Loading {
                done: done + partial.done,
                total,
                file: &names[i],
                cloud,
                partial,
            })
        };
        let doc = if cloud {
            // Read as it comes: only what is kept of the points is ever in memory.
            let mut reader = xyz::Reader::new(&names[i], content.len());
            content.each(|piece| reader.feed(piece, &mut report));
            reader.finish(&mut report)
        } else {
            obj::parse_with_progress(&content.whole(), &mut report)
        }
        .map_err(|error| BundleError {
            file: names[i].clone(),
            error,
        })?;
        done += content.len();
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
    // Whether each image file could be read, and the last one decoded (each is sampled
    // as soon as it is decoded, then dropped: see `Bundle::face_textures`).
    let mut readable: HashMap<usize, bool> = HashMap::new();
    let mut decoded: Option<(usize, Image)> = None;
    // Per part: each face's texture color (empty when none has one).
    let mut samples: Vec<Vec<Option<[u8; 3]>>> = Vec::new();
    let mut any_library = false;
    // Per part: material name → (Kd, texture).
    let mut definitions: Vec<Definitions> = Vec::new();
    for (pi, (i, doc)) in parts.iter().enumerate() {
        samples.push(Vec::new());
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
        for m in libs {
            any_library = true;
            entries[m].role = Role::Materials;
            let lib: Library = mtl::parse_library(&files[m].content.whole());
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
                if readable.get(&img) == Some(&false) {
                    continue;
                }
                if decoded.as_ref().is_none_or(|(d, _)| *d != img) {
                    decoded = None;
                    match texture::decode(&files[img].content.whole()) {
                        Ok(image) => {
                            entries[img].role = Role::Texture;
                            decoded = Some((img, image));
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
                        }
                    }
                    readable.insert(img, decoded.is_some());
                }
                let Some((_, image)) = &decoded else {
                    continue;
                };
                let s = &mut samples[pi];
                if s.is_empty() {
                    s.resize(doc.faces.len(), None);
                }
                crate::convert::sample_texture(doc, mat, &Texture { image, map: t }, s);
                slot.1 = Some(MaterialTexture {
                    image: img,
                    map: t.clone(),
                });
            }
        }
        definitions.push(defs);
    }

    drop(decoded);

    // ---- one document -------------------------------------------------------------------
    let several = parts.len() > 1;
    let face_textures = if samples.iter().all(Vec::is_empty) {
        Vec::new()
    } else {
        let parts_samples = samples
            .into_iter()
            .zip(&parts)
            .map(|(s, (_, d))| {
                if s.is_empty() {
                    vec![None; d.faces.len()]
                } else {
                    s
                }
            })
            .collect();
        concat(parts_samples, |_, _| {})
    };
    let (doc, renames) = merge(&names, parts, &definitions, several);
    let mut palette = mtl::Palette::new();
    for (pi, defs) in definitions.into_iter().enumerate() {
        for (mat, (kd, _)) in defs {
            let name = renames.get(&(pi, mat.clone())).cloned().unwrap_or(mat);
            if let Some(kd) = kd {
                palette.entry(name.clone()).or_insert(kd);
            }
        }
    }
    let mut doc = doc;
    doc.diagnostics.extend(notes.into_vec());

    // ---- identity -----------------------------------------------------------------------
    for (e, f) in entries.iter_mut().zip(&files) {
        if e.sha256.is_none() && e.role != Role::NotUsed {
            e.sha256 = Some(f.content.sha256());
        }
    }
    let source_sha256 = source_sha256(&entries, &geometry);
    let (display, source_len) = if geometry.len() == 1 {
        let g = geometry[0];
        (names[g].clone(), files[g].content.len() as u64)
    } else {
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
        (display, len)
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
        face_textures,
        files: entries,
        stem: stem_of(&display).to_owned(),
        name: display,
        source_sha256,
        source_len,
        geometry,
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
        let doc = parts
            .into_iter()
            .next()
            .map(|(_, d)| d)
            .unwrap_or_else(|| ObjDocument {
                faces: obj::Elements::empty(),
                lines: obj::Elements::empty(),
                points: obj::Elements::empty(),
                ..Default::default()
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
    let any_uvs = parts.iter().any(|(_, d)| !d.face_uvs.is_empty());
    let any_weights = parts.iter().any(|(_, d)| !d.weights.is_empty());
    let mut names_index: [HashMap<String, u32>; 3] = Default::default();
    // Object and group names already used by an earlier file.
    let mut taken: [std::collections::HashSet<String>; 2] = Default::default();
    let mut header_taken = false;
    let mut attr_index: HashMap<ElementAttrs, u32> = HashMap::new();
    // The large arrays of each file, joined once every file is in (see `coords::concat`):
    // a point cloud's are moved rather than copied, so they never take twice their size.
    let mut positions = Vec::new();
    let mut coords = Vec::new();
    let mut texcoords = Vec::new();
    let mut weights = Vec::new();
    let mut face_uvs = Vec::new();
    let mut elements: [Joined; 3] = Default::default();
    // Per file: where its vertices, texture coordinates and element indices start.
    let mut bases: Vec<(u32, u32, [u32; 3])> = Vec::new();
    let mut attr_maps: Vec<Vec<u32>> = Vec::new();
    let (mut v0, mut t0, mut i0) = (0u32, 0u32, [0u32; 3]);
    for (pi, (fi, mut d)) in parts.into_iter().enumerate() {
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
        bases.push((v0, t0, i0));
        let (n, n_uv) = (d.positions.len(), d.texcoords.len());
        // Coordinates, their text and colors.
        positions.push(std::mem::take(&mut d.positions));
        coords.push(std::mem::take(&mut d.coords));
        out.colors.append(std::mem::take(&mut d.colors));
        texcoords.push(std::mem::take(&mut d.texcoords));
        if any_weights {
            weights.push(if d.weights.is_empty() {
                vec![1.0; n]
            } else {
                std::mem::take(&mut d.weights)
            });
        }
        if any_uvs {
            face_uvs.push(if d.face_uvs.is_empty() {
                vec![NO_UV; d.faces.indices.len()]
            } else {
                std::mem::take(&mut d.face_uvs)
            });
        }
        // Elements.
        for (k, el) in [&mut d.faces, &mut d.lines, &mut d.points]
            .into_iter()
            .enumerate()
        {
            i0[k] += el.indices.len() as u32;
            elements[k].push(std::mem::take(el));
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
        attr_maps.push(attr_map);
        v0 += n as u32;
        t0 += n_uv as u32;
    }

    out.positions = concat(positions, |_, _| {});
    out.coords = Coords::concat(coords).expect("coordinate text within 4 GB");
    out.texcoords = concat(texcoords, |_, _| {});
    out.weights = concat(weights, |_, _| {});
    out.face_uvs = concat(face_uvs, |p, t| {
        if *t != NO_UV {
            *t += bases[p].1;
        }
    });
    for (k, (dst, parts)) in [&mut out.faces, &mut out.lines, &mut out.points]
        .into_iter()
        .zip(elements)
        .enumerate()
    {
        *dst = parts.join(k, &bases, &attr_maps);
    }
    (out, renames)
}

/// One kind of element (faces, lines or points) of every file, to join.
#[derive(Default)]
struct Joined {
    offsets: Vec<Vec<u32>>,
    indices: Vec<Vec<u32>>,
    attr: Vec<Vec<u32>>,
    line: Vec<Vec<u64>>,
}

impl Joined {
    fn push(&mut self, mut e: obj::Elements) {
        // Each element's end, without the leading 0 (moved down in place).
        e.offsets.remove(0);
        self.offsets.push(e.offsets);
        self.indices.push(e.indices);
        self.attr.push(e.attr);
        self.line.push(e.line);
    }

    /// The elements of kind `k` (0 faces, 1 lines, 2 points) of every file, in order,
    /// pointing at the joined vertices and attributes.
    fn join(
        self,
        k: usize,
        bases: &[(u32, u32, [u32; 3])],
        attr_maps: &[Vec<u32>],
    ) -> obj::Elements {
        let total: usize = self.indices.iter().map(Vec::len).sum();
        u32::try_from(total).expect("more than 4 billion element references");
        // Part 0 is the leading 0; file p is part p + 1.
        let mut offsets = vec![vec![0u32]];
        offsets.extend(self.offsets);
        obj::Elements {
            offsets: concat(offsets, |p, o| {
                if p > 0 {
                    *o += bases[p - 1].2[k];
                }
            }),
            indices: concat(self.indices, |p, v| *v += bases[p].0),
            attr: concat(self.attr, |p, a| *a = attr_maps[p][*a as usize]),
            line: concat(self.line, |_, _| {}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(path: &str, bytes: &'a [u8]) -> InputFile<'a> {
        InputFile {
            path: path.into(),
            content: Content::Bytes(bytes),
            sha256: None,
            modified: None,
        }
    }

    #[test]
    fn hashes_given_after_loading_give_the_same_identity() {
        let a = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".as_slice();
        let b = b"v 5 0 0\nv 6 0 0\nv 5 1 0\nf 1 2 3\n".as_slice();
        for set in [
            vec![("a.obj", a)],
            vec![("a.obj", a), ("b.obj", b), ("notes.txt", b"x".as_slice())],
        ] {
            let now = load(
                set.iter().map(|(p, x)| file(p, x)).collect(),
                "site",
                |_| {},
            )
            .unwrap();
            let later_files = set
                .iter()
                .map(|(p, x)| InputFile {
                    sha256: Some(String::new()),
                    ..file(p, x)
                })
                .collect();
            let mut later = load(later_files, "site", |_| {}).unwrap();
            assert_ne!(later.source_sha256, now.source_sha256);
            later.set_hashes(&set.iter().map(|(_, x)| sha256_hex(x)).collect::<Vec<_>>());
            assert_eq!(later.source_sha256, now.source_sha256);
            let hashes = |b: &Bundle| b.files.iter().map(|f| f.sha256.clone()).collect::<Vec<_>>();
            assert_eq!(hashes(&later).len(), hashes(&now).len());
            // Unused files are hashed only when given (the engine doesn't hash them itself).
            for (l, n) in hashes(&later).iter().zip(hashes(&now)) {
                if n.is_some() {
                    assert_eq!(l, &n);
                }
            }
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
            |_| {},
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
            |_| {},
        )
        .unwrap();
        assert_eq!(b.name, "site.zip");
        assert_eq!(b.doc.positions.len(), 4);
        assert_eq!(b.doc.coord_text(2, 2), "-0.000000");
        assert_eq!(b.doc.coord_text(3, 0), "10.5");
        assert_eq!(b.doc.objects, ["part", "scan"]);
        assert_eq!(b.doc.faces.get(0), &[0, 1, 2]);
        assert_eq!(b.doc.points.get(0), &[3]);
        assert!(b.doc.colors.get(3).is_some());
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
            |_| {},
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
            |_| {},
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
            |_| {},
        )
        .err()
        .unwrap();
        assert_eq!(e.file, "bad.xyz");
    }
}
