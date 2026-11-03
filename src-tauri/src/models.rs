//! Offline translation models (Firefox Translations, MPL-2.0) embedded into
//! the executable as gzip. The webview asks for them over binary IPC and
//! decompresses them itself; see `models/README.md`.

macro_rules! model {
    ($pair:literal, $file:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/models/", $pair, "/", $file))
    };
}

/// Gzip bytes of one model part: `part` is `model`, `lex` or `vocab`.
pub fn get(pair: &str, part: &str) -> Option<&'static [u8]> {
    Some(match (pair, part) {
        ("ruen", "model") => model!("ruen", "model.bin.gz"),
        ("ruen", "lex") => model!("ruen", "lex.bin.gz"),
        ("ruen", "vocab") => model!("ruen", "vocab.spm.gz"),
        ("enru", "model") => model!("enru", "model.bin.gz"),
        ("enru", "lex") => model!("enru", "lex.bin.gz"),
        ("enru", "vocab") => model!("enru", "vocab.spm.gz"),
        _ => return None,
    })
}
