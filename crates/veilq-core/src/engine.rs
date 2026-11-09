//! `extract -> detect -> decide -> apply -> verify` for line-oriented formats
//! (docs/plan.md §3): plain text, NDJSON, JSON and CSV.
//!
//! **extract** cuts the input into segments: a text chunk, a JSON string or
//! number, a CSV cell. Each knows its raw bytes in the input and its decoded
//! text, and a context that is not part of it (a cell's column name, a
//! value's key). **detect** and **decide** run on the decoded text
//! ([`Detector::findings`]). **apply** replaces the findings in the decoded
//! text and encodes the segment again in place: the rest of the input stays
//! byte for byte, its layout and quoting included. **verify** runs the
//! detector over every changed segment once more; a finding outside the
//! replacements stops the run (ADR-0003).
//!
//! Text, NDJSON and CSV stream in chunks of about [`CHUNK`] bytes at line or
//! record boundaries; a text chunk keeps its last lines for the next one, so
//! a value across the boundary is seen whole. JSON is read whole.

use std::io::{BufRead, Write};
use std::ops::Range;
use std::path::Path;

use crate::detect::{Candidate, Detector};
use crate::fake::Faker;
use crate::finding::{Finding, Location, Report};
use crate::mapping::Entry;
use crate::{Action, Error, Policy, PseudonymKey};

/// The size a chunk grows to before it is processed.
pub const CHUNK: usize = 64 << 10;
/// Lines a text chunk keeps for the next one.
const CARRY_LINES: usize = 3;

/// An input format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Ndjson,
    Json,
    /// `delimiter`: `None` to take the most frequent of `,`, `;` and tab in
    /// the first line.
    Csv {
        delimiter: Option<u8>,
    },
}

impl Format {
    /// `text`, `ndjson`, `json`, `csv`, `tsv`.
    pub fn from_name(name: &str) -> Result<Self, Error> {
        Ok(match name {
            "text" => Self::Text,
            "ndjson" | "jsonl" => Self::Ndjson,
            "json" => Self::Json,
            "csv" => Self::Csv { delimiter: None },
            "tsv" => Self::Csv {
                delimiter: Some(b'\t'),
            },
            other => return Err(Error::UnsupportedFormat(other.to_owned())),
        })
    }

    /// The format a file's extension names. Anything else is an error: a
    /// format veilq does not know is not read as text (ADR-0003).
    pub fn from_path(path: &Path) -> Result<Self, Error> {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "txt" | "log" | "md" | "text" => Ok(Self::Text),
            "" => Err(Error::UnsupportedFormat(format!(
                "{} (no extension; name one with --format)",
                path.display()
            ))),
            other => Self::from_name(other)
                .map_err(|_| Error::UnsupportedFormat(format!(".{other} ({})", path.display()))),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Ndjson => "ndjson",
            Self::Json => "json",
            Self::Csv {
                delimiter: Some(b'\t'),
            } => "tsv",
            Self::Csv { .. } => "csv",
        }
    }
}

/// The result of a run over a whole input in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Redacted {
    /// The output; for a scan, empty.
    pub text: String,
    pub report: Report,
    /// Per finding, its byte range in the input.
    pub input_spans: Vec<Range<usize>>,
    /// Per finding, the byte range of its replacement in the output.
    pub output_spans: Vec<Range<usize>>,
}

/// The pipeline of one run: detector, policy, key and faker.
pub struct Engine {
    detector: Detector,
    key: Option<PseudonymKey>,
    faker: Faker,
    action: Option<Action>,
}

impl Engine {
    /// `fake_seed` makes `fake`'s stand-ins (random per run, or derived from
    /// the key); `action` overrides the policy's actions.
    pub fn new(
        policy: &Policy,
        key: Option<PseudonymKey>,
        fake_seed: &[u8],
        action: Option<Action>,
    ) -> Result<Self, Error> {
        let pseudonymizes = match action {
            Some(a) => a == Action::Pseudonymize,
            None => {
                policy.defaults.action == Action::Pseudonymize
                    || policy
                        .entities
                        .values()
                        .any(|r| r.action == Some(Action::Pseudonymize))
            }
        };
        if pseudonymizes && key.is_none() {
            return Err(Error::KeyMissing);
        }
        Ok(Self {
            detector: Detector::new(policy)?,
            key,
            faker: Faker::new(fake_seed),
            action,
        })
    }

    fn action_for(&self, c: &Candidate) -> Action {
        self.action
            .unwrap_or_else(|| self.detector.policy().action_for(&c.entity))
    }

    /// `input` redacted as a whole, with the spans a viewer marks.
    pub fn redact_str(&self, input: &str, format: Format) -> Result<Redacted, Error> {
        let mut out = Vec::new();
        let run = self.run(input.as_bytes(), Some(&mut out), format)?;
        Ok(Redacted {
            text: String::from_utf8(out).expect("the output is built from strings"),
            report: run.report,
            input_spans: run.input_spans,
            output_spans: run.output_spans,
        })
    }

    /// The findings in `input`, nothing replaced.
    pub fn scan_str(&self, input: &str, format: Format) -> Result<Redacted, Error> {
        let run = self.run(input.as_bytes(), None, format)?;
        Ok(Redacted {
            text: String::new(),
            report: run.report,
            input_spans: run.input_spans,
            output_spans: Vec::new(),
        })
    }

    /// Reads `input` in chunks; with `output`, writes it redacted (a
    /// redaction), else only finds (a scan). On an error, what was written
    /// so far is redacted, but incomplete; the caller removes it where it
    /// can.
    pub fn run(
        &self,
        mut input: impl BufRead,
        mut output: Option<&mut dyn Write>,
        format: Format,
    ) -> Result<Run, Error> {
        let mut state = State::default();
        let mut pending = String::new();
        let mut line = Vec::new();
        let mut line_no = 0u64;
        let mut in_quotes = false;
        let mut delimiter = match format {
            Format::Csv { delimiter } => delimiter,
            _ => None,
        };
        loop {
            line.clear();
            let n = input.read_until(b'\n', &mut line).map_err(Error::Stream)?;
            let eof = n == 0;
            if !eof {
                line_no += 1;
                let text =
                    std::str::from_utf8(&line).map_err(|_| Error::NotUtf8 { line: line_no })?;
                if let Format::Csv { .. } = format {
                    if delimiter.is_none() {
                        delimiter = Some(sniff_delimiter(text));
                    }
                    in_quotes ^= text.bytes().filter(|b| *b == b'"').count() % 2 == 1;
                }
                pending.push_str(text);
            }
            let ready = match format {
                Format::Json => eof,
                Format::Csv { .. } => !in_quotes && (eof || pending.len() >= CHUNK),
                _ => eof || pending.len() >= CHUNK,
            };
            if eof && in_quotes {
                return Err(Error::Format(format!(
                    "line {line_no}: a quoted field is not closed"
                )));
            }
            if ready && !pending.is_empty() {
                let consumed = match format {
                    Format::Text => self.text_chunk(&pending, eof, &mut state, &mut output)?,
                    Format::Ndjson => self.ndjson_chunk(&pending, &mut state, &mut output)?,
                    Format::Json => {
                        let segments = json_segments(&pending, "")?;
                        self.emit(&pending, pending.len(), segments, &mut state, &mut output)?;
                        pending.len()
                    }
                    Format::Csv { .. } => {
                        let d = delimiter.unwrap_or(b',');
                        let segments = csv_segments(&pending, d, &mut state.csv)?;
                        self.emit(&pending, pending.len(), segments, &mut state, &mut output)?;
                        pending.len()
                    }
                };
                pending.drain(..consumed);
            }
            if eof {
                break;
            }
        }
        if let Some(out) = output {
            out.flush().map_err(Error::Stream)?;
        }
        Ok(Run {
            report: Report::new(state.findings),
            input_spans: state.input_spans,
            output_spans: state.output_spans,
            mapping: state.mapping,
        })
    }

    /// A text chunk: everything up to its last lines, unless the input ends.
    /// Returns how many bytes it consumed.
    fn text_chunk(
        &self,
        chunk: &str,
        eof: bool,
        state: &mut State,
        output: &mut Option<&mut dyn Write>,
    ) -> Result<usize, Error> {
        let mut cut = chunk.len();
        if !eof {
            let starts: Vec<usize> = std::iter::once(0)
                .chain(chunk.match_indices('\n').map(|(i, _)| i + 1))
                .filter(|&i| i < chunk.len())
                .collect();
            if starts.len() > CARRY_LINES {
                cut = starts[starts.len() - CARRY_LINES];
                // A finding across the cut goes whole into the next chunk.
                for f in self.detector.findings(chunk, "") {
                    if f.start < cut && f.end > cut {
                        cut = chunk[..f.start].rfind('\n').map_or(0, |i| i + 1);
                    }
                }
                if cut == 0 {
                    cut = chunk.len();
                }
            }
        }
        let segment = Segment::plain(&chunk[..cut]);
        self.emit(chunk, cut, vec![segment], state, output)?;
        Ok(cut)
    }

    fn ndjson_chunk(
        &self,
        chunk: &str,
        state: &mut State,
        output: &mut Option<&mut dyn Write>,
    ) -> Result<usize, Error> {
        let mut segments = Vec::new();
        let mut at = 0;
        for (i, line) in chunk.split_inclusive('\n').enumerate() {
            let body = line.trim_end_matches(['\n', '\r']);
            if !body.trim().is_empty() {
                let n = state.line + i as u64 + 1;
                let mut s =
                    json_segments(body, "").map_err(|e| Error::Format(format!("line {n}: {e}")))?;
                for seg in &mut s {
                    seg.shift(at);
                }
                segments.extend(s);
            }
            at += line.len();
        }
        self.emit(chunk, chunk.len(), segments, state, output)?;
        Ok(chunk.len())
    }

    /// Finds, decides, applies and verifies `segments` of `chunk[..upto]`,
    /// writes the result and advances `state` past it.
    fn emit(
        &self,
        chunk: &str,
        upto: usize,
        segments: Vec<Segment>,
        state: &mut State,
        output: &mut Option<&mut dyn Write>,
    ) -> Result<(), Error> {
        let lines = LineIndex::new(&chunk[..upto], state.line);
        let mut out = String::with_capacity(upto);
        let mut at = 0;
        for seg in segments {
            let found: Vec<Candidate> = self
                .detector
                .findings(&seg.text, &seg.context)
                .into_iter()
                .filter(|c| seg.raw_of(c.start) < upto)
                .collect();
            if found.is_empty() {
                continue;
            }
            let (raw, ranges, pseudonyms) = match output {
                Some(_) => self.apply(&seg, &found, &mut state.mapping)?,
                None => (String::new(), Vec::new(), vec![None; found.len()]),
            };
            out.push_str(&chunk[at..seg.raw.start]);
            let out_base = state.out_offset + out.len();
            for ((c, pseudonym), i) in found.iter().zip(pseudonyms).zip(0..) {
                let (s, e) = (seg.raw_of(c.start), seg.raw_of(c.end));
                state.input_spans.push(state.offset + s..state.offset + e);
                if let Some(r) = ranges.get(i) {
                    state
                        .output_spans
                        .push(out_base + r.start..out_base + r.end);
                }
                state.findings.push(Finding {
                    entity: c.entity.clone(),
                    location: seg.location(c, state.offset, &lines),
                    confidence: c.confidence,
                    detector: c.detector.clone(),
                    pseudonym,
                });
            }
            if output.is_some() {
                out.push_str(&raw);
            } else {
                out.push_str(&chunk[seg.raw.clone()]);
            }
            at = seg.raw.end;
        }
        out.push_str(&chunk[at..upto]);
        if let Some(w) = output {
            w.write_all(out.as_bytes()).map_err(Error::Stream)?;
        }
        state.offset += upto;
        state.out_offset += out.len();
        state.line += chunk[..upto].bytes().filter(|b| *b == b'\n').count() as u64;
        Ok(())
    }

    /// The segment with `found` replaced, encoded as it was; the
    /// replacements' ranges in it; the pseudonym of each finding. What a
    /// `pseudonymize` or `fake` replacement stood for goes to `mapping`.
    #[allow(clippy::type_complexity)]
    fn apply(
        &self,
        seg: &Segment,
        found: &[Candidate],
        mapping: &mut Vec<Entry>,
    ) -> Result<(String, Vec<Range<usize>>, Vec<Option<String>>), Error> {
        let mut pieces: Vec<(String, bool)> = Vec::new();
        let mut pseudonyms = Vec::new();
        let mut at = 0;
        for c in found {
            pieces.push((seg.text[at..c.start].to_owned(), false));
            let value = &seg.text[c.start..c.end];
            let action = self.action_for(c);
            let replacement =
                action.replacement(&c.entity, value, self.key.as_ref(), &self.faker)?;
            pseudonyms.push((action == Action::Pseudonymize).then(|| replacement.clone()));
            if matches!(action, Action::Pseudonymize | Action::Fake) {
                mapping.push(Entry {
                    replacement: replacement.clone(),
                    entity: c.entity.as_str().to_owned(),
                    value: value.to_owned(),
                });
            }
            pieces.push((replacement, true));
            at = c.end;
        }
        pieces.push((seg.text[at..].to_owned(), false));

        // verify: nothing above the threshold outside the replacements.
        let new: String = pieces.iter().map(|(p, _)| p.as_str()).collect();
        let mut replaced = Vec::new();
        let mut pos = 0;
        for (p, r) in &pieces {
            if *r {
                replaced.push(pos..pos + p.len());
            }
            pos += p.len();
        }
        for f in self.detector.findings(&new, &seg.context) {
            if replaced
                .iter()
                .all(|r| f.end <= r.start || f.start >= r.end)
            {
                return Err(Error::Leftover {
                    entity: f.entity.as_str().to_owned(),
                    detector: f.detector,
                });
            }
        }

        let quote = seg.enc.needs_quotes(&new);
        let mut raw = String::new();
        let mut ranges = Vec::new();
        if quote {
            raw.push('"');
        }
        for (p, r) in &pieces {
            let start = raw.len();
            seg.enc.encode_into(p, quote, &mut raw);
            if *r {
                ranges.push(start..raw.len());
            }
        }
        if quote {
            raw.push('"');
        }
        Ok((raw, ranges, pseudonyms))
    }
}

/// What a streamed run found.
pub struct Run {
    pub report: Report,
    pub input_spans: Vec<Range<usize>>,
    pub output_spans: Vec<Range<usize>>,
    /// What each `pseudonymize` and `fake` replacement stood for: for a
    /// mapping file only (ADR-0018), never for a report.
    pub mapping: Vec<Entry>,
}

#[derive(Default)]
struct State {
    /// Input bytes and lines before the current chunk.
    offset: usize,
    line: u64,
    out_offset: usize,
    findings: Vec<Finding>,
    input_spans: Vec<Range<usize>>,
    output_spans: Vec<Range<usize>>,
    mapping: Vec<Entry>,
    csv: CsvState,
}

#[derive(Default)]
struct CsvState {
    /// Rows read so far.
    row: u64,
    header: Vec<String>,
}

/// Lines of a chunk: the line and column of a byte offset.
struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
    first: u64,
}

impl<'a> LineIndex<'a> {
    fn new(text: &'a str, lines_before: u64) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self {
            text,
            starts,
            first: lines_before + 1,
        }
    }

    fn at(&self, offset: usize) -> (u64, u64) {
        let i = self.starts.partition_point(|&s| s <= offset) - 1;
        let column = self.text[self.starts[i]..offset.min(self.text.len())]
            .chars()
            .count()
            + 1;
        (self.first + i as u64, column as u64)
    }
}

// ------------------------------------------------------------------ segments

#[derive(Debug, Clone, Copy, PartialEq)]
enum Enc {
    Plain,
    Json,
    /// A JSON number; with a finding it becomes a string.
    JsonNumber,
    CsvQuoted,
    CsvUnquoted(u8),
}

impl Enc {
    fn needs_quotes(self, new: &str) -> bool {
        match self {
            Self::JsonNumber => true,
            Self::CsvUnquoted(d) => new
                .bytes()
                .any(|b| b == d || b == b'"' || b == b'\n' || b == b'\r'),
            _ => false,
        }
    }

    fn encode_into(self, s: &str, quoted: bool, out: &mut String) {
        match self {
            Self::Plain => out.push_str(s),
            Self::Json | Self::JsonNumber => {
                let json = serde_json::to_string(s).expect("a string serialises");
                out.push_str(&json[1..json.len() - 1]);
            }
            Self::CsvQuoted => out.push_str(&s.replace('"', "\"\"")),
            Self::CsvUnquoted(_) if quoted => out.push_str(&s.replace('"', "\"\"")),
            Self::CsvUnquoted(_) => out.push_str(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Place {
    Text,
    Json { pointer: String },
    Csv { row: u64, column: u64 },
}

#[derive(Debug, Clone)]
struct Segment {
    /// The raw bytes in the chunk.
    raw: Range<usize>,
    text: String,
    /// (decoded offset, raw offset) where an escape ends: between two marks
    /// the bytes are the same.
    marks: Vec<(usize, usize)>,
    context: String,
    enc: Enc,
    place: Place,
}

impl Segment {
    fn plain(text: &str) -> Self {
        Self {
            raw: 0..text.len(),
            text: text.to_owned(),
            marks: vec![(0, 0)],
            context: String::new(),
            enc: Enc::Plain,
            place: Place::Text,
        }
    }

    fn raw_of(&self, decoded: usize) -> usize {
        let i = self.marks.partition_point(|(d, _)| *d <= decoded) - 1;
        let (d, r) = self.marks[i];
        r + (decoded - d)
    }

    fn shift(&mut self, by: usize) {
        self.raw = self.raw.start + by..self.raw.end + by;
        for m in &mut self.marks {
            m.1 += by;
        }
    }

    fn location(&self, c: &Candidate, offset: usize, lines: &LineIndex) -> Location {
        match &self.place {
            Place::Text => {
                let (line, column) = lines.at(self.raw_of(c.start));
                Location::Span {
                    part: None,
                    start: (offset + self.raw_of(c.start)) as u64,
                    end: (offset + self.raw_of(c.end)) as u64,
                    line,
                    column,
                }
            }
            Place::Json { pointer } => {
                let (line, column) = lines.at(self.raw_of(c.start));
                Location::Span {
                    part: Some(pointer.clone()),
                    start: c.start as u64,
                    end: c.end as u64,
                    line,
                    column,
                }
            }
            Place::Csv { row, column } => Location::Cell {
                sheet: None,
                row: *row,
                column: *column,
                start: c.start as u64,
                end: c.end as u64,
            },
        }
    }
}

// ------------------------------------------------------------------ JSON

/// The strings and numbers of one JSON document, keys included. A key is a
/// segment without context; a value's context is its key (an array's
/// elements: the array's).
fn json_segments(src: &str, context: &str) -> Result<Vec<Segment>, Error> {
    serde_json::from_str::<serde::de::IgnoredAny>(src).map_err(|e| Error::Format(e.to_string()))?;
    let mut scan = JsonScan {
        s: src,
        b: src.as_bytes(),
        i: 0,
        path: Vec::new(),
        out: Vec::new(),
    };
    scan.value(context);
    Ok(scan.out)
}

struct JsonScan<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    path: Vec<String>,
    out: Vec<Segment>,
}

impl JsonScan<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn pointer(&self) -> String {
        self.path
            .iter()
            .map(|p| format!("/{}", p.replace('~', "~0").replace('/', "~1")))
            .collect()
    }

    fn value(&mut self, context: &str) {
        self.ws();
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                self.ws();
                if self.b[self.i] == b'}' {
                    self.i += 1;
                    return;
                }
                loop {
                    self.ws();
                    let mut key = self.string();
                    let name = key.text.clone();
                    self.path.push(name.clone());
                    key.place = Place::Json {
                        pointer: self.pointer(),
                    };
                    self.out.push(key);
                    self.ws();
                    self.i += 1; // ':'
                    self.value(&name);
                    self.path.pop();
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    if c == b'}' {
                        break;
                    }
                }
            }
            b'[' => {
                self.i += 1;
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return;
                }
                let mut n = 0;
                loop {
                    self.path.push(n.to_string());
                    self.value(context);
                    self.path.pop();
                    n += 1;
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    if c == b']' {
                        break;
                    }
                }
            }
            b'"' => {
                let mut seg = self.string();
                seg.context = context.to_owned();
                seg.place = Place::Json {
                    pointer: self.pointer(),
                };
                self.out.push(seg);
            }
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                while self.i < self.b.len() && b"+-0123456789.eE".contains(&self.b[self.i]) {
                    self.i += 1;
                }
                self.out.push(Segment {
                    raw: start..self.i,
                    text: self.s[start..self.i].to_owned(),
                    marks: vec![(0, start)],
                    context: context.to_owned(),
                    enc: Enc::JsonNumber,
                    place: Place::Json {
                        pointer: self.pointer(),
                    },
                });
            }
            _ => {
                while self.i < self.b.len() && self.b[self.i].is_ascii_alphabetic() {
                    self.i += 1;
                }
            }
        }
    }

    /// The string at `self.i` (its opening quote), decoded.
    fn string(&mut self) -> Segment {
        self.i += 1;
        let start = self.i;
        let mut text = String::new();
        let mut marks = vec![(0, start)];
        loop {
            match self.b[self.i] {
                b'"' => break,
                b'\\' => {
                    let (c, len) = self.escape();
                    text.push(c);
                    self.i += len;
                    marks.push((text.len(), self.i));
                }
                _ => {
                    let run_end = self.b[self.i..]
                        .iter()
                        .position(|b| *b == b'"' || *b == b'\\')
                        .map_or(self.b.len(), |p| self.i + p);
                    text.push_str(&self.s[self.i..run_end]);
                    self.i = run_end;
                }
            }
        }
        let end = self.i;
        self.i += 1;
        Segment {
            raw: start..end,
            text,
            marks,
            context: String::new(),
            enc: Enc::Json,
            place: Place::Text,
        }
    }

    /// The escape at `self.i` and its length in bytes.
    fn escape(&self) -> (char, usize) {
        let hex = |at: usize| u32::from_str_radix(&self.s[at..at + 4], 16).unwrap_or(0xFFFD);
        match self.b[self.i + 1] {
            b'n' => ('\n', 2),
            b't' => ('\t', 2),
            b'r' => ('\r', 2),
            b'b' => ('\u{8}', 2),
            b'f' => ('\u{c}', 2),
            b'u' => {
                let hi = hex(self.i + 2);
                if (0xD800..0xDC00).contains(&hi) && self.b.get(self.i + 6) == Some(&b'\\') {
                    let lo = hex(self.i + 8);
                    let c = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                    (char::from_u32(c).unwrap_or('\u{FFFD}'), 12)
                } else {
                    (char::from_u32(hi).unwrap_or('\u{FFFD}'), 6)
                }
            }
            other => (char::from(other), 2),
        }
    }
}

// ------------------------------------------------------------------ CSV

/// `,`, `;` or tab: whichever the first line has most of outside quotes.
fn sniff_delimiter(line: &str) -> u8 {
    let mut counts = [0usize; 3];
    let mut quoted = false;
    for b in line.bytes() {
        match b {
            b'"' => quoted = !quoted,
            b',' if !quoted => counts[0] += 1,
            b';' if !quoted => counts[1] += 1,
            b'\t' if !quoted => counts[2] += 1,
            _ => {}
        }
    }
    let max = counts.iter().copied().max().unwrap_or(0);
    b",;\t"[counts
        .iter()
        .position(|&c| c == max && max > 0)
        .unwrap_or(0)]
}

/// The cells of whole records (RFC 4180). The first row is the header: its
/// names are the context of their columns.
fn csv_segments(src: &str, d: u8, state: &mut CsvState) -> Result<Vec<Segment>, Error> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut column = 1u64;
    let mut row = state.row + 1;
    while i < b.len() {
        let mut seg = if b[i] == b'"' {
            i += 1;
            let start = i;
            let mut text = String::new();
            let mut marks = vec![(0, start)];
            loop {
                if i >= b.len() {
                    return Err(Error::Format(format!(
                        "row {row}: a quoted field is not closed"
                    )));
                }
                if b[i] == b'"' {
                    if b.get(i + 1) == Some(&b'"') {
                        text.push('"');
                        i += 2;
                        marks.push((text.len(), i));
                        continue;
                    }
                    break;
                }
                let run_end = b[i..]
                    .iter()
                    .position(|c| *c == b'"')
                    .map_or(b.len(), |p| i + p);
                text.push_str(&src[i..run_end]);
                i = run_end;
            }
            let end = i;
            i += 1;
            if i < b.len() && b[i] != d && b[i] != b'\n' && b[i] != b'\r' {
                return Err(Error::Format(format!(
                    "row {row}: text after a closing quote"
                )));
            }
            Segment {
                raw: start..end,
                text,
                marks,
                context: String::new(),
                enc: Enc::CsvQuoted,
                place: Place::Text,
            }
        } else {
            let start = i;
            while i < b.len() && b[i] != d && b[i] != b'\n' && b[i] != b'\r' {
                i += 1;
            }
            Segment {
                raw: start..i,
                text: src[start..i].to_owned(),
                marks: vec![(0, start)],
                context: String::new(),
                enc: Enc::CsvUnquoted(d),
                place: Place::Text,
            }
        };
        if row == 1 {
            state.header.push(seg.text.clone());
        } else if let Some(name) = state.header.get(column as usize - 1) {
            seg.context = name.clone();
        }
        seg.place = Place::Csv { row, column };
        if !seg.text.is_empty() {
            out.push(seg);
        }
        if i < b.len() && b[i] == d {
            i += 1;
            column += 1;
            continue;
        }
        if i < b.len() && b[i] == b'\r' {
            i += 1;
        }
        if i < b.len() && b[i] == b'\n' {
            i += 1;
        }
        state.row = row;
        row += 1;
        column = 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        Engine::new(&Policy::default(), None, b"seed", None).unwrap()
    }

    #[test]
    fn text_is_redacted_in_place() {
        let input = "Hallo Jonas Brandt,\nRückruf unter 0421 5550173.\n";
        let r = engine().redact_str(input, Format::Text).unwrap();
        assert_eq!(r.text, "Hallo <PERSON>,\nRückruf unter <PHONE_NUMBER>.\n");
        assert_eq!(&input[r.input_spans[1].clone()], "0421 5550173");
        assert_eq!(&r.text[r.output_spans[1].clone()], "<PHONE_NUMBER>");
        assert_eq!(
            r.report.findings[1].location,
            Location::Span {
                part: None,
                start: 35,
                end: 47,
                line: 2,
                column: 15
            }
        );
    }

    #[test]
    fn json_keeps_its_layout_and_escapes() {
        let input = "{\n  \"name\": \"Mira Albers\",\n  \"note\": \"Tel.\\u0020 0421 5550173\",\n  \"n\": 4711\n}\n";
        let r = engine().redact_str(input, Format::Json).unwrap();
        assert_eq!(
            r.text,
            "{\n  \"name\": \"<PERSON>\",\n  \"note\": \"Tel.  <PHONE_NUMBER>\",\n  \"n\": 4711\n}\n"
        );
        serde_json::from_str::<serde_json::Value>(&r.text).unwrap();
        let Location::Span { part, line, .. } = &r.report.findings[1].location else {
            panic!()
        };
        assert_eq!((part.as_deref(), *line), (Some("/note"), 3));
        assert_eq!(&input[r.input_spans[1].clone()], "0421 5550173");
    }

    #[test]
    fn a_number_that_is_personal_data_becomes_a_string() {
        let r = engine().redact_str("{\"telefon\": 04215550173}", Format::Json);
        // A JSON number cannot start with 0: serde refuses it, veilq too.
        assert!(matches!(r, Err(Error::Format(_))));
        let r = engine()
            .redact_str("{\"steuer_id\": 57381942656}", Format::Json)
            .unwrap();
        assert_eq!(r.text, "{\"steuer_id\": \"<TAX_ID>\"}");
    }

    #[test]
    fn ndjson_lines_are_documents() {
        let input = "{\"user\":\"lena.vogt@example.org\"}\n\n{\"by\":\"Jonas Brandt\"}\n";
        let r = engine().redact_str(input, Format::Ndjson).unwrap();
        assert_eq!(
            r.text,
            "{\"user\":\"<EMAIL_ADDRESS>\"}\n\n{\"by\":\"<PERSON>\"}\n"
        );
        let Location::Span { line, .. } = r.report.findings[1].location else {
            panic!()
        };
        assert_eq!(line, 3);
        assert!(matches!(
            engine().redact_str("{\"a\":1}\n{oops}\n", Format::Ndjson),
            Err(Error::Format(m)) if m.starts_with("line 2")
        ));
    }

    #[test]
    fn csv_cells_know_their_column() {
        let input =
            "name;geburtsdatum;notiz\nMira Albers;14.03.1987;\"Hinweis; Bruder Tobias Kern\"\n";
        let r = engine()
            .redact_str(input, Format::Csv { delimiter: None })
            .unwrap();
        assert_eq!(
            r.text,
            "name;geburtsdatum;notiz\n<PERSON>;<DATE_OF_BIRTH>;\"Hinweis; Bruder <PERSON>\"\n"
        );
        assert_eq!(
            r.report.findings[1].location,
            Location::Cell {
                sheet: None,
                row: 2,
                column: 2,
                start: 0,
                end: 10
            }
        );
    }

    #[test]
    fn a_csv_stand_in_with_a_delimiter_is_quoted() {
        let policy = Policy::parse("version = 1\n[defaults]\naction = \"fake\"\n").unwrap();
        let e = Engine::new(&policy, None, b"seed", None).unwrap();
        let input = "id,anschrift\n1,\"Lindenweg 12, 28195 Bremen\"\n2,Wohnhaft Lindenweg 12\n3,Lindenweg 12 28195 Bremen\n";
        let r = e
            .redact_str(input, Format::Csv { delimiter: None })
            .unwrap();
        let lines: Vec<&str> = r.text.lines().collect();
        assert!(
            lines[1].starts_with("1,\"") && lines[1].contains(", "),
            "{}",
            r.text
        );
        assert!(
            lines[3].starts_with("3,\"") && lines[3].ends_with('"'),
            "{}",
            r.text
        );
        assert!(!r.text.contains("Lindenweg"), "{}", r.text);
    }

    #[test]
    fn a_value_across_a_chunk_boundary_is_found() {
        let filler = "Zeile ohne Daten.\n".repeat(CHUNK / 18 + 10);
        let input =
            format!("{filler}Frau Mira\nAlbers, Anschrift Lindenweg 12,\n28195 Bremen\n{filler}");
        let r = engine().redact_str(&input, Format::Text).unwrap();
        assert!(!r.text.contains("Lindenweg"), "address across the cut");
        assert!(r.text.len() > 2 * filler.len());
        // The address over two lines is one finding: its line break goes with it.
        assert!(
            r.text.contains("Anschrift <ADDRESS>\n"),
            "{}",
            &r.text[filler.len()..filler.len() + 80]
        );
        assert_eq!(
            r.text.matches('\n').count(),
            input.matches('\n').count() - 1
        );
    }

    #[test]
    fn what_is_not_text_is_an_error() {
        assert!(matches!(
            engine().run(&[0x48, 0xff, 0x0a][..], None, Format::Text),
            Err(Error::NotUtf8 { line: 1 })
        ));
        assert!(Format::from_path(Path::new("a.docx")).is_err());
        assert!(Format::from_path(Path::new("Makefile")).is_err());
        assert_eq!(Format::from_path(Path::new("x.LOG")).unwrap(), Format::Text);
    }

    #[test]
    fn pseudonyms_are_reported() {
        let policy = Policy::parse(
            "version = 1\n[defaults]\naction = \"pseudonymize\"\n[pseudonymize]\nkey_env = \"K\"\n",
        )
        .unwrap();
        assert!(matches!(
            Engine::new(&policy, None, b"s", None),
            Err(Error::KeyMissing)
        ));
        let key = PseudonymKey::new(b"0123456789abcdef".to_vec()).unwrap();
        let e = Engine::new(&policy, Some(key), b"s", None).unwrap();
        let r = e
            .redact_str("mail an a.b@example.org und a.b@example.org", Format::Text)
            .unwrap();
        let p = r.report.findings[0].pseudonym.clone().unwrap();
        assert!(p.starts_with("[EMAIL_ADDRESS:"));
        assert_eq!(r.text, format!("mail an {p} und {p}"));
    }
}
