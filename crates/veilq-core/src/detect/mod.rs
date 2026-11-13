//! `detect` and `decide` (docs/plan.md §3.1): pattern rules from the
//! built-in locale packs and the policy find candidates; the policy's allow
//! lists, thresholds and the overlap rule decide which are findings.
//!
//! A locale pack is a policy's `[[patterns]]` in a file of its own,
//! `locales/<locale>/patterns.toml`, compiled into the binary (ADR-0010,
//! ADR-0011). Entity types are the same in every locale (`TAX_ID`); the
//! detector names the locale (`pattern:de.tax-id`).

pub mod validate;

use regex::Regex;
use serde::Deserialize;

use crate::finding::EntityType;
use crate::policy::{Context, PatternRule, Policy};
use crate::{Error, pseudonym};

/// The locales a policy can name.
pub const LOCALES: &[&str] = &["de", "en"];

/// The packs compiled into the binary; `intl` always runs.
const PACKS: &[(&str, &str)] = &[
    ("intl", include_str!("../../locales/intl/patterns.toml")),
    ("de", include_str!("../../locales/de/patterns.toml")),
    ("en", include_str!("../../locales/en/patterns.toml")),
];

/// What a failed `validate` costs (ADR-0012): a match without context falls
/// below the default threshold, one with context stays above it.
pub const VALIDATION_PENALTY: f32 = 0.3;

/// What a failed checksum costs in text read by OCR (ADR-0037): a misread
/// character fails the check as surely as a wrong value, so an IBAN read
/// from a picture stays above the default threshold without context.
pub const OCR_CHECKSUM_PENALTY: f32 = 0.15;

/// From this many characters on a context word also counts at the end of a
/// word: the head of a German compound (`Senderkonto`, `Mobiltelefon`). A
/// shorter one (`ip`, `tel`, `name`) would sit inside too many others.
const COMPOUND_TAIL: usize = 5;

/// Where a text comes from: written, or read from a picture by OCR.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Text,
    Ocr,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    patterns: Vec<PatternRule>,
}

/// The rules of the pack `locale`, as written.
pub fn pack(locale: &str) -> Result<Vec<PatternRule>, String> {
    let (_, text) = PACKS
        .iter()
        .find(|(l, _)| *l == locale)
        .ok_or_else(|| format!("no pack for locale `{locale}`"))?;
    let file: PackFile = toml::from_str(text).map_err(|e| format!("pack {locale}: {e}"))?;
    for rule in &file.patterns {
        rule.check(&format!("pack {locale}, pattern"))?;
    }
    Ok(file.patterns)
}

/// A span that may be personal data, before the policy decides.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Byte offsets into the text it was found in.
    pub start: usize,
    pub end: usize,
    pub entity: EntityType,
    pub confidence: f32,
    pub detector: String,
}

/// What the policy decided on a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// A finding.
    Kept,
    /// Below its type's threshold.
    BelowThreshold,
    /// The allow lists name it.
    Allowed,
    /// A longer or more confident candidate claims its span.
    Overlap,
    /// A name of nothing but role, title and greeting words.
    RoleWords,
    /// An account a system makes (`Public`, `root`).
    Account,
    /// One word with a capital inside, which NER took for a name.
    BrandWord,
    /// Inside one of veilq's own placeholders.
    Placeholder,
}

/// A candidate, the policy's verdict on it and the threshold it was held
/// to; `raised` when a birth date beside a name lifted its confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub candidate: Candidate,
    pub verdict: Verdict,
    pub threshold: f32,
    pub raised: bool,
}

#[derive(Clone)]
struct Rule {
    detector: String,
    entity: EntityType,
    regex: Regex,
    confidence: f32,
    context: Option<Context>,
    validate: Option<String>,
    group: Option<usize>,
}

impl Rule {
    fn compile(rule: &PatternRule, detector: String) -> Result<Self, String> {
        let regex = Regex::new(&rule.regex).map_err(|e| format!("{detector}: {e}"))?;
        let group = regex.capture_names().position(|n| n == Some("pii"));
        let context = rule.context.clone().map(|mut c| {
            c.words = c.words.iter().map(|w| w.to_lowercase()).collect();
            c
        });
        Ok(Self {
            detector,
            entity: rule.entity.clone(),
            regex,
            confidence: rule.confidence,
            context,
            validate: rule.validate.clone(),
            group,
        })
    }
}

/// Allow lists, compiled.
#[derive(Clone)]
struct Allow {
    values: Vec<String>,
    patterns: Vec<Regex>,
}

/// The rules of one run: the `intl` pack, the policy's locales, the policy's
/// own patterns, and how the policy decides.
#[derive(Clone)]
pub struct Detector {
    rules: Vec<Rule>,
    allow: Allow,
    /// The NER model, when it is installed (ADR-0022).
    #[cfg(feature = "ner")]
    ner: Option<std::sync::Arc<crate::ner::Ner>>,
    policy: Policy,
}

impl Detector {
    pub fn new(policy: &Policy) -> Result<Self, Error> {
        let invalid = |message: String| Error::Policy {
            path: "veilq.toml".into(),
            message,
        };
        let mut rules = Vec::new();
        let locales = std::iter::once("intl").chain(policy.locales.iter().map(String::as_str));
        for locale in locales {
            for rule in pack(locale).map_err(invalid)? {
                let detector = format!("pattern:{locale}.{}", rule.name);
                rules.push(Rule::compile(&rule, detector).map_err(invalid)?);
            }
        }
        for rule in &policy.patterns {
            let detector = format!("policy:{}", rule.name);
            rules.push(Rule::compile(rule, detector).map_err(invalid)?);
        }
        let allow = Allow {
            values: policy
                .allow
                .values
                .iter()
                .map(|v| pseudonym::normalise_plain(v))
                .collect(),
            patterns: policy
                .allow
                .patterns
                .iter()
                .map(|p| Regex::new(&format!("^(?:{p})$")).map_err(|e| invalid(e.to_string())))
                .collect::<Result<_, _>>()?,
        };
        Ok(Self {
            rules,
            allow,
            #[cfg(feature = "ner")]
            ner: None,
            policy: policy.clone(),
        })
    }

    /// Runs the NER model beside the patterns.
    #[cfg(feature = "ner")]
    pub fn set_ner(&mut self, ner: std::sync::Arc<crate::ner::Ner>) {
        self.ner = Some(ner);
    }

    /// Whether the NER model runs.
    pub fn has_ner(&self) -> bool {
        #[cfg(feature = "ner")]
        return self.ner.is_some();
        #[cfg(not(feature = "ner"))]
        false
    }

    /// Every candidate in `text`. `context` is what stands before the text
    /// and is not part of it: a column's or a key's name. A failing model is
    /// an error (ADR-0003). In text read by OCR a failed checksum costs
    /// less (ADR-0037).
    pub fn candidates(
        &self,
        text: &str,
        context: &str,
        source: Source,
    ) -> Result<Vec<Candidate>, Error> {
        let mut out = Vec::new();
        #[cfg(feature = "ner")]
        if let Some(ner) = &self.ner {
            out.extend(ner.candidates(text)?);
        }
        for rule in &self.rules {
            for caps in rule.regex.captures_iter(text) {
                let m = match rule.group {
                    Some(g) => match caps.get(g) {
                        Some(m) => m,
                        None => continue,
                    },
                    None => caps.get(0).expect("group 0 always matches"),
                };
                if m.is_empty() {
                    continue;
                }
                // A rule at 0 finds with its context only: below the
                // threshold, a match would still claim its span. It reads
                // the context's last line, a column's name: a picture's
                // line puts the head of its table's column there, after
                // the lines beside it (`visual::neighbours`).
                let (context, before) = if rule.confidence <= 0.0 {
                    (context.rsplit('\n').next().unwrap_or(""), "")
                } else {
                    (context, &text[..m.start()])
                };
                let mut confidence = rule.confidence;
                if let Some(c) = &rule.context
                    && has_context(c, context, before)
                {
                    confidence += c.boost;
                }
                if confidence <= 0.0 {
                    continue;
                }
                if let Some(v) = &rule.validate
                    && !validate::check(v, m.as_str())
                {
                    confidence -= if source == Source::Ocr && validate::is_checksum(v) {
                        OCR_CHECKSUM_PENALTY
                    } else {
                        VALIDATION_PENALTY
                    };
                }
                out.push(Candidate {
                    start: m.start(),
                    end: m.end(),
                    entity: rule.entity.clone(),
                    confidence: round(confidence.clamp(0.0, 1.0)),
                    detector: rule.detector.clone(),
                });
            }
        }
        Ok(out)
    }

    /// The findings in `text`: of overlapping candidates the longest, then
    /// the most confident; of those, the ones the allow lists do not name, at
    /// or above their type's threshold. Sorted by position.
    ///
    /// Overlaps are resolved before the threshold: a long candidate below it
    /// (an IBAN with a wrong checksum) still claims its span, so that a short
    /// rule does not find a phone number in its digits.
    pub fn findings(
        &self,
        text: &str,
        context: &str,
        source: Source,
    ) -> Result<Vec<Candidate>, Error> {
        Ok(self
            .decide(text, context, source)?
            .into_iter()
            .filter(|d| d.verdict == Verdict::Kept)
            .map(|d| d.candidate)
            .collect())
    }

    /// Every candidate in `text` with what [`findings`](Self::findings)
    /// decided on it and why, sorted by position: what `scan --explain`
    /// shows (#40). The candidates kept are exactly the findings.
    pub fn decide(
        &self,
        text: &str,
        context: &str,
        source: Source,
    ) -> Result<Vec<Decision>, Error> {
        let mut out = Vec::new();
        let mut kept = Vec::new();
        for c in self.candidates(text, context, source)? {
            let value = &text[c.start..c.end];
            let dropped = if c.entity.as_str() != "PERSON" {
                None
            } else if only_role_words(value) {
                Some(Verdict::RoleWords)
            } else if ACCOUNTS.contains(&value.to_lowercase().as_str()) {
                Some(Verdict::Account)
            } else if c.detector == "ner" && brand_word(value) {
                Some(Verdict::BrandWord)
            } else {
                None
            };
            match dropped {
                Some(verdict) => out.push(self.decision(c, verdict, false)),
                None => kept.push(c),
            }
        }
        kept.sort_by(|a, b| {
            (b.end - b.start)
                .cmp(&(a.end - a.start))
                .then(b.confidence.total_cmp(&a.confidence))
                .then(a.start.cmp(&b.start))
        });
        // A candidate below its threshold claims its span from other types
        // (an IBAN with a wrong checksum keeps a phone number out of its
        // digits), not from a finding of its own type: `Herr Jonas Brandt`
        // as a field is below it, `Jonas Brandt` after `Herr` is not.
        let meets = |c: &Candidate| c.confidence >= round(self.threshold(&c.entity));
        let mut chosen: Vec<Candidate> = Vec::new();
        for c in kept {
            let overlaps = |k: &Candidate| c.start < k.end && k.start < c.end;
            let yields = |k: &Candidate| k.entity == c.entity && !meets(k) && meets(&c);
            if chosen.iter().all(|k| !overlaps(k) || yields(k)) {
                let (gone, stay): (Vec<_>, Vec<_>) = chosen.into_iter().partition(|k| overlaps(k));
                chosen = stay;
                for k in gone {
                    out.push(self.decision(k, Verdict::Overlap, false));
                }
                chosen.push(c);
            } else {
                out.push(self.decision(c, Verdict::Overlap, false));
            }
        }
        let before: Vec<f32> = chosen.iter().map(|c| c.confidence).collect();
        self.birth_dates_beside_names(text, &mut chosen);
        for (c, before) in chosen.into_iter().zip(before) {
            let raised = c.confidence > before;
            let verdict = if c.confidence < round(self.threshold(&c.entity)) {
                Verdict::BelowThreshold
            } else if self.allowed(&text[c.start..c.end]) {
                Verdict::Allowed
            } else {
                Verdict::Kept
            };
            out.push(self.decision(c, verdict, raised));
        }
        out.sort_by_key(|d| (d.candidate.start, d.candidate.end));
        Ok(out)
    }

    fn decision(&self, candidate: Candidate, verdict: Verdict, raised: bool) -> Decision {
        Decision {
            threshold: round(self.threshold(&candidate.entity)),
            candidate,
            verdict,
            raised,
        }
    }

    /// A date that can be a birth date, beside a name, is one: a record's
    /// header shows it with an icon, not a word (`Mira Albers  14.03.1987
    /// 0421 …`). Beside: at most [`BESIDE`] bytes apart, on the same line or
    /// at the start of the line right below the name (a picture's header puts
    /// the name above its details); an icon read as a stray mark or two may
    /// come before the date.
    fn birth_dates_beside_names(&self, text: &str, chosen: &mut [Candidate]) {
        const BESIDE: usize = 40;
        let person = EntityType::new("PERSON").expect("a valid type");
        let names: Vec<(usize, usize)> = chosen
            .iter()
            .filter(|c| c.entity == person && c.confidence >= round(self.threshold(&person)))
            .map(|c| (c.start, c.end))
            .collect();
        if names.is_empty() {
            return;
        }
        for c in chosen
            .iter_mut()
            .filter(|c| c.entity.as_str() == "DATE_OF_BIRTH")
        {
            let beside = names.iter().any(|&(s, e)| {
                let gap = if e <= c.start {
                    &text[e..c.start]
                } else if c.end <= s {
                    return s - c.end <= BESIDE && !text[c.end..s].contains('\n');
                } else {
                    return false;
                };
                if gap.len() > BESIDE {
                    return false;
                }
                match gap.split_once('\n') {
                    None => true,
                    Some((_, below)) => !below.contains('\n') && below.trim().chars().count() <= 2,
                }
            });
            if beside && can_be_birth_date(&text[c.start..c.end]) {
                c.confidence = c.confidence.max(round(self.threshold(&c.entity)).max(0.75));
            }
        }
    }

    pub fn threshold(&self, entity: &EntityType) -> f32 {
        self.policy
            .entities
            .get(entity)
            .and_then(|r| r.min_confidence)
            .unwrap_or(self.policy.defaults.min_confidence)
    }

    fn allowed(&self, value: &str) -> bool {
        let n = pseudonym::normalise_plain(value);
        self.allow.values.contains(&n) || self.allow.patterns.iter().any(|p| p.is_match(value))
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }
}

/// Whether `value` is a valid date with a four-digit year at least ten years
/// back: someone's birth date, not an appointment or a deadline.
fn can_be_birth_date(value: &str) -> bool {
    let year_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| 1970 + d.as_secs() / 31_556_952)
        .unwrap_or(2026);
    let year = value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| p.len() == 4)
        .find_map(|p| p.parse::<u64>().ok());
    matches!(year, Some(y) if (1900..=year_now.saturating_sub(10)).contains(&y))
        && validate::date(value)
}

/// Whether `span` is one word with a capital inside after four letters or
/// more (`PowerShell`, `LinkedIn`): a product's or a company's name, which
/// NER takes for a person's in a logo or a title bar. A name's prefix is
/// shorter (`McDonald`, `DeVries`, `MacArthur`).
pub(crate) fn brand_word(span: &str) -> bool {
    let chars: Vec<char> = span.chars().collect();
    chars.iter().all(|c| c.is_alphabetic())
        && chars.first().is_some_and(|c| c.is_uppercase())
        && (4..chars.len()).any(|i| chars[i].is_uppercase() && chars[i - 1].is_lowercase())
}

/// The accounts a system makes, whose home directory names no person.
const ACCOUNTS: &[&str] = &[
    "public",
    "default",
    "defaultuser0",
    "administrator",
    "admin",
    "guest",
    "gast",
    "root",
    "runner",
    "runneradmin",
    "user",
    "shared",
    "%username%",
    "$user",
];

/// Words a span of only these is not a name.
pub(crate) const NOT_NAMES: &[&str] = &[
    "der",
    "die",
    "das",
    "den",
    "dem",
    "des",
    "ein",
    "eine",
    "einen",
    "herr",
    "herrn",
    "frau",
    "dr",
    "doktor",
    "prof",
    "professor",
    // Clergy: a title before a name (`Pater Ole`), no name alone
    // (`Der Pater, der …`).
    "pater",
    "pfarrer",
    "pfarrerin",
    "priester",
    "priesterin",
    "papst",
    "pope",
    "priest",
    "kunde",
    "kundin",
    "kunden",
    "mitarbeiter",
    "mitarbeiterin",
    "kollege",
    "kollegin",
    "chef",
    "chefin",
    "mandant",
    "mandantin",
    "patient",
    "patientin",
    // The heads of a list's name columns: a head is no name in its own
    // column.
    "name",
    "vorname",
    "nachname",
    "familienname",
    "gegner",
    "gegnerin",
    "schuldner",
    "schuldnerin",
    "gläubigerin",
    "kläger",
    "klägerin",
    "beklagte",
    "beklagter",
    "beteiligte",
    "beteiligter",
    "inhaber",
    "inhaberin",
    "the",
    "mr",
    "mrs",
    "ms",
    "customer",
    "colleague",
    "manager",
    "agent",
    // Greetings and contact labels: what is left when `remove` took the
    // name and number after them.
    "hallo",
    "hello",
    "hi",
    "hey",
    "dear",
    "liebe",
    "lieber",
    "tel",
    "telefon",
    "phone",
    "fax",
    "mobil",
    "mobile",
    // Pronouns and the roles of a call or a message: a sentence starts
    // with them.
    "ich",
    "wir",
    "du",
    "er",
    "sie",
    "man",
    "anrufer",
    "anruferin",
    "absender",
    "absenderin",
    "empfänger",
    "empfängerin",
    "nutzer",
    "nutzerin",
    "benutzer",
    "benutzerin",
    "caller",
    "sender",
    "recipient",
    "user",
    "team",
];

/// Stems of German words that mean a person or a group of people. German
/// writes every noun with a capital, and NER calls a word that means a
/// person one (`Kein Inhalt mit Personen.`). Each takes the endings in
/// [`PERSON_NOUN_ENDINGS`] and may end a compound (`Kontaktperson`). Words
/// that are common surnames as well (`Richter`, `Schüler`, `Bürger`,
/// `Gast`) are left out: a name is not dropped to spare a false positive.
const PERSON_NOUNS: &[&str] = &[
    "person",
    "mensch",
    "leut",
    "kind",
    "eltern",
    "gäste",
    "zeug",
    "partner",
    "nachbar",
    "kund",
    "nehmer",
    "geber",
    "mieter",
    "käufer",
    "kläger",
    "antragsteller",
    "inhaber",
    "eigentümer",
    "bewerber",
    "besucher",
    "bewohner",
    "einwohner",
    "betreuer",
    "vertreter",
    "arbeiter",
    "schuldner",
    "gläubiger",
    "mitglied",
    "empfänger",
    "absender",
    "nutzer",
    "anwender",
    "kolleg",
    "verfasser",
    "verwalter",
    "ersteller",
    // Adjectives used as nouns.
    "betroffen",
    "beteiligt",
    "beklagt",
    "beschuldigt",
    "angehörig",
    "bevollmächtigt",
    "teilnehmend",
];

/// The endings of [`PERSON_NOUNS`]: plural, feminine, case.
const PERSON_NOUN_ENDINGS: &[&str] = &[
    "", "e", "en", "er", "ern", "em", "es", "n", "s", "in", "innen",
];

/// Whether `word` means a person or a group of people ([`PERSON_NOUNS`]),
/// alone, inflected or at the end of a compound, or starts with `Personen`.
pub(crate) fn person_noun(word: &str) -> bool {
    let w = word.to_lowercase();
    w.starts_with("personen")
        || PERSON_NOUNS.iter().any(|stem| {
            w.ends_with(stem)
                || PERSON_NOUN_ENDINGS
                    .iter()
                    .any(|e| !e.is_empty() && w.strip_suffix(e).is_some_and(|s| s.ends_with(stem)))
        })
}

/// The length in bytes of `span`'s first word when it is the genitive of
/// one of `names` (`Brandts`, `Albers'` beside `Jonas Brandt`, `Mira
/// Albers`) and more words follow: German puts what a name owns after it
/// (`Brandts Kapuze`), and NER takes both for the name. `None` otherwise.
pub(crate) fn genitive_head<'a>(
    span: &str,
    mut names: impl Iterator<Item = &'a str>,
) -> Option<usize> {
    let end = span.find(char::is_whitespace)?;
    if !span[end..].chars().any(char::is_alphabetic) {
        return None;
    }
    let word = &span[..end];
    let stems: Vec<&str> = ["'s", "’s", "s", "'", "’"]
        .iter()
        .filter_map(|s| word.strip_suffix(s))
        .filter(|s| s.chars().count() >= 2 && s.chars().all(char::is_alphabetic))
        .collect();
    names.any(|n| stems.contains(&n)).then_some(end)
}

/// The words of a name: its runs of letters, two or more long.
pub(crate) fn name_words(span: &str) -> impl Iterator<Item = &str> {
    span.split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 2)
}

/// Whether `span` is nothing but role, title and greeting words
/// ([`NOT_NAMES`]): `Herr Doktor`, `Frau Kundin` name no person, whichever
/// detector found them.
pub(crate) fn only_role_words(span: &str) -> bool {
    let mut words = span
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 2)
        .peekable();
    words.peek().is_some() && words.all(|w| NOT_NAMES.contains(&w.to_lowercase().as_str()))
}

/// Confidences in thousandths: `1.0 - 0.3` is `0.7`, not `0.69999998`.
fn round(x: f32) -> f32 {
    (x * 1000.0).round() / 1000.0
}

/// Whether one of `c.words` is in the last `c.window` characters before the
/// match: of `context` (a column's name) and the text before it. A word that
/// starts with a letter or digit must start a word, or, from
/// [`COMPOUND_TAIL`] characters on, end one: `Senderkonto` holds `konto`,
/// `Zip` does not hold `ip` (ADR-0037).
fn has_context(c: &Context, context: &str, before: &str) -> bool {
    let joined = format!("{context}\n{before}").to_lowercase();
    let start = joined
        .char_indices()
        .rev()
        .nth(c.window.saturating_sub(1))
        .map_or(0, |(i, _)| i);
    let window = &joined[start..];
    c.words.iter().any(|w| {
        let starts_word = w.chars().next().is_some_and(char::is_alphanumeric);
        let tail = w.chars().count() >= COMPOUND_TAIL;
        window.match_indices(w.as_str()).any(|(i, _)| {
            let at_start = window[..i]
                .chars()
                .next_back()
                .is_none_or(|p| !p.is_alphanumeric());
            let at_end = window[i + w.len()..]
                .chars()
                .next()
                .is_none_or(|n| !n.is_alphanumeric());
            !starts_word || at_start || (tail && at_end)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(text: &str, context: &str) -> Vec<(String, String)> {
        read(text, context, Source::Text)
    }

    fn read(text: &str, context: &str, source: Source) -> Vec<(String, String)> {
        let d = Detector::new(&Policy::default()).unwrap();
        d.findings(text, context, source)
            .unwrap()
            .into_iter()
            .map(|c| {
                (
                    c.entity.as_str().to_owned(),
                    text[c.start..c.end].to_owned(),
                )
            })
            .collect()
    }

    fn found(text: &str, entity: &str, value: &str) -> bool {
        findings(text, "")
            .iter()
            .any(|(e, v)| e == entity && v == value)
    }

    #[test]
    fn a_role_is_no_name_whoever_finds_it() {
        // `Herr` and `Frau` before a title or a role: the pattern took the
        // word after them for a name.
        for text in [
            "Herr Doktor kommt gleich.",
            "Bitte Frau Kundin informieren.",
        ] {
            assert!(findings(text, "").is_empty(), "{text}");
        }
        assert!(
            found("Herr Doktor Kern kommt.", "PERSON", "Kern")
                || found("Herr Doktor Kern kommt.", "PERSON", "Doktor Kern")
        );
        assert!(found("Frau Albers kommt.", "PERSON", "Albers"));
    }

    #[test]
    fn from_is_no_name_cue() {
        // `from` names places and companies as often as people.
        assert!(findings("in ONNX (fp16, from Hugging Face; veilq ships", "").is_empty());
        assert!(findings("Data from New York shows", "").is_empty());
        assert!(found("Approved by Jonas Brandt.", "PERSON", "Jonas Brandt"));
        assert!(found("Dear Mira Albers,", "PERSON", "Mira Albers"));
    }

    #[test]
    fn every_pack_compiles() {
        for (locale, _) in PACKS {
            assert!(!pack(locale).unwrap().is_empty(), "{locale}");
        }
    }

    #[test]
    fn the_demo_letter() {
        let text = "Hallo Jonas Brandt,\n\nFrau Mira Albers (geb. 14.03.1987) bittet um Rückruf \
            unter 0421 5550173 oder mira.albers@example.com. Anschrift: Lindenweg 12, 28195 Bremen.\n\
            IBAN DE89 3704 0044 0532 0130 00, Kennzeichen HB-MA 412, Steuer-ID 57 381 942 656.\n\
            Login von IP 203.0.113.24; Aktenzeichen 3 O 128/26.\n\nViele Grüße\nLena Vogt\n";
        for (entity, value) in [
            ("PERSON", "Jonas Brandt"),
            ("PERSON", "Mira Albers"),
            ("DATE_OF_BIRTH", "14.03.1987"),
            ("PHONE_NUMBER", "0421 5550173"),
            ("EMAIL_ADDRESS", "mira.albers@example.com"),
            ("ADDRESS", "Lindenweg 12, 28195 Bremen"),
            ("IBAN_CODE", "DE89 3704 0044 0532 0130 00"),
            ("LICENSE_PLATE", "HB-MA 412"),
            ("TAX_ID", "57 381 942 656"),
            ("IP_ADDRESS", "203.0.113.24"),
            ("COURT_FILE_NUMBER", "3 O 128/26"),
            ("PERSON", "Lena Vogt"),
        ] {
            assert!(
                found(text, entity, value),
                "{entity} {value}: {:?}",
                findings(text, "")
            );
        }
    }

    #[test]
    fn names_after_keys_and_roles() {
        assert!(found(
            "body={\"name\":\"Ida Sommer\",\"x\":1}",
            "PERSON",
            "Ida Sommer"
        ));
        assert!(found(
            "Bruder von Mira Albers; Kfz",
            "PERSON",
            "Mira Albers"
        ));
        assert!(!found("name=Projekt", "PERSON", "Projekt"));
    }

    #[test]
    fn an_address_without_a_street_word_is_known_by_its_postcode() {
        // A record's header: the street has no -straße or -weg, the town
        // has more than one word.
        for (text, value) in [
            (
                "Mandantin  Am Anger 7, 15938 Lindau Ost-Heide  Status",
                "Am Anger 7, 15938 Lindau Ost-Heide",
            ),
            ("Am Markt 4, 28195 Bremen", "Am Markt 4, 28195 Bremen"),
            (
                "Lindenweg 12, 15806 Zossen Ortsteil Wünsdorf",
                "Lindenweg 12, 15806 Zossen Ortsteil Wünsdorf",
            ),
        ] {
            assert!(
                found(text, "ADDRESS", value),
                "{text}: {:?}",
                findings(text, "")
            );
        }
        // A town on the next line is not part of the address.
        assert!(found(
            "Lindenweg 12, 28195 Bremen\nTelefon folgt",
            "ADDRESS",
            "Lindenweg 12, 28195 Bremen"
        ));
    }

    #[test]
    fn a_word_with_a_capital_inside_is_a_brand() {
        for word in ["PowerShell", "WordPress", "LinkedIn", "CloudDesk"] {
            assert!(brand_word(word), "{word}");
        }
        for name in [
            "McDonald",
            "MacArthur",
            "DeVries",
            "DiCaprio",
            "Brandt",
            "Mira Albers",
            "JBrandt",
        ] {
            assert!(!brand_word(name), "{name}");
        }
    }

    #[test]
    fn a_home_directory_names_its_user() {
        for (text, value) in [
            (
                r"root C:\Users\jonas.brandt  policy built-in",
                "jonas.brandt",
            ),
            (
                r"C:\Users\JBrandt\AppData\Roaming\veilq\veilq.toml",
                "JBrandt",
            ),
            ("open /home/mira/notes.txt", "mira"),
            ("path=/Users/mira.albers/Desktop", "mira.albers"),
        ] {
            assert!(
                found(text, "PERSON", value),
                "{text}: {:?}",
                findings(text, "")
            );
        }
        // The accounts a system makes are nobody.
        for text in [
            r"C:\Users\Public\Documents",
            r"C:\Users\Default\AppData",
            "/home/runner/work",
        ] {
            assert!(
                !findings(text, "").iter().any(|(e, _)| e == "PERSON"),
                "{text}: {:?}",
                findings(text, "")
            );
        }
    }

    #[test]
    fn a_date_beside_a_name_is_a_birth_date() {
        // A record's header shows the date of birth with an icon, no word.
        assert!(found(
            "Herr Jonas Brandt  14.03.1987  0421 5550173",
            "DATE_OF_BIRTH",
            "14.03.1987"
        ));
        assert!(found(
            "14.03.1987 Frau Mira Albers",
            "DATE_OF_BIRTH",
            "14.03.1987"
        ));
        // The name above, the date at the start of the line below.
        assert!(found(
            "Frau Mira Albers\n14.03.1987  0421 5550173",
            "DATE_OF_BIRTH",
            "14.03.1987"
        ));
        // A date without a name, a name further away or a date inside the
        // next line's sentence, a date too recent to be someone's birth
        // date: none of them.
        for text in [
            "Rechnung vom 14.03.1987, Betrag 12,00 EUR",
            "Herr Jonas Brandt\n\n\nTermin am 14.03.1987",
            "Herr Jonas Brandt\nTermin am 14.03.1987",
            "Termin mit Herrn Brandt am 14.03.2026",
        ] {
            assert!(
                !findings(text, "").iter().any(|(e, _)| e == "DATE_OF_BIRTH"),
                "{text}: {:?}",
                findings(text, "")
            );
        }
    }

    #[test]
    fn a_cut_address_is_still_one() {
        // A screen cuts a long value (`mira.albers@exampl…`), and OCR does
        // not always read the dots: the name before `@` is personal all the
        // same.
        for (text, value) in [
            ("mira.albers@exampl", "mira.albers@exampl"),
            ("Konto: mira.albers@exampl…", "mira.albers@exampl"),
            ("mira.albers@example.c", "mira.albers@example.c"),
            ("Login root@buildhost-3 ok", "root@buildhost-3"),
            (
                "Kontakt: mira.albers@example.com.",
                "mira.albers@example.com",
            ),
        ] {
            assert!(
                found(text, "EMAIL_ADDRESS", value),
                "{text}: {:?}",
                findings(text, "")
            );
        }
        // A package's version or tag and an image's digest are no address.
        for text in [
            "npm i lodash@latest vue@next react@18.3.1",
            "FROM node@sha256:4f2a9c",
            "uses: actions/checkout@v4 and Swatinem/rust-cache@d1527c8f67ef",
            "a = x@y",
            "Hallo @jonas, siehe @media",
        ] {
            assert!(
                findings(text, "").is_empty(),
                "{text}: {:?}",
                findings(text, "")
            );
        }
    }

    #[test]
    fn lookalikes_stay() {
        let text = "Rechnung 2026-0421-5550 vom 02.11.2026, Version 10.4.21.7, \
            Referenz DE89 3704 0044 0532 0130 01, Nummer 57381942065.";
        assert_eq!(findings(text, ""), vec![]);
    }

    #[test]
    fn a_failed_check_with_context_is_still_found() {
        // A mistyped IBAN next to its label (ADR-0012).
        assert!(found(
            "IBAN: DE89 3704 0044 0532 0130 01",
            "IBAN_CODE",
            "DE89 3704 0044 0532 0130 01"
        ));
        // A valid check digit without context is not enough for a tax ID.
        assert!(!found("Nummer 57381942656", "TAX_ID", "57381942656"));
        assert!(found("Steuer-ID 57381942656", "TAX_ID", "57381942656"));
    }

    #[test]
    fn a_column_name_is_context() {
        assert_eq!(findings("14.03.1987", ""), vec![]);
        assert_eq!(
            findings("14.03.1987", "geburtsdatum"),
            vec![("DATE_OF_BIRTH".into(), "14.03.1987".into())]
        );
        assert_eq!(
            findings("Mira Albers", "name"),
            vec![("PERSON".into(), "Mira Albers".into())]
        );
        assert_eq!(findings("Mira Albers", "produkt"), vec![]);
    }

    #[test]
    fn context_words_start_words() {
        let c = Context {
            words: vec!["ip".into()],
            boost: 0.3,
            window: 20,
        };
        assert!(has_context(&c, "", "Login von IP "));
        assert!(!has_context(&c, "", "Zip "));
        assert!(has_context(&c, "client_ip", ""));
        assert!(!has_context(&c, "ip", &"x".repeat(40)));
    }

    #[test]
    fn long_context_words_end_compounds() {
        let c = Context {
            words: vec!["konto".into(), "name".into()],
            boost: 0.05,
            window: 20,
        };
        assert!(has_context(&c, "Senderkonto*", "Bank - "));
        assert!(has_context(&c, "", "Empfängerkonto: "));
        assert!(has_context(&c, "", "Kontoinhaber "));
        // Inside a word, or a short word at a compound's end, is no context.
        assert!(!has_context(&c, "", "Bankkontonummern "));
        assert!(!has_context(&c, "", "Dateiname: "));
    }

    #[test]
    fn a_compound_label_lifts_a_mistyped_iban() {
        // DE89 3704 0044 0532 0130 00 with one 0 lost.
        let iban = "DE89 3704 0044 0532 0130 0";
        assert_eq!(
            findings(&format!("Senderkonto: {iban}"), ""),
            vec![("IBAN_CODE".into(), iban.into())]
        );
        assert_eq!(findings(&format!("Bank - {iban}"), ""), vec![]);
    }

    #[test]
    fn a_failed_checksum_read_by_ocr_costs_less() {
        // OCR loses a 0 of a run: the checksum fails, the IBAN is still one.
        let iban = "DE8937040044053201300";
        assert_eq!(findings(iban, ""), vec![]);
        assert_eq!(
            read(iban, "", Source::Ocr),
            vec![("IBAN_CODE".into(), iban.into())]
        );
        // A card number and a tax ID without context stay below the
        // threshold, read or written.
        assert_eq!(read("Nummer 4111 1111 1111 1112", "", Source::Ocr), vec![]);
        assert_eq!(read("Nummer 57381942657", "", Source::Ocr), vec![]);
        // A form check costs what it always does: an impossible date beside
        // its label is no birth date, read or written.
        assert_eq!(read("geboren 31.02.1987", "", Source::Ocr), vec![]);
    }

    #[test]
    fn the_policy_adds_patterns_and_allows_values() {
        let policy = Policy::parse(
            r#"
version = 1
locales = ["de"]
[[patterns]]
name = "contract"
entity = "CONTRACT_NUMBER"
regex = '\b\d{4}-\d{4}\b'
confidence = 0.5
context = { words = ["vertrag"], boost = 0.4 }
[allow]
values = ["Mira Albers"]
"#,
        )
        .unwrap();
        let d = Detector::new(&policy).unwrap();
        let text = "Vertrag 2026-0815 mit Frau Mira Albers";
        let f = d.findings(text, "", Source::Text).unwrap();
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].detector, "policy:contract");
        assert_eq!(&text[f[0].start..f[0].end], "2026-0815");
    }

    #[test]
    fn a_genitive_of_a_name_is_its_head() {
        let names = ["Mira", "Albers", "Jonas", "Brandt"];
        let head = |span: &str| genitive_head(span, names.iter().copied());
        assert_eq!(head("Brandts Kapuze"), Some("Brandts".len()));
        assert_eq!(head("Albers’ Hut"), Some("Albers’".len()));
        assert_eq!(head("Brandt's Hut"), Some("Brandt's".len()));
        // A name of its own, one word, or a genitive of no name found.
        assert_eq!(head("Jonas Brandt"), None);
        assert_eq!(head("Brandts"), None);
        assert_eq!(head("Vogts Kapuze"), None);
        assert_eq!(head("Brandts  4,6"), None);
    }

    #[test]
    fn a_name_below_its_threshold_does_not_hide_one_above() {
        // `name-field` takes the whole value at 0.35; the salutation's name
        // inside it is a finding.
        assert!(
            found("Herr Jonas Brandt", "PERSON", "Jonas Brandt"),
            "{:?}",
            findings("Herr Jonas Brandt", "")
        );
        // Another type still claims its span: a phone number in an IBAN
        // with a wrong checksum stays part of it.
        let text = "DE89 3704 0044 0532 0130 01";
        assert!(
            findings(text, "").iter().all(|(e, _)| e != "PHONE_NUMBER"),
            "{:?}",
            findings(text, "")
        );
    }

    #[test]
    fn a_column_of_a_list_names_a_person_or_a_street() {
        let has = |text: &str, column: &str, entity: &str| {
            findings(text, column)
                .iter()
                .any(|(e, v)| e == entity && v == text)
        };
        // One word under a column that names a person.
        for column in ["Gegner", "Nachname", "Schuldnerin", "Prozessgegner", "Name"] {
            assert!(has("Brauner", column, "PERSON"), "{column}");
        }
        // Elsewhere one word is no name, and no candidate either.
        for column in ["", "Status", "Firmenname", "Ort"] {
            assert!(findings("Brauner", column).is_empty(), "{column}");
        }
        // A column's head is no name in its own column.
        assert!(findings("Gegner", "Mandant").is_empty());

        // A whole value under a column that names a street, cut or not.
        for text in [
            "Eichbreite",
            "Hachenburg ...",
            "Am Güterba …",
            "Lindenweg 12",
        ] {
            assert!(has(text, "Straße", "ADDRESS"), "{text}");
        }
        assert!(findings("Eichbreite", "Ort").is_empty());
        assert!(findings("Eichbreite", "E-Mail").is_empty());
    }

    #[test]
    fn an_address_in_the_cells_of_a_row_is_one() {
        // A picture's row joins its cells with two spaces.
        for (text, value) in [
            (
                "Kerstin Volkmann  Brauner  Eichbreite  96  31785  Hameln",
                "Eichbreite  96  31785  Hameln",
            ),
            (
                "Wessel  Hachenburg   ...  5  65936  Frankfurt",
                "Hachenburg   ...  5  65936  Frankfurt",
            ),
            (
                "FE warten  Siedlungsstr  ( ..  3  73312  Geislingen",
                "Siedlungsstr  ( ..  3  73312  Geislingen",
            ),
            (
                "Ole Hansen  Trebuser Str.  3d  15517  Fürstenwal ..",
                "Trebuser Str.  3d  15517  Fürstenwal ..",
            ),
            (
                "Pia Sommer  Im Grün  5  77855  Achern",
                "Im Grün  5  77855  Achern",
            ),
        ] {
            assert!(
                found(text, "ADDRESS", value),
                "{text}: {:?}",
                findings(text, "")
            );
        }
        // The street word as a word of its own, without a comma.
        for (text, value) in [
            (
                "Wir wohnen Trebuser Str. 3d 15517 Fürstenwalde.",
                "Trebuser Str. 3d 15517 Fürstenwalde",
            ),
            (
                "Berliner Straße 12 10115 Berlin",
                "Berliner Straße 12 10115 Berlin",
            ),
        ] {
            assert!(
                found(text, "ADDRESS", value),
                "{text}: {:?}",
                findings(text, "")
            );
        }
    }
}
