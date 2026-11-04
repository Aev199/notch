//! Preparing screenshots for the Windows OCR engine (the engine itself is
//! WinRT and lives in the Tauri shell).

use image::{imageops::FilterType, ImageBuffer, Rgba};

/// Screen text is small (11–14 px at 100 % scaling); the OCR engine reads it
/// much better enlarged. Images whose longest side is below this get 2×.
const UPSCALE_BELOW: u32 = 1600;

/// Scales an RGBA screenshot so that it fits `max_side` (the engine's
/// `MaxImageDimension`) and small screenshots are enlarged, then converts it
/// to BGRA as the engine wants. Returns `(width, height, bgra)`.
pub fn prepare(width: u32, height: u32, rgba: Vec<u8>, max_side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let longest = width.max(height).max(1);
    let max_side = max_side.max(1);
    let mut scale = if longest < UPSCALE_BELOW { 2.0 } else { 1.0 };
    if (longest as f64 * scale) > max_side as f64 {
        scale = max_side as f64 / longest as f64;
    }
    let buffer: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, rgba).ok_or("размер изображения не совпадает с данными")?;
    let buffer = if (scale - 1.0).abs() > f64::EPSILON {
        let w = ((width as f64 * scale).round() as u32).clamp(1, max_side);
        let h = ((height as f64 * scale).round() as u32).clamp(1, max_side);
        image::imageops::resize(&buffer, w, h, FilterType::CatmullRom)
    } else {
        buffer
    };
    let (w, h) = buffer.dimensions();
    let mut bgra = buffer.into_raw();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Ok((w, h, bgra))
}

/// Joins recognised lines; drops empty ones and stray spaces.
pub fn join_lines<I: IntoIterator<Item = String>>(lines: I) -> String {
    lines.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_images_are_doubled_and_swapped_to_bgra() {
        let rgba = [10u8, 20, 30, 255].repeat(4 * 2);
        let (w, h, bgra) = prepare(4, 2, rgba, 10_000).unwrap();
        assert_eq!((w, h), (8, 4));
        assert_eq!(&bgra[..4], &[30, 20, 10, 255]);
        assert_eq!(bgra.len(), 8 * 4 * 4);
    }

    #[test]
    fn big_images_fit_the_engine_limit() {
        let (w, h, _) = prepare(4000, 1000, vec![0; 4000 * 1000 * 4], 2600).unwrap();
        assert_eq!((w, h), (2600, 650));
        let (w, h, _) = prepare(2000, 1000, vec![0; 2000 * 1000 * 4], 10_000).unwrap();
        assert_eq!((w, h), (2000, 1000));
        // 2× would overflow the limit: capped instead.
        let (w, _, _) = prepare(1500, 100, vec![0; 1500 * 100 * 4], 2600).unwrap();
        assert_eq!(w, 2600);
    }

    #[test]
    fn lines_are_cleaned() {
        assert_eq!(join_lines(["  Привет ".into(), "".into(), "world".into()]), "Привет\nworld");
        assert!(prepare(2, 2, vec![0; 3], 100).is_err());
    }
}
