//! WinAnsi glyph widths (1/1000 em) for PDF built-in Type1 fonts.
//! Tables derived from TCPDF font metrics (LGPL) — Times variants; Courier is fixed at 600.

include!("pdf_width_tables.inc.rs");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfFont {
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
}

impl PdfFont {
    pub const fn widths(&self) -> &'static [u16; 256] {
        match self {
            PdfFont::TimesRoman => &TIMES_ROMAN,
            PdfFont::TimesBold => &TIMES_BOLD,
            PdfFont::TimesItalic => &TIMES_ITALIC,
            PdfFont::TimesBoldItalic => &TIMES_BOLDITALIC,
            PdfFont::Courier => &COURIER,
            PdfFont::CourierBold => &COURIER_BOLD,
            PdfFont::CourierOblique => &COURIER_OBLIQUE,
            PdfFont::CourierBoldOblique => &COURIER_BOLDOBLIQUE,
        }
    }

    /// PDF resource name (F1..F8) used in content streams.
    pub const fn pdf_name(&self) -> &'static str {
        match self {
            PdfFont::TimesRoman => "F1",
            PdfFont::TimesBold => "F2",
            PdfFont::TimesItalic => "F3",
            PdfFont::TimesBoldItalic => "F4",
            PdfFont::Courier => "F5",
            PdfFont::CourierBold => "F6",
            PdfFont::CourierOblique => "F7",
            PdfFont::CourierBoldOblique => "F8",
        }
    }

    /// Map inline style to a PDF built-in font (WinAnsi-encoded).
    pub fn from_span(bold: bool, italic: bool, code: bool) -> PdfFont {
        if code {
            return match (bold, italic) {
                (true, true) => PdfFont::CourierBoldOblique,
                (true, false) => PdfFont::CourierBold,
                (false, true) => PdfFont::CourierOblique,
                (false, false) => PdfFont::Courier,
            };
        }
        match (bold, italic) {
            (true, true) => PdfFont::TimesBoldItalic,
            (true, false) => PdfFont::TimesBold,
            (false, true) => PdfFont::TimesItalic,
            (false, false) => PdfFont::TimesRoman,
        }
    }
}

/// Map a Unicode scalar to a WinAnsi byte index (0–255). Non-Latin-1 uses `?` (0x3F).
#[inline]
pub fn winansi_index(ch: char) -> usize {
    let c = ch as u32;
    if c < 256 {
        return c as usize;
    }
    // Minimal high-Latin1 mapping for common punctuation already normalized elsewhere.
    b'?' as usize
}

pub fn char_width_pt(ch: char, font: PdfFont, size_pt: f32) -> f32 {
    let w = font.widths()[winansi_index(ch)] as f32;
    w * size_pt / 1000.0
}

pub fn text_width_pt(s: &str, font: PdfFont, size_pt: f32) -> f32 {
    let wtab = font.widths();
    let mut total = 0.0_f32;
    for ch in s.chars() {
        total += wtab[winansi_index(ch)] as f32;
    }
    total * size_pt / 1000.0
}

/// Width of longest prefix of `s` that fits in `max_width_pt` at `size_pt` using `font`.
pub fn prefix_width_pt(s: &str, font: PdfFont, size_pt: f32, max_width_pt: f32) -> usize {
    if s.is_empty() {
        return 0;
    }
    let mut acc = 0.0_f32;
    let wtab = font.widths();
    let mut idx = 0;
    for (i, ch) in s.char_indices() {
        let w = wtab[winansi_index(ch)] as f32 * size_pt / 1000.0;
        if acc + w > max_width_pt + 1e-4 {
            break;
        }
        acc += w;
        idx = i + ch.len_utf8();
    }
    idx
}
