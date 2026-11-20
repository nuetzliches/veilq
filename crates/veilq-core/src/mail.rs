//! EML (ADR-0020): a mail's headers, bodies and attachments, written back as
//! MIME.
//!
//! Headers are read decoded (RFC 2047 encoded words) with their name before
//! them, so that `From: Lena Vogt <...>` reads as a header line does; only
//! those that say who wrote to whom and about what are kept, the transport's
//! and clients' are removed (ADR-0063). A part's file name is read and
//! replaced like text, its extension kept. Text
//! parts are decoded from their transfer encoding and charset and redacted in
//! their format: plain text, HTML (its text and its `href`, `title`, `alt`,
//! `src`, `content` and `data-*` values, one segment over the whole body so
//! that a name across tags is found; comments removed), CSV, JSON, a nested
//! mail, an Office attachment. A part veilq cannot
//! read (an archive) is an error. Images and PDFs are read with OCR
//! (ADR-0023); without it a PDF is an error and an image follows
//! `[office] embedded`.
//! What changed is encoded again: the same transfer encoding, UTF-8.

use std::collections::HashMap;
use std::sync::LazyLock;

use base64::Engine as _;
use regex::Regex;

use crate::detect::Candidate;
use crate::finding::{Finding, Location};
use crate::office::{self, Outcome, Piece, Place, Segment};
use crate::policy::Embedded;
use crate::{Engine, Error, Format};

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(message.to_string())
}

/// Headers that say how the mail is built, not what it says: kept unread
/// (a part's file name in them is read on its own).
const STRUCTURAL: &[&str] = &[
    "content-type",
    "content-transfer-encoding",
    "mime-version",
    "date",
    "content-id",
    "content-disposition",
];

/// Headers that say who wrote to whom and about what: kept and read. Every
/// other one (`Received`, `Return-Path`, `X-…`, `DKIM-Signature`, ARC,
/// `Authentication-Results`, a client's own) is removed: host names, IP
/// addresses and internal ids no detector takes (ADR-0063).
const READ: &[&str] = &[
    "from",
    "sender",
    "reply-to",
    "to",
    "cc",
    "bcc",
    "subject",
    "comments",
    "keywords",
    "message-id",
    "in-reply-to",
    "references",
    "content-description",
    "content-location",
    "content-language",
];

pub fn process(engine: &Engine, input: &[u8], apply: bool) -> Result<Outcome, Error> {
    // Pictures inside a container stay unmarked (ADR-0050).
    let engine = &engine.without_watermark(true);
    let mut out = Outcome {
        output: Vec::new(),
        findings: Vec::new(),
        mapping: Vec::new(),
        review_reasons: Vec::new(),
        weak_lines: Vec::new(),
        removed_headers: 0,
    };
    let eol = if input.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    out.output = part(engine, input, "", eol, apply, &mut out)?;
    if out.removed_headers > 0 {
        out.review_reasons.push(format!(
            "{} transport or client header(s) (Received, X-…) were removed unread",
            out.removed_headers
        ));
    }
    Ok(out)
}

struct Header {
    name: String,
    /// The value, unfolded.
    value: String,
    /// The header as it was, folding and line end included.
    raw: String,
}

/// The header block and the body: split at the first empty line.
fn split(raw: &[u8]) -> (&[u8], &[u8]) {
    for (i, w) in raw.windows(4).enumerate() {
        if w == b"\r\n\r\n" {
            return (&raw[..i + 2], &raw[i + 4..]);
        }
    }
    for (i, w) in raw.windows(2).enumerate() {
        if w == b"\n\n" {
            return (&raw[..i + 1], &raw[i + 2..]);
        }
    }
    (raw, &[])
}

fn headers(block: &str) -> Vec<Header> {
    let mut out: Vec<Header> = Vec::new();
    for line in block.split_inclusive('\n') {
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            let h = out.last_mut().expect("checked");
            h.raw.push_str(line);
            h.value.push(' ');
            h.value.push_str(line.trim());
        } else if let Some((name, value)) = line.split_once(':') {
            out.push(Header {
                name: name.trim().to_owned(),
                value: value.trim().to_owned(),
                raw: line.to_owned(),
            });
        } else if !line.trim().is_empty() {
            out.push(Header {
                name: String::new(),
                value: line.trim().to_owned(),
                raw: line.to_owned(),
            });
        }
    }
    out
}

fn header<'a>(hs: &'a [Header], name: &str) -> Option<&'a str> {
    hs.iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

/// `type/subtype` (lower case) and its parameters.
fn content_type(hs: &[Header]) -> (String, HashMap<String, String>) {
    let v = header(hs, "content-type").unwrap_or("text/plain");
    let mut parts = v.split(';');
    let mime = parts.next().unwrap_or("").trim().to_lowercase();
    (mime, params(parts))
}

fn params<'a>(parts: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| {
            (
                k.trim().to_lowercase(),
                v.trim().trim_matches('"').to_owned(),
            )
        })
        .collect()
}

fn filename(hs: &[Header]) -> Option<String> {
    let d = header(hs, "content-disposition").map(|v| params(v.split(';').skip(1)));
    d.and_then(|p| extended(&p, "filename").or_else(|| p.get("filename").cloned()))
        .or_else(|| {
            let ps = content_type(hs).1;
            extended(&ps, "name").or_else(|| ps.get("name").cloned())
        })
        .map(|n| decode_words(&n))
}

/// An RFC 2231 parameter (`filename*=utf-8''Akte%20B.pdf`, or continued as
/// `filename*0*`, `filename*1*`), decoded; `None` without one.
fn extended(ps: &HashMap<String, String>, key: &str) -> Option<String> {
    let mut pieces: Vec<(usize, bool, &String)> = ps
        .iter()
        .filter_map(|(k, v)| {
            let rest = k.strip_prefix(key)?.strip_prefix('*')?;
            let (n, encoded) = match rest.strip_suffix('*') {
                Some(n) => (n, true),
                None => (rest, rest.is_empty()),
            };
            let n = if n.is_empty() { 0 } else { n.parse().ok()? };
            Some((n, encoded, v))
        })
        .collect();
    if pieces.is_empty() {
        return None;
    }
    pieces.sort_by_key(|p| p.0);
    let mut bytes = Vec::new();
    for (k, (_, encoded, v)) in pieces.into_iter().enumerate() {
        let v = if k == 0 && encoded {
            // charset'language'value; only the first piece names them.
            v.splitn(3, '\'').nth(2).unwrap_or(v)
        } else {
            v.as_str()
        };
        if encoded {
            let b = v.as_bytes();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%'
                    && let Some(x) = v
                        .get(i + 1..i + 3)
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    bytes.push(x);
                    i += 3;
                } else {
                    bytes.push(b[i]);
                    i += 1;
                }
            }
        } else {
            bytes.extend_from_slice(v.as_bytes());
        }
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// A file name with what a detector finds in its stem replaced (ADR-0063),
/// in characters every file system takes; its extension stays. The new
/// name if anything was found, and the findings reported at `at`.
pub(crate) fn redact_file_name(
    engine: &Engine,
    name: &str,
    part: &str,
    at: (u64, usize),
    apply: bool,
    out: &mut Outcome,
) -> Result<Option<String>, Error> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.len() <= 5 && e.chars().all(char::is_alphanumeric) => {
            (s, Some(e))
        }
        _ => (name, None),
    };
    // Words, not a run of underscores: `Vertrag_Ole_Faber` reads as text;
    // a placeholder written into a name before (`[PERSON]`,
    // `[PERSON-0a1b2c3d]`) reads as the placeholder it stands for.
    static READ_AS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\[([A-Z][A-Z0-9_]*)(?:-([0-9a-f]{8}))?\]|[_+]").expect("a valid regex")
    });
    let spaced = READ_AS
        .replace_all(stem, |c: &regex::Captures| match (c.get(1), c.get(2)) {
            (Some(t), Some(h)) => format!("[{}:{}]", t.as_str(), h.as_str()),
            (Some(t), None) => format!("<{}>", t.as_str()),
            _ => " ".to_owned(),
        })
        .into_owned();
    let mut found = engine.findings(&spaced, "")?;
    if found.is_empty() {
        return Ok(None);
    }
    let (new, pseudonyms) = if apply {
        let (pieces, p) = engine.replace(&spaced, "", &mut found, &mut out.mapping)?;
        (pieces.into_iter().map(|(p, _)| p).collect::<String>(), p)
    } else {
        let mut s = String::new();
        let mut at = 0;
        for c in &found {
            s.push_str(&spaced[at..c.start]);
            s.push_str(&format!("<{}>", c.entity.as_str()));
            at = c.end;
        }
        (s + &spaced[at..], vec![None; found.len()])
    };
    for (c, p) in found.iter().zip(pseudonyms) {
        out.findings.push(finding(c, part, at, &spaced, p));
    }
    // A placeholder in what a file name may hold: `<PERSON>` as `[PERSON]`,
    // `[PERSON:0a1b2c3d]` as `[PERSON-0a1b2c3d]`; read back as above.
    static WRITE_AS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<([A-Z][A-Z0-9_]*)>|\[([A-Z][A-Z0-9_]*):([0-9a-f]{8})\]")
            .expect("a valid regex")
    });
    let new = WRITE_AS.replace_all(&new, |c: &regex::Captures| match c.get(1) {
        Some(t) => format!("[{}]", t.as_str()),
        None => format!("[{}-{}]", &c[2], &c[3]),
    });
    let mut safe: String = new
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Back to the stem's own separators where nothing was replaced.
    if !stem.contains(' ') {
        safe = safe.replace(' ', "_");
    }
    Ok(Some(match ext {
        Some(e) => format!("{safe}.{e}"),
        None => safe,
    }))
}

/// A `Content-Type` or `Content-Disposition` header with its `name` or
/// `filename` parameter (and their RFC 2231 forms) set to `new`.
fn with_file_name(h: &Header, new: &str, eol: &str) -> String {
    let mut pieces = h.value.split(';');
    let first = pieces.next().unwrap_or("").trim().to_owned();
    let key = if h.name.eq_ignore_ascii_case("content-type") {
        "name"
    } else {
        "filename"
    };
    let mut out = format!("{}: {first}", h.name);
    let mut had = false;
    for p in pieces {
        let k = p.split('=').next().unwrap_or("").trim().to_lowercase();
        if k == key || k.starts_with(&format!("{key}*")) {
            had = true;
            continue;
        }
        if !p.trim().is_empty() {
            out.push_str(&format!(";{eol} {}", p.trim()));
        }
    }
    if !had {
        return h.raw.clone();
    }
    if new.is_ascii() {
        out.push_str(&format!(";{eol} {key}=\"{new}\""));
    } else {
        let encoded: String = new
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        out.push_str(&format!(";{eol} {key}*=utf-8''{encoded}"));
    }
    out + eol
}

/// One part (or the whole mail): headers and body redacted, written again.
fn part(
    engine: &Engine,
    raw: &[u8],
    label: &str,
    eol: &str,
    apply: bool,
    out: &mut Outcome,
) -> Result<Vec<u8>, Error> {
    let (head, body) = split(raw);
    let head =
        std::str::from_utf8(head).map_err(|_| error(format!("{label}headers are not UTF-8")))?;
    let hs = headers(head);
    // The part's file name, read in the header that names it.
    let original = filename(&hs);
    let renamed = match &original {
        Some(n) => {
            let line = hs
                .iter()
                .position(|h| {
                    h.name.eq_ignore_ascii_case("content-disposition")
                        || h.name.eq_ignore_ascii_case("content-type")
                })
                .unwrap_or(0) as u64
                + 1;
            redact_file_name(engine, n, &format!("{label}headers"), (line, 0), apply, out)?
        }
        None => None,
    };
    let mut head_out = String::new();
    let mut removed = 0;
    for (i, h) in hs.iter().enumerate() {
        let lower = h.name.to_lowercase();
        if h.name.is_empty() || STRUCTURAL.contains(&lower.as_str()) {
            match &renamed {
                Some(new) if matches!(lower.as_str(), "content-type" | "content-disposition") => {
                    head_out.push_str(&with_file_name(h, new, eol));
                }
                _ => head_out.push_str(&h.raw),
            }
            continue;
        }
        if !READ.contains(&lower.as_str()) {
            removed += 1;
            continue;
        }
        // Read as the line `Name: value`, redacted in the value only: what a
        // detector finds in the name (NER once read `To` as a name) is
        // dropped or cut back to the value, so the name cannot change.
        let decoded = decode_words(&h.value);
        let line = format!("{}: {decoded}", h.name);
        let prefix = h.name.len() + 2;
        let found = engine
            .findings(&line, "")?
            .into_iter()
            .filter(|c| c.end > prefix)
            .map(|mut c| {
                c.start = c.start.max(prefix) - prefix;
                c.end -= prefix;
                c
            })
            .collect();
        match redact_line(
            engine,
            &decoded,
            found,
            &format!("{}:", h.name),
            &format!("{label}headers"),
            (i as u64 + 1, prefix),
            apply,
            out,
        )? {
            Some(new) => {
                head_out.push_str(&format!("{}: {}{eol}", h.name, encode_words(&new)));
            }
            None => head_out.push_str(&h.raw),
        }
    }
    out.removed_headers += removed;
    let (mime, ps) = content_type(&hs);
    let cte = header(&hs, "content-transfer-encoding")
        .unwrap_or("7bit")
        .trim()
        .to_lowercase();
    let new_body: Vec<u8> = if mime.starts_with("multipart/") {
        let boundary = ps
            .get("boundary")
            .ok_or_else(|| error(format!("{label}multipart without a boundary")))?;
        multipart(engine, body, boundary, label, eol, apply, out)?
    } else if mime == "message/rfc822" {
        part(engine, body, &format!("{label}message "), eol, apply, out)?
    } else {
        let decoded = decode_body(body, &cte).map_err(|e| error(format!("{label}{e}")))?;
        // The report names a part by its new name, never by the old one.
        let shown = format!(
            "{label}{}",
            renamed.as_deref().or(original.as_deref()).unwrap_or(&mime)
        );
        let new = leaf(
            engine,
            &decoded,
            &mime,
            &ps,
            original.as_deref(),
            &shown,
            eol,
            apply,
            out,
        )?;
        match new {
            None => body.to_vec(),
            Some((bytes, text)) => {
                if text
                    && !ps.get("charset").is_some_and(|c| {
                        c.eq_ignore_ascii_case("utf-8") || c.eq_ignore_ascii_case("us-ascii")
                    })
                {
                    let mut ps = ps.clone();
                    ps.insert("charset".into(), "utf-8".into());
                    set_content_type(&mut head_out, &mime, &ps, eol);
                }
                let target = if cte == "7bit" && !bytes.is_ascii() {
                    "quoted-printable".to_owned()
                } else {
                    cte.clone()
                };
                if target != cte {
                    set_header(&mut head_out, "Content-Transfer-Encoding", &target, eol);
                }
                encode_body(&bytes, &target, eol)
            }
        }
    };
    let mut result = head_out.into_bytes();
    result.extend_from_slice(eol.as_bytes());
    result.extend_from_slice(&new_body);
    Ok(result)
}

/// A header value with what was `found` in it: the findings reported, and
/// with `apply` the new text if anything was found. `at` is its line and
/// the column (in ASCII characters) the value starts at.
#[allow(clippy::too_many_arguments)]
fn redact_line(
    engine: &Engine,
    text: &str,
    mut found: Vec<Candidate>,
    context: &str,
    part: &str,
    at: (u64, usize),
    apply: bool,
    out: &mut Outcome,
) -> Result<Option<String>, Error> {
    if found.is_empty() {
        return Ok(None);
    }
    let (new, pseudonyms) = if apply {
        let (pieces, pseudonyms) = engine.replace(text, context, &mut found, &mut out.mapping)?;
        (
            Some(pieces.into_iter().map(|(p, _)| p).collect::<String>()),
            pseudonyms,
        )
    } else {
        (None, vec![None; found.len()])
    };
    for (c, p) in found.iter().zip(pseudonyms) {
        out.findings.push(finding(c, part, at, text, p));
    }
    Ok(new)
}

fn finding(
    c: &Candidate,
    part: &str,
    (line, shift): (u64, usize),
    text: &str,
    pseudonym: Option<String>,
) -> Finding {
    Finding {
        entity: c.entity.clone(),
        location: Location::Span {
            part: Some(part.to_owned()),
            start: (shift + c.start) as u64,
            end: (shift + c.end) as u64,
            line,
            column: (shift + text[..c.start].chars().count()) as u64 + 1,
        },
        confidence: c.confidence,
        detector: c.detector.clone(),
        pseudonym,
    }
}

fn multipart(
    engine: &Engine,
    body: &[u8],
    boundary: &str,
    label: &str,
    eol: &str,
    apply: bool,
    out: &mut Outcome,
) -> Result<Vec<u8>, Error> {
    let delim = format!("--{boundary}");
    let text = body;
    // Positions of delimiter lines.
    let mut marks = Vec::new();
    let mut i = 0;
    while let Some(p) = find(&text[i..], delim.as_bytes()) {
        let at = i + p;
        let line_start = at == 0 || text[at - 1] == b'\n';
        if line_start {
            marks.push(at);
        }
        i = at + delim.len();
    }
    if marks.is_empty() {
        return Err(error(format!("{label}multipart: no boundary line")));
    }
    let mut result = Vec::new();
    result.extend_from_slice(&text[..marks[0]]);
    for (k, &m) in marks.iter().enumerate() {
        let after = m + delim.len();
        let closing = text[after..].starts_with(b"--");
        let line_end = text[after..]
            .iter()
            .position(|c| *c == b'\n')
            .map_or(text.len(), |p| after + p + 1);
        result.extend_from_slice(&text[m..line_end]);
        if closing {
            result.extend_from_slice(&text[line_end..]);
            return Ok(result);
        }
        let end = marks.get(k + 1).copied().unwrap_or(text.len());
        // The line break before the next delimiter belongs to it.
        let mut content_end = end;
        if content_end > line_end && text[content_end - 1] == b'\n' {
            content_end -= 1;
            if content_end > line_end && text[content_end - 1] == b'\r' {
                content_end -= 1;
            }
        }
        let inner = &text[line_end..content_end.max(line_end)];
        let label = format!("{label}part {}{}", k + 1, ".");
        let new = part(
            engine,
            inner,
            &format!("{} ", label.trim_end_matches('.')),
            eol,
            apply,
            out,
        )?;
        result.extend_from_slice(&new);
        result.extend_from_slice(&text[content_end.max(line_end)..end]);
    }
    Err(error(format!("{label}multipart: no closing boundary")))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// A leaf part's content redacted: the new bytes and whether they are text.
#[allow(clippy::too_many_arguments)]
fn leaf(
    engine: &Engine,
    data: &[u8],
    mime: &str,
    ps: &HashMap<String, String>,
    name: Option<&str>,
    label: &str,
    eol: &str,
    apply: bool,
    out: &mut Outcome,
) -> Result<Option<(Vec<u8>, bool)>, Error> {
    let by_name = name.and_then(|n| Format::from_path(std::path::Path::new(n)).ok());
    let format = match (mime, by_name) {
        (_, Some(f)) => Some(f),
        ("text/plain", None) => Some(Format::Text),
        ("text/csv", None) => Some(Format::Csv { delimiter: None }),
        ("application/json", None) => Some(Format::Json),
        _ => None,
    };
    if mime == "text/html" && by_name.is_none() {
        let text = decode_charset(data, ps.get("charset").map(String::as_str))?;
        return Ok(html(engine, &text, label, apply, out)?.map(|t| (t.into_bytes(), true)));
    }
    if name.is_some_and(|n| n.to_lowercase().ends_with(".eml")) {
        let new = part(engine, data, &format!("{label} "), eol, apply, out)?;
        return Ok(apply.then_some((new, false)));
    }
    if let Some(f) = format.filter(|f| !f.is_picture()) {
        if f.is_container() {
            let (bytes, run) = engine
                .run_bytes(data, f, apply)
                .map_err(|e| error(format!("{label}: {e}")))?;
            relabel(run.report.findings, label, out);
            out.mapping.extend(run.mapping);
            out.review_reasons.extend(run.report.review_reasons);
            return Ok(apply.then_some((bytes, false)));
        }
        let text = decode_charset(data, ps.get("charset").map(String::as_str))?;
        let (bytes, run) = engine
            .run_bytes(text.as_bytes(), f, apply)
            .map_err(|e| error(format!("{label}: {e}")))?;
        let changed = !run.report.findings.is_empty();
        relabel(run.report.findings, label, out);
        out.mapping.extend(run.mapping);
        return Ok((apply && changed).then_some((bytes, true)));
    }
    let picture = match (format, mime) {
        (Some(f), _) if f.is_picture() => Some(f),
        (None, "image/png") => Some(Format::Png),
        (None, "image/jpeg") => Some(Format::Jpeg),
        (None, "image/webp") => Some(Format::Webp),
        (None, "image/gif") => Some(Format::Gif),
        (None, "application/pdf") => Some(Format::Pdf),
        _ => None,
    };
    if let Some(f) = picture.filter(|f| engine.reads(*f)) {
        let (bytes, run) = engine
            .run_bytes(data, f, apply)
            .map_err(|e| error(format!("{label}: {e}")))?;
        relabel(run.report.findings, label, out);
        out.review_reasons.extend(run.report.review_reasons);
        return Ok(apply.then_some((bytes, false)));
    }
    if picture == Some(Format::Pdf) {
        return Err(error(format!(
            "{label}: a PDF, and OCR and PDFium are not installed to read it (`veilq models fetch ocr`, `veilq models fetch pdf`)"
        )));
    }
    if picture.is_some() || mime.starts_with("image/") {
        return match engine.policy().office.embedded {
            Embedded::Error => Err(error(format!(
                "{label}: an image, and OCR is not installed to read it (`veilq models fetch ocr`, or set [office] embedded = \"remove\" to blank it)"
            ))),
            Embedded::Remove => {
                out.review_reasons.push(format!(
                    "{label}: an image was blanked, not read (OCR is not installed)"
                ));
                let blank = if mime == "image/gif" || picture == Some(Format::Gif) {
                    office::BLANK_GIF
                } else {
                    office::BLANK_PNG
                };
                Ok(apply.then_some((blank.to_vec(), false)))
            }
        };
    }
    Err(error(format!(
        "{label}: a part of type {mime} veilq cannot read"
    )))
}

/// Findings of an inner run, their place prefixed with the part's label.
fn relabel(findings: Vec<Finding>, label: &str, out: &mut Outcome) {
    for mut f in findings {
        f.location = match f.location {
            Location::Span {
                part,
                start,
                end,
                line,
                column,
            } => Location::Span {
                part: Some(match part {
                    Some(p) => format!("{label} {p}"),
                    None => label.to_owned(),
                }),
                start,
                end,
                line,
                column,
            },
            Location::Cell {
                sheet,
                row,
                column,
                start,
                end,
            } => Location::Cell {
                sheet: Some(match sheet {
                    Some(s) => format!("{label} {s}"),
                    None => label.to_owned(),
                }),
                row,
                column,
                start,
                end,
            },
            Location::Region {
                part,
                frame,
                x,
                y,
                width,
                height,
            } => Location::Region {
                part: Some(match part {
                    Some(p) => format!("{label} {p}"),
                    None => label.to_owned(),
                }),
                frame,
                x,
                y,
                width,
                height,
            },
        };
        out.findings.push(f);
    }
}

// ------------------------------------------------------------------ HTML

/// The attributes of an HTML tag (the text between `<` and `>`): each
/// name in lower case and its value's range in `tag`, quotes left out.
fn html_attrs(tag: &str) -> Vec<(String, std::ops::Range<usize>)> {
    let b = tag.as_bytes();
    let mut out = Vec::new();
    // Past the tag's name.
    let mut j = b
        .iter()
        .position(|c| c.is_ascii_whitespace())
        .unwrap_or(b.len());
    while j < b.len() {
        while j < b.len() && (b[j].is_ascii_whitespace() || b[j] == b'/') {
            j += 1;
        }
        let k = j;
        while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'=' && b[j] != b'/' {
            j += 1;
        }
        if k == j {
            break;
        }
        let key = tag[k..j].to_lowercase();
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if b.get(j) != Some(&b'=') {
            continue;
        }
        j += 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let range = match b.get(j) {
            Some(q @ (b'"' | b'\'')) => {
                let start = j + 1;
                let end = b[start..]
                    .iter()
                    .position(|c| c == q)
                    .map_or(b.len(), |p| start + p);
                j = (end + 1).min(b.len());
                start..end
            }
            _ => {
                let start = j;
                while j < b.len() && !b[j].is_ascii_whitespace() {
                    j += 1;
                }
                start..j
            }
        };
        out.push((key, range));
    }
    out
}

/// Tags whose text runs on: a name may be split by them.
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "br",
    "tr",
    "td",
    "th",
    "li",
    "ul",
    "ol",
    "table",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "blockquote",
    "pre",
    "body",
    "html",
    "head",
    "title",
    "section",
    "article",
    "header",
    "footer",
];

/// HTML: one segment over all text (a block tag reads as a line break), and
/// one per `href`, `title`, `alt`, `src`, `content` and `data-*` value;
/// comments outside a style sheet or script removed. The new HTML if
/// anything changed.
fn html(
    engine: &Engine,
    s: &str,
    label: &str,
    apply: bool,
    out: &mut Outcome,
) -> Result<Option<String>, Error> {
    let b = s.as_bytes();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut attrs: Vec<Piece> = Vec::new();
    // Comments are removed (ADR-0063), except in a style sheet or a
    // script, which old mail wraps in one: there they are read as text.
    let mut dropped: Vec<std::ops::Range<usize>> = Vec::new();
    let mut in_code = false;
    let mut i = 0;
    while i < b.len() {
        if s[i..].starts_with("<!--") {
            let len = s[i..].find("-->").map_or(s.len() - i, |p| p + 3);
            if in_code {
                let (text, _) = crate::xml::unescape(&s[i..i + len], i);
                pieces.push(Piece {
                    raw: Some(i..i + len),
                    text,
                    space_tag: None,
                });
            } else {
                dropped.push(i..i + len);
                pieces.push(Piece {
                    raw: None,
                    text: "\n".into(),
                    space_tag: None,
                });
            }
            i += len;
            continue;
        }
        let tag_start = b[i] == b'<'
            && b.get(i + 1)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/' || *c == b'!');
        if tag_start {
            let mut j = i + 1;
            let mut quote = None;
            while j < b.len() {
                match (quote, b[j]) {
                    (None, b'>') => break,
                    (None, q @ (b'"' | b'\'')) => quote = Some(q),
                    (Some(q), c) if c == q => quote = None,
                    _ => {}
                }
                j += 1;
            }
            let tag = &s[i + 1..j.min(s.len())];
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_lowercase();
            if BLOCKS.contains(&name.as_str()) {
                pieces.push(Piece {
                    raw: None,
                    text: "\n".into(),
                    space_tag: None,
                });
            }
            if matches!(name.as_str(), "style" | "script") {
                in_code = !tag.starts_with('/');
            }
            let base = i + 1;
            for (key, range) in html_attrs(tag) {
                let read = matches!(key.as_str(), "href" | "title" | "alt" | "src" | "content")
                    || key.starts_with("data-");
                if read && !range.is_empty() {
                    let (raw_s, raw_e) = (base + range.start, base + range.end);
                    let (text, _) = crate::xml::unescape(&s[raw_s..raw_e], raw_s);
                    attrs.push(Piece {
                        raw: Some(raw_s..raw_e),
                        text,
                        space_tag: None,
                    });
                }
            }
            i = (j + 1).min(b.len());
            continue;
        }
        let end = b[i + 1..]
            .iter()
            .position(|c| *c == b'<')
            .map_or(b.len(), |p| i + 1 + p);
        let (text, _) = crate::xml::unescape(&s[i..end], i);
        pieces.push(Piece {
            raw: Some(i..end),
            text,
            space_tag: None,
        });
        i = end;
    }
    let mut edits: Vec<(std::ops::Range<usize>, String)> =
        dropped.into_iter().map(|r| (r, String::new())).collect();
    let body = Segment {
        pieces,
        context: String::new(),
        forced: None,
        place: Place::Part {
            part: label.to_owned(),
            n: 1,
        },
        rewrite: None,
    };
    office::edit_segment(engine, &body, apply, out, &mut edits)?;
    for (k, a) in attrs.into_iter().enumerate() {
        let seg = Segment {
            pieces: vec![a],
            context: String::new(),
            forced: None,
            place: Place::Part {
                part: format!("{label} attribute"),
                n: k as u64 + 1,
            },
            rewrite: None,
        };
        office::edit_segment(engine, &seg, apply, out, &mut edits)?;
    }
    Ok(office::apply_edits(s, edits))
}

// ------------------------------------------------------------------ encodings

fn decode_body(body: &[u8], cte: &str) -> Result<Vec<u8>, String> {
    match cte {
        "base64" => {
            let clean: Vec<u8> = body
                .iter()
                .copied()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            base64::engine::general_purpose::STANDARD
                .decode(clean)
                .map_err(|e| format!("base64: {e}"))
        }
        "quoted-printable" => Ok(decode_qp(body)),
        "7bit" | "8bit" | "binary" | "" => Ok(body.to_vec()),
        other => Err(format!("transfer encoding {other} veilq cannot read")),
    }
}

fn encode_body(data: &[u8], cte: &str, eol: &str) -> Vec<u8> {
    match cte {
        "base64" => {
            let e = base64::engine::general_purpose::STANDARD.encode(data);
            let mut out = String::new();
            for chunk in e.as_bytes().chunks(76) {
                out.push_str(std::str::from_utf8(chunk).expect("base64 is ASCII"));
                out.push_str(eol);
            }
            out.into_bytes()
        }
        "quoted-printable" => encode_qp(data, eol).into_bytes(),
        _ => data.to_vec(),
    }
}

fn decode_qp(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] == b'=' {
            if body.get(i + 1) == Some(&b'\r') && body.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if body.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let Some(h) = body
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(h);
                i += 3;
                continue;
            }
        }
        out.push(body[i]);
        i += 1;
    }
    out
}

fn encode_qp(data: &[u8], eol: &str) -> String {
    let mut out = String::new();
    let mut line = 0;
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        if c == b'\n' || (c == b'\r' && data.get(i + 1) == Some(&b'\n')) {
            out.push_str(eol);
            line = 0;
            i += if c == b'\r' { 2 } else { 1 };
            continue;
        }
        // Whitespace before a line end, `=` and what is not printable ASCII
        // are written as `=XX`.
        let trailing_space =
            (c == b' ' || c == b'\t') && matches!(data.get(i + 1), None | Some(b'\r' | b'\n'));
        let piece = if trailing_space || c == b'=' || !(32..127).contains(&c) && c != b'\t' {
            format!("={c:02X}")
        } else {
            (c as char).to_string()
        };
        if line + piece.len() > 75 {
            out.push('=');
            out.push_str(eol);
            line = 0;
        }
        line += piece.len();
        out.push_str(&piece);
        i += 1;
    }
    out
}

fn decode_charset(data: &[u8], charset: Option<&str>) -> Result<String, Error> {
    let cs = charset.unwrap_or("us-ascii").to_lowercase();
    match cs.as_str() {
        "utf-8" | "utf8" | "us-ascii" | "ascii" => {
            String::from_utf8(data.to_vec()).map_err(|_| error(format!("a part is not {cs}")))
        }
        "iso-8859-1" | "latin1" | "iso-8859-15" => Ok(data.iter().map(|b| *b as char).collect()),
        "windows-1252" | "cp1252" => Ok(data.iter().map(|b| cp1252(*b)).collect()),
        other => Err(error(format!("charset {other} veilq cannot read"))),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&b) {
        HIGH[(b - 0x80) as usize]
    } else {
        b as char
    }
}

/// RFC 2047: `=?utf-8?B?...?=` and `=?iso-8859-1?Q?...?=` words decoded;
/// whitespace between two encoded words dropped.
pub fn decode_words(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    let mut last_encoded = false;
    while let Some(p) = rest.find("=?") {
        let (before, from) = rest.split_at(p);
        let word = from[2..].splitn(3, '?').collect::<Vec<_>>();
        let decoded = (word.len() == 3)
            .then(|| {
                word[2]
                    .find("?=")
                    .map(|e| (word[0], word[1], &word[2][..e]))
            })
            .flatten()
            .and_then(|(cs, enc, text)| {
                let bytes = match enc.to_ascii_uppercase().as_str() {
                    "B" => base64::engine::general_purpose::STANDARD
                        .decode(text)
                        .ok()?,
                    "Q" => decode_qp(text.replace('_', " ").as_bytes()),
                    _ => return None,
                };
                let s = decode_charset(&bytes, Some(cs)).ok()?;
                Some((s, 2 + cs.len() + 1 + enc.len() + 1 + text.len() + 2))
            });
        match decoded {
            Some((s, len)) => {
                if !(last_encoded && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&s);
                rest = &from[len..];
                last_encoded = true;
            }
            None => {
                out.push_str(before);
                out.push_str("=?");
                rest = &from[2..];
                last_encoded = false;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A header value with every word that is not ASCII as a UTF-8 encoded word.
fn encode_words(v: &str) -> String {
    v.split(' ')
        .map(|w| {
            if w.is_ascii() {
                w.to_owned()
            } else {
                format!(
                    "=?utf-8?B?{}?=",
                    base64::engine::general_purpose::STANDARD.encode(w)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The written headers with their `Content-Type` line (and its folded
/// continuation) replaced by one that says `mime` with `ps`.
fn set_content_type(head: &mut String, mime: &str, ps: &HashMap<String, String>, eol: &str) {
    let mut lines: Vec<String> = head.split_inclusive('\n').map(str::to_owned).collect();
    let Some(at) = lines
        .iter()
        .position(|l| l.to_lowercase().starts_with("content-type:"))
    else {
        return;
    };
    let mut end = at + 1;
    while end < lines.len() && lines[end].starts_with([' ', '\t']) {
        end += 1;
    }
    let mut keys: Vec<&String> = ps.keys().collect();
    keys.sort();
    let params: String = keys
        .iter()
        .map(|k| format!("; {k}=\"{}\"", ps[*k]))
        .collect();
    lines.splice(at..end, [format!("Content-Type: {mime}{params}{eol}")]);
    *head = lines.concat();
}

fn set_header(head: &mut String, name: &str, value: &str, eol: &str) {
    let lower = head.to_lowercase();
    let key = format!("{}:", name.to_lowercase());
    match lower.find(&key) {
        Some(at) if at == 0 || lower.as_bytes()[at - 1] == b'\n' => {
            let end = lower[at..].find('\n').map_or(head.len(), |p| at + p + 1);
            head.replace_range(at..end, &format!("{name}: {value}{eol}"));
        }
        _ => head.push_str(&format!("{name}: {value}{eol}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Policy;

    fn engine() -> Engine {
        Engine::new(&Policy::default(), None, b"seed", None).unwrap()
    }

    const MAIL: &str = "From: =?utf-8?B?TGVuYSBWb2d0?= <lena.vogt@example.org>\r\n\
To: Jonas Brandt <jonas.brandt@example.org>\r\n\
Subject: =?utf-8?Q?R=C3=BCckruf_Frau_Mira_Albers?=\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Frau Albers bittet um R=C3=BCckruf unter 0421 5550173.\r\n\
--b1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>Viele Gr&uuml;&szlig;e an <b>Frau Mira</b> Albers, <a href=\"mailto:mira.albers@example.com\">Mail</a></p>\r\n\
--b1\r\n\
Content-Type: text/csv; name=\"k.csv\"\r\n\
Content-Disposition: attachment; filename=\"k.csv\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
bmFtZSx0ZWxlZm9uCk1pcmEgQWxiZXJzLDA0MjEgNTU1MDE3Mwo=\r\n\
--b1--\r\n";

    #[test]
    fn a_header_name_is_never_redacted() {
        // A detector that takes the header's name for a finding (NER read
        // `To` as a name) must not shift its value: the name is context.
        let policy = Policy::parse(
            "version = 1\n[[patterns]]\nname = \"to\"\nentity = \"PERSON\"\nregex = '\\bTo\\b'\n",
        )
        .unwrap();
        let e = Engine::new(&policy, None, b"s", None).unwrap();
        let mail = "From: a@b.example\r\nTo: team nord\r\nSubject: x\r\n\r\nHallo\r\n";
        let r = process(&e, mail.as_bytes(), true).unwrap();
        let out = String::from_utf8(r.output).unwrap();
        assert!(out.contains("\r\nTo: team nord\r\n"), "{out}");
        assert!(
            r.findings.iter().all(|f| f.entity.as_str() != "PERSON"),
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn headers_bodies_and_attachments_are_redacted() {
        let r = process(&engine(), MAIL.as_bytes(), true).unwrap();
        let out = String::from_utf8(r.output.clone()).unwrap();
        for gone in [
            "TGVuYSBWb2d0",
            "lena.vogt",
            "Jonas",
            "Albers",
            "5550173",
            "mira.albers",
            "Mk1pcmEgQWxiZXJz",
        ] {
            assert!(!out.contains(gone), "{gone} in\n{out}");
        }
        let csv_b64 = out
            .split("base64\r\n\r\n")
            .nth(1)
            .unwrap()
            .split("\r\n")
            .next()
            .unwrap();
        let csv = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(csv_b64)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(csv, "name,telefon\n<PERSON>,<PHONE_NUMBER>\n");
        assert!(out.contains("<b>Frau &lt;PERSON&gt;</b>"), "{out}");
        assert!(out.ends_with("--b1--\r\n"));
        assert!(r.findings.iter().any(
            |f| matches!(&f.location, Location::Cell { sheet: Some(s), .. } if s.contains("k.csv"))
        ));
        // The output is a mail again: it redacts to itself.
        let again = process(&engine(), &r.output, true).unwrap();
        assert!(again.findings.is_empty(), "{:?}", again.findings);
    }

    #[test]
    fn what_veilq_cannot_read_is_an_error() {
        let pdf = "Content-Type: application/pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0=\r\n";
        assert!(process(&engine(), pdf.as_bytes(), true).is_err());
        let koi8 = "Content-Type: text/plain; charset=koi8-r\r\n\r\nabc\r\n";
        assert!(process(&engine(), koi8.as_bytes(), true).is_err());
    }

    #[test]
    fn transport_headers_file_names_and_html_beyond_the_text() {
        // ADR-0063: what no detector takes (hosts, ids) is removed, a file
        // name is read like text, and HTML is read past href, title and alt.
        let mail = "Received: from mx1.intern.example ([10.20.30.40]) by mx2.intern.example\r\n\
\tfor <kai.lindqvist@example.org>; Fri, 20 Nov 2026 10:00:00 +0100\r\n\
X-Originating-IP: [10.20.30.41]\r\n\
From: Kai Lindqvist <kai.lindqvist@example.org>\r\n\
Subject: Unterlagen\r\n\
Date: Fri, 20 Nov 2026 10:00:00 +0100\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\
\r\n\
--b1\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<html><head><style><!-- p { margin: 0 } --></style></head><body><!-- Entwurf von Frau Tilda Brenner --><p>Anbei.</p><img src=\"https://track.example/o.gif?r=tilda.brenner@example.org\" data-recipient=\"tilda.brenner@example.org\"></body></html>\r\n\
--b1\r\n\
Content-Type: text/plain; charset=utf-8; name=\"Akte_Frau_Tilda_Brenner.txt\"\r\n\
Content-Disposition: attachment;\r\n\
\x20filename*=utf-8''Akte_Frau_Tilda_Brenner.txt\r\n\
\r\n\
Siehe Anlage.\r\n\
--b1--\r\n";
        let r = process(&engine(), mail.as_bytes(), true).unwrap();
        let out = String::from_utf8(r.output).unwrap();
        for gone in [
            "Received",
            "X-Originating-IP",
            "10.20.30",
            "Brenner",
            "Lindqvist",
        ] {
            assert!(!out.contains(gone), "{gone} in\n{out}");
        }
        for kept in [
            "Subject: Unterlagen\r\n",
            "Date: Fri, 20 Nov 2026",
            "<style><!-- p { margin: 0 } --></style>",
            "filename=\"Akte_Frau_[PERSON].txt\"",
            "name=\"Akte_Frau_[PERSON].txt\"",
        ] {
            assert!(out.contains(kept), "{kept} not in\n{out}");
        }
        assert!(!out.contains("<!-- Entwurf"), "{out}");
        assert!(
            r.review_reasons
                .iter()
                .any(|m| m.starts_with("2 transport or client header(s)")),
            "{:?}",
            r.review_reasons
        );
        // The report names the attachment by its new name.
        assert!(
            r.findings
                .iter()
                .all(|f| !format!("{:?}", f.location).contains("Brenner"))
        );
        // A placeholder in a file name reads as one: the output redacts to
        // itself.
        let again = process(&engine(), out.as_bytes(), true).unwrap();
        assert!(again.findings.is_empty(), "{:?}", again.findings);
    }

    #[test]
    fn rfc_2231_names() {
        let ps = params(["filename*0*=utf-8''Akte%20", "filename*1*=K%C3%BChn.pdf"].into_iter());
        assert_eq!(extended(&ps, "filename").as_deref(), Some("Akte Kühn.pdf"));
        assert_eq!(extended(&ps, "name"), None);
    }

    #[test]
    fn encoded_words() {
        assert_eq!(
            decode_words("=?utf-8?B?TGVuYSBWb2d0?= <a@b>"),
            "Lena Vogt <a@b>"
        );
        assert_eq!(
            decode_words("=?iso-8859-1?Q?R=FCckruf?= =?utf-8?Q?_Albers?="),
            "Rückruf Albers"
        );
        assert_eq!(
            encode_words("Grüße an <PERSON>"),
            "=?utf-8?B?R3LDvMOfZQ==?= an <PERSON>"
        );
    }
}
