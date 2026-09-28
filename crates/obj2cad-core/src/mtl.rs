//! Minimal MTL reader: only what maps to CAD (diffuse color per material).

use std::collections::HashMap;

/// Material name → diffuse color (`Kd`), components in 0..=1.
pub type Palette = HashMap<String, [f64; 3]>;

/// Parse MTL text. Unknown statements are ignored: MTL is presentation data and only
/// `Kd` has a CAD counterpart (entity true color). Malformed `Kd` lines are skipped.
pub fn parse(src: &[u8]) -> Palette {
    let text = String::from_utf8_lossy(src);
    let mut out = Palette::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("newmtl") {
            if name.starts_with(char::is_whitespace) || name.is_empty() {
                current = Some(name.trim().to_owned());
            }
        } else if let (Some(rest), Some(name)) = (line.strip_prefix("Kd"), current.as_ref()) {
            let v: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            if v.len() == 3 && v.iter().all(|c| c.is_finite()) {
                out.insert(name.clone(), [v[0], v[1], v[2]]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_kd() {
        let p = super::parse(b"newmtl wood\nKa 0 0 0\nKd 0.5 0.25 1\n\nnewmtl bad\nKd x\n");
        assert_eq!(p["wood"], [0.5, 0.25, 1.0]);
        assert!(!p.contains_key("bad"));
    }
}
