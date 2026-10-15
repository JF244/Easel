//! Catches images that come back blank (one flat colour) so Easel can say why
//! instead of saving a black square to the gallery.
//!
//! The usual causes, in the order they turn up:
//! - Sampling settings meant for another model family: Flux at CFG 6 burns to
//!   black, SD/SDXL at 4 steps and CFG 1 stays a flat smear.
//! - The VAE decoding at half precision and overflowing (SDXL's VAE, AMD
//!   cards). ComfyUI saves the all-black result without reporting an error.

use image::GenericImageView;

/// Standard deviation of brightness (0-255) below which an image counts as blank.
/// Real images, even foggy ones, sit well above 8; flat fills sit near 0.
const FLAT_LIMIT: f64 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flat {
  Black,
  White,
  Grey,
}

/// `Some(..)` when the image is one flat colour. Files Easel can't decode
/// (video, unusual formats) are never called blank.
pub fn check(bytes: &[u8]) -> Option<Flat> {
  let img = image::load_from_memory(bytes).ok()?;
  let (w, h) = img.dimensions();
  if w == 0 || h == 0 {
    return None;
  }
  // Sample a grid instead of every pixel; 64x64 points is plenty for "flat".
  let step_x = (w / 64).max(1);
  let step_y = (h / 64).max(1);
  let (mut sum, mut sum_sq, mut n) = (0.0f64, 0.0f64, 0.0f64);
  let mut y = 0;
  while y < h {
    let mut x = 0;
    while x < w {
      let p = img.get_pixel(x, y).0;
      let luma = 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
      sum += luma;
      sum_sq += luma * luma;
      n += 1.0;
      x += step_x;
    }
    y += step_y;
  }
  let mean = sum / n;
  let spread = (sum_sq / n - mean * mean).max(0.0).sqrt();
  if spread >= FLAT_LIMIT {
    return None;
  }
  Some(if mean < 24.0 {
    Flat::Black
  } else if mean > 231.0 {
    Flat::White
  } else {
    Flat::Grey
  })
}

pub fn describe(flat: Flat) -> &'static str {
  match flat {
    Flat::Black => "all black",
    Flat::White => "all white",
    Flat::Grey => "one flat colour",
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use image::{ImageBuffer, ImageFormat, Rgb};
  use std::io::Cursor;

  #[test]
  fn flat_images_are_blank() {
    assert_eq!(check(&png(|_, _| [0, 0, 0])), Some(Flat::Black));
    assert_eq!(check(&png(|_, _| [255, 255, 255])), Some(Flat::White));
    assert_eq!(check(&png(|_, _| [120, 118, 121])), Some(Flat::Grey));
  }

  #[test]
  fn a_gradient_is_not_blank() {
    assert_eq!(check(&png(|x, y| [(x * 2) as u8, (y * 2) as u8, 90])), None);
  }

  #[test]
  fn a_dark_photo_with_detail_is_not_blank() {
    // A night scene: mostly dark, with a few lit windows.
    assert_eq!(check(&png(|x, y| if (x / 16 + y / 16) % 5 == 0 { [200, 180, 90] } else { [8, 8, 12] })), None);
  }

  #[test]
  fn files_it_cant_read_are_left_alone() {
    assert_eq!(check(b"not an image"), None);
  }

  fn png(f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let img = ImageBuffer::from_fn(128, 128, |x, y| Rgb(f(x, y)));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
  }
}
