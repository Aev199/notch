//! Text recognition with the OCR engine built into Windows 10/11
//! (`Windows.Media.Ocr`): offline, uses the OCR language packs that come with
//! the installed display languages.

use serde::Serialize;

#[derive(Serialize)]
pub struct OcrText {
    pub text: String,
    /// BCP-47 tag of the recogniser that produced the text, e.g. `ru`.
    pub language: String,
}

#[cfg(windows)]
pub fn recognize(width: u32, height: u32, rgba: Vec<u8>) -> Result<OcrText, String> {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    let err = |e: windows::core::Error| format!("распознавание: {}", e.message());
    // WinRT needs an initialised apartment on this (fresh) thread.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    // The user's languages first; then Russian and English if their packs exist.
    let mut engines = Vec::new();
    if let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() {
        engines.push(engine);
    }
    for tag in ["ru", "en-US"] {
        let Ok(lang) = Language::CreateLanguage(&windows::core::HSTRING::from(tag)) else { continue };
        if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
            if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&lang) {
                engines.push(engine);
            }
        }
    }
    if engines.is_empty() {
        return Err("в Windows не установлен ни один язык распознавания текста (Параметры → Время и язык → Язык)".into());
    }

    let max = OcrEngine::MaxImageDimension().map_err(err)?;
    let (w, h, bgra) = notch_core::ocr::prepare(width, height, rgba, max)?;
    let writer = DataWriter::new().map_err(err)?;
    writer.WriteBytes(&bgra).map_err(err)?;
    let buffer = writer.DetachBuffer().map_err(err)?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32).map_err(err)?;

    // Each engine reads its own script best; keep the longest result.
    let mut best: Option<OcrText> = None;
    for engine in engines {
        let Ok(result) = engine.RecognizeAsync(&bitmap).and_then(|op| op.join()) else { continue };
        let lines = result
            .Lines()
            .map(|lines| lines.into_iter().filter_map(|l| l.Text().ok()).map(|t| t.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        let text = notch_core::ocr::join_lines(lines);
        let language = engine.RecognizerLanguage().and_then(|l| l.LanguageTag()).map(|t| t.to_string()).unwrap_or_default();
        let score = text.chars().filter(|c| c.is_alphanumeric()).count();
        if best.as_ref().is_none_or(|b| score > b.text.chars().filter(|c| c.is_alphanumeric()).count()) {
            best = Some(OcrText { text, language });
        }
    }
    match best {
        Some(found) if !found.text.is_empty() => Ok(found),
        _ => Err("текст на скриншоте не найден".into()),
    }
}

#[cfg(not(windows))]
pub fn recognize(_: u32, _: u32, _: Vec<u8>) -> Result<OcrText, String> {
    Err("распознавание текста есть только в Windows".into())
}
