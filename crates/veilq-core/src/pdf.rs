//! PDFs (ADR-0023): every page rendered at 200 dpi with PDFium, read with
//! OCR and redacted like an image; the output is a new PDF of those images.
//!
//! What a PDF holds besides its picture (the text layer, hidden text,
//! annotations' contents, form values, links, attachments, scripts,
//! metadata) is not carried over: a page is what it shows, nothing else.
//! The report says so as a review reason, since a text layer that searching
//! and copying relied on is gone.
//!
//! PDFium is loaded at run time from the model directory, like ONNX
//! Runtime; a PDF it cannot open (encrypted, damaged) is an error.

use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

use image::RgbaImage;
use pdfium_render::prelude::*;

use crate::Engine;
use crate::Error;
use crate::ocr::Ocr;
use crate::office::Outcome;

const DPI: f32 = 200.0;

/// Said in every report of a PDF.
pub const IMAGE_ONLY: &str = "the PDF was rewritten as images of its pages: its text layer, links, \
     forms, attachments and metadata are gone";

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(format!("PDF: {message}"))
}

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// PDFium from `library`; it is bound once per process.
pub fn pdfium(library: &Path) -> Result<&'static Pdfium, Error> {
    PDFIUM
        .get_or_init(|| {
            Pdfium::bind_to_library(library)
                .map(Pdfium::new)
                .map_err(|e| format!("cannot load PDFium from {}: {e}", library.display()))
        })
        .as_ref()
        .map_err(|e| Error::Format(e.clone()))
}

fn render_config() -> PdfRenderConfig {
    PdfRenderConfig::new()
        .scale_page_by_factor(DPI / 72.0)
        .render_form_data(true)
        .render_annotations(true)
}

fn render(page: &PdfPage, config: &PdfRenderConfig, n: usize) -> Result<RgbaImage, Error> {
    let bitmap = page.render_with_config(config).map_err(error)?;
    let (w, h) = (bitmap.width() as u32, bitmap.height() as u32);
    RgbaImage::from_raw(w, h, bitmap.as_rgba_bytes())
        .ok_or_else(|| error(format!("page {n} rendered to no image")))
}

/// The first `max` pages of a PDF as PNG, in the pixels redaction reads them
/// in (a finding's region is in these), and the number of pages: what a
/// viewer sets beside the findings.
pub fn pages(pdfium: &Pdfium, bytes: &[u8], max: usize) -> Result<(Vec<Vec<u8>>, usize), Error> {
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(error)?;
    let config = render_config();
    let mut out = Vec::new();
    for (i, page) in doc.pages().iter().enumerate().take(max) {
        let img = render(&page, &config, i + 1)?;
        out.push(crate::visual::encode(&img, image::ImageFormat::Png)?);
    }
    Ok((out, doc.pages().len() as usize))
}

/// A PDF, redacted page by page (pages counted from 1).
pub fn redact(
    engine: &Engine,
    ocr: &Ocr,
    pdfium: &Pdfium,
    bytes: &[u8],
    apply: bool,
) -> Result<Outcome, Error> {
    let input = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(error)?;
    let mut out = Outcome {
        output: Vec::new(),
        findings: Vec::new(),
        mapping: Vec::new(),
        review_reasons: vec![IMAGE_ONLY.into()],
    };
    let mut output = if apply {
        Some(pdfium.create_new_pdf().map_err(error)?)
    } else {
        None
    };
    let config = render_config();
    for (i, page) in input.pages().iter().enumerate() {
        let mut img = render(&page, &config, i + 1)?;
        crate::visual::redact_picture(engine, ocr, &mut img, Some(i as u64 + 1), apply, &mut out)?;
        if let Some(doc) = &mut output {
            let jpeg = crate::visual::encode(&img, image::ImageFormat::Jpeg)?;
            let (pw, ph) = (page.width(), page.height());
            let mut object =
                PdfPageImageObject::new_from_jpeg_reader(doc, Cursor::new(jpeg)).map_err(error)?;
            object.scale(pw.value, ph.value).map_err(error)?;
            let mut new = doc
                .pages_mut()
                .create_page_at_end(PdfPagePaperSize::Custom(pw, ph))
                .map_err(error)?;
            new.objects_mut().add_image_object(object).map_err(error)?;
        }
    }
    if let Some(doc) = output {
        out.output = doc.save_to_bytes().map_err(error)?;
    }
    Ok(out)
}
