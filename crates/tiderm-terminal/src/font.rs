//! Shared glyph rasterization via swash. Loads a monospace **fallback chain** — the
//! user's selected font (primary) plus a sequence of fallbacks — derives integer cell
//! metrics from the *primary* font, and rasterizes coverage (R8 alpha) masks on demand
//! with a cache. Both the software and GPU renderers consume `CachedGlyph`s from here (the
//! GPU path packs them into an atlas; the software path blends them directly).
//!
//! ## Why a chain
//! A single font rarely covers everything a TUI draws. When a font lacks a glyph,
//! `charmap().map(ch)` returns 0 and swash rasterizes `.notdef` → the `□` tofu box. So,
//! per character, we walk a chain and rasterize from the **first** font that actually maps
//! it: primary → bundled JetBrains Mono (broad coverage) → the platform's symbol + emoji
//! fonts (Segoe on Windows, Apple Symbols/Emoji on macOS, Noto/Symbola via fontconfig on
//! Linux — see [`fallback_specs`]; emoji are monochrome here, colour is a follow-up) →
//! last-resort the primary's `.notdef` so truly-unknown codepoints still draw *something*.
//!
//! Cell metrics stay driven by the primary font so the grid stays monospace; an oversized
//! fallback glyph (e.g. an emoji) is uniformly shrunk by a per-face `fit` factor so it
//! snaps into the cell box instead of resizing the row.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform};
use swash::{FontRef, GlyphId};

#[path = "font_source.rs"]
mod font_source;
use font_source::{fallback_specs, FallbackSpec, FontBytes};

/// Max entries in the glyph rasterization cache before LRU eviction. Each entry owns a
/// `mask` `Vec<u8>` (~100–400 B), so this bounds worst-case RAM to a few MB even for
/// CJK-heavy terminals that would otherwise grow the `HashMap` without limit.
const GLYPH_CACHE_CAP: usize = 8192;
/// Max entries in the `resolve(ch)` fallback-chain memo before LRU eviction.
const RESOLVE_CACHE_CAP: usize = 4096;

#[derive(Clone)]
pub struct CachedGlyph {
    pub mask: Vec<u8>, // coverage, row-major, w*h bytes (Content::Mask). Empty if blank.
    pub w: u32,
    pub h: u32,
    pub left: i32, // bearing from pen origin
    pub top: i32,  // bearing above baseline
}

/// Glyph cache / atlas key. `font_id` is the index into the fallback chain that the glyph
/// was resolved from — it MUST be part of the key so a gid from one font can't collide with
/// the same gid from another (different fonts number their glyphs independently).
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct GlyphKey {
    pub font_id: u16,
    pub gid: u16,
    pub bold: bool,
    pub italic: bool,
}

/// One font in the fallback chain: its raw bytes plus a uniform `fit` scale applied when
/// rasterizing. `fit == 1.0` for the primary (so its output is byte-identical to the
/// pre-fallback renderer) and `<= 1.0` for a fallback whose natural line-height exceeds the
/// primary cell, so its glyphs shrink into the cell box and never change the row height.
struct FontFace {
    data: FontBytes,
    index: usize,
    fit: f32,
}

struct FontSlot {
    source: Option<FallbackSpec>,
    face: Option<FontFace>,
    missing: bool,
}

impl FontSlot {
    fn get(&mut self, px: f32, cell_h: u32) -> Option<&FontFace> {
        if self.face.is_none() && !self.missing {
            self.face = self.source.as_ref().and_then(|source| {
                let data = source.load()?;
                let fit = compute_fit(data.as_slice(), px, cell_h)?;
                Some(FontFace {
                    data,
                    index: 0,
                    fit,
                })
            });
            self.missing = self.face.is_none();
        }
        self.face.as_ref()
    }
}

impl FontFace {
    #[inline]
    fn font(&self) -> FontRef<'_> {
        // Cheap: parses only the table directory. Re-derived per use so `Font` needn't hold
        // a self-referential `FontRef` borrowing `data`.
        FontRef::from_index(self.data.as_slice(), self.index).unwrap()
    }
}

/// Monotonic source of per-`Font` generation ids. Every `Font` constructed gets a fresh,
/// unique id so consumers that cache by glyph (e.g. the GPU atlas) can detect a font/size
/// reload — the new `Font` carries a different generation — and invalidate stale entries.
static FONT_GEN: AtomicU64 = AtomicU64::new(1);

pub struct Font {
    /// The fallback chain. `faces[0]` is the primary (selected) font; `1..` are fallbacks.
    faces: Vec<FontSlot>,
    scale: ScaleContext,
    cache: HashMap<GlyphKey, CachedGlyph>,
    /// Insertion order for LRU eviction of `cache` (oldest entry popped past `GLYPH_CACHE_CAP`).
    cache_order: VecDeque<GlyphKey>,
    /// Memoized `resolve(ch)` results so the fallback chain (each face parses its table
    /// directory + charmap lookup) is walked at most once per distinct char, not per char
    /// per frame.
    resolve_cache: HashMap<char, (u16, u16)>,
    /// Insertion order for LRU eviction of `resolve_cache` (oldest entry popped past
    /// `RESOLVE_CACHE_CAP`).
    resolve_order: VecDeque<char>,
    /// Unique identity of this `Font` instance — see [`FONT_GEN`]. Bumps on every load.
    generation: u64,
    px: f32,
    /// Integer cell metrics in physical px — driven entirely by the primary font.
    pub cell_w: u32,
    pub cell_h: u32,
    pub ascent: i32, // baseline offset from cell top
}

impl Font {
    pub fn from_path(path: &str, px: f32) -> anyhow::Result<Self> {
        Self::from_path_index(path, 0, px)
    }

    /// Load the selected face of a system font collection (TTC/OTC).
    pub fn from_path_index(path: &str, index: u32, px: f32) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        Self::from_data(data, index as usize, px)
    }

    /// Build a `Font` from in-memory TTF bytes (no filesystem access). Lets callers that
    /// already hold font bytes — e.g. the memory-budget harness feeding the bundled
    /// JetBrains Mono — instantiate a `Font` headlessly, and avoids a disk round-trip.
    pub fn from_bytes(data: &[u8], px: f32) -> anyhow::Result<Self> {
        Self::from_data(data.to_vec(), 0, px)
    }

    /// Shared constructor: derive integer cell metrics from the primary font, then build
    /// the fallback chain. `data` is owned so it can be moved into `faces[0]`.
    fn from_data(data: Vec<u8>, index: usize, px: f32) -> anyhow::Result<Self> {
        let font = FontRef::from_index(&data, index)
            .ok_or_else(|| anyhow::anyhow!("not a valid font face"))?;

        let m = font.metrics(&[]).scale(px);
        let cell_h = (m.ascent + m.descent + m.leading).round().max(1.0) as u32;
        let ascent = m.ascent.round() as i32;
        // Monospace advance: measure a representative glyph.
        let gm = font.glyph_metrics(&[]).scale(px);
        let gid = font.charmap().map('M');
        let adv = gm.advance_width(gid);
        let cell_w = if adv > 0.5 {
            adv.round().max(1.0) as u32
        } else {
            (px * 0.6).round() as u32
        };
        let _ = font; // release the borrow of `data` before moving it into `FontFace`

        // Primary first (fit forced to 1.0 → identical output to the single-font path),
        // then the fallback chain. Each fallback is appended only if it loads.
        let mut faces = vec![FontSlot {
            source: None,
            face: Some(FontFace {
                data: FontBytes::Owned(data),
                index,
                fit: 1.0,
            }),
            missing: false,
        }];
        faces.extend(fallback_specs().into_iter().map(|source| FontSlot {
            source: Some(source),
            face: None,
            missing: false,
        }));

        Ok(Font {
            faces,
            scale: ScaleContext::new(),
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            resolve_cache: HashMap::new(),
            resolve_order: VecDeque::new(),
            generation: FONT_GEN.fetch_add(1, Ordering::Relaxed),
            px,
            cell_w,
            cell_h,
            ascent,
        })
    }

    /// This `Font`'s unique generation id. It changes whenever a new `Font` is loaded (a
    /// font/size reload constructs a fresh one), so a renderer that caches glyphs by key can
    /// compare against it and drop stale entries on reload. See [`crate::render::GpuRenderer`].
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Walk the fallback chain and return `(font_id, gid)` for the **first** font that maps
    /// `ch`. Falls back to `(0, 0)` — the primary font's `.notdef` — so an unmapped char
    /// still draws something (and stays cached under the primary, like before).
    #[inline]
    pub fn resolve(&mut self, ch: char) -> (u16, u16) {
        if let Some(&hit) = self.resolve_cache.get(&ch) {
            return hit;
        }
        // Cold path: walk the chain once (each `face.font()` parses the table directory),
        // then memoize so subsequent frames are a single HashMap lookup.
        let mut result = (0, 0);
        for (i, slot) in self.faces.iter_mut().enumerate() {
            let Some(face) = slot.get(self.px, self.cell_h) else {
                continue;
            };
            let gid = face.font().charmap().map(ch);
            if gid != 0 {
                result = (i as u16, gid);
                break;
            }
        }
        self.resolve_cache.insert(ch, result);
        self.resolve_order.push_back(ch);
        if self.resolve_order.len() > RESOLVE_CACHE_CAP
            && let Some(old) = self.resolve_order.pop_front()
        {
            self.resolve_cache.remove(&old);
        }
        result
    }

    pub fn rasterize(&mut self, key: GlyphKey) -> &CachedGlyph {
        if !self.cache.contains_key(&key) {
            let glyph = self.render_glyph(key);
            self.cache.insert(key, glyph);
            self.cache_order.push_back(key);
            if self.cache_order.len() > GLYPH_CACHE_CAP
                && let Some(old) = self.cache_order.pop_front()
            {
                self.cache.remove(&old);
            }
        }
        self.cache.get(&key).unwrap()
    }

    /// Number of rasterized glyphs currently cached. Bounded by [`GLYPH_CACHE_CAP`] — the
    /// LRU evicts the oldest entry past it, so this never exceeds the cap regardless of how
    /// many distinct codepoints a session draws (CJK-heavy terminals included).
    #[inline]
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Bytes held by the cached glyph masks (sum of `mask.len()`, the per-glyph coverage
    /// `Vec<u8>`). This is the real heap cost of the glyph cache; the P1 LRU cap keeps it to
    /// `GLYPH_CACHE_CAP * ~max-mask-size` worst case instead of unbounded growth.
    pub fn cache_bytes(&self) -> usize {
        self.cache.values().map(|g| g.mask.len()).sum()
    }

    /// Number of memoized `resolve(ch)` fallback-chain hits. Bounded by [`RESOLVE_CACHE_CAP`].
    #[inline]
    pub fn resolve_cache_len(&self) -> usize {
        self.resolve_cache.len()
    }

    fn render_glyph(&mut self, key: GlyphKey) -> CachedGlyph {
        let index = (key.font_id as usize).min(self.faces.len() - 1);
        let index = if self.faces[index].get(self.px, self.cell_h).is_some() {
            index
        } else {
            0 // defensive: stale font_id → primary
        };
        let face = self.faces[index]
            .get(self.px, self.cell_h)
            .expect("primary font loaded");
        let fit = face.fit;
        let font = face.font();
        // A fallback glyph is rasterized at a reduced size (`px * fit`) so it fits the cell
        // box; the primary's fit is 1.0, so the primary path is unchanged.
        let mut scaler = self
            .scale
            .builder(font)
            .size(self.px * fit)
            .hint(true)
            .build();

        let mut render = Render::new(&[Source::Outline, Source::Bitmap(StrikeWith::BestFit)]);
        render.format(Format::Alpha);
        if key.italic {
            // ~12-degree synthetic slant.
            render.transform(Some(Transform::skew(
                Angle::from_degrees(-12.0),
                Angle::ZERO,
            )));
        }
        if key.bold {
            render.embolden(self.px * fit * 0.03);
        }

        match render.render(&mut scaler, key.gid as GlyphId) {
            Some(img) if img.placement.width > 0 && img.placement.height > 0 => {
                // We only ever request Alpha → Content::Mask (1 byte/px).
                let (w, h) = (img.placement.width, img.placement.height);
                let mask = match img.content {
                    Content::Mask => img.data,
                    // Defensive: collapse any color result to luminance-ish coverage.
                    _ => img
                        .data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| p[3])
                        .collect::<Vec<u8>>(),
                };
                CachedGlyph {
                    mask,
                    w,
                    h,
                    left: img.placement.left,
                    top: img.placement.top,
                }
            }
            _ => CachedGlyph {
                mask: Vec::new(),
                w: 0,
                h: 0,
                left: 0,
                top: 0,
            },
        }
    }
}

/// Uniform shrink factor so this fallback's glyphs fit the primary cell height. Returns
/// `None` if the bytes aren't a valid font (so the face is dropped from the chain).
fn compute_fit(data: &[u8], px: f32, cell_h: u32) -> Option<f32> {
    let font = FontRef::from_index(data, 0)?;
    let m = font.metrics(&[]).scale(px);
    let lh = m.ascent + m.descent + m.leading;
    let fit = if lh > cell_h as f32 && lh > 0.0 {
        cell_h as f32 / lh
    } else {
        1.0
    };
    Some(fit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_does_not_load_fallback_font_files() {
        let mut font = jetbrains_font();
        assert_eq!(
            font.faces.iter().filter(|slot| slot.face.is_some()).count(),
            1
        );
        assert_eq!(font.resolve('M').0, 0);
        assert_eq!(
            font.faces.iter().filter(|slot| slot.face.is_some()).count(),
            1
        );
    }

    #[cfg(windows)]
    #[test]
    fn chinese_fallback_is_file_backed() {
        let mut font = jetbrains_font();
        let (id, glyph) = font.resolve('中');
        assert!(id > 0 && glyph > 0);
        assert!(matches!(
            font.faces[id as usize].face.as_ref().unwrap().data,
            FontBytes::Mapped(_)
        ));
    }

    /// Build the primary from bundled bytes. Parallel fixtures must not truncate
    /// a shared temporary file; fallback faces still exercise their real file sources.
    fn jetbrains_font() -> Font {
        Font::from_bytes(font_source::JETBRAINS_MONO, 14.0).unwrap()
    }

    #[test]
    fn nerd_icons_resolve_past_the_primary() {
        // U+F121 (font-awesome "code") lives only in the bundled Symbols Nerd Font
        // (JetBrains Mono carries the powerline E0B0s itself, but not this range) —
        // it must resolve to a non-primary face with a real gid on every platform.
        let mut f = jetbrains_font();
        let (font_id, gid) = f.resolve('\u{f121}');
        assert!(
            font_id > 0 && gid != 0,
            "nerd icon fell to ({font_id}, {gid})"
        );
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn emoji_resolve_via_a_system_fallback_when_installed() {
        // 😀 is in no bundled font; with a fontconfig emoji font installed the chain
        // must pick it up (the original Linux bug: the chain probed only Segoe paths,
        // so emoji were tofu regardless of the selected family). Skipped quietly on a
        // box without any Noto Emoji.
        if font_source::fontconfig_family_file("Noto Emoji").is_none()
            && font_source::fontconfig_family_file("Noto Color Emoji").is_none()
        {
            return;
        }
        let mut f = jetbrains_font();
        let (font_id, gid) = f.resolve('😀');
        assert!(font_id > 0 && gid != 0, "emoji fell to ({font_id}, {gid})");
    }
}
