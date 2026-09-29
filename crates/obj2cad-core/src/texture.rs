//! Texture images, decoded in pure Rust so the command-line tool and the browser get
//! the same pixels (and so the same drawing) from the same file.

use crate::color;
use crate::mtl::TextureRef;

/// An 8-bit sRGB image, rows top to bottom.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// Opacity per pixel; empty when the image is opaque.
    pub alpha: Vec<u8>,
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
        alpha: Vec::new(),
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
    let (rgb, alpha): (Vec<u8>, Vec<u8>) = match frame.color_type {
        png::ColorType::Rgb => (px.to_vec(), Vec::new()),
        png::ColorType::Rgba => (
            px.chunks_exact(4)
                .flat_map(|c| [c[0], c[1], c[2]])
                .collect(),
            px.chunks_exact(4).map(|c| c[3]).collect(),
        ),
        png::ColorType::Grayscale => (px.iter().flat_map(|&l| [l, l, l]).collect(), Vec::new()),
        png::ColorType::GrayscaleAlpha => (
            px.chunks_exact(2).flat_map(|c| [c[0]; 3]).collect(),
            px.chunks_exact(2).map(|c| c[1]).collect(),
        ),
        png::ColorType::Indexed => return Err("unexpected indexed PNG after expansion".into()),
    };
    // Fully opaque images need no alpha.
    let alpha = if alpha.iter().all(|&a| a == 255) {
        Vec::new()
    } else {
        alpha
    };
    Ok(Image {
        width: w,
        height: h,
        rgb,
        alpha,
    })
}

impl Image {
    /// Bilinear sample at texture coordinates (u, v) in linear light, premultiplied by
    /// opacity: returns (color × opacity, opacity). Outside 0..1 the image repeats, or
    /// its edge extends when `clamp` is set (`-clamp on`). OBJ's v runs bottom to top;
    /// image rows run top to bottom.
    pub fn sample(&self, u: f64, v: f64, clamp: bool) -> ([f64; 3], f64) {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let (u, v) = if clamp {
            (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
        } else {
            (u - u.floor(), v - v.floor())
        };
        let x = u * w - 0.5;
        let y = (1.0 - v) * h - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let index = |i: f64, n: u32| {
            if clamp {
                i.clamp(0.0, f64::from(n - 1)) as usize
            } else {
                i.rem_euclid(f64::from(n)) as usize
            }
        };
        let texel = |xi: f64, yi: f64| {
            let at = index(yi, self.height) * self.width as usize + index(xi, self.width);
            let a = self.alpha.get(at).map_or(1.0, |&a| f64::from(a) / 255.0);
            let c = [0, 1, 2].map(|k| color::TO_LINEAR[self.rgb[at * 3 + k] as usize] * a);
            (c, a)
        };
        let corners = [
            (texel(x0, y0), (1.0 - fx) * (1.0 - fy)),
            (texel(x0 + 1.0, y0), fx * (1.0 - fy)),
            (texel(x0, y0 + 1.0), (1.0 - fx) * fy),
            (texel(x0 + 1.0, y0 + 1.0), fx * fy),
        ];
        let mut c = [0.0; 3];
        let mut a = 0.0;
        for ((ci, ai), wi) in corners {
            for k in 0..3 {
                c[k] += ci[k] * wi;
            }
            a += ai * wi;
        }
        (c, a)
    }
}

/// A texture as a material uses it.
pub struct Texture<'a> {
    pub image: &'a Image,
    pub map: &'a TextureRef,
}

/// Samples per texel along each side (4 per texel), and per triangle side at most (so at
/// most 4096 samples per triangle).
const STEPS_PER_TEXEL: f64 = 2.0;
const MAX_STEPS: f64 = 64.0;

impl Texture<'_> {
    /// The color a face shows from far enough away to see it as one color: the texture
    /// averaged over the face's area in linear light, about four samples per texel, with
    /// transparent parts left out. `uvs` are the face's corners' texture coordinates.
    pub fn face_color(&self, uvs: &[[f32; 2]]) -> [u8; 3] {
        let m = self.map;
        let uv: Vec<[f64; 2]> = uvs
            .iter()
            .map(|t| {
                [
                    f64::from(t[0]) * m.scale[0] + m.offset[0],
                    f64::from(t[1]) * m.scale[1] + m.offset[1],
                ]
            })
            .collect();
        let texels = f64::from(self.image.width) * f64::from(self.image.height);
        let (mut seen, mut all) = (color::Mix::default(), color::Mix::default());
        let mut add = |p: [f64; 2], w: f64| {
            let (c, a) = self.image.sample(p[0], p[1], m.clamp);
            seen.add_linear(if a > 0.0 { c.map(|x| x / a) } else { c }, w * a);
            all.add_linear(if a > 0.0 { c.map(|x| x / a) } else { c }, w);
        };
        let mut total_area = 0.0;
        // A fan of triangles from the first corner, each split into steps² equal triangles
        // sampled at their centers.
        for k in 1..uv.len().saturating_sub(1) {
            let (a, b, c) = (uv[0], uv[k], uv[k + 1]);
            let e1 = [b[0] - a[0], b[1] - a[1]];
            let e2 = [c[0] - a[0], c[1] - a[1]];
            let area = (e1[0] * e2[1] - e1[1] * e2[0]).abs() / 2.0;
            total_area += area;
            let steps = ((area * texels).sqrt() * STEPS_PER_TEXEL)
                .ceil()
                .clamp(1.0, MAX_STEPS);
            let n = steps as u32;
            let w = area / (steps * steps);
            let at = |s: f64, t: f64| [a[0] + e1[0] * s + e2[0] * t, a[1] + e1[1] * s + e2[1] * t];
            for i in 0..n {
                for j in 0..n - i {
                    let (fi, fj) = (f64::from(i), f64::from(j));
                    add(at((fi + 1.0 / 3.0) / steps, (fj + 1.0 / 3.0) / steps), w);
                    if i + j + 1 < n {
                        add(at((fi + 2.0 / 3.0) / steps, (fj + 2.0 / 3.0) / steps), w);
                    }
                }
            }
        }
        if total_area == 0.0 {
            // No area in texture space (every corner on one point or line): the center.
            let n = uv.len() as f64;
            let c = uv
                .iter()
                .fold([0.0; 2], |s, p| [s[0] + p[0] / n, s[1] + p[1] / n]);
            add(c, 1.0);
        }
        seen.srgb().or_else(|| all.srgb()).unwrap_or([0, 0, 0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, rgb: Vec<u8>, alpha: Vec<u8>) -> Image {
        Image {
            width,
            height,
            rgb,
            alpha,
        }
    }

    fn srgb(c: ([f64; 3], f64)) -> [u8; 3] {
        c.0.map(color::to_srgb)
    }

    #[test]
    fn samples_with_v_up_repeat_and_clamp() {
        // 2 × 2: top row red, green; bottom row blue, white.
        let img = image(
            2,
            2,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            vec![],
        );
        assert_eq!(srgb(img.sample(0.25, 0.75, false)), [255, 0, 0]);
        assert_eq!(srgb(img.sample(0.25, 0.25, false)), [0, 0, 255]);
        assert_eq!(srgb(img.sample(1.25, -0.75, false)), [0, 0, 255]);
        // Clamped, the edge extends instead of wrapping around.
        assert_eq!(srgb(img.sample(-0.5, 0.75, true)), [255, 0, 0]);
        assert_eq!(srgb(img.sample(1.9, 0.75, true)), [0, 255, 0]);
    }

    fn face(img: &Image, uvs: &[[f32; 2]], clamp: bool) -> [u8; 3] {
        let map = TextureRef {
            file: String::new(),
            offset: [0.0; 2],
            scale: [1.0; 2],
            clamp,
        };
        Texture {
            image: img,
            map: &map,
        }
        .face_color(uvs)
    }

    #[test]
    fn a_face_shows_its_area_mixed_in_linear_light() {
        // 64 × 64 stripes, black and white: from afar they reflect half the light.
        let rgb = (0..64 * 64)
            .flat_map(|i| [if i % 2 == 0 { 0 } else { 255 }; 3])
            .collect();
        let img = image(64, 64, rgb, vec![]);
        let quad = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let c = face(&img, &quad, false);
        assert!(c.iter().all(|&x| x.abs_diff(188) <= 2), "{c:?}");
        // A face well inside one texel shows (nearly) that texel.
        let tiny = [
            [0.5 / 64.0, 0.5 / 64.0],
            [0.51 / 64.0, 0.5 / 64.0],
            [0.5 / 64.0, 0.51 / 64.0],
        ];
        assert!(face(&img, &tiny, false)[0] < 16);
    }

    #[test]
    fn transparent_parts_are_left_out() {
        // Left half opaque red, right half transparent (and green underneath).
        let rgb = vec![255, 0, 0, 0, 255, 0];
        let img = image(2, 1, rgb, vec![255, 0]);
        let quad = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let c = face(&img, &quad, true);
        assert_eq!((c[0] > 250, c[1] < 5), (true, true), "{c:?}");
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
        assert_eq!((img.rgb, img.alpha), (vec![10, 20, 30], vec![]));
    }
}
