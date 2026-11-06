//! Unsigned source media. Every asset is generated procedurally so the repository has no
//! third-party content and the pixels match the story each manifest tells.

use std::{path::Path, process::Command};

use anyhow::{bail, Result};
use image::{Rgb, RgbImage};

pub const W: u32 = 640;
pub const H: u32 = 480;

/// Small deterministic PRNG so regenerated files are byte-stable apart from signatures.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn clamp(v: f32) -> u8 {
    v.clamp(0.0, 255.0) as u8
}

/// Smooth, saturated plasma: stands in for a text-to-image model output.
pub fn dream(seed: f32) -> RgbImage {
    RgbImage::from_fn(W, H, |x, y| {
        let (u, v) = (x as f32 / 60.0, y as f32 / 60.0);
        let a = (u + seed).sin() + (v * 1.3 - seed).cos() + ((u + v) * 0.7).sin();
        let b = ((u * u + v * v).sqrt() * 0.9 - seed * 2.0).sin();
        Rgb([
            clamp(128.0 + 120.0 * (a * 1.1).sin()),
            clamp(128.0 + 120.0 * (a + b * 2.0).cos()),
            clamp(128.0 + 120.0 * (b * 3.0 + seed).sin()),
        ])
    })
}

/// Sky, horizon and sensor noise: stands in for a camera capture.
pub fn photo(seed: u64) -> RgbImage {
    let mut rng = XorShift(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let horizon = H as f32 * 0.62;
    RgbImage::from_fn(W, H, |x, y| {
        let n = (rng.next() - 0.5) * 22.0;
        let yf = y as f32;
        let hill = horizon - 30.0 * ((x as f32) / 90.0).sin();
        if yf < hill {
            let t = yf / hill;
            Rgb([clamp(90.0 + 80.0 * t + n), clamp(140.0 + 60.0 * t + n), clamp(220.0 - 20.0 * t + n)])
        } else {
            let t = (yf - hill) / (H as f32 - hill);
            Rgb([clamp(70.0 - 30.0 * t + n), clamp(110.0 - 40.0 * t + n), clamp(50.0 - 20.0 * t + n)])
        }
    })
}

/// Flat shapes on a paper background: stands in for a hand-made digital illustration.
pub fn illustration() -> RgbImage {
    RgbImage::from_fn(W, H, |x, y| {
        let (xf, yf) = (x as f32, y as f32);
        if (xf - 220.0).powi(2) + (yf - 200.0).powi(2) < 110.0f32.powi(2) {
            Rgb([230, 120, 60])
        } else if (360..560).contains(&x) && (240..420).contains(&y) {
            Rgb([60, 110, 200])
        } else if yf > 420.0 - (xf - 320.0).abs() * 0.3 && y > 380 {
            Rgb([40, 40, 40])
        } else {
            Rgb([245, 238, 220])
        }
    })
}

/// Mandelbrot set: media made purely by a (non-trained) algorithm.
pub fn fractal() -> RgbImage {
    RgbImage::from_fn(W, H, |x, y| {
        let c = (x as f64 / W as f64 * 3.0 - 2.2, y as f64 / H as f64 * 2.4 - 1.2);
        let (mut zr, mut zi, mut i) = (0.0f64, 0.0f64, 0u32);
        while zr * zr + zi * zi < 4.0 && i < 80 {
            (zr, zi) = (zr * zr - zi * zi + c.0, 2.0 * zr * zi + c.1);
            i += 1;
        }
        let t = i as f32 / 80.0;
        Rgb([clamp(255.0 * t.sqrt()), clamp(180.0 * t), clamp(90.0 + 160.0 * (1.0 - t))])
    })
}

/// Window chrome, a sidebar and text-like bars: stands in for a screenshot.
pub fn screen() -> RgbImage {
    RgbImage::from_fn(W, H, |x, y| {
        if y < 28 {
            Rgb([210, 210, 215])
        } else if x < 150 {
            Rgb([40, 44, 52])
        } else if (y - 28) % 22 < 9 && x < 150 + ((y * 37) % 380) {
            Rgb([90, 90, 100])
        } else {
            Rgb([250, 250, 252])
        }
    })
}

/// Centre crop to 75%, the operation the crop fixtures record.
pub fn crop(img: &RgbImage) -> RgbImage {
    let (w, h) = (img.width() * 3 / 4, img.height() * 3 / 4);
    image::imageops::crop_imm(img, (img.width() - w) / 2, (img.height() - h) / 2, w, h).to_image()
}

/// Brightness and contrast change, the operation the colour-adjustment fixtures record.
pub fn adjust(img: &RgbImage) -> RgbImage {
    image::imageops::contrast(&image::imageops::brighten(img, 12), 15.0)
}

/// Copies `patch` into `base` at (`x0`, `y0`), blending the edge so the seam is soft.
pub fn paste(base: &mut RgbImage, patch: &RgbImage, x0: u32, y0: u32, w: u32, h: u32) {
    for y in 0..h {
        for x in 0..w {
            let edge = (x.min(w - 1 - x).min(y).min(h - 1 - y)) as f32 / 12.0;
            let a = edge.min(1.0);
            let p = patch.get_pixel(x % patch.width(), y % patch.height());
            let b = base.get_pixel_mut(x0 + x, y0 + y);
            for c in 0..3 {
                b[c] = clamp(p[c] as f32 * a + b[c] as f32 * (1.0 - a));
            }
        }
    }
}

/// 16-bit mono PCM WAV. `ai` gives a clean synthetic chirp; otherwise a noisy "field" tone.
pub fn write_wav(path: &Path, ai: bool) -> Result<()> {
    const RATE: u32 = 16_000;
    let mut rng = XorShift(7);
    let samples: Vec<i16> = (0..RATE * 2)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let v = if ai {
                (std::f32::consts::TAU * (220.0 + 180.0 * t) * t).sin() * 0.5
            } else {
                (std::f32::consts::TAU * 440.0 * t).sin() * 0.2 + (rng.next() - 0.5) * 0.3
            };
            (v * i16::MAX as f32) as i16
        })
        .collect();
    let data_len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Two-second synthetic MP4 made with ffmpeg's test sources, with an AAC tone track if `audio`.
pub fn write_mp4(path: &Path, audio: bool) -> Result<()> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-loglevel", "error", "-y", "-f", "lavfi", "-i", "mandelbrot=size=320x240:rate=24"]);
    if audio {
        cmd.args(["-f", "lavfi", "-i", "sine=frequency=330:sample_rate=48000", "-c:a", "aac", "-shortest"]);
    }
    let status = cmd
        .args(["-t", "2", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-movflags", "+faststart"])
        .arg(path)
        .status()?;
    if !status.success() {
        bail!("ffmpeg failed to create {}", path.display());
    }
    Ok(())
}
