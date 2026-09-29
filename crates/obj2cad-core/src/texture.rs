//! Texture images, decoded in pure Rust so the command-line tool and the browser get
//! the same pixels (and so the same drawing) from the same file.

use crate::mtl::TextureRef;

/// An 8-bit RGB image, rows top to bottom.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// Images larger than this are rejected (a 16k × 16k texture is 768 MB of pixels).
pub const MAX_PIXELS: u64 = 64 << 20;

/// Decode a JPEG or PNG.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        decode_jpeg(bytes)
    } else if bytes.starts_with(b"\x89PNG") {
        decode_png(bytes)
    } else {
        Err("not a JPEG or PNG image".into())
    }
}

fn check_size(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("the image is empty".into());
    }
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("the image is too large ({w} × {h})"));
    }
    Ok(())
}

fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    let mut d = jpeg_decoder::Decoder::new(bytes);
    d.read_info().map_err(|e| e.to_string())?;
    let info = d.info().ok_or("no image header")?;
    check_size(info.width.into(), info.height.into())?;
    let px = d.decode().map_err(|e| e.to_string())?;
    let rgb = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => px,
        jpeg_decoder::PixelFormat::L8 => px.iter().flat_map(|&l| [l, l, l]).collect(),
        jpeg_decoder::PixelFormat::L16 => px.chunks_exact(2).flat_map(|c| [c[0]; 3]).collect(),
        jpeg_decoder::PixelFormat::CMYK32 => px
            .chunks_exact(4)
            .flat_map(|c| {
                // Adobe CMYK JPEGs store inverted values.
                let k = u32::from(c[3]);
                [0, 1, 2].map(|i| (u32::from(c[i]) * k / 255) as u8)
            })
            .collect(),
    };
    Ok(Image {
        width: info.width.into(),
        height: info.height.into(),
        rgb,
    })
}

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width, reader.info().height);
    check_size(w, h)?;
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("the image is too large")?
    ];
    let frame = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let px = &buf[..frame.buffer_size()];
    let rgb = match frame.color_type {
        png::ColorType::Rgb => px.to_vec(),
        png::ColorType::Rgba => px
            .chunks_exact(4)
            .flat_map(|c| [c[0], c[1], c[2]])
            .collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&l| [l, l, l]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks_exact(2).flat_map(|c| [c[0]; 3]).collect(),
        png::ColorType::Indexed => return Err("unexpected indexed PNG after expansion".into()),
    };
    Ok(Image {
        width: w,
        height: h,
        rgb,
    })
}

impl Image {
    /// Bilinear sample at texture coordinates (u, v), repeating outside 0..1. OBJ's v
    /// runs bottom to top; image rows run top to bottom.
    pub fn sample(&self, u: f64, v: f64) -> [f64; 3] {
        let (w, h) = (self.width as f64, self.height as f64);
        let x = (u - u.floor()) * w - 0.5;
        let y = (1.0 - (v - v.floor())) * h - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let wrap = |i: f64, n: u32| (i.rem_euclid(f64::from(n))) as usize;
        let px = |xi: f64, yi: f64| {
            let at = (wrap(yi, self.height) * self.width as usize + wrap(xi, self.width)) * 3;
            [0, 1, 2].map(|c| f64::from(self.rgb[at + c]))
        };
        let (a, b, c, d) = (
            px(x0, y0),
            px(x0 + 1.0, y0),
            px(x0, y0 + 1.0),
            px(x0 + 1.0, y0 + 1.0),
        );
        [0, 1, 2].map(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }
}

/// A texture as a material uses it.
pub struct Texture<'a> {
    pub image: &'a Image,
    pub map: &'a TextureRef,
}

impl Texture<'_> {
    /// The color of a face from its corners' texture coordinates: the mean of samples at
    /// the centroid and halfway from the centroid to each corner.
    pub fn face_color(&self, uvs: &[[f32; 2]]) -> [f64; 3] {
        let n = uvs.len() as f64;
        let c = uvs.iter().fold([0.0; 2], |acc, uv| {
            [acc[0] + f64::from(uv[0]) / n, acc[1] + f64::from(uv[1]) / n]
        });
        let tex = |u: f64, v: f64| {
            let m = self.map;
            self.image
                .sample(u * m.scale[0] + m.offset[0], v * m.scale[1] + m.offset[1])
        };
        let mut sum = tex(c[0], c[1]);
        for uv in uvs {
            let s = tex(
                (c[0] + f64::from(uv[0])) / 2.0,
                (c[1] + f64::from(uv[1])) / 2.0,
            );
            for k in 0..3 {
                sum[k] += s[k];
            }
        }
        sum.map(|x| x / (n + 1.0))
    }
}

/// Reduce colors to at most `k` representatives (median cut on the weighted colors).
/// Returns the palette and, for each input color, the index of its representative.
pub fn quantize(colors: &[[u8; 3]], k: usize) -> (Vec<[u8; 3]>, Vec<u16>) {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<[u8; 3], u64> = BTreeMap::new();
    for &c in colors {
        *counts.entry(c).or_default() += 1;
    }
    let distinct: Vec<([u8; 3], u64)> = counts.into_iter().collect();
    let mut boxes: Vec<Vec<([u8; 3], u64)>> = vec![distinct];
    while boxes.len() < k.max(1) {
        // Split the box with the widest channel range (ties: most weight, then first).
        let range = |b: &Vec<([u8; 3], u64)>| {
            (0..3)
                .map(|ch| {
                    let (lo, hi) = b.iter().fold((255u8, 0u8), |(lo, hi), (c, _)| {
                        (lo.min(c[ch]), hi.max(c[ch]))
                    });
                    (hi.saturating_sub(lo), ch)
                })
                .max_by_key(|&(r, ch)| (r, std::cmp::Reverse(ch)))
                .unwrap_or((0, 0))
        };
        let Some((i, (r, ch))) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| (i, range(b)))
            .max_by_key(|&(i, (r, _))| (r, std::cmp::Reverse(i)))
        else {
            break;
        };
        if r == 0 {
            break;
        }
        let mut b = boxes.swap_remove(i);
        b.sort_by_key(|(c, _)| (c[ch], *c));
        let total: u64 = b.iter().map(|x| x.1).sum();
        let (mut acc, mut cut) = (0u64, 1usize);
        for (j, x) in b.iter().enumerate() {
            acc += x.1;
            if acc * 2 >= total {
                cut = (j + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let right = b.split_off(cut);
        boxes.push(b);
        boxes.push(right);
    }
    boxes.sort_by_key(|b| b[0].0);
    let palette: Vec<[u8; 3]> = boxes
        .iter()
        .map(|b| {
            let w: u64 = b.iter().map(|x| x.1).sum();
            [0, 1, 2].map(|ch| {
                ((b.iter().map(|x| u64::from(x.0[ch]) * x.1).sum::<u64>() as f64 / w as f64)
                    .round()) as u8
            })
        })
        .collect();
    let mut index: BTreeMap<[u8; 3], u16> = BTreeMap::new();
    for (i, b) in boxes.iter().enumerate() {
        for (c, _) in b {
            index.insert(*c, i as u16);
        }
    }
    (palette, colors.iter().map(|c| index[c]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_with_v_up_and_repeat() {
        // 2 × 2: top row red, green; bottom row blue, white.
        let img = Image {
            width: 2,
            height: 2,
            rgb: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
        };
        assert_eq!(img.sample(0.25, 0.75), [255.0, 0.0, 0.0]);
        assert_eq!(img.sample(0.25, 0.25), [0.0, 0.0, 255.0]);
        assert_eq!(img.sample(1.25, -0.75), [0.0, 0.0, 255.0]);
    }

    #[test]
    fn quantizes_deterministically() {
        let colors: Vec<[u8; 3]> = (0..200u32)
            .map(|i| [(i % 256) as u8, (i * 7 % 256) as u8, 40])
            .collect();
        let (p, idx) = quantize(&colors, 8);
        assert_eq!(p.len(), 8);
        assert_eq!(quantize(&colors, 8), (p.clone(), idx.clone()));
        let (p, idx) = quantize(&[[1, 2, 3], [1, 2, 3]], 8);
        assert_eq!((p, idx), (vec![[1, 2, 3]], vec![0, 0]));
    }

    #[test]
    fn decodes_png() {
        // 1 × 1 RGB PNG, color (10, 20, 30).
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, 1, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header()
                .unwrap()
                .write_image_data(&[10, 20, 30])
                .unwrap();
        }
        let img = decode(&out).unwrap();
        assert_eq!(img.rgb, vec![10, 20, 30]);
    }
}
