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
pub struct Outcome {
    pub output: Vec<u8>,
    pub findings: Vec<Finding>,
    pub mapping: Vec<Entry>,
    pub review_reasons: Vec<String>,
}

const PARAGRAPHS: &[&str] = &["p", "si"];
const TEXTS: &[&str] = &["t", "delText", "instrText"];
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
    /// The raw range of the whole thing to rewrite when it is a cell value
    /// that becomes an inline string; `None` for text in place.
    pub(crate) cell: Option<CellRewrite>,
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
    };
    let names: Vec<String> = entries.iter().map(|e| e.0.clone()).collect();
    let workbook = Workbook::read(&entries)?;
    let mut new_data: HashMap<String, Vec<u8>> = HashMap::new();
    let mut blanked = 0;
    // The shared strings last: the sheets say first which columns show them.
    let mut order: Vec<&(String, Vec<u8>, CompressionMethod)> = entries.iter().collect();
    order.sort_by_key(|e| e.0 == "xl/sharedStrings.xml");
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
                part(engine, name, text, apply, &mut out)?
            };
            if let Some(c) = changed {
                new_data.insert(name.clone(), c.into_bytes());
            }
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

/// The segments of a part: paragraphs, other text, the metadata.
fn segments(name: &str, s: &str) -> Result<Vec<Segment>, Error> {
    let tokens = xml::tokenize(s)?;
    let mut out: Vec<Segment> = Vec::new();
    // Open paragraphs (nested ones in text boxes), each collecting pieces.
    let mut open: Vec<Vec<Piece>> = Vec::new();
    // The element stack: local names; for text elements their start tag.
    let mut stack: Vec<(String, Option<Range<usize>>, bool)> = Vec::new();
    let custom = name == "docProps/custom.xml";
    let comment_authors = name.ends_with("commentAuthors.xml");
    let n = |out: &Vec<Segment>| out.len() as u64 + 1;
    let push = |out: &mut Vec<Segment>, pieces: Vec<Piece>, forced: Option<&'static str>| {
        if pieces.iter().any(|p| !p.text.trim().is_empty()) {
            let place = Place::Part {
                part: name.to_owned(),
                n: n(out),
            };
            out.push(Segment {
                pieces,
                context: String::new(),
                forced,
                place,
                cell: None,
            });
        }
    };
    for t in &tokens {
        match t {
            Token::Tag {
                name: tag,
                kind,
                range,
                attrs,
            } => {
                let l = xml::local(tag);
                for (a, v) in attrs {
                    let al = xml::local(a);
                    let forced = if PERSON_ATTRS.contains(&al) || (comment_authors && al == "name")
                    {
                        Some("PERSON")
                    } else {
                        None
                    };
                    let external = al == "Target"
                        && attrs.iter().any(|(k, v2)| {
                            xml::local(k) == "TargetMode" && &s[v2.clone()] == "External"
                        });
                    if forced.is_some() || external {
                        let (text, _) = xml::unescape(&s[v.clone()], v.start);
                        push(
                            &mut out,
                            vec![Piece {
                                raw: Some(v.clone()),
                                text,
                                space_tag: None,
                            }],
                            forced,
                        );
                    }
                }
                match kind {
                    TagKind::Start => {
                        if PARAGRAPHS.contains(&l) {
                            open.push(Vec::new());
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
                        if PARAGRAPHS.contains(&l)
                            && let Some(pieces) = open.pop()
                        {
                            push(&mut out, pieces, None);
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
                } else if PERSON_ELEMENTS.contains(&element.as_str()) {
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
                    _ => push(&mut out, vec![piece], forced),
                }
            }
        }
    }
    Ok(out)
}

/// Redacts one XML part; the new text if it changed.
fn part(
    engine: &Engine,
    name: &str,
    s: &str,
    apply: bool,
    out: &mut Outcome,
) -> Result<Option<String>, Error> {
    let segs = segments(name, s)?;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for seg in segs {
        edit_segment(engine, &seg, apply, out, &mut edits)?;
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
    if let Some(cell) = &seg.cell {
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
                    edits.push((raw.clone(), xml::escape(&new, false)));
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
                cell: None,
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
                        cell: None,
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
                        cell: None,
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
                        cell: Some(CellRewrite {
                            element: c.element.clone(),
                            reference: c.reference.clone(),
                            style: c.style.clone(),
                        }),
                    };
                    edit_segment(engine, &seg, apply, out, &mut edits)?;
                }
                _ => {}
            }
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
            let mut all: Vec<Candidate> = Vec::new();
            for ctx in &contexts {
                for c in engine.findings(&text, ctx)? {
                    if all.iter().all(|a| c.end <= a.start || c.start >= a.end) {
                        all.push(c);
                    }
                }
            }
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
}
