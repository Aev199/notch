//! Which clipboard contents count as a screenshot for the shelf.
//!
//! Programs that copy their own objects (AutoCAD entities, Excel cells,
//! Word/Visio drawings…) also put a rendered picture on the clipboard. That
//! picture is not something the user wants on the shelf. Such copies always
//! bring document formats along (OLE embedding, metafiles, spreadsheet or
//! rich-text data, the program's private formats), while a screenshot or a
//! browser's "Copy image" carries only image formats (and maybe HTML).

/// Standard clipboard format ids that mean "a document object was copied".
const OBJECT_IDS: [u32; 4] = [
    3,  // CF_METAFILEPICT
    4,  // CF_SYLK
    5,  // CF_DIF
    14, // CF_ENHMETAFILE
];

/// Registered format names (case-insensitive prefixes) of the same kind.
const OBJECT_NAMES: [&str; 16] = [
    "embed source",
    "embedded object",
    "object descriptor",
    "link source",
    "objectlink",
    "ownerlink",
    "native",
    "biff",
    "xml spreadsheet",
    "rich text format",
    "csv",
    "art::",
    "autocad",
    "acad",
    "visio",
    "hyperlink",
];

/// `formats`: (id, registered name or "") of everything on the clipboard.
pub fn is_object_copy<'a>(formats: impl IntoIterator<Item = (u32, &'a str)>) -> bool {
    formats.into_iter().any(|(id, name)| {
        if OBJECT_IDS.contains(&id) {
            return true;
        }
        let name = name.trim().to_ascii_lowercase();
        !name.is_empty() && OBJECT_NAMES.iter().any(|p| name.starts_with(p))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CF_BITMAP: u32 = 2;
    const CF_DIB: u32 = 8;
    const CF_DIBV5: u32 = 17;
    const CF_UNICODETEXT: u32 = 13;

    #[test]
    fn screenshots_and_browser_images_go_to_the_shelf() {
        // Win+Shift+S / PrtScr
        assert!(!is_object_copy([(CF_BITMAP, ""), (CF_DIB, ""), (CF_DIBV5, "")]));
        assert!(!is_object_copy([(CF_DIB, ""), (49_300, "PNG"), (CF_DIBV5, "")]));
        // Chrome / Edge "Copy image"
        assert!(!is_object_copy([(49_350, "HTML Format"), (CF_DIB, ""), (49_300, "PNG")]));
    }

    #[test]
    fn office_and_cad_copies_do_not() {
        // Excel cell
        let excel = [
            (49_161, "DataObject"),
            (49_400, "Biff12"),
            (49_401, "XML Spreadsheet"),
            (49_350, "HTML Format"),
            (CF_UNICODETEXT, ""),
            (49_309, "Rich Text Format"),
            (14, ""),
            (CF_BITMAP, ""),
            (49_166, "Embed Source"),
        ];
        assert!(is_object_copy(excel));
        // AutoCAD entities
        assert!(is_object_copy([(49_500, "AutoCAD.r25"), (49_166, "Embed Source"), (CF_BITMAP, "")]));
        assert!(is_object_copy([(49_501, "AcadDocument"), (CF_DIB, "")]));
        // Office drawing (shapes)
        assert!(is_object_copy([(49_600, "Art::GVML ClipFormat"), (CF_DIB, "")]));
        // A bare metafile is a vector drawing from some program too.
        assert!(is_object_copy([(14, ""), (CF_BITMAP, "")]));
    }
}
