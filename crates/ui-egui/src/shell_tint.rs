//! Shell theme tint: the dark themes take their hue from the desktop shell's active palette.
//!
//! The desktop shells (island, noctalia, caelestia, the Waybar static mode) publish a
//! `Colors.qml` through the `~/.config/themes/.caelestia-use` selector, which points at the
//! dynamic provider's folder or at the named static theme. The app reads that one file through
//! `Services::shell_palette` (this crate does no I/O), so dynamic and static modes both work.
//!
//! The tint never repaints the UI from scratch. Each token of the selected dark theme keeps its
//! OKLab lightness and alpha and receives only a hue and a small chroma from the palette, so
//! the WCAG contrast the theme was built with survives and surfaces stay gray-adjacent while
//! text stays white-adjacent. A final pass checks every text token against every surface it
//! sits on and backs the tint off where it would cost contrast.

use crate::theme::Tokens;
use egui::Color32;

/// The shell colours the tint reads. Missing optional entries fall back to the theme's own.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellPalette {
    pub background: [u8; 3],
    pub background_gray: [u8; 3],
    pub foreground: [u8; 3],
    pub primary: [u8; 3],
    pub danger: Option<[u8; 3]>,
    pub warning: Option<[u8; 3]>,
}

impl ShellPalette {
    /// Read `property color name: "#rrggbb"` lines of a shell's `Colors.qml`. Entries written as
    /// `Qt.rgba(..)` (translucent overlays) are ignored. `None` when a required colour is missing.
    pub fn parse(qml: &str) -> Option<Self> {
        let find = |name: &str| {
            qml.lines().find_map(|line| {
                let rest = line.trim().strip_prefix("readonly ").unwrap_or(line.trim()).strip_prefix("property color ")?;
                let (key, value) = rest.split_once(':')?;
                if key.trim() != name {
                    return None;
                }
                let hex = value.trim().strip_prefix('"')?.strip_prefix('#')?;
                let hex = hex.get(..6)?;
                let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
                Some([byte(0)?, byte(2)?, byte(4)?])
            })
        };
        let background = find("background")?;
        Some(Self {
            background,
            background_gray: find("backgroundGray").unwrap_or(background),
            foreground: find("foreground")?,
            primary: find("primary")?,
            danger: find("danger"),
            warning: find("warning"),
        })
    }
}

// ------------------------------------------------------------------ colour maths

fn to_linear(u: u8) -> f32 {
    let c = f32::from(u) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

/// sRGB bytes to OKLab `[L, a, b]`.
fn oklab(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(to_linear);
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab to linear sRGB, unclamped so callers can test the gamut.
fn linear_from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s,
        -0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// Lightness, chroma and hue (radians) of a colour.
#[derive(Clone, Copy, Debug)]
struct Lch {
    l: f32,
    c: f32,
    h: f32,
}

fn lch(rgb: [u8; 3]) -> Lch {
    let [l, a, b] = oklab(rgb);
    Lch { l, c: a.hypot(b), h: b.atan2(a) }
}

/// The colour at lightness `l` with hue `h` and (at most) chroma `c`; chroma shrinks until the
/// colour fits the sRGB gamut, so lightness is never traded for saturation.
fn at(l: f32, c: f32, h: f32) -> [u8; 3] {
    let l = l.clamp(0.0, 1.0);
    let mut c = c.max(0.0);
    for _ in 0..32 {
        let lin = linear_from_oklab([l, c * h.cos(), c * h.sin()]);
        if lin.iter().all(|v| (-0.001..=1.001).contains(v)) || c < 1e-4 {
            return lin.map(from_linear);
        }
        c *= 0.92;
    }
    linear_from_oklab([l, 0.0, 0.0]).map(from_linear)
}

fn luminance(c: Color32) -> f32 {
    0.2126 * to_linear(c.r()) + 0.7152 * to_linear(c.g()) + 0.0722 * to_linear(c.b())
}

/// WCAG 2 contrast ratio of two opaque colours.
fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `orig` with its lightness and alpha kept and the hue and chroma replaced.
fn retint(orig: Color32, h: f32, c: f32) -> Color32 {
    let [r, g, b, a] = orig.to_srgba_unmultiplied();
    let [r, g, b] = at(lch([r, g, b]).l, c, h);
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// [`retint`] with the lightness moved by `dl` first, but not below `floor`.
fn retint_by(orig: Color32, h: f32, c: f32, dl: f32, floor: f32) -> Color32 {
    let [r, g, b, a] = orig.to_srgba_unmultiplied();
    let [r, g, b] = at((lch([r, g, b]).l + dl).clamp(floor, 0.98), c, h);
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

// ------------------------------------------------------------------ the tint

/// Surface tint strength in OKLab chroma. The floor makes the palette visible on an 8-bit
/// display; the ceiling keeps long sessions comfortable on the lightest dark themes.
const SURFACE_CHROMA: std::ops::RangeInclusive<f32> = 0.025..=0.06;
/// The most chroma text may take from the shell's foreground colour.
const TEXT_CHROMA_MAX: f32 = 0.03;

/// `c` at its own lightness with hue `h` and chroma `chroma`, lifted lighter until it holds
/// `floor`:1 on every `(surface, original surface)` pair, or as much as the stock theme managed
/// there if that was less.
fn legible(c: Color32, h: f32, chroma: f32, floor: f32, on: &[(Color32, Color32)], original: Color32) -> Color32 {
    let ok = |c: Color32| on.iter().all(|(s, o)| s.a() != 255 || contrast(c, *s) >= contrast(original, *o).min(floor) - 0.02);
    let mut l = lch([c.r(), c.g(), c.b()]).l;
    let mut out = retint(c, h, chroma);
    while !ok(out) && l < 1.0 {
        l += 0.01;
        let [r, g, b] = at(l, chroma, h);
        out = Color32::from_rgb(r, g, b);
    }
    out
}

/// Tint a dark theme's tokens with the shell palette. Light themes are returned unchanged.
///
/// Surfaces take the palette background's hue and strength and sit at the lightness of its panel
/// colour, keeping the theme's own steps between them; text keeps its lightness and takes the
/// palette foreground's hue; the accent family follows the palette's primary.
pub fn tint(orig: Tokens, p: &ShellPalette) -> Tokens {
    if !orig.dark() {
        return orig;
    }
    let primary = lch(p.primary);
    let bg = lch(p.background);
    let gray = lch(p.background_gray);
    let fg = lch(p.foreground);
    // The most colourful of the palette's backgrounds sets the surface tint; a palette whose
    // backgrounds are all neutral borrows the primary's hue.
    let base = if gray.c > bg.c { gray } else { bg };
    let (surface_h, surface_c) = if base.c < 0.008 {
        (primary.h, (primary.c * 0.35).clamp(*SURFACE_CHROMA.start(), *SURFACE_CHROMA.end()))
    } else {
        (base.h, base.c.clamp(*SURFACE_CHROMA.start(), *SURFACE_CHROMA.end()))
    };
    let (text_h, text_c) = if fg.c < 0.006 { (surface_h, 0.01) } else { (fg.h, fg.c.min(TEXT_CHROMA_MAX)) };
    // Surfaces move as one block so the panel sits at the lightness of the shell's own panel
    // colour; every step between canvas, dock, panel, field, hover and pressed is kept, so the
    // theme's depth ordering and the text's contrast survive.
    let dl = gray.l.clamp(0.16, 0.42) - lch([orig.card.r(), orig.card.g(), orig.card.b()]).l;
    // Nothing goes darker than the shell's own background, so the canvas and borders never crush.
    let floor = (bg.l - 0.02).clamp(0.08, 0.3);
    let surf = |c: Color32| retint_by(c, surface_h, surface_c, dl, floor);

    let mut t = Tokens {
        chrome: surf(orig.chrome),
        canvas: surf(orig.canvas),
        canvas_dot: surf(orig.canvas_dot),
        dock: surf(orig.dock),
        card: surf(orig.card),
        card_border: surf(orig.card_border),
        field: surf(orig.field),
        field_border: surf(orig.field_border),
        hover: surf(orig.hover),
        pressed: surf(orig.pressed),
        separator: surf(orig.separator),
        tab_strip: surf(orig.tab_strip),
        row_selected: surf(orig.row_selected),
        histogram_bg: surf(orig.histogram_bg),
        ..orig
    };
    let on = [
        (t.card, orig.card),
        (t.chrome, orig.chrome),
        (t.dock, orig.dock),
        (t.field, orig.field),
        (t.hover, orig.hover),
        (t.row_selected, orig.row_selected),
        (t.tab_strip, orig.tab_strip),
    ];
    // Text and icons: the palette's foreground tint, never below the contrast the theme had
    // (capped at WCAG's 4.5:1 for body text and 3:1 for secondary text and icons).
    t.text = legible(orig.text, text_h, text_c, 4.5, &on, orig.text);
    t.text_dim = legible(orig.text_dim, text_h, text_c, 3.0, &on, orig.text_dim);
    t.text_faint = legible(orig.text_faint, text_h, text_c, 3.0, &on, orig.text_faint);
    t.icon = legible(orig.icon, text_h, text_c, 3.0, &on, orig.icon);

    // Accent family: the shell's primary, lightened only as far as 3:1 against the panels needs.
    let mut accent_l = primary.l;
    let accent = loop {
        let candidate = rgb(at(accent_l, primary.c, primary.h));
        if accent_l >= 0.95 || [t.card, t.chrome, t.field].iter().all(|s| contrast(candidate, *s) >= 3.0) {
            break candidate;
        }
        accent_l += 0.01;
    };
    let on_accent = |bg: Color32| {
        let light = rgb(at(0.97, text_c, text_h));
        let dark = rgb(at(0.2, surface_c, surface_h));
        if contrast(light, bg) >= contrast(dark, bg) { light } else { dark }
    };
    t.accent = accent;
    if orig.primary_bg == orig.accent {
        // Pro layout: one solid accent, with its label colour chosen for contrast on it.
        t.accent_border = accent;
        t.primary_bg = accent;
        t.accent_text = on_accent(accent);
        t.primary_text = t.accent_text;
        t.accent_soft = surf(orig.accent_soft);
    } else {
        // Studio layout: translucent accent washes and light accent text on the panels.
        let wash = |c: Color32, l: f32| {
            let [_, _, _, a] = c.to_srgba_unmultiplied();
            let [r, g, b] = at(l, primary.c, primary.h);
            Color32::from_rgba_unmultiplied(r, g, b, a)
        };
        t.accent_soft = wash(orig.accent_soft, accent_l);
        t.accent_border = wash(orig.accent_border, (accent_l + 0.1).min(0.95));
        let mut text_l = lch([orig.accent_text.r(), orig.accent_text.g(), orig.accent_text.b()]).l;
        t.accent_text = loop {
            let candidate = rgb(at(text_l, primary.c.min(0.06), primary.h));
            if text_l >= 0.98 || contrast(candidate, t.card) >= 4.5 {
                break candidate;
            }
            text_l += 0.01;
        };
        t.primary_bg = retint(orig.primary_bg, text_h, text_c);
        t.primary_text = surf(orig.primary_text);
    }
    // The palette's own signal colours, at the lightness the theme chose for its own.
    for (slot, source, original) in [(&mut t.danger, p.danger, orig.danger), (&mut t.warning, p.warning, orig.warning)] {
        if let Some(source) = source {
            let s = lch(source);
            *slot = retint(original, s.h, s.c.min(0.16));
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeKind;

    const ISLAND: &str = r##"pragma Singleton

import Quickshell
import QtQuick

Singleton {
    readonly property color background: "#151520"
    readonly property color backgroundAlpha: Qt.rgba(37 / 255, 37 / 255, 44 / 255, 0.87)
    readonly property color backgroundGray: "#25252c"
    readonly property color foreground: "#d0d0b3"
    readonly property color primary: "#9eaed1"
    readonly property color danger: "#d69990"
}
"##;

    fn island() -> ShellPalette {
        ShellPalette::parse(ISLAND).unwrap()
    }

    #[test]
    fn parses_shell_colors_and_ignores_rgba_entries() {
        let p = island();
        assert_eq!((p.background, p.primary, p.foreground), ([0x15, 0x15, 0x20], [0x9e, 0xae, 0xd1], [0xd0, 0xd0, 0xb3]));
        assert_eq!((p.danger, p.warning), (Some([0xd6, 0x99, 0x90]), None));
    }

    #[test]
    fn missing_required_colors_or_garbage_is_not_a_palette() {
        assert_eq!(ShellPalette::parse(""), None);
        assert_eq!(ShellPalette::parse("property color primary: \"#zzzzzz\""), None);
        assert_eq!(ShellPalette::parse("property color background: \"#12\""), None);
    }

    #[test]
    fn dark_themes_get_a_hue_and_keep_their_contrast() {
        let p = island();
        for kind in [ThemeKind::Pro, ThemeKind::ProMedium, ThemeKind::Studio, ThemeKind::SoftDark, ThemeKind::BreezeDark] {
            let orig = Tokens::for_kind(kind);
            let t = tint(orig, &p);
            assert_ne!(t.card, orig.card, "{kind:?} card gets the tint");
            assert!(t.card.b() > t.card.r() + 3, "{kind:?}: island panels lean blue: {:?}", t.card);
            for (label, tinted, floor) in [("text", t.text, 4.5), ("dim", t.text_dim, 3.0), ("faint", t.text_faint, 3.0), ("icon", t.icon, 3.0)] {
                for (surface, original_surface) in [(t.card, orig.card), (t.chrome, orig.chrome), (t.field, orig.field)] {
                    let original = match label {
                        "text" => orig.text,
                        "dim" => orig.text_dim,
                        "faint" => orig.text_faint,
                        _ => orig.icon,
                    };
                    let kept = contrast(original, original_surface).min(floor) - 0.1;
                    assert!(contrast(tinted, surface) >= kept, "{kind:?} {label} on {surface:?}");
                }
            }
            assert!(contrast(t.accent, t.card) >= 3.0, "{kind:?} accent on card");
        }
    }

    #[test]
    fn light_themes_and_unreadable_palettes_leave_tokens_usable() {
        let light = Tokens::for_kind(ThemeKind::ProLight);
        assert_eq!(tint(light, &island()), light);
        // A pitch-black "primary" must be lifted to a visible accent, not left invisible.
        let dim = ShellPalette { primary: [10, 10, 10], ..island() };
        let t = tint(Tokens::for_kind(ThemeKind::ProMedium), &dim);
        assert!(contrast(t.accent, t.card) >= 3.0);
    }
}
