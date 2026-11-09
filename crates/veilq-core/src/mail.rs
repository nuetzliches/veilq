//! EML (ADR-0020): a mail's headers, bodies and attachments, written back as
//! MIME.
//!
//! Headers are read decoded (RFC 2047 encoded words) with their name before
//! them, so that `From: Lena Vogt <...>` reads as a header line does. Text
//! parts are decoded from their transfer encoding and charset and redacted in
//! their format: plain text, HTML (its text and its `href`, `title` and `alt`
//! values, one segment over the whole body so that a name across tags is
//! found), CSV, JSON, a nested mail, an Office attachment. A part veilq cannot
//! read (a PDF, an archive) is an error; images follow `[office] embedded`.
//! What changed is encoded again: the same transfer encoding, UTF-8.

use std::collections::HashMap;

use base64::Engine as _;

use crate::detect::Candidate;
use crate::finding::{Finding, Location};
use crate::office::{self, Outcome, Piece, Place, Segment};
use crate::policy::Embedded;
use crate::{Engine, Error, Format};

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(message.to_string())
}

/// Headers that say how the mail is built, not what it says.
const STRUCTURAL: &[&str] = &[
    "content-type",
    "content-transfer-encoding",
    "mime-version",
    "date",
    "content-id",
    "content-disposition",
];

pub fn process(engine: &Engine, input: &[u8], apply: bool) -> Result<Outcome, Error> {
    let mut out = Outcome {
        output: Vec::new(),
        findings: Vec::new(),
        mapping: Vec::new(),
        review_reasons: Vec::new(),
    };
    let eol = if input.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    out.output = part(engine, input, "", eol, apply, &mut out)?;
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
    d.and_then(|p| p.get("filename").cloned())
        .or_else(|| content_type(hs).1.get("name").cloned())
        .map(|n| decode_words(&n))
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
    let mut head_out = String::new();
    for (i, h) in hs.iter().enumerate() {
        if h.name.is_empty() || STRUCTURAL.contains(&h.name.to_lowercase().as_str()) {
            head_out.push_str(&h.raw);
            continue;
        }
        let decoded = decode_words(&h.value);
        let line = format!("{}: {decoded}", h.name);
        let prefix = h.name.len() + 2;
        match redact_line(
            engine,
            &line,
            &h.name,
            &format!("{label}headers"),
            i as u64 + 1,
            apply,
            out,
        )? {
            Some(new) => {
                let value = new.get(prefix..).unwrap_or(&new);
                head_out.push_str(&format!("{}: {}{eol}", h.name, encode_words(value)));
            }
            None => head_out.push_str(&h.raw),
        }
    }
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
        let name = filename(&hs);
        let shown = format!("{label}{}", name.as_deref().unwrap_or(&mime));
        let new = leaf(
            engine,
            &decoded,
            &mime,
            &ps,
            name.as_deref(),
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

/// A header line or other short text: its findings reported, and with
/// `apply` the new text if anything was found.
fn redact_line(
    engine: &Engine,
    text: &str,
    context: &str,
    part: &str,
    n: u64,
    apply: bool,
    out: &mut Outcome,
) -> Result<Option<String>, Error> {
    let found = engine.findings(text, context);
    if found.is_empty() {
        return Ok(None);
    }
    let (new, pseudonyms) = if apply {
        let (pieces, pseudonyms) = engine.replace(text, context, &found, &mut out.mapping)?;
        (
            Some(pieces.into_iter().map(|(p, _)| p).collect::<String>()),
            pseudonyms,
        )
    } else {
        (None, vec![None; found.len()])
    };
    for (c, p) in found.iter().zip(pseudonyms) {
        out.findings.push(finding(c, part, n, text, p));
    }
    Ok(new)
}

fn finding(c: &Candidate, part: &str, n: u64, text: &str, pseudonym: Option<String>) -> Finding {
    Finding {
        entity: c.entity.clone(),
        location: Location::Span {
            part: Some(part.to_owned()),
            start: c.start as u64,
            end: c.end as u64,
            line: n,
            column: text[..c.start].chars().count() as u64 + 1,
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
    if let Some(f) = format {
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
    if mime.starts_with("image/") {
        return match engine.policy().office.embedded {
            Embedded::Error => Err(error(format!(
                "{label}: images are not read until M4 (set [office] embedded = \"remove\" to blank them)"
            ))),
            Embedded::Remove => {
                out.review_reasons.push(format!(
                    "{label}: an image was blanked, not read (M4 reads it)"
                ));
                let blank = if mime == "image/gif" {
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
            other => other,
        };
        out.findings.push(f);
    }
}

// ------------------------------------------------------------------ HTML

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
/// one per `href`, `title` and `alt` value. The new HTML if anything changed.
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
    let mut i = 0;
    while i < b.len() {
        if s[i..].starts_with("<!--") {
            i += s[i..].find("-->").map_or(s.len() - i, |p| p + 3);
            pieces.push(Piece {
                raw: None,
                text: "\n".into(),
                space_tag: None,
            });
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
            for key in ["href", "title", "alt"] {
                let lower = tag.to_lowercase();
                let mut from = 0;
                while let Some(p) = lower[from..].find(key) {
                    let at = from + p;
                    from = at + key.len();
                    let before_ok = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric();
                    let rest = lower[from..].trim_start();
                    if !before_ok || !rest.starts_with('=') {
                        continue;
                    }
                    let eq = from + (lower[from..].len() - rest.len());
                    let v = lower[eq + 1..].trim_start();
                    let vstart = eq + 1 + (lower[eq + 1..].len() - v.len());
                    let (vs, ve) = match v.as_bytes().first() {
                        Some(q @ (b'"' | b'\'')) => {
                            let e = v[1..].find(*q as char).map_or(v.len(), |e| e + 1);
                            (vstart + 1, vstart + e)
                        }
                        _ => (
                            vstart,
                            vstart
                                + v.find(|c: char| c.is_whitespace() || c == '>')
                                    .unwrap_or(v.len()),
                        ),
                    };
                    let (raw_s, raw_e) = (i + 1 + vs, i + 1 + ve);
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
    let mut edits = Vec::new();
    let body = Segment {
        pieces,
        context: String::new(),
        forced: None,
        place: Place::Part {
            part: label.to_owned(),
            n: 1,
        },
        cell: None,
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
            cell: None,
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
