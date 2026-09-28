use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational: something was carried over in a non-obvious way.
    Info,
    /// Something in the source is not represented in the output.
    Warning,
}

/// Stable identifiers so the UI and reports can group and explain diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    /// `vt` data is not representable in DXF/DWG.
    TexcoordsDropped,
    /// `vn` data is not representable in DXF/DWG.
    NormalsDropped,
    /// `vp` parameter-space vertices are not used by any supported element.
    ParamVerticesDropped,
    /// Homogeneous `w` component present; DXF has no place for it.
    VertexWeightDropped,
    /// Face with fewer than three vertex references.
    FaceTooSmall,
    /// Face references the same vertex more than once.
    FaceRepeatedVertex,
    /// Free-form geometry statement (cstype/curv/surf/...), not yet converted.
    FreeformNotConverted,
    /// Statement keyword not part of the OBJ spec.
    UnknownStatement,
    /// Render-only statement (usemap, lod, bevel, ...) with no CAD meaning.
    RenderAttributeIgnored,
    /// Smoothing groups are recorded but have no DXF equivalent.
    SmoothingGroupsIgnored,
    /// A vertex line had an unusual number of components.
    UnusualVertexArity,
    /// Some but not all vertices carry colors.
    PartialVertexColors,
    /// Text was not valid UTF-8 and was decoded lossily.
    NonUtf8Text,
    /// Element referenced more than one group; only the first names its layer.
    MultipleGroups,
    /// Per-vertex colors are not written (DXF entities carry one color).
    VertexColorsDropped,
    /// Materials referenced but no MTL was supplied.
    MaterialColorsUnavailable,
    /// An object/group name had to be changed to be a valid, unique layer name.
    LayerRenamed,
    /// A mesh was split so each part stays within AutoCAD's default face limit.
    MeshSplit,
    /// Vertices not used by any element are not written.
    UnreferencedVertices,
    /// A vertex shared by faces in different entities is written once per entity.
    SharedVerticesRepeated,
    /// Vertices no element uses were written as POINT entities (point cloud, or on request).
    LooseVerticesAsPoints,
    /// Layers were left out on request.
    LayersExcluded,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: Code,
    /// 1-based line of the first occurrence.
    pub line: u64,
    /// How many times this code fired in total.
    pub count: u64,
    pub message: String,
}

/// Collects diagnostics, folding repeats of the same code into one entry with a count.
#[derive(Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn push(
        &mut self,
        severity: Severity,
        code: Code,
        line: u64,
        message: impl FnOnce() -> String,
    ) {
        if let Some(d) = self.items.iter_mut().find(|d| d.code == code) {
            d.count += 1;
            return;
        }
        self.items.push(Diagnostic {
            severity,
            code,
            line,
            count: 1,
            message: message(),
        });
    }

    pub fn into_vec(mut self) -> Vec<Diagnostic> {
        self.items
            .sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
        self.items
    }
}
