//! DOCX, XLSX and PPTX (ADR-0019): the ZIP and its XML parts edited in place.
//!
//! Every XML part is read, not only the visible text: headers, footers,
//! notes, comments, tracked deletions, hidden sheets, cached formula values,
//! speaker notes, chart labels, document properties. Text inside a paragraph
//! (`w:p`, `a:p`, a shared string `si`) is one segment over all its runs, so
//! that a name split across runs is found; its replacement goes into the run
//! where the finding starts, the other runs lose their part of it, and every
//! run keeps its formatting. Text elsewhere is a segment of its own.
//!
//! Who wrote and edited the file (author, last modified by, company, manager,
//! comment authors, custom properties) is replaced whatever its content: the
//! detector is `metadata`. Images (PNG, JPEG, WebP, GIF) are read with OCR
//! when it is installed (ADR-0023). Other images and embedded objects, and
//! images without OCR, fail a run, or with `[office] embedded = "remove"`
//! they are blanked and the report asks for a review. A nested Office file (a chart's
//! workbook) is redacted like the outer one.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read, Write};
use std::ops::Range;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::detect::Candidate;
use crate::finding::{EntityType, Finding, Location};
use crate::mapping::Entry;
use crate::policy::Embedded;
use crate::xml::{self, TagKind, Token};
use crate::{Engine, Error, Format};

/// What a container run found, and its output.
#[derive(Default)]
pub struct Outcome {
    pub output: Vec<u8>,
    pub findings: Vec<Finding>,
    pub mapping: Vec<Entry>,
    pub review_reasons: Vec<String>,
    /// Lines of a picture OCR read badly, where nothing was filled: their
    /// place, as the messages say it; one review reason at the end (#52).
    pub weak_lines: Vec<String>,
    /// Mail headers removed unread (ADR-0063); one review reason at the end.
    pub removed_headers: usize,
}

const PARAGRAPHS: &[&str] = &["p", "si"];
const TEXTS: &[&str] = &["t", "delText", "instrText"];
/// Tracked deletions: their text and the text around them are alternatives
/// (before and after the change), so a deletion reads as a line of its own,
/// not glued to the next word.
const DELETIONS: &[&str] = &["del", "moveFrom"];

/// A line break that is in no element: it separates, and is never written.
fn separator() -> Piece {
    Piece {
        raw: None,
        text: "\n".into(),
        space_tag: None,
    }
}
/// Elements whose whole text is who wrote or owns the file.
const PERSON_ELEMENTS: &[&str] = &["creator", "lastModifiedBy", "Manager", "author"];
const PERSON_ATTRS: &[&str] = &["author", "initials", "displayName", "userId"];

/// The picture format of a part, by its name.
fn picture(lower: &str) -> Option<Format> {
    let ext = lower.rsplit_once('.')?.1;
    Format::from_name(ext).ok().filter(|f| f.is_picture())
}

/// A 1x1 white image per format, for `embedded = "remove"`.
pub(crate) const BLANK_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xFF, 0xFF, 0x3F,
    0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];
pub(crate) const BLANK_GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xFF, 0xFF, 0xFF,
    0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44,
    0x01, 0x00, 0x3B,
];
const BLANK_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>";

/// A white 1x1 picture in the format a part's name says.
fn blank_picture(lower: &str) -> Result<Vec<u8>, Error> {
    if lower.ends_with(".jpeg") || lower.ends_with(".jpg") {
        let white = image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255]));
        let mut bytes = Cursor::new(Vec::new());
        white
            .write_to(&mut bytes, image::ImageFormat::Jpeg)
            .map_err(error)?;
        Ok(bytes.into_inner())
    } else if lower.ends_with(".gif") {
        Ok(BLANK_GIF.to_vec())
    } else {
        Ok(BLANK_PNG.to_vec())
    }
}

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(message.to_string())
}

/// One piece of a segment: a text node (`raw`: its range in the part) or a
/// line break or tab between runs, which is read but never written.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub(crate) raw: Option<Range<usize>>,
    pub(crate) text: String,
    /// The start tag of the text element, when `xml:space` may be missing.
    pub(crate) space_tag: Option<Range<usize>>,
}

#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub(crate) pieces: Vec<Piece>,
    pub(crate) context: String,
    /// Replace the whole text as this type, whatever it holds.
    pub(crate) forced: Option<&'static str>,
    pub(crate) place: Place,
    /// What else changes when the text does; `None` for text in place.
    pub(crate) rewrite: Option<Rewrite>,
}

#[derive(Debug, Clone)]
pub(crate) enum Rewrite {
    /// A cell value that becomes an inline string.
    Cell(CellRewrite),
    /// The local name (its range) of a pivot cache's `n` or `d` element,
    /// which becomes `s` when its value becomes text.
    Retag(Range<usize>),
}

#[derive(Debug, Clone)]
pub(crate) struct CellRewrite {
    /// The `<c ...>` element from its start to its end.
    element: Range<usize>,
    /// The start tag's `t` attribute value range, or where to put one.
    reference: String,
    style: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum Place {
    /// The n-th segment of a part (from 1).
    Part { part: String, n: u64 },
    Cell {
        sheet: String,
        row: u64,
        column: u64,
    },
}

impl Segment {
    fn text(&self) -> String {
        self.pieces.iter().map(|p| p.text.as_str()).collect()
    }
}

/// Redacts (or with `apply == false` scans) an Office file.
pub fn process(engine: &Engine, input: &[u8], apply: bool) -> Result<Outcome, Error> {
    // Pictures inside a container stay unmarked (ADR-0050).
    let engine = &engine.without_watermark(true);
    let mut archive = ZipArchive::new(Cursor::new(input))
        .map_err(|e| error(format!("not an Office file: {e}")))?;
    let mut entries: Vec<(String, Vec<u8>, CompressionMethod)> = Vec::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(error)?;
        let mut data = Vec::new();
        f.read_to_end(&mut data).map_err(error)?;
        let name = f.name().map_err(error)?.into_owned();
        entries.push((name, data, f.compression()));
    }
    let mut out = Outcome {
        output: Vec::new(),
        findings: Vec::new(),
        mapping: Vec::new(),
        review_reasons: Vec::new(),
        weak_lines: Vec::new(),
        removed_headers: 0,
    };
    let names: Vec<String> = entries.iter().map(|e| e.0.clone()).collect();
    let mut workbook = Workbook::read(&entries)?;
    // Before any sheet is read: a renamed sheet is reported by its new name.
    let mut workbook_edits = Some(workbook.names(engine, &entries, apply, &mut out)?);
    let mut table_edits = table_names(engine, &entries, apply, &mut out)?;
    let pivots = pivot_fields(&entries)?;
    let mut new_data: HashMap<String, Vec<u8>> = HashMap::new();
    let mut blanked = 0;
    // What became of each pivot cache's fields, by its definition (#157).
    let mut pivot_changes: HashMap<String, FieldChanges> = HashMap::new();
    // The shared strings last: the sheets say first which columns show them.
    // A pivot cache's definition after its records: they say what became
    // of its fields.
    let mut order: Vec<&(String, Vec<u8>, CompressionMethod)> = entries.iter().collect();
    order.sort_by_key(|e| {
        (
            e.0 == "xl/sharedStrings.xml",
            e.0.starts_with("xl/pivotCache/pivotCacheDefinition"),
        )
    });
    for (name, data, _) in order {
        if name.ends_with('/') {
            continue;
        }
        let lower = name.to_lowercase();
        if lower.ends_with(".xml") || lower.ends_with(".rels") || lower.ends_with(".vml") {
            let text =
                std::str::from_utf8(data).map_err(|_| error(format!("{name} is not UTF-8 XML")))?;
            let changed = if workbook.is_sheet(name) {
                workbook.sheet(engine, name, text, apply, &mut out)?
            } else if name == "xl/sharedStrings.xml" {
                workbook.shared_strings(engine, text, apply, &mut out)?
            } else {
                let edits = if name == "xl/workbook.xml" {
                    workbook_edits.take().unwrap_or_default()
                } else if apply {
                    table_edits.remove(name).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let records = pivots.get(name);
                let fields = records.map(|(f, _)| f.as_slice());
                let (changed, found_at) = part(engine, name, text, fields, edits, apply, &mut out)?;
                let definition = name.starts_with("xl/pivotCache/pivotCacheDefinition");
                if let Some((_, def)) = records {
                    let changes = pivot_changes.entry(def.clone()).or_default();
                    note_fields(text, true, &found_at, changes)?;
                    changed
                } else if definition && apply {
                    let mut changes = pivot_changes.remove(name).unwrap_or_default();
                    note_fields(text, false, &found_at, &mut changes)?;
                    let now = changed.as_deref().unwrap_or(text);
                    shared_items(now, &changes)?.or(changed)
                } else {
                    changed
                }
            };
            // Comments and processing instructions go unread (ADR-0063);
            // nothing above reads or edits them.
            let kept = changed.as_deref().unwrap_or(text);
            let dropped = xml::comments(kept);
            let changed = if dropped.is_empty() {
                changed
            } else {
                apply_edits(
                    kept,
                    dropped.into_iter().map(|r| (r, String::new())).collect(),
                )
            };
            if let Some(c) = changed {
                new_data.insert(name.clone(), c.into_bytes());
            }
        } else if lower.starts_with("docprops/thumbnail.") {
            // A small picture of the first page: replaced, never read.
            new_data.insert(name.clone(), blank_picture(&lower)?);
        } else if let Some(kind) = nested_office(&lower) {
            let _ = kind;
            let inner = process(engine, data, apply).map_err(|e| error(format!("{name}: {e}")))?;
            out.findings.extend(inner.findings);
            out.mapping.extend(inner.mapping);
            out.review_reasons.extend(inner.review_reasons);
            new_data.insert(name.clone(), inner.output);
        } else if lower.contains("printersettings") {
            out.review_reasons.push(format!(
                "{name}: printer settings are kept as they are, unread"
            ));
        } else if lower.contains("/fonts/")
            && (lower.ends_with(".odttf") || lower.ends_with(".ttf"))
        {
            // Embedded fonts: glyphs, no content.
        } else if let Some(f) = picture(&lower).filter(|f| engine.reads(*f)) {
            let (bytes, run) = engine
                .run_bytes(data, f, apply)
                .map_err(|e| error(format!("{name}: {e}")))?;
            for mut finding in run.report.findings {
                if let Location::Region { part, .. } = &mut finding.location {
                    *part = Some(name.clone());
                }
                out.findings.push(finding);
            }
            out.review_reasons.extend(run.report.review_reasons);
            if apply {
                new_data.insert(name.clone(), bytes);
            }
        } else {
            match engine.policy().office.embedded {
                Embedded::Error => {
                    return Err(error(format!(
                        "{name}: an embedded object or image veilq cannot read (images need OCR: `veilq models fetch ocr`; set [office] embedded = \"remove\" to blank it)"
                    )));
                }
                Embedded::Remove => {
                    let blank: &[u8] = if lower.ends_with(".gif") {
                        BLANK_GIF
                    } else if lower.ends_with(".svg") {
                        BLANK_SVG
                    } else if [
                        ".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff", ".emf", ".wmf", ".wdp",
                    ]
                    .iter()
                    .any(|e| lower.ends_with(e))
                    {
                        BLANK_PNG
                    } else {
                        return Err(error(format!(
                            "{name}: an embedded object veilq cannot blank; remove it from the file first"
                        )));
                    };
                    blanked += 1;
                    new_data.insert(name.clone(), blank.to_vec());
                }
            }
        }
    }
    if blanked > 0 {
        out.review_reasons.push(format!(
            "{blanked} embedded image(s) were blanked, not read"
        ));
    }
    if apply {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data, method) in &entries {
            let options = SimpleFileOptions::default().compression_method(match method {
                CompressionMethod::Stored => CompressionMethod::Stored,
                _ => CompressionMethod::Deflated,
            });
            if name.ends_with('/') {
                writer.add_directory(name.clone(), options).map_err(error)?;
                continue;
            }
            writer.start_file(name.clone(), options).map_err(error)?;
            writer
                .write_all(new_data.get(name).unwrap_or(data))
                .map_err(error)?;
        }
        out.output = writer.finish().map_err(error)?.into_inner();
    }
    let _ = names;
    Ok(out)
}

fn nested_office(lower: &str) -> Option<&'static str> {
    if !lower.contains("/embeddings/") {
        return None;
    }
    [".docx", ".xlsx", ".pptx"]
        .into_iter()
        .find(|e| lower.ends_with(e))
}

/// An attribute that holds no text for a reader: a namespace, an id, a
/// reference to another part, a relationship's or content's type, a
/// revision mark (ADR-0063), a font's name and classification (a theme's
/// `MV Boli` was taken for a person, its `panose` for a phone number, and
/// Excel no longer opened the file). Every other attribute is read.
fn structural(attr: &str) -> bool {
    let l = xml::local(attr);
    attr.starts_with("xmlns")
        || matches!(l, "typeface" | "panose" | "pitchFamily" | "charset")
        || attr.starts_with("r:")
        || attr.starts_with("xml:")
        || l.starts_with("rsid")
        || l.ends_with("Id")
        || matches!(
            l,
            "id" | "Id"
                | "uid"
                | "guid"
                | "uri"
                | "spid"
                | "Ignorable"
                | "ProcessContent"
                | "MustUnderstand"
                | "PreserveElements"
                | "PreserveAttributes"
                | "Requires"
                | "Type"
                | "ContentType"
                | "PartName"
                | "Extension"
                | "Target"
                | "TargetMode"
        )
}

/// Attributes another part names (a sheet in a formula, a defined name):
/// read where their names are decided ([`Workbook::names`]), not here.
fn named_elsewhere(part: &str, element: &str, attr: &str) -> bool {
    (part == "xl/workbook.xml" && matches!(element, "sheet" | "definedName") && attr == "name")
        // A table's name ([`table_names`]).
        || (part.starts_with("xl/tables/")
            && element == "table"
            && matches!(attr, "name" | "displayName"))
        // A calculation feature Excel needs (`microsoft.com:Single`): no
        // text, and no name a person gave.
        || (element == "feature" && attr == "name")
}

/// The segments of a part: paragraphs, other text, attributes, the
/// metadata. `fields`: for a pivot cache's records, its fields' names.
fn segments(name: &str, s: &str, fields: Option<&[String]>) -> Result<Vec<Segment>, Error> {
    segments_of(name, s, &xml::tokenize(s)?, fields, None)
}

/// [`segments`] of a part tokenized already, without the tokens in `skip`
/// (a worksheet's `sheetData`, whose cells are read as cells, #158).
fn segments_of(
    name: &str,
    s: &str,
    tokens: &[Token],
    fields: Option<&[String]>,
    skip: Option<Range<usize>>,
) -> Result<Vec<Segment>, Error> {
    let skipped = |r: &Range<usize>| {
        skip.as_ref()
            .is_some_and(|k| k.start <= r.start && r.end <= k.end)
    };
    let tokens = tokens.iter().filter(|t| match t {
        Token::Tag { range, .. } => !skipped(range),
        Token::Text(r) | Token::CData(r) => !skipped(r),
    });
    let mut out: Vec<Segment> = Vec::new();
    // Open paragraphs (nested ones in text boxes), each collecting pieces.
    let mut open: Vec<Vec<Piece>> = Vec::new();
    // The element stack: local names; for text elements their start tag.
    let mut stack: Vec<(String, Option<Range<usize>>, bool)> = Vec::new();
    let custom = name == "docProps/custom.xml";
    let comment_authors = name.ends_with("commentAuthors.xml");
    let pivot = name.starts_with("xl/pivotCache/");
    // A pivot cache's definition: the field whose items are read; its
    // records: the place of a value in its record.
    let mut field = String::new();
    let mut in_record = 0usize;
    let n = |out: &Vec<Segment>| out.len() as u64 + 1;
    let push = |out: &mut Vec<Segment>,
                pieces: Vec<Piece>,
                forced: Option<&'static str>,
                context: String,
                rewrite: Option<Rewrite>| {
        if pieces.iter().any(|p| !p.text.trim().is_empty()) {
            let place = Place::Part {
                part: name.to_owned(),
                n: n(out),
            };
            out.push(Segment {
                pieces,
                context,
                forced,
                place,
                rewrite,
            });
        }
    };
    for t in tokens {
        match t {
            Token::Tag {
                name: tag,
                kind,
                range,
                attrs,
            } => {
                let l = xml::local(tag);
                let parent = stack.last().map(|e| e.0.as_str()).unwrap_or_default();
                // A value of a pivot cache: under its field's name.
                let mut context = String::new();
                if pivot && *kind != TagKind::End {
                    if l == "cacheField" {
                        field = attrs
                            .iter()
                            .find(|(a, _)| a == "name")
                            .map(|(_, v)| xml::unescape(&s[v.clone()], 0).0)
                            .unwrap_or_default();
                    }
                    if l == "r" {
                        in_record = 0;
                    } else if parent == "r" {
                        context = fields
                            .and_then(|f| f.get(in_record))
                            .cloned()
                            .unwrap_or_default();
                        in_record += 1;
                    } else if fields.is_none() {
                        context = field.clone();
                    }
                }
                for (a, v) in attrs {
                    let al = xml::local(a);
                    let value = &s[v.clone()];
                    // A table's display name is no person's: it is read
                    // where tables are named (`table_names`).
                    let forced = if (crate::patch::on("office-person-metadata")
                        && PERSON_ATTRS.contains(&al)
                        && !named_elsewhere(name, l, al))
                        || (comment_authors && al == "name")
                    {
                        Some("PERSON")
                    } else {
                        None
                    };
                    let external = al == "Target"
                        && attrs.iter().any(|(k, v2)| {
                            xml::local(k) == "TargetMode" && &s[v2.clone()] == "External"
                        });
                    let read = forced.is_some()
                        || external
                        || (!structural(a)
                            && !named_elsewhere(name, l, al)
                            // A record's index into its field's items.
                            && !(pivot && l == "x")
                            && value.chars().any(char::is_alphanumeric));
                    if !read {
                        continue;
                    }
                    let (text, _) = xml::unescape(value, v.start);
                    let pivot_value = pivot && al == "v";
                    // An OLAP item with children (`<n v="1"><x/></n>`) keeps
                    // its name: its end tag would not follow.
                    let rewrite = (pivot_value
                        && matches!(l, "n" | "d")
                        && *kind == TagKind::Empty)
                        .then(|| {
                            let at = range.start + 1;
                            let local = at + tag.len() - l.len();
                            Rewrite::Retag(local..at + tag.len())
                        });
                    push(
                        &mut out,
                        vec![Piece {
                            raw: Some(v.clone()),
                            text,
                            space_tag: None,
                        }],
                        forced,
                        if pivot_value {
                            context.clone()
                        } else {
                            String::new()
                        },
                        rewrite,
                    );
                }
                match kind {
                    TagKind::Start => {
                        if PARAGRAPHS.contains(&l) {
                            open.push(Vec::new());
                        }
                        if DELETIONS.contains(&l)
                            && let Some(p) = open.last_mut()
                        {
                            p.push(separator());
                        }
                        let space =
                            !attrs.iter().any(|(a, _)| a == "xml:space") && tag.starts_with("w:");
                        stack.push((l.to_owned(), Some(range.clone()), space));
                    }
                    TagKind::Empty => {
                        if let Some(p) = open.last_mut()
                            && matches!(l, "br" | "cr" | "tab")
                        {
                            p.push(Piece {
                                raw: None,
                                text: if l == "tab" { "\t".into() } else { "\n".into() },
                                space_tag: None,
                            });
                        }
                    }
                    TagKind::End => {
                        stack.pop();
                        if DELETIONS.contains(&l)
                            && let Some(p) = open.last_mut()
                        {
                            p.push(separator());
                        }
                        if PARAGRAPHS.contains(&l)
                            && let Some(pieces) = open.pop()
                        {
                            push(&mut out, pieces, None, String::new(), None);
                        }
                    }
                }
            }
            Token::Text(r) | Token::CData(r) => {
                let raw = &s[r.clone()];
                if raw.trim().is_empty() && open.is_empty() {
                    continue;
                }
                let (text, _) = if matches!(t, Token::CData(_)) {
                    (raw.to_owned(), Vec::new())
                } else {
                    xml::unescape(raw, r.start)
                };
                let (element, start_tag, space) = stack.last().cloned().unwrap_or_default();
                let forced = if custom && stack.iter().any(|(e, _, _)| e == "property") {
                    Some("METADATA")
                } else if crate::patch::on("office-person-metadata")
                    && PERSON_ELEMENTS.contains(&element.as_str())
                {
                    Some("PERSON")
                } else if element == "Company" {
                    Some("ORGANIZATION")
                } else {
                    None
                };
                let piece = Piece {
                    raw: Some(r.clone()),
                    text,
                    space_tag: if space && TEXTS.contains(&element.as_str()) {
                        start_tag
                    } else {
                        None
                    },
                };
                match open.last_mut() {
                    Some(p) if TEXTS.contains(&element.as_str()) && forced.is_none() => {
                        p.push(piece)
                    }
                    // Whitespace between runs belongs to no text element.
                    Some(_) if raw.trim().is_empty() => {}
                    _ => push(&mut out, vec![piece], forced, String::new(), None),
                }
            }
        }
    }
    Ok(out)
}

/// Where a segment had findings: its first byte, and whether its element
/// becomes text.
type FoundAt = Vec<(usize, bool)>;

/// Redacts one XML part; the new text if it changed, and where it had
/// findings.
fn part(
    engine: &Engine,
    name: &str,
    s: &str,
    fields: Option<&[String]>,
    mut edits: Vec<(Range<usize>, String)>,
    apply: bool,
    out: &mut Outcome,
) -> Result<(Option<String>, FoundAt), Error> {
    let mut found_at = Vec::new();
    for seg in segments(name, s, fields)? {
        if !edit_segment(engine, &seg, apply, out, &mut edits)?.is_empty()
            && let Some(at) = seg.pieces.iter().find_map(|p| p.raw.as_ref())
        {
            found_at.push((at.start, matches!(seg.rewrite, Some(Rewrite::Retag(_)))));
        }
    }
    Ok((apply_edits(s, edits), found_at))
}

/// What became of a pivot cache's field (#157).
#[derive(Default, Clone, Copy)]
struct FieldChange {
    /// A value of it held a finding.
    found: bool,
    /// A number or date of it became text.
    retagged: bool,
    /// A number or date of it is left.
    numbers_left: bool,
}

/// Per field of a pivot cache, by its place.
type FieldChanges = BTreeMap<usize, FieldChange>;

/// Adds to `changes` what became of the values of a pivot cache's part,
/// its findings at `found_at`: in its records, a value's field is its
/// place in its record; in its definition, the `cacheField` that holds it.
fn note_fields(
    s: &str,
    records: bool,
    found_at: &[(usize, bool)],
    changes: &mut FieldChanges,
) -> Result<(), Error> {
    // Each value: its tag, its field, whether it is a number or a date.
    let mut values: Vec<(Range<usize>, usize, bool)> = Vec::new();
    let (mut in_record, mut field) = (0usize, 0usize);
    let mut parent: Vec<String> = Vec::new();
    for t in xml::tokenize(s)? {
        let Token::Tag {
            name, kind, range, ..
        } = t
        else {
            continue;
        };
        let l = xml::local(&name).to_owned();
        let under = |p: &str| parent.last().is_some_and(|q| q == p);
        if records && l == "r" {
            in_record = 0;
        } else if records && under("r") && kind != TagKind::End {
            values.push((range.clone(), in_record, matches!(l.as_str(), "n" | "d")));
            in_record += 1;
        } else if !records && under("sharedItems") && kind != TagKind::End {
            values.push((range.clone(), field, matches!(l.as_str(), "n" | "d")));
        } else if !records && l == "cacheField" && kind != TagKind::Start {
            field += 1;
        }
        match kind {
            TagKind::Start => parent.push(l),
            TagKind::End => {
                parent.pop();
            }
            TagKind::Empty => {}
        }
    }
    for (tag, f, number) in values {
        let found = found_at
            .iter()
            .find(|(at, _)| tag.start <= *at && *at < tag.end);
        let e = changes.entry(f).or_default();
        e.found |= found.is_some();
        let retagged = found.is_some_and(|(_, r)| *r);
        e.retagged |= retagged;
        e.numbers_left |= number && !retagged;
    }
    Ok(())
}

/// A pivot cache's definition `s` with its fields' `sharedItems` made to
/// match what became of their values (#157): a field with a finding loses
/// its smallest and largest value (they are its values), and one whose
/// numbers or dates became text no longer says it holds no text; where
/// numbers or dates are left it holds mixed types, where none are it no
/// longer says it holds any (Excel rejects a cache that does).
fn shared_items(s: &str, changes: &FieldChanges) -> Result<Option<String>, Error> {
    let mut edits = Vec::new();
    let mut field = 0usize;
    let mut in_field = false;
    for t in xml::tokenize(s)? {
        let Token::Tag {
            name,
            kind,
            range,
            attrs,
        } = t
        else {
            continue;
        };
        let l = xml::local(&name);
        if l == "cacheField" {
            match kind {
                TagKind::Start => in_field = true,
                TagKind::Empty => field += 1,
                TagKind::End => {
                    in_field = false;
                    field += 1;
                }
            }
            continue;
        }
        if !(in_field && l == "sharedItems" && kind != TagKind::End) {
            continue;
        }
        let Some(&FieldChange {
            found,
            retagged,
            numbers_left,
        }) = changes.get(&field)
        else {
            continue;
        };
        let mixed = retagged && numbers_left;
        let mut tag = format!("<{name}");
        for (a, v) in &attrs {
            let gone = (found
                && matches!(a.as_str(), "minValue" | "maxValue" | "minDate" | "maxDate"))
                || (retagged
                    && matches!(
                        a.as_str(),
                        "containsSemiMixedTypes" | "containsString" | "containsNonDate"
                    )
                    && &s[v.clone()] == "0")
                || (retagged && a == "containsMixedTypes")
                || (retagged
                    && !numbers_left
                    && matches!(
                        a.as_str(),
                        "containsNumber" | "containsInteger" | "containsDate"
                    ));
            if !gone {
                tag.push_str(&format!(" {a}=\"{}\"", &s[v.clone()]));
            }
        }
        if mixed {
            tag.push_str(" containsMixedTypes=\"1\"");
        }
        tag.push_str(if kind == TagKind::Empty { "/>" } else { ">" });
        edits.push((range, tag));
    }
    Ok(apply_edits(s, edits))
}

/// Finds in a segment, reports, and (when applying) adds the edits.
pub(crate) fn edit_segment(
    engine: &Engine,
    seg: &Segment,
    apply: bool,
    out: &mut Outcome,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<Vec<Candidate>, Error> {
    let text = seg.text();
    let mut found: Vec<Candidate> = match seg.forced {
        Some(entity) if !text.trim().is_empty() => vec![Candidate {
            start: 0,
            end: text.len(),
            entity: EntityType::new(entity).expect("a valid type"),
            confidence: 1.0,
            detector: "metadata".into(),
        }],
        Some(_) => Vec::new(),
        None => engine.findings(&text, &seg.context)?,
    };
    if found.is_empty() {
        return Ok(found);
    }
    let pseudonyms = if apply {
        let (pieces, pseudonyms) =
            engine.replace(&text, &seg.context, &mut found, &mut out.mapping)?;
        let replacements: Vec<String> = pieces
            .into_iter()
            .filter(|(_, r)| *r)
            .map(|(p, _)| p)
            .collect();
        distribute(seg, &found, &replacements, edits)?;
        pseudonyms
    } else {
        vec![None; found.len()]
    };
    for (c, pseudonym) in found.iter().zip(pseudonyms) {
        out.findings.push(Finding {
            entity: c.entity.clone(),
            location: location(seg, &text, c),
            confidence: c.confidence,
            detector: c.detector.clone(),
            pseudonym,
        });
    }
    Ok(found)
}

fn location(seg: &Segment, text: &str, c: &Candidate) -> Location {
    match &seg.place {
        Place::Part { part, n } => Location::Span {
            part: Some(part.clone()),
            start: c.start as u64,
            end: c.end as u64,
            line: *n,
            column: text[..c.start].chars().count() as u64 + 1,
        },
        Place::Cell { sheet, row, column } => Location::Cell {
            sheet: Some(sheet.clone()),
            row: *row,
            column: *column,
            start: c.start as u64,
            end: c.end as u64,
        },
    }
}

/// Spreads the replacements over the segment's pieces: each one into the
/// piece where its finding starts, the covered text out of every piece.
fn distribute(
    seg: &Segment,
    found: &[Candidate],
    replacements: &[String],
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), Error> {
    if let Some(Rewrite::Retag(name)) = &seg.rewrite {
        // A pivot cache's number or date that becomes text.
        edits.push((name.clone(), "s".into()));
    }
    if let Some(Rewrite::Cell(cell)) = &seg.rewrite {
        // A cell value that becomes an inline string.
        let text = seg.text();
        let mut new = String::new();
        let mut at = 0;
        for (c, r) in found.iter().zip(replacements) {
            new.push_str(&text[at..c.start]);
            new.push_str(r);
            at = c.end;
        }
        new.push_str(&text[at..]);
        let style = cell
            .style
            .as_ref()
            .map(|s| format!(" s=\"{s}\""))
            .unwrap_or_default();
        edits.push((
            cell.element.clone(),
            format!(
                "<c r=\"{}\"{style} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                cell.reference,
                xml::escape(&new, false)
            ),
        ));
        return Ok(());
    }
    let mut pending = String::new();
    let mut offset = 0;
    for p in &seg.pieces {
        let (a, b) = (offset, offset + p.text.len());
        offset = b;
        let mut new = std::mem::take(&mut pending);
        let mut pos = a;
        while pos < b {
            if let Some((k, c)) = found.iter().enumerate().find(|(_, c)| c.start == pos) {
                new.push_str(&replacements[k]);
                pos = c.end;
                continue;
            }
            if found.iter().any(|c| c.start < pos && pos < c.end) {
                pos += p.text[pos - a..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            let ch = p.text[pos - a..].chars().next().expect("on a boundary");
            new.push(ch);
            pos += ch.len_utf8();
        }
        match &p.raw {
            None => {
                // A break between runs is read, not written: what goes in
                // its place goes into the next piece of text.
                let kept = new
                    .strip_prefix(p.text.as_str())
                    .map(str::to_owned)
                    .unwrap_or(new);
                pending.push_str(&kept);
            }
            Some(raw) => {
                if new != p.text {
                    // Quotes escaped too: the text may be an attribute's.
                    edits.push((raw.clone(), xml::escape(&new, true)));
                    if let Some(tag) = &p.space_tag
                        && (new.starts_with(char::is_whitespace)
                            || new.ends_with(char::is_whitespace))
                    {
                        edits.push((tag.end - 1..tag.end - 1, " xml:space=\"preserve\"".into()));
                    }
                }
            }
        }
    }
    if !pending.is_empty() {
        return Err(error("a replacement had no text to go into"));
    }
    Ok(())
}

/// `s` with the edits (sorted, not overlapping) applied; `None` without any.
pub(crate) fn apply_edits(s: &str, mut edits: Vec<(Range<usize>, String)>) -> Option<String> {
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    for (r, new) in edits {
        if r.start < at {
            continue;
        }
        out.push_str(&s[at..r.start]);
        out.push_str(&new);
        at = r.end;
    }
    out.push_str(&s[at..]);
    Some(out)
}

// ------------------------------------------------------------------ XLSX

/// Every text of a workbook that may name a sheet: formulas of cells,
/// defined names, data validations, conditional formats and charts, a
/// pivot cache's source, a hyperlink's place.
fn formula_texts(entries: &[(String, Vec<u8>, CompressionMethod)]) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    for (name, data, _) in entries {
        if !name.starts_with("xl/") || !name.ends_with(".xml") {
            continue;
        }
        let Ok(s) = std::str::from_utf8(data) else {
            continue;
        };
        let mut element = String::new();
        for t in xml::tokenize(s)? {
            match t {
                Token::Tag {
                    name, kind, attrs, ..
                } => {
                    let l = xml::local(&name);
                    if kind == TagKind::Start {
                        element = l.to_owned();
                    }
                    for (a, v) in &attrs {
                        let al = xml::local(a);
                        if (l == "worksheetSource" && al == "sheet")
                            || (l == "hyperlink" && al == "location")
                            || (matches!(l, "cacheField" | "calculatedItem") && al == "formula")
                        {
                            out.push(xml::unescape(&s[v.clone()], 0).0);
                        }
                    }
                }
                Token::Text(r) => {
                    if matches!(
                        element.as_str(),
                        "f" | "definedName"
                            | "formula"
                            | "formula1"
                            | "formula2"
                            | "calculatedColumnFormula"
                            | "totalsRowFormula"
                    ) {
                        out.push(xml::unescape(&s[r], 0).0);
                    }
                }
                Token::CData(_) => {}
            }
        }
    }
    Ok(out)
}

/// Edits by part.
type PartEdits = HashMap<String, Vec<(Range<usize>, String)>>;

/// The names of tables, their columns and pivot caches' fields (#156):
/// formulas use them (`Tabelle1[@[Spalte]]`), and each must stay apart from
/// the others of its table or cache. One that holds personal data is an
/// error when a formula names it. A column's or field's name is otherwise
/// replaced as any attribute, unless it would become the same as another
/// (an error); a table's name, a name Excel allows only some characters
/// in, is renamed as a sheet is ([`table_name`]). The edits to the tables.
fn table_names(
    engine: &Engine,
    entries: &[(String, Vec<u8>, CompressionMethod)],
    apply: bool,
    out: &mut Outcome,
) -> Result<PartEdits, Error> {
    // Each group: its part, what its names are, and the names.
    let mut groups: Vec<(String, &'static str, Vec<String>)> = Vec::new();
    // Each table's name and display name: its part, the value's range, the
    // name.
    let mut tables: Vec<(String, Range<usize>, String)> = Vec::new();
    for (name, data, _) in entries {
        let (columns, what) = if name.starts_with("xl/tables/") && name.ends_with(".xml") {
            ("tableColumn", "a table column's name")
        } else if name.starts_with("xl/pivotCache/pivotCacheDefinition") && name.ends_with(".xml") {
            ("cacheField", "a pivot field's name")
        } else {
            continue;
        };
        let s = String::from_utf8_lossy(data);
        let mut names = Vec::new();
        for t in xml::tokenize(&s)? {
            let Token::Tag {
                name: tag,
                kind,
                attrs,
                ..
            } = t
            else {
                continue;
            };
            let l = xml::local(&tag);
            if kind == TagKind::End || !matches!(l, "table" | "tableColumn" | "cacheField") {
                continue;
            }
            let value = |k: &str| {
                attrs
                    .iter()
                    .find(|(a, _)| a == k)
                    .map(|(_, v)| xml::unescape(&s[v.clone()], 0).0)
            };
            if l == "table" {
                for (a, v) in &attrs {
                    if matches!(a.as_str(), "name" | "displayName") {
                        tables.push((name.clone(), v.clone(), xml::unescape(&s[v.clone()], 0).0));
                    }
                }
            } else if l == columns {
                names.extend(value("name"));
            }
        }
        groups.push((name.clone(), what, names));
    }
    let mut formulas: Option<Vec<String>> = None;
    // Tables: renamed where their name holds personal data.
    let mut taken: Vec<String> = tables.iter().map(|t| t.2.to_lowercase()).collect();
    let mut renamed: HashMap<String, String> = HashMap::new();
    let mut edits = PartEdits::new();
    for (n, (part, range, text)) in tables.iter().enumerate() {
        let new = match renamed.get(text) {
            Some(new) => new.clone(),
            None => {
                let mut found = engine.findings(text, "")?;
                let Some(first) = found.first() else {
                    continue;
                };
                let entity = first.entity.as_str().to_owned();
                let formulas = match &mut formulas {
                    Some(f) => f,
                    None => formulas.insert(formula_texts(entries)?),
                };
                let lower = text.to_lowercase();
                if formulas.iter().any(|f| f.to_lowercase().contains(&lower)) {
                    return Err(error(format!(
                        "{part}: a table's name holds {entity}, and a formula names it; veilq does not rewrite formulas"
                    )));
                }
                let (replaced, pseudonyms) = if apply {
                    let (pieces, p) = engine.replace(text, "", &mut found, &mut out.mapping)?;
                    (pieces.into_iter().map(|(p, _)| p).collect::<String>(), p)
                } else {
                    let s = found
                        .iter()
                        .map(|c| c.entity.as_str())
                        .collect::<Vec<_>>()
                        .join("_");
                    (s, vec![None; found.len()])
                };
                let seg = Segment {
                    pieces: Vec::new(),
                    context: String::new(),
                    forced: None,
                    place: Place::Part {
                        part: part.clone(),
                        n: n as u64 + 1,
                    },
                    rewrite: None,
                };
                for (c, pseudonym) in found.iter().zip(pseudonyms) {
                    out.findings.push(Finding {
                        entity: c.entity.clone(),
                        location: location(&seg, text, c),
                        confidence: c.confidence,
                        detector: c.detector.clone(),
                        pseudonym,
                    });
                }
                let new = table_name(&replaced, &taken);
                taken.push(new.to_lowercase());
                renamed.insert(text.clone(), new.clone());
                new
            }
        };
        edits
            .entry(part.clone())
            .or_default()
            .push((range.clone(), xml::escape(&new, true)));
    }
    for (part, what, names) in groups {
        let mut seen: Vec<String> = Vec::new();
        for text in names {
            let mut found = engine.findings(&text, "")?;
            if found.is_empty() {
                seen.push(text.to_lowercase());
                continue;
            }
            let entity = found[0].entity.as_str().to_owned();
            let formulas = match &mut formulas {
                Some(f) => f,
                None => formulas.insert(formula_texts(entries)?),
            };
            let lower = text.to_lowercase();
            if formulas.iter().any(|f| f.to_lowercase().contains(&lower)) {
                return Err(error(format!(
                    "{part}: {what} holds {entity}, and a formula names it; veilq does not rewrite formulas"
                )));
            }
            if apply {
                let (pieces, _) = engine.replace(&text, "", &mut found, &mut Vec::new())?;
                let new = pieces
                    .into_iter()
                    .map(|(p, _)| p)
                    .collect::<String>()
                    .to_lowercase();
                if seen.contains(&new) {
                    return Err(error(format!(
                        "{part}: {what} holds {entity}, and replaced it would become the same as another; veilq does not rename them apart"
                    )));
                }
                seen.push(new);
            }
        }
    }
    Ok(edits)
}

/// A table's new name: what Excel allows (letters, digits, `_` and `.`, a
/// letter or `_` first, no cell reference such as `A1`), and not one the
/// workbook has.
fn table_name(replaced: &str, taken: &[String]) -> String {
    let mut base = String::new();
    for c in replaced.chars() {
        let c = if c.is_alphanumeric() || c == '.' {
            c
        } else {
            '_'
        };
        if !(c == '_' && base.ends_with('_')) {
            base.push(c);
        }
    }
    if !base.starts_with(|c: char| c.is_alphabetic() || c == '_') || cell_ref(&base).is_some() {
        base.insert(0, '_');
    }
    let base: String = base.chars().take(240).collect();
    let mut name = base.clone();
    let mut k = 2;
    while taken.contains(&name.to_lowercase()) {
        name = format!("{base}_{k}");
        k += 1;
    }
    name
}

/// Whether a formula (or a pivot cache's source sheet) names `sheet`.
fn names_sheet(formula: &str, sheet: &str) -> bool {
    let f = formula.to_lowercase();
    let s = sheet.to_lowercase();
    f == s || f.contains(&format!("{s}!")) || f.contains(&format!("'{}'!", s.replace('\'', "''")))
}

/// A sheet's new name: what Excel allows (no `\ / ? * [ ] :`, at most 31
/// characters), and not one the workbook has.
fn sheet_name(replaced: &str, taken: &[String]) -> String {
    let clean: String = replaced
        .chars()
        .filter(|c| !matches!(c, '[' | ']'))
        .map(|c| {
            if matches!(c, '\\' | '/' | '?' | '*' | ':') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('\'');
    let base: String = clean.chars().take(31).collect();
    let mut name = base.clone();
    let mut k = 2;
    while taken.contains(&name.to_lowercase()) {
        let suffix = format!(" ({k})");
        name = base
            .chars()
            .take(31 - suffix.chars().count())
            .collect::<String>()
            + &suffix;
        k += 1;
    }
    name
}

/// A cell reference `B12` as (row, column).
fn cell_ref(r: &str) -> Option<(u64, u64)> {
    let letters: String = r.chars().take_while(char::is_ascii_alphabetic).collect();
    let row: u64 = r[letters.len()..].parse().ok()?;
    let col = letters.bytes().fold(0u64, |a, c| {
        a * 26 + u64::from(c.to_ascii_uppercase() - b'A' + 1)
    });
    Some((row, col))
}

/// A cell that shows a shared string: sheet, row, column, its column's name.
type Shown = (String, u64, u64, String);

/// What the workbook says: sheet names by part, and the shared strings.
#[derive(Default)]
struct Workbook {
    sheets: BTreeMap<String, String>,
    shared: Vec<Segment>,
    /// Per shared string: the cells that show it and their column's name.
    uses: std::cell::RefCell<HashMap<usize, Vec<Shown>>>,
}

impl Workbook {
    fn read(entries: &[(String, Vec<u8>, CompressionMethod)]) -> Result<Self, Error> {
        let get = |n: &str| {
            entries
                .iter()
                .find(|e| e.0 == n)
                .map(|e| String::from_utf8_lossy(&e.1).into_owned())
        };
        let Some(wb) = get("xl/workbook.xml") else {
            return Ok(Self::default());
        };
        let rels = get("xl/_rels/workbook.xml.rels").unwrap_or_default();
        let mut targets = HashMap::new();
        for t in xml::tokenize(&rels)? {
            if let Token::Tag { attrs, .. } = t {
                let a = |k: &str| {
                    attrs
                        .iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, r)| rels[r.clone()].to_owned())
                };
                if let (Some(id), Some(target)) = (a("Id"), a("Target")) {
                    let path = target.trim_start_matches('/');
                    let path = if path.starts_with("xl/") {
                        path.to_owned()
                    } else {
                        format!("xl/{path}")
                    };
                    targets.insert(id, path);
                }
            }
        }
        let mut sheets = BTreeMap::new();
        for t in xml::tokenize(&wb)? {
            if let Token::Tag { name, attrs, .. } = t
                && xml::local(&name) == "sheet"
            {
                let a = |k: &str| {
                    attrs
                        .iter()
                        .find(|(n, _)| xml::local(n) == k)
                        .map(|(_, r)| xml::unescape(&wb[r.clone()], 0).0)
                };
                if let (Some(n), Some(id)) = (a("name"), a("id"))
                    && let Some(p) = targets.get(&id)
                {
                    sheets.insert(p.clone(), n);
                }
            }
        }
        // Shared strings are counted by `si`, including empty ones.
        let shared = Self::index_shared(get("xl/sharedStrings.xml").as_deref())?;
        Ok(Self {
            sheets,
            shared,
            uses: Default::default(),
        })
    }

    /// The names in `xl/workbook.xml` that formulas use (ADR-0063): a sheet
    /// whose name holds personal data is renamed, unless a formula names
    /// the sheet; a defined name that holds some is an error. A renamed
    /// sheet is reported under its new name. The edits to the workbook.
    fn names(
        &mut self,
        engine: &Engine,
        entries: &[(String, Vec<u8>, CompressionMethod)],
        apply: bool,
        out: &mut Outcome,
    ) -> Result<Vec<(Range<usize>, String)>, Error> {
        let Some(entry) = entries.iter().find(|e| e.0 == "xl/workbook.xml") else {
            return Ok(Vec::new());
        };
        let wb =
            std::str::from_utf8(&entry.1).map_err(|_| error("xl/workbook.xml is not UTF-8 XML"))?;
        let mut taken: Vec<String> = self.sheets.values().map(|n| n.to_lowercase()).collect();
        let mut formulas: Option<Vec<String>> = None;
        let mut edits = Vec::new();
        let mut n = 0;
        for t in xml::tokenize(wb)? {
            let Token::Tag { name, attrs, .. } = t else {
                continue;
            };
            let l = xml::local(&name);
            if !matches!(l, "sheet" | "definedName") {
                continue;
            }
            let Some((_, v)) = attrs.iter().find(|(a, _)| a == "name") else {
                continue;
            };
            n += 1;
            let (text, _) = xml::unescape(&wb[v.clone()], 0);
            let mut found = engine.findings(&text, "")?;
            let Some(first) = found.first() else {
                continue;
            };
            let what = first.entity.as_str().to_owned();
            if l == "definedName" {
                return Err(error(format!(
                    "xl/workbook.xml: a defined name holds {what}; veilq does not rename what formulas use"
                )));
            }
            let formulas = match &mut formulas {
                Some(f) => f,
                None => formulas.insert(formula_texts(entries)?),
            };
            if formulas.iter().any(|f| names_sheet(f, &text)) {
                return Err(error(format!(
                    "xl/workbook.xml: a sheet's name holds {what}, and a formula names the sheet; veilq does not rewrite formulas"
                )));
            }
            let pseudonyms;
            let replaced = if apply {
                let (pieces, p) = engine.replace(&text, "", &mut found, &mut out.mapping)?;
                pseudonyms = p;
                pieces.into_iter().map(|(p, _)| p).collect::<String>()
            } else {
                pseudonyms = vec![None; found.len()];
                let mut s = String::new();
                let mut at = 0;
                for c in &found {
                    s.push_str(&text[at..c.start]);
                    s.push_str(&format!("<{}>", c.entity.as_str()));
                    at = c.end;
                }
                s + &text[at..]
            };
            let new = sheet_name(&replaced, &taken);
            taken.push(new.to_lowercase());
            let seg = Segment {
                pieces: Vec::new(),
                context: String::new(),
                forced: None,
                place: Place::Part {
                    part: "xl/workbook.xml".into(),
                    n,
                },
                rewrite: None,
            };
            for (c, pseudonym) in found.iter().zip(pseudonyms) {
                out.findings.push(Finding {
                    entity: c.entity.clone(),
                    location: location(&seg, &text, c),
                    confidence: c.confidence,
                    detector: c.detector.clone(),
                    pseudonym,
                });
            }
            for shown in self.sheets.values_mut() {
                if *shown == text {
                    shown.clone_from(&new);
                }
            }
            edits.push((v.clone(), xml::escape(&new, true)));
        }
        Ok(edits)
    }

    /// The shared strings in `si` order (an empty one as an empty segment).
    fn index_shared(text: Option<&str>) -> Result<Vec<Segment>, Error> {
        let Some(s) = text else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        let mut current: Option<Vec<Piece>> = None;
        let mut element = String::new();
        for t in xml::tokenize(s)? {
            match t {
                Token::Tag { name, kind, .. } => {
                    let l = xml::local(&name).to_owned();
                    match (kind, l.as_str()) {
                        (TagKind::Start, "si") => current = Some(Vec::new()),
                        (TagKind::Empty, "si") => out.push(Vec::new()),
                        (TagKind::End, "si") => out.push(current.take().unwrap_or_default()),
                        // Phonetic hints repeat the text; read, not written separately.
                        (TagKind::Start, _) => element = l,
                        _ => {}
                    }
                }
                Token::Text(r) => {
                    if let Some(p) = current.as_mut()
                        && element == "t"
                    {
                        let (text, _) = xml::unescape(&s[r.clone()], r.start);
                        p.push(Piece {
                            raw: Some(r),
                            text,
                            space_tag: None,
                        });
                    }
                }
                Token::CData(_) => {}
            }
        }
        Ok(out
            .into_iter()
            .enumerate()
            .map(|(i, pieces)| Segment {
                pieces,
                context: String::new(),
                forced: None,
                place: Place::Part {
                    part: "xl/sharedStrings.xml".into(),
                    n: i as u64 + 1,
                },
                rewrite: None,
            })
            .collect())
    }

    fn is_sheet(&self, name: &str) -> bool {
        self.sheets.contains_key(name)
    }

    fn shared_text(&self, i: usize) -> String {
        self.shared.get(i).map(Segment::text).unwrap_or_default()
    }

    /// A worksheet: inline strings, formula results and numbers with their
    /// column's name as context; shared strings are noted for later.
    fn sheet(
        &self,
        engine: &Engine,
        name: &str,
        s: &str,
        apply: bool,
        out: &mut Outcome,
    ) -> Result<Option<String>, Error> {
        let sheet = self
            .sheets
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned());
        let tokens = xml::tokenize(s)?;
        // Pass 1: the header row.
        let mut header: HashMap<u64, String> = HashMap::new();
        let cells = cells(s, &tokens);
        for c in &cells {
            if c.row == 1 {
                let text = match c.t.as_deref() {
                    Some("s") => c
                        .value
                        .as_ref()
                        .and_then(|v| v.1.trim().parse().ok())
                        .map(|i| self.shared_text(i)),
                    _ => c
                        .inline
                        .as_ref()
                        .map(|p| p.iter().map(|x| x.text.as_str()).collect())
                        .or(c.value.as_ref().map(|v| v.1.clone())),
                };
                header.insert(c.column, text.unwrap_or_default());
            }
        }
        let mut edits = Vec::new();
        for c in cells {
            let context = if c.row == 1 {
                String::new()
            } else {
                header.get(&c.column).cloned().unwrap_or_default()
            };
            let place = Place::Cell {
                sheet: sheet.clone(),
                row: c.row,
                column: c.column,
            };
            if let Some(f) = &c.formula {
                let found = engine.findings(&f.1, "")?;
                if !found.is_empty() {
                    return Err(error(format!(
                        "{sheet}!{}: a formula holds {}; veilq does not rewrite formulas",
                        c.reference,
                        found[0].entity.as_str()
                    )));
                }
            }
            match (c.t.as_deref(), &c.value, &c.inline) {
                (Some("s"), Some(v), _) => {
                    if let Ok(i) = v.1.trim().parse::<usize>() {
                        self.uses.borrow_mut().entry(i).or_default().push((
                            sheet.clone(),
                            c.row,
                            c.column,
                            context,
                        ));
                    }
                }
                (Some("inlineStr"), _, Some(pieces)) => {
                    let seg = Segment {
                        pieces: pieces.clone(),
                        context,
                        forced: None,
                        place,
                        rewrite: None,
                    };
                    edit_segment(engine, &seg, apply, out, &mut edits)?;
                }
                (Some("str"), Some(v), _) => {
                    let seg = Segment {
                        pieces: vec![Piece {
                            raw: Some(v.0.clone()),
                            text: v.1.clone(),
                            space_tag: None,
                        }],
                        context,
                        forced: None,
                        place,
                        rewrite: None,
                    };
                    edit_segment(engine, &seg, apply, out, &mut edits)?;
                }
                (None | Some("n") | Some("d"), Some(v), _) => {
                    // A number or date that is personal data becomes text; a
                    // formula would compute it again: an error.
                    if c.formula.is_some() {
                        if let Some(f) = engine.findings(&v.1, &context)?.first() {
                            return Err(error(format!(
                                "{sheet}!{}: a formula's result is {}; veilq does not rewrite formulas",
                                c.reference,
                                f.entity.as_str()
                            )));
                        }
                        continue;
                    }
                    let seg = Segment {
                        pieces: vec![Piece {
                            raw: Some(v.0.clone()),
                            text: v.1.clone(),
                            space_tag: None,
                        }],
                        context,
                        forced: None,
                        place,
                        rewrite: Some(Rewrite::Cell(CellRewrite {
                            element: c.element.clone(),
                            reference: c.reference.clone(),
                            style: c.style.clone(),
                        })),
                    };
                    edit_segment(engine, &seg, apply, out, &mut edits)?;
                }
                _ => {}
            }
        }
        // Everything outside the cells: headers and footers, data
        // validations and their prompts, hyperlinks' tooltips (ADR-0063).
        // The cells were read above: their tokens are left out (#158).
        for seg in segments_of(name, s, &tokens, None, sheet_data(&tokens))? {
            edit_segment(engine, &seg, apply, out, &mut edits)?;
        }
        Ok(apply_edits(s, edits))
    }

    /// The shared strings: each one read in every context it is shown in
    /// (its cells' column names, and none), the union of the findings
    /// replaced once.
    fn shared_strings(
        &self,
        engine: &Engine,
        s: &str,
        apply: bool,
        out: &mut Outcome,
    ) -> Result<Option<String>, Error> {
        let uses = self.uses.borrow();
        let mut edits = Vec::new();
        for (i, seg) in self.shared.iter().enumerate() {
            let text = seg.text();
            if text.trim().is_empty() {
                continue;
            }
            let shown: Vec<Shown> = uses.get(&i).cloned().unwrap_or_default();
            let mut contexts: Vec<String> = shown.iter().map(|u| u.3.clone()).collect();
            contexts.push(String::new());
            contexts.sort();
            contexts.dedup();
            // Of overlapping findings of the readings the longest, then the
            // most confident: a full name under its column, not the words a
            // file's names (ADR-0047) find in it without one. Of two alike,
            // the one a detector found itself, not the file's list
            // repeating it (ADR-0059).
            // A candidate the decider lifts counts once, where it is
            // written, not once per reading (#129).
            let mut every: Vec<(Candidate, bool)> = Vec::new();
            for ctx in &contexts {
                every.extend(engine.findings_uncounted(&text, ctx)?);
            }
            let listed = |c: &Candidate| {
                c.detector == crate::names::DETECTOR || c.detector == crate::names::VALUES_DETECTOR
            };
            every.sort_by(|(a, _), (b, _)| {
                (b.end - b.start)
                    .cmp(&(a.end - a.start))
                    .then(b.confidence.total_cmp(&a.confidence))
                    .then(listed(a).cmp(&listed(b)))
                    .then(a.start.cmp(&b.start))
            });
            let mut all: Vec<Candidate> = Vec::new();
            let mut lifted = 0;
            for (c, l) in every {
                if all.iter().all(|a| c.end <= a.start || c.start >= a.end) {
                    lifted += usize::from(l);
                    all.push(c);
                }
            }
            engine.count_lifted(lifted);
            if all.is_empty() {
                continue;
            }
            all.sort_by_key(|c| c.start);
            let context = contexts
                .iter()
                .find(|c| !c.is_empty())
                .cloned()
                .unwrap_or_default();
            let pseudonyms = if apply {
                let (pieces, pseudonyms) =
                    engine.replace(&text, &context, &mut all, &mut out.mapping)?;
                let replacements: Vec<String> = pieces
                    .into_iter()
                    .filter(|(_, r)| *r)
                    .map(|(p, _)| p)
                    .collect();
                distribute(seg, &all, &replacements, &mut edits)?;
                pseudonyms
            } else {
                vec![None; all.len()]
            };
            let places: Vec<Place> = if shown.is_empty() {
                vec![seg.place.clone()]
            } else {
                shown
                    .iter()
                    .map(|u| Place::Cell {
                        sheet: u.0.clone(),
                        row: u.1,
                        column: u.2,
                    })
                    .collect()
            };
            for place in places {
                let at = Segment {
                    place,
                    ..seg.clone()
                };
                for (c, p) in all.iter().zip(&pseudonyms) {
                    out.findings.push(Finding {
                        entity: c.entity.clone(),
                        location: location(&at, &text, c),
                        confidence: c.confidence,
                        detector: c.detector.clone(),
                        pseudonym: p.clone(),
                    });
                }
            }
        }
        Ok(apply_edits(s, edits))
    }
}

struct Cell {
    element: Range<usize>,
    reference: String,
    row: u64,
    column: u64,
    t: Option<String>,
    style: Option<String>,
    /// `<v>`: its raw range and decoded text.
    value: Option<(Range<usize>, String)>,
    formula: Option<(Range<usize>, String)>,
    inline: Option<Vec<Piece>>,
}

/// Where a worksheet's `sheetData` is: its cells, read as cells.
fn sheet_data(tokens: &[Token]) -> Option<Range<usize>> {
    let mut start = None;
    for t in tokens {
        if let Token::Tag {
            name, kind, range, ..
        } = t
            && xml::local(name) == "sheetData"
        {
            match kind {
                TagKind::Start => start = Some(range.start),
                TagKind::End => return start.map(|s| s..range.end),
                TagKind::Empty => return None,
            }
        }
    }
    None
}

/// Per pivot cache's records part, its fields' names and the definition
/// that points to it.
fn pivot_fields(
    entries: &[(String, Vec<u8>, CompressionMethod)],
) -> Result<HashMap<String, (Vec<String>, String)>, Error> {
    let mut out = HashMap::new();
    for (name, data, _) in entries {
        let Some(file) = name.strip_prefix("xl/pivotCache/") else {
            continue;
        };
        if !file.starts_with("pivotCacheDefinition") || !file.ends_with(".xml") {
            continue;
        }
        let s = String::from_utf8_lossy(data);
        let mut fields = Vec::new();
        for t in xml::tokenize(&s)? {
            if let Token::Tag {
                name, attrs, kind, ..
            } = t
                && kind != TagKind::End
                && xml::local(&name) == "cacheField"
            {
                fields.push(
                    attrs
                        .iter()
                        .find(|(a, _)| a == "name")
                        .map(|(_, v)| xml::unescape(&s[v.clone()], 0).0)
                        .unwrap_or_default(),
                );
            }
        }
        let rels = format!("xl/pivotCache/_rels/{file}.rels");
        let Some((_, rels, _)) = entries.iter().find(|e| e.0 == rels) else {
            continue;
        };
        let rels = String::from_utf8_lossy(rels);
        for t in xml::tokenize(&rels)? {
            if let Token::Tag { attrs, .. } = t {
                let a = |k: &str| {
                    attrs
                        .iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, r)| rels[r.clone()].to_owned())
                };
                if let (Some(kind), Some(target)) = (a("Type"), a("Target"))
                    && kind.ends_with("/pivotCacheRecords")
                {
                    let path = match target.strip_prefix('/') {
                        Some(abs) => abs.to_owned(),
                        None => format!("xl/pivotCache/{target}"),
                    };
                    out.insert(path, (fields.clone(), name.clone()));
                }
            }
        }
    }
    Ok(out)
}

fn cells(s: &str, tokens: &[Token]) -> Vec<Cell> {
    let mut out = Vec::new();
    let mut current: Option<Cell> = None;
    let mut element = String::new();
    for t in tokens {
        match t {
            Token::Tag {
                name,
                kind,
                range,
                attrs,
            } => {
                let l = xml::local(name);
                let a = |k: &str| {
                    attrs
                        .iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, r)| s[r.clone()].to_owned())
                };
                match (kind, l) {
                    (TagKind::Start | TagKind::Empty, "c") => {
                        let reference = a("r").unwrap_or_default();
                        let (row, column) = cell_ref(&reference).unwrap_or((0, 0));
                        let cell = Cell {
                            element: range.clone(),
                            reference,
                            row,
                            column,
                            t: a("t"),
                            style: a("s"),
                            value: None,
                            formula: None,
                            inline: None,
                        };
                        if *kind == TagKind::Start {
                            current = Some(cell);
                        }
                    }
                    (TagKind::End, "c") => {
                        if let Some(mut c) = current.take() {
                            c.element = c.element.start..range.end;
                            out.push(c);
                        }
                    }
                    (TagKind::Start, "is") => {
                        if let Some(c) = current.as_mut() {
                            c.inline = Some(Vec::new());
                        }
                    }
                    (TagKind::Start, l) => element = l.to_owned(),
                    _ => {}
                }
            }
            Token::Text(r) => {
                if let Some(c) = current.as_mut() {
                    let (text, _) = xml::unescape(&s[r.clone()], r.start);
                    match element.as_str() {
                        "v" => c.value = Some((r.clone(), text)),
                        "f" => c.formula = Some((r.clone(), text)),
                        "t" => {
                            if let Some(p) = c.inline.as_mut() {
                                p.push(Piece {
                                    raw: Some(r.clone()),
                                    text,
                                    space_tag: None,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Token::CData(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Policy;

    fn engine(policy: &str) -> Engine {
        Engine::new(&Policy::parse(policy).unwrap(), None, b"seed", None).unwrap()
    }

    fn default_engine() -> Engine {
        engine("version = 1\n")
    }

    /// A ZIP of the given parts.
    fn zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for (n, d) in parts {
            w.start_file(*n, SimpleFileOptions::default()).unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn unzip(data: &[u8], name: &str) -> String {
        let mut a = ZipArchive::new(Cursor::new(data)).unwrap();
        let mut s = String::new();
        a.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn a_name_across_runs_is_found_and_runs_keep_their_formatting() {
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:r><w:t xml:space="preserve">Frau </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>Mi</w:t></w:r><w:r><w:t>ra Al</w:t></w:r><w:r><w:t>bers, Tel. 0421 5550173</w:t></w:r></w:p></w:body></w:document>"#
        );
        let input = zip(&[("word/document.xml", doc.as_bytes())]);
        let r = process(&default_engine(), &input, true).unwrap();
        let out = unzip(&r.output, "word/document.xml");
        assert!(
            out.contains(r#"<w:rPr><w:b/></w:rPr><w:t>&lt;PERSON&gt;</w:t>"#),
            "{out}"
        );
        assert!(out.contains("<w:t></w:t>"), "{out}");
        assert!(out.contains(", Tel. &lt;PHONE_NUMBER&gt;"), "{out}");
        assert!(!out.contains("Albers") && !out.contains("5550173"));
        assert_eq!(r.findings.len(), 2);
        let Location::Span { part, line, .. } = &r.findings[0].location else {
            panic!()
        };
        assert_eq!((part.as_deref(), *line), (Some("word/document.xml"), 1));
    }

    #[test]
    fn a_deletion_does_not_run_into_the_text_after_it() {
        // Deleted and inserted text are alternatives, not one word: glued
        // together (`5550198siehe`), the phone number matched only in part
        // and its last digits stayed.
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:r><w:t xml:space="preserve">Ansprechpartner: </w:t></w:r><w:del w:id="1" w:author="A"><w:r><w:delText>+49 89 5550198</w:delText></w:r></w:del><w:ins w:id="2" w:author="A"><w:r><w:t>siehe Anlage</w:t></w:r></w:ins></w:p></w:body></w:document>"#
        );
        let input = zip(&[("word/document.xml", doc.as_bytes())]);
        let r = process(&default_engine(), &input, true).unwrap();
        let xml = unzip(&r.output, "word/document.xml");
        assert!(!xml.contains("5550198"), "{xml}");
        assert!(xml.contains("<w:t>siehe Anlage</w:t>"), "{xml}");
    }

    #[test]
    fn metadata_comments_and_deletions_are_redacted() {
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:r><w:t xml:space="preserve">Ansprechpartner: </w:t></w:r><w:del w:id="1" w:author="Lena Vogt"><w:r><w:delText>Herr Tobias Kern</w:delText></w:r></w:del></w:p></w:body></w:document>"#
        );
        let core = r#"<cp:coreProperties xmlns:cp="c" xmlns:dc="d"><dc:creator>Lena Vogt</dc:creator><cp:lastModifiedBy>Jonas Brandt</cp:lastModifiedBy><dc:title>Vertrag</dc:title></cp:coreProperties>"#;
        let comments = format!(
            r#"<w:comments {W}><w:comment w:id="0" w:author="Jonas Brandt" w:initials="JB"><w:p><w:r><w:t>Bitte mit 0421 5550173 abgleichen.</w:t></w:r></w:p></w:comment></w:comments>"#
        );
        let input = zip(&[
            ("word/document.xml", doc.as_bytes()),
            ("docProps/core.xml", core.as_bytes()),
            ("word/comments.xml", comments.as_bytes()),
        ]);
        let r = process(&default_engine(), &input, true).unwrap();
        let all: String = [
            "word/document.xml",
            "docProps/core.xml",
            "word/comments.xml",
        ]
        .iter()
        .map(|n| unzip(&r.output, n))
        .collect();
        for gone in ["Lena", "Vogt", "Tobias", "Kern", "Jonas", "JB", "5550173"] {
            assert!(!all.contains(gone), "{gone} in {all}");
        }
        assert!(all.contains("<dc:title>Vertrag</dc:title>"));
        assert!(r.findings.iter().any(|f| f.detector == "metadata"));
    }

    #[test]
    fn xlsx_cells_use_their_column_and_shared_strings_are_replaced_once() {
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Kunden" sheetId="1" r:id="rId1"/><sheet name="Intern" sheetId="2" state="hidden" r:id="rId2"/></sheets></workbook>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#;
        let sst = r#"<sst><si><t>name</t></si><si><t>geburtsdatum</t></si><si><t>Mira Albers</t></si><si><t>14.03.1987</t></si><si><t>Steuer-ID 57381942656</t></si></sst>"#;
        let s1 = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2" t="s"><v>3</v></c><c r="C2"><v>4215550173</v></c></row><row r="5"><c r="A5" t="str"><f>A2</f><v>Mira Albers</v></c></row></sheetData></worksheet>"#;
        let s2 = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>4</v></c></row></sheetData></worksheet>"#;
        let input = zip(&[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/sharedStrings.xml", sst.as_bytes()),
            ("xl/worksheets/sheet1.xml", s1.as_bytes()),
            ("xl/worksheets/sheet2.xml", s2.as_bytes()),
        ]);
        let r = process(&default_engine(), &input, true).unwrap();
        let sst_out = unzip(&r.output, "xl/sharedStrings.xml");
        assert!(
            !sst_out.contains("Mira")
                && !sst_out.contains("1987")
                && !sst_out.contains("57381942656"),
            "{sst_out}"
        );
        assert!(sst_out.contains("<t>name</t>"));
        let s1_out = unzip(&r.output, "xl/worksheets/sheet1.xml");
        assert!(
            !s1_out.contains("Mira Albers"),
            "the cached formula value: {s1_out}"
        );
        let birth = r
            .findings
            .iter()
            .find(|f| f.entity.as_str() == "DATE_OF_BIRTH")
            .unwrap();
        assert_eq!(
            birth.location,
            Location::Cell {
                sheet: Some("Kunden".into()),
                row: 2,
                column: 2,
                start: 0,
                end: 10
            }
        );
        assert!(r.findings.iter().any(
            |f| matches!(&f.location, Location::Cell { sheet: Some(s), .. } if s == "Intern")
        ));
    }

    #[test]
    fn a_shared_string_counts_its_lifted_candidate_once() {
        // Read under its column and without one: two readings, one finding,
        // one lifted candidate (#129).
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Termine" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let rels = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let sst = r#"<sst><si><t>notiz</t></si><si><t>Termin mit Frau Lena Brandt am 03.12.2026 bestätigt.</t></si></sst>"#;
        let s1 = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row><row r="2"><c r="A2" t="s"><v>1</v></c></row></sheetData></worksheet>"#;
        let input = zip(&[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/sharedStrings.xml", sst.as_bytes()),
            ("xl/worksheets/sheet1.xml", s1.as_bytes()),
        ]);
        let decider = std::sync::Arc::new(
            crate::decider::Decider::from_json(crate::decider::tests::BESIDE_NAME).unwrap(),
        );
        let (_, run) = default_engine()
            .with_decider(decider)
            .run_bytes(&input, crate::Format::Xlsx, false)
            .unwrap();
        let dates = run
            .report
            .findings
            .iter()
            .filter(|f| f.entity.as_str() == "DATE_OF_BIRTH")
            .count();
        assert_eq!(dates, 1);
        assert!(
            run.report
                .review_reasons
                .contains(&crate::decider::lifted_reason(1)),
            "{:?}",
            run.report.review_reasons
        );
    }

    #[test]
    fn images_without_ocr_fail_unless_the_policy_blanks_them() {
        let doc = format!(
            r#"<w:document {W}><w:body><w:p><w:r><w:t>Hallo</w:t></w:r></w:p></w:body></w:document>"#
        );
        let input = zip(&[
            ("word/document.xml", doc.as_bytes()),
            ("word/media/image1.png", b"\x89PNG..."),
        ]);
        assert!(
            matches!(process(&default_engine(), &input, true), Err(Error::Format(m)) if m.contains("veilq models fetch ocr"))
        );
        let e = engine("version = 1\n[office]\nembedded = \"remove\"\n");
        let r = process(&e, &input, true).unwrap();
        let mut a = ZipArchive::new(Cursor::new(&r.output)).unwrap();
        let mut img = Vec::new();
        a.by_name("word/media/image1.png")
            .unwrap()
            .read_to_end(&mut img)
            .unwrap();
        assert_eq!(img, BLANK_PNG);
        assert_eq!(r.review_reasons.len(), 1);
    }

    #[test]
    fn not_a_zip_is_an_error() {
        assert!(process(&default_engine(), b"plain text", true).is_err());
    }

    #[test]
    fn attributes_comments_and_the_thumbnail_are_read_or_dropped() {
        // ADR-0063: an alt text, a tooltip and a field's instruction are
        // read; ids, revision marks and styles stay byte for byte.
        let doc = format!(
            r#"<?xml version="1.0"?><w:document {W} xmlns:wp="wp"><w:body><!-- Entwurf für Herrn Kai Lindqvist --><w:p w:rsidR="00A1B2C3"><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:hyperlink w:tooltip="Rückruf unter 0421 5550164" r:id="rId4"><w:r><w:t>Rückruf</w:t></w:r></w:hyperlink><w:fldSimple w:instr=" HYPERLINK &quot;mailto:ole.faber@example.org&quot; "><w:r><w:t>Mail</w:t></w:r></w:fldSimple><w:r><w:drawing><wp:docPr id="7" name="Bild 1" descr="Foto: Frau Tilda Brenner"/></w:drawing></w:r></w:p></w:body></w:document>"#
        );
        let input = zip(&[
            ("word/document.xml", doc.as_bytes()),
            ("docProps/thumbnail.jpeg", b"\xFF\xD8 not a real picture"),
        ]);
        let r = process(&default_engine(), &input, true).unwrap();
        let out = unzip(&r.output, "word/document.xml");
        for gone in ["Lindqvist", "5550164", "ole.faber", "Brenner"] {
            assert!(!out.contains(gone), "{gone} in {out}");
        }
        for kept in [
            r#"<?xml version="1.0"?>"#,
            r#"w:rsidR="00A1B2C3""#,
            r#"<w:pStyle w:val="Heading1"/>"#,
            r#"r:id="rId4""#,
            r#"name="Bild 1""#,
            // The quotes around the address stay escaped.
            r#"w:instr=" HYPERLINK &quot;mailto:&lt;EMAIL_ADDRESS&gt;&quot; ""#,
        ] {
            assert!(out.contains(kept), "{kept} not in {out}");
        }
        assert!(!out.contains("<!--"), "{out}");
        let mut a = ZipArchive::new(Cursor::new(&r.output)).unwrap();
        let mut thumb = Vec::new();
        a.by_name("docProps/thumbnail.jpeg")
            .unwrap()
            .read_to_end(&mut thumb)
            .unwrap();
        let thumb = image::load_from_memory(&thumb).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (1, 1));
    }

    const WB_RELS: &str = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#;
    const SHEET: &str = r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Hallo</t></is></c></row></sheetData></worksheet>"#;

    #[test]
    fn a_sheet_named_after_a_person_is_renamed_unless_a_formula_names_it() {
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Übersicht" sheetId="1" r:id="rId1"/><sheet name="Akte Herr Ole Faber" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
        let footer = r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Hallo</t></is></c></row></sheetData><dataValidations count="1"><dataValidation type="list" sqref="B2" prompt="Rückfragen an 0421 5550164"><formula1>"ja,nein"</formula1></dataValidation></dataValidations><headerFooter><oddFooter>&amp;LBearbeitet von Frau Tilda Brenner</oddFooter></headerFooter></worksheet>"#;
        let parts = |s1: &str| {
            zip(&[
                ("xl/workbook.xml", wb.as_bytes()),
                ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
                ("xl/worksheets/sheet1.xml", s1.as_bytes()),
                ("xl/worksheets/sheet2.xml", footer.as_bytes()),
            ])
        };
        let r = process(&default_engine(), &parts(SHEET), true).unwrap();
        let wb_out = unzip(&r.output, "xl/workbook.xml");
        assert!(
            wb_out.contains(r#"<sheet name="Akte Herr &lt;PERSON&gt;" sheetId="2""#),
            "{wb_out}"
        );
        let s2 = unzip(&r.output, "xl/worksheets/sheet2.xml");
        assert!(!s2.contains("Brenner") && !s2.contains("5550164"), "{s2}");
        // No report names the sheet by its old name.
        assert!(
            r.findings
                .iter()
                .all(|f| !format!("{:?}", f.location).contains("Faber"))
        );
        // A formula that names the sheet would break: an error.
        let formula = r#"<worksheet><sheetData><row r="1"><c r="A1"><f>'Akte Herr Ole Faber'!A1</f><v>0</v></c></row></sheetData></worksheet>"#;
        let e = process(&default_engine(), &parts(formula), true)
            .err()
            .unwrap();
        assert!(e.to_string().contains("a formula names the sheet"), "{e}");
        assert!(!e.to_string().contains("Faber"));
    }

    #[test]
    fn a_pivot_cache_is_read_under_its_fields_names() {
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Daten" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let def = r#"<pivotCacheDefinition r:id="rId1"><cacheSource type="worksheet"><worksheetSource ref="A1:B3" sheet="Daten"/></cacheSource><cacheFields count="2"><cacheField name="geburtsdatum" numFmtId="0"><sharedItems><s v="14.03.1987"/><s v="02.11.1990"/></sharedItems></cacheField><cacheField name="Steuer-ID" numFmtId="0"><sharedItems containsNumber="1"/></cacheField></cacheFields></pivotCacheDefinition>"#;
        let def_rels = r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheRecords" Target="pivotCacheRecords1.xml"/></Relationships>"#;
        let rec = r#"<pivotCacheRecords count="2"><r><x v="0"/><n v="57381942656"/></r><r><x v="1"/><n v="12"/></r></pivotCacheRecords>"#;
        let input = zip(&[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
            ("xl/worksheets/sheet1.xml", SHEET.as_bytes()),
            ("xl/pivotCache/pivotCacheDefinition1.xml", def.as_bytes()),
            (
                "xl/pivotCache/_rels/pivotCacheDefinition1.xml.rels",
                def_rels.as_bytes(),
            ),
            ("xl/pivotCache/pivotCacheRecords1.xml", rec.as_bytes()),
        ]);
        let r = process(&default_engine(), &input, true).unwrap();
        let def_out = unzip(&r.output, "xl/pivotCache/pivotCacheDefinition1.xml");
        assert!(
            !def_out.contains("1987") && !def_out.contains("1990"),
            "{def_out}"
        );
        let rec_out = unzip(&r.output, "xl/pivotCache/pivotCacheRecords1.xml");
        // A number that is personal data becomes text; an index stays.
        assert!(rec_out.contains(r#"<x v="0"/><s v="&lt;"#), "{rec_out}");
        assert!(rec_out.contains(r#"<n v="12"/>"#), "{rec_out}");
        assert!(!rec_out.contains("57381942656"), "{rec_out}");
    }

    /// A workbook with sheet `Daten` showing `cell`, and a table over it
    /// whose columns are `columns` (#156).
    fn table(columns: &[&str], cell: &str) -> Vec<u8> {
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Daten" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let sheet = format!(
            r#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Hallo</t></is></c></row><row r="3">{cell}</row></sheetData></worksheet>"#
        );
        let cols: String = columns
            .iter()
            .enumerate()
            .map(|(i, c)| format!(r#"<tableColumn id="{}" name="{c}"/>"#, i + 1))
            .collect();
        let t = format!(
            r#"<table id="1" name="Tabelle1" displayName="Tabelle1" ref="A1:B2"><tableColumns count="{}">{cols}</tableColumns></table>"#,
            columns.len()
        );
        zip(&[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
            ("xl/tables/table1.xml", t.as_bytes()),
        ])
    }

    #[test]
    fn a_table_column_named_after_a_person_that_a_formula_uses_is_an_error() {
        let formula = r#"<c r="A3"><f>Tabelle1[@[Herr Kai Lindqvist]]</f><v>0</v></c>"#;
        let e = process(
            &default_engine(),
            &table(&["Herr Kai Lindqvist", "Betrag"], formula),
            true,
        )
        .err()
        .unwrap();
        assert!(e.to_string().contains("a formula names it"), "{e}");
        assert!(!e.to_string().contains("Lindqvist"), "{e}");
    }

    #[test]
    fn two_table_columns_that_would_become_one_are_an_error() {
        let e = process(
            &default_engine(),
            &table(&["Herr Kai Lindqvist", "Herr Ole Faber"], ""),
            true,
        )
        .err()
        .unwrap();
        assert!(e.to_string().contains("would become the same"), "{e}");
        // One such column, no formula: replaced as any attribute.
        let r = process(
            &default_engine(),
            &table(&["Herr Kai Lindqvist", "Betrag"], ""),
            true,
        )
        .unwrap();
        let t = unzip(&r.output, "xl/tables/table1.xml");
        assert!(
            !t.contains("Lindqvist") && t.contains(r#"name="Betrag""#),
            "{t}"
        );
    }

    #[test]
    fn a_table_named_after_a_person_gets_a_name_excel_allows() {
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Daten" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let t = r#"<table id="1" name="Akte Herr Ole Faber" displayName="Akte Herr Ole Faber" ref="A1:A2"><tableColumns count="1"><tableColumn id="1" name="Betrag"/></tableColumns></table>"#;
        let input = zip(&[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
            ("xl/worksheets/sheet1.xml", SHEET.as_bytes()),
            ("xl/tables/table1.xml", t.as_bytes()),
        ]);
        let r = process(&default_engine(), &input, true).unwrap();
        let out = unzip(&r.output, "xl/tables/table1.xml");
        assert!(
            out.contains(r#"name="Akte_Herr_PERSON_" displayName="Akte_Herr_PERSON_""#),
            "{out}"
        );
        assert!(r.findings.iter().any(|f| f.entity.as_str() == "PERSON"));
    }

    #[test]
    fn a_tables_display_name_is_no_persons() {
        // Every table's `displayName` was replaced as a person's, and Excel
        // no longer opened the file.
        let r = process(&default_engine(), &table(&["Betrag"], ""), true).unwrap();
        assert!(r.findings.is_empty(), "{:?}", r.findings);
    }

    #[test]
    fn a_tables_new_name() {
        assert_eq!(table_name("<PERSON>", &[]), "_PERSON_");
        assert_eq!(
            table_name("Akte <PERSON>", &["akte_person_".into()]),
            "Akte_PERSON__2"
        );
        assert_eq!(table_name("4 <PERSON>", &[]), "_4_PERSON_");
        assert_eq!(table_name("AB12", &[]), "_AB12");
    }

    #[test]
    fn fonts_and_calculation_features_are_not_read() {
        // A theme's font names were taken for persons and its panose for a
        // phone number, a calculation feature's name for a person: Excel
        // no longer opened the file.
        let wb = r#"<workbook xmlns:r="r" xmlns:xcalcf="x"><sheets><sheet name="Daten" sheetId="1" r:id="rId1"/></sheets><extLst><ext><xcalcf:calcFeatures><xcalcf:feature name="microsoft.com:Single"/></xcalcf:calcFeatures></ext></extLst></workbook>"#;
        let theme = r#"<a:theme xmlns:a="a" name="Office"><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Aptos Display" panose="02110004020202020204"/><a:font script="Thaa" typeface="MV Boli"/><a:font script="Sinh" typeface="Iskoola Pota"/></a:majorFont></a:fontScheme></a:theme>"#;
        let input = zip(&[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
            ("xl/worksheets/sheet1.xml", SHEET.as_bytes()),
            ("xl/theme/theme1.xml", theme.as_bytes()),
        ]);
        let r = process(&default_engine(), &input, true).unwrap();
        assert!(r.findings.is_empty(), "{:?}", r.findings);
        let mut a = ZipArchive::new(Cursor::new(&r.output)).unwrap();
        let mut out = String::new();
        a.by_name("xl/theme/theme1.xml")
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        assert_eq!(out, theme);
    }

    #[test]
    fn a_pivot_field_whose_numbers_become_text_says_so() {
        // #157: tax ids stored as numbers.
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Daten" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let def = r#"<pivotCacheDefinition r:id="rId1"><cacheSource type="worksheet"><worksheetSource ref="A1:B3" sheet="Daten"/></cacheSource><cacheFields count="2"><cacheField name="Steuer-ID" numFmtId="0"><sharedItems containsSemiMixedTypes="0" containsString="0" containsNumber="1" containsInteger="1" minValue="57381942656" maxValue="57381942656"/></cacheField><cacheField name="anzahl" numFmtId="0"><sharedItems containsSemiMixedTypes="0" containsString="0" containsNumber="1" containsInteger="1" minValue="2" maxValue="12"/></cacheField></cacheFields></pivotCacheDefinition>"#;
        let def_rels = r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheRecords" Target="pivotCacheRecords1.xml"/></Relationships>"#;
        let run = |second: &str| {
            let rec = format!(
                r#"<pivotCacheRecords count="2"><r><n v="57381942656"/><n v="2"/></r><r><n v="{second}"/><n v="12"/></r></pivotCacheRecords>"#
            );
            let input = zip(&[
                ("xl/workbook.xml", wb.as_bytes()),
                ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
                ("xl/worksheets/sheet1.xml", SHEET.as_bytes()),
                ("xl/pivotCache/pivotCacheDefinition1.xml", def.as_bytes()),
                (
                    "xl/pivotCache/_rels/pivotCacheDefinition1.xml.rels",
                    def_rels.as_bytes(),
                ),
                ("xl/pivotCache/pivotCacheRecords1.xml", rec.as_bytes()),
            ]);
            let r = process(&default_engine(), &input, true).unwrap();
            let rec_out = unzip(&r.output, "xl/pivotCache/pivotCacheRecords1.xml");
            assert!(rec_out.contains(r#"<r><s v="&lt;"#), "{rec_out}");
            unzip(&r.output, "xl/pivotCache/pivotCacheDefinition1.xml")
        };
        // No number left: it says it holds none (Excel rejects the file
        // that says it holds numbers and has none).
        let def_out = run("57381942656");
        assert!(
            def_out.contains(r#"<cacheField name="Steuer-ID" numFmtId="0"><sharedItems/>"#),
            "{def_out}"
        );
        // A number left: mixed types.
        let def_out = run("4");
        assert!(
            def_out.contains(
                r#"<cacheField name="Steuer-ID" numFmtId="0"><sharedItems containsNumber="1" containsInteger="1" containsMixedTypes="1"/>"#
            ),
            "{def_out}"
        );
        // A field that kept its numbers keeps its flags.
        assert!(
            def_out.contains(r#"containsString="0" containsNumber="1" containsInteger="1" minValue="2" maxValue="12"/>"#),
            "{def_out}"
        );
    }

    #[test]
    fn a_defined_name_that_holds_personal_data_is_an_error() {
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Daten" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="ole.faber@example.org">Daten!$A$1</definedName></definedNames></workbook>"#;
        let input = zip(&[
            ("xl/workbook.xml", wb.as_bytes()),
            ("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes()),
            ("xl/worksheets/sheet1.xml", SHEET.as_bytes()),
        ]);
        let e = process(&default_engine(), &input, true).err().unwrap();
        assert!(e.to_string().contains("a defined name holds"), "{e}");
    }
}
