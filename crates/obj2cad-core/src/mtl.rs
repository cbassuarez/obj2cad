//! Minimal MTL reader: only what maps to CAD (diffuse color per material, and the
//! diffuse texture used to color faces).

use std::collections::HashMap;

/// Material name → diffuse color (`Kd`), components in 0..=1.
pub type Palette = HashMap<String, [f64; 3]>;

/// A material's diffuse texture (`map_Kd`) and the UV transform its options apply.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureRef {
    /// The image file as written in the MTL (may include folders).
    pub file: String,
    /// `-o u v`: added to texture coordinates.
    pub offset: [f64; 2],
    /// `-s u v`: multiplies texture coordinates.
    pub scale: [f64; 2],
    /// `-clamp on`: outside 0..1 the image's edge extends instead of repeating.
    pub clamp: bool,
}

/// Everything obj2cad uses from an MTL file.
#[derive(Debug, Clone, Default)]
pub struct Library {
    pub colors: Palette,
    pub textures: HashMap<String, TextureRef>,
}

/// Parse MTL text into diffuse colors. See [`parse_library`].
pub fn parse(src: &[u8]) -> Palette {
    parse_library(src).colors
}

/// Parse MTL text. Unknown statements are ignored: MTL is presentation data and only
/// `Kd` (entity true color) and `map_Kd` (face colors) have a CAD counterpart.
/// Malformed `Kd` lines are skipped.
pub fn parse_library(src: &[u8]) -> Library {
    let text = String::from_utf8_lossy(src);
    let mut out = Library::default();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        let (kw, rest) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(k, r)| (k, r.trim()));
        match (kw, current.as_ref()) {
            ("newmtl", _) => current = Some(rest.to_owned()),
            ("Kd", Some(name)) => {
                let v: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect();
                if v.len() == 3 && v.iter().all(|c| c.is_finite()) {
                    out.colors.insert(name.clone(), [v[0], v[1], v[2]]);
                }
            }
            ("map_Kd", Some(name)) => {
                if let Some(t) = texture_ref(rest) {
                    out.textures.insert(name.clone(), t);
                }
            }
            _ => {}
        }
    }
    out
}

/// `map_Kd [options] file`. Options take a fixed number of arguments, except `-o`, `-s`
/// and `-t`, which take one to three numbers.
fn texture_ref(rest: &str) -> Option<TextureRef> {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mut t = TextureRef {
        file: String::new(),
        offset: [0.0, 0.0],
        scale: [1.0, 1.0],
        clamp: false,
    };
    let mut i = 0;
    while i < toks.len() && toks[i].starts_with('-') && toks[i].len() > 1 {
        let opt = toks[i];
        i += 1;
        let numbers = |i: &mut usize| {
            let mut v = Vec::new();
            while v.len() < 3 && *i + 1 < toks.len() {
                match toks[*i].parse::<f64>() {
                    Ok(x) if x.is_finite() => v.push(x),
                    _ => break,
                }
                *i += 1;
            }
            v
        };
        match opt {
            "-o" | "-s" => {
                let v = numbers(&mut i);
                let target = if opt == "-o" {
                    &mut t.offset
                } else {
                    &mut t.scale
                };
                for (k, x) in v.iter().take(2).enumerate() {
                    target[k] = *x;
                }
            }
            "-t" => {
                numbers(&mut i);
            }
            "-clamp" => {
                t.clamp = toks.get(i).is_some_and(|x| x.eq_ignore_ascii_case("on"));
                i += 1;
            }
            "-mm" => i += 2,
            _ => i += 1, // -blendu, -blendv, -bm, -boost, -cc, -imfchan, -texres
        }
    }
    t.file = toks.get(i..)?.join(" ");
    (!t.file.is_empty()).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_kd() {
        let p = parse(b"newmtl wood\nKa 0 0 0\nKd 0.5 0.25 1\n\nnewmtl bad\nKd x\n");
        assert_eq!(p["wood"], [0.5, 0.25, 1.0]);
        assert!(!p.contains_key("bad"));
    }

    #[test]
    fn reads_textures_with_options() {
        let l = parse_library(
            b"newmtl a\nmap_Kd tex/Wood Grain.JPG\nnewmtl b\nmap_Kd -s 2 3 1 -o 0.5 0 -bm 1 -clamp on b.png\nnewmtl c\nmap_Kd\n",
        );
        assert_eq!(l.textures["a"].file, "tex/Wood Grain.JPG");
        assert_eq!(l.textures["b"].file, "b.png");
        assert_eq!(l.textures["b"].scale, [2.0, 3.0]);
        assert_eq!(l.textures["b"].offset, [0.5, 0.0]);
        assert!(l.textures["b"].clamp && !l.textures["a"].clamp);
        assert!(!l.textures.contains_key("c"));
    }
}
