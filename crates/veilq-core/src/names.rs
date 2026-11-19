//! The names a file holds (ADR-0047): a name found once is found wherever
//! the file names it.
//!
//! Each person a detector finds at or above its threshold puts the words of
//! its name on a list of the run: three letters or more, no role word, no
//! brand, no system account, and no small word inside a capitalised name
//! (`von` in `Ursula von der Leyen`). Every text read after that finds each
//! word again as a whole word, matched like a deny list's value (ADR-0044):
//! any case, Turkish `İ`/`ı` folded, NFC and NFD, with the genitive. It is a
//! `PERSON` at the confidence of the finding it came from, detector
//! [`DETECTOR`]. In text read by OCR, a word cut by a window's edge counts
//! too: the start of a name's word with dots after it (`Brand..`), and the
//! end of one at the start of a line (`randt,`).
//!
//! The list holds a file's values too (ADR-0059): each other finding kept at
//! or above its threshold whose value has five characters or more, one a
//! digit or an `@` (a file number, an IBAN, a phone number, an address), is
//! found again wherever the file holds it whole, the spaces in it as they
//! come, as its type at its confidence, detector [`VALUES_DETECTOR`]. In OCR
//! text a word one edit from a name's word of six letters or more, with its
//! first letter, is that word read wrong (`Severln`), at nine tenths of its
//! confidence.
//!
//! A list learns forward. Each text read also notes its words outside its
//! findings, so the run knows when a name was learned after it already
//! stood unfound ([`Names::late`]); a caller with the whole input reads it
//! again with the list known from the start ([`Names::seeded`]).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::Error;
use crate::detect::{self, Candidate, Source};
use crate::finding::EntityType;

/// The detector of a file's names.
pub(crate) const DETECTOR: &str = "names:file";
/// The detector of a file's values (ADR-0059).
pub(crate) const VALUES_DETECTOR: &str = "values:file";

/// Letters a word of a name needs to go on the list.
const MIN_WORD: usize = 3;
/// Letters the end of a word needs to count at the start of a line.
const MIN_TAIL: usize = 5;
/// The words of a firm's legal form, as [`key`] has them: a party's firm
/// (ADR-0059) puts its name on the list, not `GmbH`.
const LEGAL_FORMS: &[&str] = &[
    "gmbh",
    "ggmbh",
    "mbh",
    "ohg",
    "gbr",
    "partg",
    "mbb",
    "haftungsbeschränkt",
];
/// The ends of the words a firm's name shares with every firm of its trade
/// (`Kessler Hausverwaltung GmbH`, `Albers Malerbetrieb`), and such words
/// whole: learned from a firm, they would make every `Hausverwaltung` of
/// the file a person. Only a firm's own word goes on the list.
const TRADE_ENDINGS: &[&str] = &[
    "verwaltung",
    "betrieb",
    "betriebe",
    "bau",
    "handel",
    "service",
    "services",
    "technik",
    "immobilien",
    "beratung",
    "dienst",
    "dienste",
    "dienstleistungen",
    "gesellschaft",
    "gruppe",
    "holding",
    "consulting",
    "versicherung",
    "logistik",
    "kanzlei",
    "praxis",
    "apotheke",
    "bäckerei",
    "metzgerei",
    "werkstatt",
    "transporte",
    "systeme",
    "solutions",
    "partner",
];
const TRADE_WORDS: &[&str] = &[
    "söhne",
    "sohn",
    "gebrüder",
    "gebr",
    "erben",
    "nachf",
    "co",
    "und",
    "group",
];
/// Characters a value needs to go on the list, its spaces left out.
const MIN_VALUE: usize = 5;
/// Characters a value may have to go on the list: each start of a value is
/// kept, so a value of `n` characters costs `n²/2` bytes, and a mail address
/// of 64,000 characters took 2 GB (security review before 0.7.0). Longer
/// values are still found where a detector finds them, only not repeated.
const MAX_VALUE: usize = 128;
/// Letters a name's word needs to be found in a word read one edit off.
const MIN_SIMILAR: usize = 6;
/// The share of its word's confidence a word read one edit off has.
const SIMILAR: f32 = 0.9;

/// The names of one run, shared by the clones of its engine: a file's
/// parts, pages and frames learn into one list.
pub(crate) struct Names {
    inner: Mutex<Inner>,
    /// The file's values are on the list too (`[names] values`).
    values: bool,
    /// In OCR text a word one edit off a name's is found (`[names] similar`).
    similar: bool,
}

impl Default for Names {
    fn default() -> Self {
        Self::new(true, true)
    }
}

/// A value of the list: its type and the highest confidence it was found at.
#[derive(Clone)]
struct Value {
    entity: EntityType,
    confidence: f32,
}

#[derive(Default)]
struct Inner {
    /// Each word as its key ([`key`]), with the highest confidence it was
    /// found at.
    /// Shared with a reading in progress, copied only when one is learned
    /// meanwhile, not per text (#129).
    words: std::sync::Arc<HashMap<String, f32>>,
    /// The words in the order they were learned, as written.
    order: Vec<String>,
    /// Each value as its key ([`value_key`]).
    values: HashMap<String, Value>,
    /// The values' keys in the order they were learned.
    value_order: Vec<String>,
    /// The texts read so far.
    texts: usize,
    /// Per word ([`key`]), the first text it stood in unfound.
    seen: HashMap<String, usize>,
    /// [`seen`](Self::seen) in OCR text only, the one text where a word one
    /// edit off a name's is found.
    seen_ocr: HashMap<String, usize>,
    /// Per run of letters and digits ([`key`]), the first text it stood in
    /// unfound.
    seen_tokens: HashMap<String, usize>,
    /// A word was learned after it stood unfound in an earlier text.
    late: bool,
    /// The words, whole, built for the length of `order`.
    whole: Option<(usize, Regex)>,
    /// In OCR text, built only there: a word's start with dots after it, a
    /// word's end at the start of a line; `None` inside when there is none.
    cut: Option<(usize, Option<Regex>)>,
    /// Every start of a value's key, the key itself too: a value is found
    /// by walking a text along them, in time linear in the text however
    /// many values a file holds (a regex of them all grew with each one).
    value_prefixes: HashSet<String>,
}

/// The size a matcher of the list may compile to.
const LIMIT: usize = 64 << 20;

/// A matcher of the list. One too large is an error, not a panic (fail
/// closed, as a deny list's in `detect::deny_rule`): the run stops.
fn compile(pattern: &str) -> Result<Regex, Error> {
    compile_with(pattern, LIMIT)
}

fn compile_with(pattern: &str, limit: usize) -> Result<Regex, Error> {
    regex::RegexBuilder::new(pattern)
        .size_limit(limit)
        .build()
        .map_err(|e| {
            Error::Format(format!(
                "the names of this file (ADR-0047) make a matcher too large ({e}); set `[names] everywhere = false` to read it without them"
            ))
        })
}

impl Names {
    /// An empty list; `values` puts a file's values on it too, `similar`
    /// finds a name's word read one edit off in OCR text (ADR-0059).
    pub(crate) fn new(values: bool, similar: bool) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            values,
            similar,
        }
    }

    /// A new list that knows the words and values of this one from the
    /// start: for a second reading of the same input.
    pub(crate) fn seeded(&self) -> Self {
        let inner = self.lock();
        Self {
            inner: Mutex::new(Inner {
                words: inner.words.clone(),
                order: inner.order.clone(),
                values: inner.values.clone(),
                value_order: inner.value_order.clone(),
                value_prefixes: inner.value_prefixes.clone(),
                ..Inner::default()
            }),
            values: self.values,
            similar: self.similar,
        }
    }

    /// Whether a word was learned after it stood unfound in an earlier text:
    /// a second reading would find it there.
    pub(crate) fn late(&self) -> bool {
        self.lock().late
    }

    /// The candidates of the list's words and values in `text`.
    pub(crate) fn candidates(&self, text: &str, source: Source) -> Result<Vec<Candidate>, Error> {
        let mut out = self.value_candidates(text)?;
        let (whole, cut, words) = {
            let mut inner = self.lock();
            if inner.order.is_empty() {
                return Ok(out);
            }
            let whole = inner.whole()?;
            let cut = if source == Source::Ocr {
                inner.cut()?
            } else {
                None
            };
            (whole, cut, inner.words.clone())
        };
        let person = EntityType::new("PERSON").expect("a valid type");
        let mut push = |start: usize, end: usize, confidence: f32| {
            out.push(Candidate {
                start,
                end,
                entity: person.clone(),
                confidence,
                detector: DETECTOR.to_owned(),
            });
        };
        if source == Source::Ocr && self.similar {
            for (start, w) in words_at(text) {
                if let Some(c) = similar_confidence(&words, w) {
                    push(start, start + w.len(), c);
                }
            }
        }
        for caps in whole.captures_iter(text) {
            let m = caps.name("pii").expect("the group matches");
            if let Some(c) = confidence_of(&words, m.as_str()) {
                push(m.start(), m.end(), c);
            }
        }
        if let Some(cut) = &cut {
            for caps in cut.captures_iter(text) {
                let m = caps
                    .name("head")
                    .or_else(|| caps.name("tail"))
                    .expect("one group matches");
                let part = key(m.as_str());
                let head = caps.name("head").is_some();
                let c = words
                    .iter()
                    .filter(|(w, _)| {
                        if head {
                            w.starts_with(&part)
                        } else {
                            w.ends_with(&part)
                        }
                    })
                    .map(|(_, c)| *c)
                    .reduce(f32::max);
                if let Some(c) = c {
                    push(m.start(), m.end(), c);
                }
            }
        }
        Ok(out)
    }

    /// The candidates of the list's values in `text`: each held whole, not
    /// inside a longer run of letters or digits, in any case, with a space
    /// or none between any two of its characters; the longest at a start.
    fn value_candidates(&self, text: &str) -> Result<Vec<Candidate>, Error> {
        let inner = self.lock();
        if inner.value_order.is_empty() {
            return Ok(Vec::new());
        }
        let alnum = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
        let space = |c: char| matches!(c, ' ' | '\t' | '\u{a0}');
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let (start, first) = chars[i];
            if space(first) || (alnum(Some(first)) && alnum(text[..start].chars().next_back())) {
                i += 1;
                continue;
            }
            let mut walked = String::new();
            // Every value that ends on the way: the longest that stands
            // whole is taken, a shorter one when the longer runs on into a
            // letter or a digit (`KD-48213` in `KD-48213-AB` beside a learned
            // `KD-48213-A`).
            let mut ends = Vec::new();
            let mut j = i;
            while j < chars.len() {
                let (at, c) = chars[j];
                if space(c) {
                    // One space between two characters of the value.
                    if walked.is_empty() || chars.get(j + 1).is_none_or(|&(_, n)| space(n)) {
                        break;
                    }
                    j += 1;
                    continue;
                }
                walked.push_str(&key(c.encode_utf8(&mut [0; 4])));
                if !inner.value_prefixes.contains(&walked) {
                    break;
                }
                if inner.values.contains_key(&walked) {
                    ends.push((at + c.len_utf8(), j, walked.clone()));
                }
                j += 1;
            }
            let whole = ends
                .into_iter()
                .rev()
                .find(|(end, _, _)| !alnum(text[*end..].chars().next()));
            match whole {
                Some((end, last, k)) => {
                    let v = &inner.values[&k];
                    out.push(Candidate {
                        start,
                        end,
                        entity: v.entity.clone(),
                        confidence: v.confidence,
                        detector: VALUES_DETECTOR.to_owned(),
                    });
                    i = last + 1;
                }
                _ => i += 1,
            }
        }
        Ok(out)
    }

    /// Learns the words of `found`, the persons kept in `text`, and the
    /// values of `values`, the other findings kept there; notes the words
    /// of `text` outside every finding in `kept`. Returns whether a word or
    /// a value was new to the list.
    pub(crate) fn read(
        &self,
        text: &str,
        source: Source,
        found: &[(usize, usize, f32)],
        values: &[(usize, usize, &EntityType, f32)],
        kept: &[(usize, usize)],
    ) -> bool {
        let mut inner = self.lock();
        let now = inner.texts;
        let mut new = false;
        for &(start, end, entity, confidence) in values.iter().filter(|_| self.values) {
            let k = value_key(&text[start..end]);
            if !value_learnable(&k) {
                continue;
            }
            match inner.values.get_mut(&k) {
                Some(v) => v.confidence = v.confidence.max(confidence),
                None => {
                    // Late when it, glued, or any run of it stood unfound
                    // before; a value of short runs only (`12 34 56 78`)
                    // leaves no run to compare, so any earlier text counts.
                    let runs: Vec<String> = tokens_at(&text[start..end])
                        .map(|(_, t)| key(t))
                        .filter(|t| t.chars().count() >= MIN_WORD)
                        .collect();
                    let before = |t: &String| inner.seen_tokens.get(t).is_some_and(|&f| f < now);
                    if before(&k) || runs.iter().any(before) || (runs.is_empty() && now > 0) {
                        inner.late = true;
                    }
                    let mut prefix = String::new();
                    for c in k.chars() {
                        prefix.push(c);
                        inner.value_prefixes.insert(prefix.clone());
                    }
                    inner.values.insert(
                        k.clone(),
                        Value {
                            entity: entity.clone(),
                            confidence,
                        },
                    );
                    inner.value_order.push(k);
                    new = true;
                }
            }
        }
        for &(start, end, confidence) in found {
            let span = &text[start..end];
            let capitalised = detect::name_words(span).any(starts_upper);
            let firm = is_firm(span);
            for w in detect::name_words(span) {
                if !learnable(w, capitalised) || (firm && trade_word(w)) {
                    continue;
                }
                let k = key(w);
                match inner.words.get(&k).copied() {
                    Some(c) => {
                        if confidence > c {
                            std::sync::Arc::make_mut(&mut inner.words).insert(k, confidence);
                        }
                    }
                    None => {
                        // Late too when a word one edit off stood unfound
                        // before in OCR text: it would be found there. Only
                        // those are compared, not every word seen (#129).
                        let off = self.similar
                            && k.chars().count() >= MIN_SIMILAR
                            && inner
                                .seen_ocr
                                .iter()
                                .any(|(s, &first)| first < now && one_edit(&k, s));
                        if off || inner.seen.get(&k).is_some_and(|&first| first < now) {
                            inner.late = true;
                        }
                        std::sync::Arc::make_mut(&mut inner.words).insert(k, confidence);
                        inner.order.push(w.to_owned());
                        new = true;
                    }
                }
            }
        }
        for (start, w) in words_at(text) {
            if w.chars().count() < MIN_WORD || kept.iter().any(|&(s, e)| s <= start && start < e) {
                continue;
            }
            let k = key(w);
            // As `similar_confidence` would find it.
            if source == Source::Ocr
                && self.similar
                && k.chars().count() + 1 >= MIN_SIMILAR
                && starts_upper(w)
            {
                inner.seen_ocr.entry(k.clone()).or_insert(now);
            }
            inner.seen.entry(k).or_insert(now);
        }
        if self.values {
            for (start, t) in tokens_at(text) {
                if t.chars().count() < MIN_WORD
                    || kept.iter().any(|&(s, e)| s <= start && start < e)
                {
                    continue;
                }
                inner.seen_tokens.entry(key(t)).or_insert(now);
            }
        }
        inner.texts += 1;
        new
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Inner {
    /// The words, whole, with the genitive.
    fn whole(&mut self) -> Result<Regex, Error> {
        if let Some((n, r)) = &self.whole
            && *n == self.order.len()
        {
            return Ok(r.clone());
        }
        let r = compile(&format!(
            r"(?i)\b(?P<pii>(?:{})(?:s|'s|’s)?)\b",
            detect::alternation(self.order.clone())
        ))?;
        self.whole = Some((self.order.len(), r.clone()));
        Ok(r)
    }

    /// The words cut by a window's edge, for OCR text.
    fn cut(&mut self) -> Result<Option<Regex>, Error> {
        if let Some((n, r)) = &self.cut
            && *n == self.order.len()
        {
            return Ok(r.clone());
        }
        let heads: Vec<String> = self
            .order
            .iter()
            .flat_map(|w| {
                let chars: Vec<char> = w.chars().collect();
                (MIN_WORD..chars.len()).map(move |n| chars[..n].iter().collect::<String>())
            })
            .collect();
        let tails: Vec<String> = self
            .order
            .iter()
            .flat_map(|w| {
                let chars: Vec<char> = w.chars().collect();
                (1..=chars.len().saturating_sub(MIN_TAIL))
                    .map(move |n| chars[n..].iter().collect::<String>())
            })
            .collect();
        let mut parts = Vec::new();
        if !heads.is_empty() {
            parts.push(format!(
                r"\b(?P<head>(?:{}))(?:\.{{2,}}|…)",
                detect::alternation(heads)
            ));
        }
        if !tails.is_empty() {
            parts.push(format!(
                r"(?m:^)[^\p{{L}}\p{{N}}\n]{{0,3}}(?P<tail>(?:{}))\b",
                detect::alternation(tails)
            ));
        }
        let r = if parts.is_empty() {
            None
        } else {
            Some(compile(&format!("(?i){}", parts.join("|")))?)
        };
        self.cut = Some((self.order.len(), r.clone()));
        Ok(r)
    }
}

/// A value as the list keys it: as [`key`], its spaces left out.
fn value_key(v: &str) -> String {
    key(v).chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether a value (its key) goes on the list: five characters or more, one
/// of them a digit or an `@`. A word alone is a name's to learn, or none.
fn value_learnable(k: &str) -> bool {
    (MIN_VALUE..=MAX_VALUE).contains(&k.chars().count())
        && k.chars().any(|c| c.is_numeric() || c == '@')
}

/// The confidence of a word read one edit off a name's word of
/// [`MIN_SIMILAR`] letters or more with the same first letter; `None` for
/// a word of the list itself (found whole) and for any other.
fn similar_confidence(words: &HashMap<String, f32>, w: &str) -> Option<f32> {
    let k = key(w);
    // A name is written with a capital, and OCR keeps it: a lowercase word
    // one letter off (`richtet`, `Richter`) is a word of the text.
    if k.chars().count() + 1 < MIN_SIMILAR || !starts_upper(w) || confidence_of(words, w).is_some()
    {
        return None;
    }
    words
        .iter()
        .filter(|(n, _)| n.chars().count() >= MIN_SIMILAR && one_edit(&k, n))
        .map(|(_, c)| c * SIMILAR)
        .reduce(f32::max)
}

/// Whether `a` and `b` share their first letter and are one edit apart: a
/// letter changed, added, dropped, or two beside each other swapped.
fn one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.first() != b.first() || a == b || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let lead = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let (ra, rb) = (&a[lead..], &b[lead..]);
    match ra.len().cmp(&rb.len()) {
        std::cmp::Ordering::Equal => {
            ra[1..] == rb[1..]
                || (ra.len() >= 2 && ra[0] == rb[1] && ra[1] == rb[0] && ra[2..] == rb[2..])
        }
        std::cmp::Ordering::Less => ra == &rb[1..],
        std::cmp::Ordering::Greater => &ra[1..] == rb,
    }
}

/// A word as the list keys it: NFC, lower case, Turkish `ı` and `İ` as `i`.
fn key(w: &str) -> String {
    w.nfc()
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ı' { 'i' } else { c })
        .filter(|c| *c != '\u{307}')
        .collect()
}

/// The confidence of the word `m` names, with or without its genitive.
fn confidence_of(words: &HashMap<String, f32>, m: &str) -> Option<f32> {
    let k = key(m);
    if let Some(c) = words.get(&k) {
        return Some(*c);
    }
    ["'s", "’s", "s"]
        .iter()
        .filter_map(|g| k.strip_suffix(g))
        .find_map(|stem| words.get(stem).copied())
}

/// Whether a word of a person's name goes on the list. A small word inside a
/// capitalised name is a particle (`von`, `van`), not a name, and so is a
/// particle written with a capital at the start of one (`Von Hagen, Ole`):
/// learned, it would make every `von` of the file a person.
fn learnable(w: &str, capitalised: bool) -> bool {
    w.chars().count() >= MIN_WORD
        && (!capitalised || starts_upper(w))
        && !detect::PARTICLES.contains(&key(w).as_str())
        && !LEGAL_FORMS.contains(&key(w).as_str())
        && !detect::only_role_words(w)
        && !detect::brand_word(w)
        && !detect::ACCOUNTS.contains(&w.to_lowercase().as_str())
}

/// Whether a person's span is a firm: it ends in a legal form.
fn is_firm(span: &str) -> bool {
    let last = span
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("");
    let k = key(last);
    LEGAL_FORMS.contains(&k.as_str()) || ["ag", "kg", "ug", "ek", "k"].contains(&k.as_str())
}

/// Whether a word of a firm's name is its trade's ([`TRADE_ENDINGS`]).
fn trade_word(w: &str) -> bool {
    let k = key(w);
    TRADE_WORDS.contains(&k.as_str()) || TRADE_ENDINGS.iter().any(|e| k.ends_with(e))
}

fn starts_upper(w: &str) -> bool {
    w.chars().next().is_some_and(char::is_uppercase)
}

/// The runs of letters in `text`, with where each starts.
fn words_at(text: &str) -> impl Iterator<Item = (usize, &str)> {
    runs_at(text, char::is_alphabetic)
}

/// The runs of letters and digits in `text`, with where each starts.
fn tokens_at(text: &str) -> impl Iterator<Item = (usize, &str)> {
    runs_at(text, char::is_alphanumeric)
}

/// The runs of characters `inside` takes in `text`, with where each starts.
fn runs_at(text: &str, inside: fn(char) -> bool) -> impl Iterator<Item = (usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (inside(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &text[s..]));
    }
    out.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(names: &Names, text: &str, source: Source) -> Vec<String> {
        names
            .candidates(text, source)
            .unwrap()
            .into_iter()
            .map(|c| text[c.start..c.end].to_owned())
            .collect()
    }

    fn learn(names: &Names, text: &str, name: &str) -> bool {
        let start = text.find(name).unwrap();
        let end = start + name.len();
        names.read(
            text,
            Source::Text,
            &[(start, end, 0.9)],
            &[],
            &[(start, end)],
        )
    }

    #[test]
    fn a_name_found_once_is_found_again_in_every_form() {
        let names = Names::default();
        assert!(learn(
            &names,
            "Termin mit Jonas Brandt am Montag",
            "Jonas Brandt"
        ));
        assert_eq!(found(&names, "BRANDT rief an", Source::Text), ["BRANDT"]);
        assert_eq!(found(&names, "Brandts Akte", Source::Text), ["Brandts"]);
        assert_eq!(found(&names, "jonas fragt", Source::Text), ["jonas"]);
        // A whole word only.
        assert!(found(&names, "Brandtschutz", Source::Text).is_empty());
        let c = &names.candidates("Herr Brandt", Source::Text).unwrap()[0];
        assert_eq!((c.confidence, c.detector.as_str()), (0.9, DETECTOR));
        // Learned once: no new word.
        assert!(!learn(&names, "Jonas Brandt", "Jonas Brandt"));
    }

    #[test]
    fn role_words_brands_and_particles_stay_off_the_list() {
        let names = Names::default();
        learn(&names, "Herr Ole von Hagen", "Herr Ole von Hagen");
        learn(&names, "PowerShell", "PowerShell");
        learn(&names, "root", "root");
        // `Ole` has three letters, `von` is a particle, `Herr` a role word.
        assert_eq!(
            found(
                &names,
                "Herr Ole sprach von Hagen und PowerShell als root",
                Source::Text
            ),
            ["Ole", "Hagen"]
        );
        // A name written all in lower case (a home directory) is learned.
        learn(&names, "mira.albers", "mira.albers");
        assert_eq!(found(&names, "Frau Albers", Source::Text), ["Albers"]);
        // A particle with a capital at a name's start is no name either:
        // learned, every `von` of the file would be a person.
        let names = Names::default();
        learn(&names, "Von Hagen, Ole", "Von Hagen, Ole");
        learn(&names, "Van Dyke", "Van Dyke");
        assert_eq!(
            found(&names, "Brief von Ole an van Dyke und Hagen", Source::Text),
            ["Ole", "Dyke", "Hagen"]
        );
        // A party's firm (ADR-0059) puts its name on the list, not its
        // legal form.
        let names = Names::default();
        learn(
            &names,
            "Wendrich UG (haftungsbeschränkt) mbH GmbH",
            "Wendrich UG (haftungsbeschränkt) mbH GmbH",
        );
        assert_eq!(
            found(&names, "Wendrich GmbH, haftungsbeschränkt", Source::Text),
            ["Wendrich"]
        );
        // Nor the words it shares with every firm of its trade.
        let names = Names::default();
        learn(
            &names,
            "Kessler Hausverwaltung GmbH",
            "Kessler Hausverwaltung GmbH",
        );
        learn(
            &names,
            "Albers & Söhne Malerbetrieb KG",
            "Albers & Söhne Malerbetrieb KG",
        );
        assert_eq!(
            found(
                &names,
                "die Hausverwaltung, Kessler, der Malerbetrieb und Söhne von Albers",
                Source::Text
            ),
            ["Kessler", "Albers"]
        );
        // A person's name keeps such a word: `Bau` is a surname too.
        let person = Names::default();
        learn(&person, "Herr Jonas Bau", "Jonas Bau");
        assert_eq!(found(&person, "Grüße an Bau", Source::Text), ["Bau"]);
    }

    #[test]
    fn a_list_too_large_for_a_matcher_is_an_error_not_a_panic() {
        let words: Vec<String> = (0..2000).map(|i| format!("Name{i:04}x")).collect();
        let pattern = format!(r"(?i)\b(?:{})\b", detect::alternation(words));
        assert!(matches!(
            compile_with(&pattern, 1 << 10),
            Err(Error::Format(m)) if m.contains("everywhere = false")
        ));
        assert!(compile_with(&pattern, LIMIT).is_ok());
    }

    #[test]
    fn a_word_cut_by_a_windows_edge_is_found_in_a_picture() {
        let names = Names::default();
        learn(&names, "Jonas Brandtmeier", "Jonas Brandtmeier");
        for (text, want) in [
            ("Brandtmeier, Jon..", vec!["Brandtmeier", "Jon"]),
            ("Brandtm… Akte", vec!["Brandtm"]),
            ("andtmeier, Jonas", vec!["andtmeier", "Jonas"]),
        ] {
            let mut got = found(&names, text, Source::Ocr);
            got.sort();
            let mut want: Vec<String> = want.into_iter().map(String::from).collect();
            want.sort();
            assert_eq!(got, want, "{text}");
        }
        // Written text has no window: a cut word stays.
        assert_eq!(
            found(&names, "Brandtm… Akte", Source::Text),
            Vec::<String>::new()
        );
        // A short end, or an end inside a line, is no name.
        assert!(found(&names, "eier kaufen", Source::Ocr).is_empty());
        assert!(found(&names, "Akte andtmeier", Source::Ocr).is_empty());
    }

    #[test]
    fn a_name_learned_after_it_stood_unfound_is_late() {
        let names = Names::default();
        names.read("Brandt ruft an", Source::Text, &[], &[], &[]);
        assert!(!names.late());
        learn(&names, "Jonas Brandt", "Jonas Brandt");
        assert!(names.late());
        // Seeded, a second reading knows it from the start.
        let again = names.seeded();
        assert_eq!(found(&again, "Brandt ruft an", Source::Text), ["Brandt"]);
        again.read(
            "Brandt ruft an",
            Source::Text,
            &[(0, 6, 0.9)],
            &[],
            &[(0, 6)],
        );
        assert!(!again.late());
        // Found where it stood is not late.
        let names = Names::default();
        learn(&names, "Mira Albers ruft an", "Mira Albers");
        learn(&names, "Mira Albers", "Mira Albers");
        assert!(!names.late());
    }

    #[test]
    fn a_word_one_edit_off_is_late_in_ocr_text_only() {
        // Outside OCR no word one edit off is found: a second reading would
        // find nothing new there (#129).
        let names = Names::default();
        names.read("Wendlch ruft an", Source::Text, &[], &[], &[]);
        learn(&names, "Jonas Wendich", "Jonas Wendich");
        assert!(!names.late());
        let names = Names::default();
        names.read("Wendlch ruft an", Source::Ocr, &[], &[], &[]);
        learn(&names, "Jonas Wendich", "Jonas Wendich");
        assert!(names.late());
    }

    fn learn_value(names: &Names, text: &str, value: &str, entity: &str) -> bool {
        let start = text.find(value).unwrap();
        let end = start + value.len();
        let entity = EntityType::new(entity).unwrap();
        names.read(
            text,
            Source::Text,
            &[],
            &[(start, end, &entity, 0.8)],
            &[(start, end)],
        )
    }

    fn typed(names: &Names, text: &str) -> Vec<(String, String)> {
        names
            .candidates(text, Source::Text)
            .unwrap()
            .into_iter()
            .map(|c| {
                (
                    text[c.start..c.end].to_owned(),
                    c.entity.as_str().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn a_value_found_once_is_found_wherever_the_file_holds_it() {
        let names = Names::default();
        assert!(learn_value(
            &names,
            "Aktenzeichen: B07311",
            "B07311",
            "COURT_FILE_NUMBER"
        ));
        learn_value(
            &names,
            "IBAN DE89 3704 0044 0532 0130 00",
            "DE89 3704 0044 0532 0130 00",
            "IBAN_CODE",
        );
        assert_eq!(
            typed(&names, "Journal b07311 offen"),
            [("b07311".to_owned(), "COURT_FILE_NUMBER".to_owned())]
        );
        // Its spaces as they come.
        assert_eq!(
            typed(&names, "Konto DE89370400440532013000."),
            [("DE89370400440532013000".to_owned(), "IBAN_CODE".to_owned())]
        );
        let c = &names.candidates("B07311", Source::Text).unwrap()[0];
        assert_eq!((c.confidence, c.detector.as_str()), (0.8, VALUES_DETECTOR));
        // Whole only: not inside a longer number or word.
        assert!(typed(&names, "B073119 und XB07311").is_empty());
        // A longer value on the list that runs on into a letter leaves the
        // shorter one whole before it.
        learn_value(&names, "Kunde B07311-A", "B07311-A", "CUSTOMER_NUMBER");
        assert_eq!(
            typed(&names, "Notiz B07311-AB"),
            [("B07311".to_owned(), "COURT_FILE_NUMBER".to_owned())]
        );
        // A short value, a very long one, and one without a digit, stays
        // off the list.
        assert!(!learn_value(&names, "Nr. 4711", "4711", "ACCOUNT_NUMBER"));
        let long = format!("{}@beispiel.de", "x1".repeat(100));
        assert!(!learn_value(
            &names,
            &format!("Kontakt: {long}"),
            &long,
            "EMAIL_ADDRESS"
        ));
        assert!(!learn_value(&names, "in Bremen", "Bremen", "LOCATION"));
        // Off with `[names] values = false`.
        let off = Names::new(false, true);
        learn_value(&off, "Aktenzeichen: B07311", "B07311", "COURT_FILE_NUMBER");
        assert!(typed(&off, "Journal B07311").is_empty());
    }

    #[test]
    fn many_values_are_found_in_time_linear_in_the_text() {
        // A list of 20,000 rows, each with its own number, read row by row:
        // a regex of every value rebuilt per row took minutes for 1,000.
        let names = Names::default();
        let t = std::time::Instant::now();
        for i in 0..20_000 {
            let row = format!("Kunde KD-{i:06}");
            learn_value(&names, &row, &row[6..], "ID");
            names.candidates(&row, Source::Text).unwrap();
        }
        assert!(
            t.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            t.elapsed()
        );
        // The longest at a start, with or without spaces, whole only.
        learn_value(&names, "Kunde KD-0000123", "KD-0000123", "ID");
        assert_eq!(
            typed(&names, "kd-000012 3, KD-0000123 und XKD-000001"),
            [
                ("kd-000012 3".to_owned(), "ID".to_owned()),
                ("KD-0000123".to_owned(), "ID".to_owned())
            ]
        );
    }

    #[test]
    fn a_value_learned_after_it_stood_unfound_is_late() {
        let names = Names::default();
        names.read("Journal B07311 offen", Source::Text, &[], &[], &[]);
        assert!(!names.late());
        learn_value(
            &names,
            "Aktenzeichen: B07311",
            "B07311",
            "COURT_FILE_NUMBER",
        );
        assert!(names.late());
        let again = names.seeded();
        assert_eq!(typed(&again, "Journal B07311 offen").len(), 1);
        // Written another way before: glued, or spaced into short runs.
        let glued = Names::default();
        glued.read("Notiz KD48213", Source::Text, &[], &[], &[]);
        learn_value(&glued, "Kundennummer: KD 48213", "KD 48213", "ID");
        assert!(glued.late());
        let spaced = Names::default();
        spaced.read("Notiz 12 34 56 78", Source::Text, &[], &[], &[]);
        learn_value(
            &spaced,
            "Telefon: 12 34 56 78",
            "12 34 56 78",
            "PHONE_NUMBER",
        );
        assert!(spaced.late());
    }

    #[test]
    fn a_name_read_one_letter_off_is_the_name_in_a_picture() {
        let names = Names::default();
        learn(&names, "Kunde: Severin Wendrich", "Severin Wendrich");
        for (text, want) in [
            ("Kopie an Severln", "Severln"),
            ("Re: Wendrlch", "Wendrlch"),
            ("Sevrein", "Sevrein"),
            ("Wendrichx", "Wendrichx"),
            ("Wendich", "Wendich"),
        ] {
            assert_eq!(found(&names, text, Source::Ocr), [want], "{text}");
        }
        let c = &names.candidates("Severln", Source::Ocr).unwrap()[0];
        assert!((c.confidence - 0.9 * 0.9).abs() < 1e-6);
        // Written text is read as written; another first letter, two edits
        // and a short word are other words.
        assert!(found(&names, "Severln", Source::Text).is_empty());
        for text in ["Heverin", "Sevelim", "Wendlrch x"] {
            assert!(found(&names, text, Source::Ocr).is_empty(), "{text}");
        }
        // A lowercase word one letter off a name is a word of the text
        // (`richtet` beside `Richter`), not the name read badly.
        let lower = Names::default();
        learn(&lower, "Kundin: Ute Richter", "Ute Richter");
        assert!(found(&lower, "es richtet sich", Source::Ocr).is_empty());
        assert_eq!(found(&lower, "Frau Rlchter", Source::Ocr), ["Rlchter"]);
        let short = Names::default();
        learn(&short, "Herr Ole Kranz", "Ole Kranz");
        assert!(found(&short, "Krenz", Source::Ocr).is_empty());
        // Off with `[names] similar = false`.
        let off = Names::new(true, false);
        learn(&off, "Kunde: Severin Wendrich", "Severin Wendrich");
        assert!(found(&off, "Severln", Source::Ocr).is_empty());
    }

    #[test]
    fn one_edit_apart() {
        assert!(one_edit("severin", "severln"));
        assert!(one_edit("severin", "sevrein"));
        assert!(one_edit("wendrich", "wendrichs"));
        assert!(one_edit("wendrich", "wendich"));
        assert!(!one_edit("wendrich", "wendrich"));
        assert!(!one_edit("wendrich", "mendrich"), "first letter");
        assert!(!one_edit("wendrich", "wednrihc"));
    }
}
