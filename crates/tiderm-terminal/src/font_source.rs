//! Font byte sources. System fonts stay file-backed so unused pages remain demand paged.

use std::fs::File;
use std::path::Path;

use memmap2::{Mmap, MmapOptions};

pub(super) const JETBRAINS_MONO: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
const SYMBOLS_NERD: &[u8] = include_bytes!("../assets/fonts/SymbolsNerdFontMono-Regular.ttf");

pub(super) enum FontBytes {
    Owned(Vec<u8>),
    Static(&'static [u8]),
    Mapped(Mmap),
}

impl FontBytes {
    pub(super) fn as_slice(&self) -> &[u8] {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Static(bytes) => bytes,
            Self::Mapped(bytes) => bytes,
        }
    }
}

fn map_file(path: impl AsRef<Path>) -> Option<FontBytes> {
    let file = File::open(path).ok()?;
    // SAFETY: Fonts are opened read-only and the application never modifies their files.
    // The map owns its OS handle, so dropping `file` does not invalidate the mapped slice.
    let map = unsafe { MmapOptions::new().map(&file).ok()? };
    Some(FontBytes::Mapped(map))
}

/// A fallback source, loaded only when an earlier face lacks a character.
pub(super) enum FallbackSpec {
    Embedded(&'static [u8]),
    #[cfg_attr(not(windows), allow(dead_code))]
    WindowsFont(&'static str),
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    SystemPath(&'static str),
    #[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
    Family(&'static str),
}

impl FallbackSpec {
    pub(super) fn load(&self) -> Option<FontBytes> {
        match self {
            Self::Embedded(bytes) => Some(FontBytes::Static(bytes)),
            Self::WindowsFont(name) => {
                let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
                map_file(Path::new(&dir).join("Fonts").join(name))
            }
            Self::SystemPath(path) => map_file(path),
            Self::Family(family) => map_file(fontconfig_family_file(family)?),
        }
    }
}

/// Use a real installed font, without fontconfig's substitution from `fc-match`.
pub(super) fn fontconfig_family_file(family: &str) -> Option<String> {
    let out = std::process::Command::new("fc-list")
        .args(["--format", "%{file}\n", family])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let files: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    files
        .iter()
        .find(|f| f.contains("Regular") || f.contains("regular"))
        .or_else(|| files.first())
        .map(|f| f.to_string())
}

/// Ordered fallback chain. A character is resolved against these faces on demand.
pub(super) fn fallback_specs() -> Vec<FallbackSpec> {
    #[cfg(windows)]
    return vec![
        FallbackSpec::Embedded(JETBRAINS_MONO),
        FallbackSpec::WindowsFont("msyh.ttc"),
        FallbackSpec::WindowsFont("seguisym.ttf"),
        FallbackSpec::Embedded(SYMBOLS_NERD),
        FallbackSpec::WindowsFont("seguiemj.ttf"),
    ];
    #[cfg(target_os = "macos")]
    return vec![
        FallbackSpec::Embedded(JETBRAINS_MONO),
        FallbackSpec::SystemPath("/System/Library/Fonts/Apple Symbols.ttf"),
        FallbackSpec::Embedded(SYMBOLS_NERD),
        FallbackSpec::SystemPath("/System/Library/Fonts/Apple Color Emoji.ttc"),
    ];
    #[cfg(not(any(windows, target_os = "macos")))]
    return vec![
        FallbackSpec::Embedded(JETBRAINS_MONO),
        FallbackSpec::Family("Noto Sans Symbols 2"),
        FallbackSpec::Family("Noto Sans Symbols"),
        FallbackSpec::Family("Symbola"),
        FallbackSpec::Embedded(SYMBOLS_NERD),
        FallbackSpec::Family("Noto Emoji"),
        FallbackSpec::Family("Noto Color Emoji"),
        FallbackSpec::Family("DejaVu Sans"),
    ];
}
