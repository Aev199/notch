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

/// Raster picture files put on the clipboard next to a copied picture
/// (Office, browsers, image viewers). Previews of copied objects never have them.
const PICTURE_NAMES: [&str; 6] = ["png", "jfif", "gif", "image/png", "image/jpeg", "image/gif"];

/// Image programs: whatever they copy is a picture.
const IMAGE_APPS: [&str; 16] = [
    "mspaint.exe",
    "paintdotnet.exe",
    "photoshop.exe",
    "gimp",
    "snippingtool.exe",
    "screenclippinghost.exe",
    "sharex.exe",
    "greenshot.exe",
    "lightshot.exe",
    "i_view64.exe",
    "i_view32.exe",
    "microsoft.photos.exe",
    "photosapp.exe",
    "xnview",
    "faststone",
    "krita.exe",
];

/// CAD programs: their pictures are previews of copied entities, never wanted.
const CAD_APPS: [&str; 10] = [
    "acad.exe",
    "acadlt.exe",
    "nanocad.exe",
    "ncad.exe",
    "bricscad.exe",
    "zwcad.exe",
    "revit.exe",
    "sldworks.exe",
    "inventor.exe",
    "compas",
];

fn matches_app(list: &[&str], exe: &str) -> bool {
    let exe = exe.to_ascii_lowercase();
    list.iter().any(|a| exe == *a || (!a.ends_with(".exe") && exe.starts_with(a)))
}

/// Whether a clipboard image belongs on the shelf. `owner`: file name of the
/// program that filled the clipboard; `formats`: (id, registered name).
pub fn take_image<'a>(owner: Option<&str>, formats: &[(u32, &'a str)]) -> bool {
    if let Some(exe) = owner {
        if matches_app(&CAD_APPS, exe) {
            return false;
        }
        if matches_app(&IMAGE_APPS, exe) {
            return true;
        }
    }
    let has_picture_file =
        formats.iter().any(|(_, name)| PICTURE_NAMES.contains(&name.trim().to_ascii_lowercase().as_str()));
    has_picture_file || !is_object_copy(formats.iter().copied())
}

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

    const EMF: u32 = 14;

    #[test]
    fn pictures_from_office_and_paint_are_taken() {
        // Word / PowerPoint / Excel: a copied picture brings PNG/JFIF along
        // with its OLE and metafile formats.
        let word_picture = [
            (49_600, "Art::GVML ClipFormat"),
            (49_301, "PNG"),
            (49_302, "JFIF"),
            (EMF, ""),
            (CF_BITMAP, ""),
            (49_166, "Embed Source"),
            (49_167, "Object Descriptor"),
        ];
        assert!(take_image(Some("winword.exe"), &word_picture));
        assert!(take_image(Some("POWERPNT.EXE"), &[(49_302, "GIF"), (EMF, ""), (CF_DIB, "")]));
        // Classic Paint: OLE formats, no PNG — taken because it is Paint.
        let paint = [(49_166, "Embed Source"), (49_167, "Object Descriptor"), (CF_BITMAP, ""), (CF_DIB, "")];
        assert!(take_image(Some("mspaint.exe"), &paint));
        assert!(!take_image(Some("someapp.exe"), &paint));
        assert!(take_image(Some("gimp-2.10.exe"), &[(EMF, ""), (CF_DIB, "")]));
        // Screenshots, owner unknown or not.
        assert!(take_image(None, &[(CF_BITMAP, ""), (CF_DIB, ""), (CF_DIBV5, "")]));
        assert!(take_image(Some("explorer.exe"), &[(CF_DIB, "")]));
    }

    #[test]
    fn previews_of_copied_objects_are_not() {
        let excel_cells = [(49_400, "Biff12"), (CF_UNICODETEXT, ""), (EMF, ""), (CF_BITMAP, ""), (49_166, "Embed Source")];
        assert!(!take_image(Some("excel.exe"), &excel_cells));
        // AutoCAD is refused even if some version adds a PNG preview.
        assert!(!take_image(Some("acad.exe"), &[(49_500, "AutoCAD.r25"), (49_301, "PNG"), (CF_BITMAP, "")]));
        assert!(!take_image(Some("nanoCAD.exe"), &[(CF_DIB, "")]));
        // Office shapes without a raster file: a drawing, not a picture.
        assert!(!take_image(Some("winword.exe"), &[(49_600, "Art::GVML ClipFormat"), (EMF, ""), (CF_DIB, "")]));
    }
}
