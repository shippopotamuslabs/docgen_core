//! Math PNG rendering for DOCX export.
//!
//! `render_math_latex_to_png` expects the exact Rust `String` content produced
//! by JSON deserialization. It does not attempt to repair double-JSON-encoded
//! backslashes; callers must pass the intended LaTeX source. Scenario S4 in
//! `plans/FEAT-docx-math-png-caption/test_spec.md` intentionally preserves
//! consecutive backslashes in captions.

use std::collections::HashMap;

pub(crate) const MATH_RENDER_PIXELS_PER_POINT: f32 = 8.0;
pub(crate) const MATH_DISPLAY_PIXELS_PER_POINT: f32 = 4.0;

#[allow(dead_code)]
pub(crate) const INVALID_MATH_LATEX_FOR_DOCX: &str = "\\frac{1}{";

pub(crate) fn render_math_latex_to_png(
    latex: &str,
    display: bool,
) -> Result<Vec<u8>, crate::DocGenError> {
    if latex.trim().is_empty() {
        return Err(crate::DocGenError::GenerationError(
            "math latex is empty".to_string(),
        ));
    }

    let latex = latex.to_string();
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || render_math_latex_to_png_inner(&latex, display))
        .map_err(|e| crate::DocGenError::GenerationError(e.to_string()))?
        .join()
        .map_err(|_| crate::DocGenError::GenerationError("math PNG thread panicked".to_string()))?
}

fn render_math_latex_to_png_inner(
    latex: &str,
    display: bool,
) -> Result<Vec<u8>, crate::DocGenError> {
    let typst_math = crate::latex_to_typst::latex_to_typst_math(latex);
    let math_body = if display {
        format!("$ {typst_math} $")
    } else {
        format!("${typst_math}$")
    };
    let source = format!(
        "#set page(width: auto, height: auto, margin: 4pt, fill: none)\n\
         #set text(size: 18pt)\n\
         {math_body}\n"
    );
    let world = crate::typst_world::PocketCrewWorld::new(source, HashMap::new());
    let warned = typst::compile::<typst::layout::PagedDocument>(&world);
    let document = warned.output.map_err(|errors| {
        let msg = errors
            .iter()
            .map(|e| e.message.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        crate::DocGenError::GenerationError(msg)
    })?;
    let page = document.pages.first().ok_or_else(|| {
        crate::DocGenError::GenerationError("math PNG render produced no pages".to_string())
    })?;
    let pixmap = typst_render::render(page, MATH_RENDER_PIXELS_PER_POINT);
    pixmap
        .encode_png()
        .map_err(|e| crate::DocGenError::GenerationError(format!("PNG encode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_fraction_renders_decodable_png_bytes() {
        let png = render_math_latex_to_png("\\frac{1}{2}", true).expect("render png");
        let image = image::load_from_memory(&png).expect("decode png");
        let (width, height) = image::GenericImageView::dimensions(&image);

        assert!(width >= 40, "width was {width}");
        assert!(height >= 20, "height was {height}");
    }

    #[test]
    fn invalid_latex_fixture_returns_error() {
        assert!(render_math_latex_to_png(INVALID_MATH_LATEX_FOR_DOCX, true).is_err());
    }
}
