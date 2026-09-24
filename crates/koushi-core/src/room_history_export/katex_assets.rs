//! KaTeX files copied into every exported history folder (`assets/katex/`).

/// One file written below the export folder's `assets/katex/`.
pub(crate) struct AssetFile {
    pub(crate) path: &'static str,
    pub(crate) bytes: &'static [u8],
}

macro_rules! asset {
    ($path:literal) => {
        AssetFile {
            path: $path,
            bytes: include_bytes!(concat!("../../assets/katex/", $path)),
        }
    };
}

static KATEX_ASSETS: &[AssetFile] = &[
    asset!("katex.min.js"),
    asset!("katex.min.css"),
    asset!("fonts/KaTeX_AMS-Regular.woff2"),
    asset!("fonts/KaTeX_Caligraphic-Bold.woff2"),
    asset!("fonts/KaTeX_Caligraphic-Regular.woff2"),
    asset!("fonts/KaTeX_Fraktur-Bold.woff2"),
    asset!("fonts/KaTeX_Fraktur-Regular.woff2"),
    asset!("fonts/KaTeX_Main-Bold.woff2"),
    asset!("fonts/KaTeX_Main-BoldItalic.woff2"),
    asset!("fonts/KaTeX_Main-Italic.woff2"),
    asset!("fonts/KaTeX_Main-Regular.woff2"),
    asset!("fonts/KaTeX_Math-BoldItalic.woff2"),
    asset!("fonts/KaTeX_Math-Italic.woff2"),
    asset!("fonts/KaTeX_SansSerif-Bold.woff2"),
    asset!("fonts/KaTeX_SansSerif-Italic.woff2"),
    asset!("fonts/KaTeX_SansSerif-Regular.woff2"),
    asset!("fonts/KaTeX_Script-Regular.woff2"),
    asset!("fonts/KaTeX_Size1-Regular.woff2"),
    asset!("fonts/KaTeX_Size2-Regular.woff2"),
    asset!("fonts/KaTeX_Size3-Regular.woff2"),
    asset!("fonts/KaTeX_Size4-Regular.woff2"),
    asset!("fonts/KaTeX_Typewriter-Regular.woff2"),
];

/// The vendored KaTeX distribution (see `THIRD_PARTY_NOTICES.md`).
pub(crate) fn katex_assets() -> &'static [AssetFile] {
    KATEX_ASSETS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_vendored_file_is_embedded_and_non_empty() {
        let assets = katex_assets();
        assert!(assets.iter().all(|asset| !asset.bytes.is_empty()));
        assert!(assets.iter().any(|asset| asset.path == "katex.min.js"));
        assert!(assets.iter().any(|asset| asset.path == "katex.min.css"));
        let fonts = assets
            .iter()
            .filter(|asset| asset.path.starts_with("fonts/") && asset.path.ends_with(".woff2"))
            .count();
        assert_eq!(fonts, 20);
    }
}
