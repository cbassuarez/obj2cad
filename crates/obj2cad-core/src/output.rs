//! What every CAD writer shares: metadata, dates, GUIDs and the opening view.

use crate::convert::CadModel;
use crate::hash::sha256_hex;

/// Everything about the output that is not geometry.
pub struct Meta<'a> {
    /// Drawing custom properties (AutoCAD's DWGPROPS → Custom). Single-line keys/values.
    pub properties: &'a [(&'a str, &'a str)],
    /// Seed for the drawing's GUIDs (use the source's SHA-256).
    pub fingerprint_seed: &'a str,
    /// Creation date for the drawing, as Unix seconds: the source file's modification
    /// time, so the output stays deterministic for a given file. `None` → 1970-01-01.
    pub created_unix: Option<f64>,
}

impl Meta<'_> {
    /// The creation date as a Julian day, which is how CAD files store dates.
    pub fn julian_date(&self) -> f64 {
        self.created_unix.unwrap_or(0.0) / 86_400.0 + 2_440_587.5
    }

    pub fn fingerprint_guid(&self) -> String {
        guid(&format!("fingerprint:{}", self.fingerprint_seed))
    }

    pub fn version_guid(&self) -> String {
        guid(&format!("version:{}:{}", self.fingerprint_seed, crate::VERSION))
    }
}

/// Deterministic GUID-shaped string derived from a hash.
pub fn guid(seed: &str) -> String {
    let h = sha256_hex(seed.as_bytes()).to_uppercase();
    format!("{{{}-{}-{}-{}-{}}}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Drawings open in AutoCAD's "SE Isometric" direction, like the web preview.
pub const VIEW_DIRECTION: [f64; 3] = [1.0, -1.0, 1.0];

/// The opening view: SE isometric, centered on the model, tall enough to show all of it.
pub struct View {
    pub target: [f64; 3],
    pub height: f64,
}

pub fn fitted_view(model: &CadModel) -> Option<View> {
    let (lo, hi) = model.bounds()?;
    let target = [0, 1, 2].map(|a| lo[a] / 2.0 + hi[a] / 2.0);
    let norm = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|c| c / l)
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let d = norm(VIEW_DIRECTION);
    // Screen up: world Z projected onto the view plane; screen right: perpendicular.
    let up = norm([-d[2] * d[0], -d[2] * d[1], 1.0 - d[2] * d[2]]);
    let right = [up[1] * d[2] - up[2] * d[1], up[2] * d[0] - up[0] * d[2], up[0] * d[1] - up[1] * d[0]];
    let (mut h, mut w) = (0.0f64, 0.0f64);
    for i in 0..8 {
        let c = [0, 1, 2].map(|a| if i >> a & 1 == 1 { hi[a] } else { lo[a] } - target[a]);
        h = h.max(dot(c, up).abs() * 2.0);
        w = w.max(dot(c, right).abs() * 2.0);
    }
    // 1.34: the default viewport's aspect ratio; AutoCAD refits to the window anyway.
    let height = (h.max(w / 1.34) * 1.1).max(f64::MIN_POSITIVE);
    (height.is_finite() && target.iter().all(|c| c.is_finite())).then_some(View { target, height })
}
