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
//! A list learns forward. Each text read also notes its words outside its
//! findings, so the run knows when a name was learned after it already
//! stood unfound ([`Names::late`]); a caller with the whole input reads it
//! again with the list known from the start ([`Names::seeded`]).

use std::collections::HashMap;
use std::sync::Mutex;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use crate::detect::{self, Candidate, Source};
use crate::finding::EntityType;

/// The detector of a file's names.
pub(crate) const DETECTOR: &str = "names:file";

/// Letters a word of a name needs to go on the list.
const MIN_WORD: usize = 3;
/// Letters the end of a word needs to count at the start of a line.
const MIN_TAIL: usize = 5;

/// The names of one run, shared by the clones of its engine: a file's
/// parts, pages and frames learn into one list.
#[derive(Default)]
pub(crate) struct Names(Mutex<Inner>);

#[derive(Default)]
struct Inner {
    /// Each word as its key ([`key`]), with the highest confidence it was
    /// found at.
    words: HashMap<String, f32>,
    /// The words in the order they were learned, as written.
    order: Vec<String>,
    /// The texts read so far.
    texts: usize,
    /// Per word ([`key`]), the first text it stood in unfound.
    seen: HashMap<String, usize>,
    /// A word was learned after it stood unfound in an earlier text.
    late: bool,
    /// The regexes of `order`, built for its length.
    regex: Option<(usize, Matchers)>,
}

#[derive(Clone)]
struct Matchers {
    /// The words, whole.
    whole: Regex,
    /// In OCR text: a word's start with dots after it, a word's end at the
    /// start of a line.
    cut: Option<Regex>,
}

impl Names {
    /// A new list that knows the words of this one from the start: for a
    /// second reading of the same input.
    pub(crate) fn seeded(&self) -> Self {
        let inner = self.lock();
        Names(Mutex::new(Inner {
            words: inner.words.clone(),
            order: inner.order.clone(),
            ..Inner::default()
        }))
    }

    /// Whether a word was learned after it stood unfound in an earlier text:
    /// a second reading would find it there.
    pub(crate) fn late(&self) -> bool {
        self.lock().late
    }

    /// The candidates of the list's words in `text`.
    pub(crate) fn candidates(&self, text: &str, source: Source) -> Vec<Candidate> {
        let (matchers, words) = {
            let mut inner = self.lock();
            if inner.order.is_empty() {
                return Vec::new();
            }
            (inner.matchers(), inner.words.clone())
        };
        let person = EntityType::new("PERSON").expect("a valid type");
        let mut out = Vec::new();
        let mut push = |start: usize, end: usize, confidence: f32| {
            out.push(Candidate {
                start,
                end,
                entity: person.clone(),
                confidence,
                detector: DETECTOR.to_owned(),
            });
        };
        for caps in matchers.whole.captures_iter(text) {
            let m = caps.name("pii").expect("the group matches");
            if let Some(c) = confidence_of(&words, m.as_str()) {
                push(m.start(), m.end(), c);
            }
        }
        if source == Source::Ocr
            && let Some(cut) = &matchers.cut
        {
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
        out
    }

    /// Learns the words of `found`, the persons kept in `text`, and notes the
    /// words of `text` outside every finding in `kept`. Returns whether a
    /// word was new to the list.
    pub(crate) fn read(
        &self,
        text: &str,
        found: &[(usize, usize, f32)],
        kept: &[(usize, usize)],
    ) -> bool {
        let mut inner = self.lock();
        let now = inner.texts;
        let mut new = false;
        for &(start, end, confidence) in found {
            let span = &text[start..end];
            let capitalised = detect::name_words(span).any(starts_upper);
            for w in detect::name_words(span) {
                if !learnable(w, capitalised) {
                    continue;
                }
                let k = key(w);
                match inner.words.get_mut(&k) {
                    Some(c) => *c = c.max(confidence),
                    None => {
                        if inner.seen.get(&k).is_some_and(|&first| first < now) {
                            inner.late = true;
                        }
                        inner.words.insert(k, confidence);
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
            inner.seen.entry(key(w)).or_insert(now);
        }
        inner.texts += 1;
        new
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Inner {
    fn matchers(&mut self) -> Matchers {
        if let Some((n, m)) = &self.regex
            && *n == self.order.len()
        {
            return m.clone();
        }
        let whole = format!(
            r"(?i)\b(?P<pii>(?:{})(?:s|'s|’s)?)\b",
            detect::alternation(self.order.clone())
        );
        let whole = regex::RegexBuilder::new(&whole)
            .size_limit(64 << 20)
            .build()
            .expect("escaped words make a valid regex");
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
        let cut = (!parts.is_empty()).then(|| {
            regex::RegexBuilder::new(&format!("(?i){}", parts.join("|")))
                .size_limit(64 << 20)
                .build()
                .expect("escaped words make a valid regex")
        });
        let m = Matchers { whole, cut };
        self.regex = Some((self.order.len(), m.clone()));
        m
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
/// capitalised name is a particle (`von`, `van`), not a name.
fn learnable(w: &str, capitalised: bool) -> bool {
    w.chars().count() >= MIN_WORD
        && (!capitalised || starts_upper(w))
        && !detect::only_role_words(w)
        && !detect::brand_word(w)
        && !detect::ACCOUNTS.contains(&w.to_lowercase().as_str())
}

fn starts_upper(w: &str) -> bool {
    w.chars().next().is_some_and(char::is_uppercase)
}

/// The runs of letters in `text`, with where each starts.
fn words_at(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_alphabetic(), start) {
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
            .into_iter()
            .map(|c| text[c.start..c.end].to_owned())
            .collect()
    }

    fn learn(names: &Names, text: &str, name: &str) -> bool {
        let start = text.find(name).unwrap();
        let end = start + name.len();
        names.read(text, &[(start, end, 0.9)], &[(start, end)])
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
        let c = &names.candidates("Herr Brandt", Source::Text)[0];
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
        names.read("Brandt ruft an", &[], &[]);
        assert!(!names.late());
        learn(&names, "Jonas Brandt", "Jonas Brandt");
        assert!(names.late());
        // Seeded, a second reading knows it from the start.
        let again = names.seeded();
        assert_eq!(found(&again, "Brandt ruft an", Source::Text), ["Brandt"]);
        again.read("Brandt ruft an", &[(0, 6, 0.9)], &[(0, 6)]);
        assert!(!again.late());
        // Found where it stood is not late.
        let names = Names::default();
        learn(&names, "Mira Albers ruft an", "Mira Albers");
        learn(&names, "Mira Albers", "Mira Albers");
        assert!(!names.late());
    }
}
