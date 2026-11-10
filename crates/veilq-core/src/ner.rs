//! The NER detector (ADR-0022): a GLiNER model in ONNX finds names that no
//! word around them marks (`Beim Termin war auch Lena Vogt`).
//!
//! The model is asked for `person`, `organization` and `location`; only
//! `person` becomes a candidate (`PERSON`, detector `ner`): the other two
//! labels take the spans that are firms and places, so they are not read as
//! persons. A span that holds only role and salutation words (`Der Kunde`,
//! `Herr Doktor`) is dropped. Long texts are read in windows of words that
//! overlap. The model and ONNX Runtime are installed with `veilq models`
//! (ADR-0005); nothing here downloads.
//!
//! The input follows GLiNER's span mode, as its Python implementation builds
//! it: the prompt `<<ENT>> person <<ENT>> organization <<ENT>> location
//! <<SEP>>` before the words, a word mask on each word's first token, every
//! span of 1 to `max_width` words; the logits are `[1, words, max_width,
//! labels]`, a sigmoid gives the score.

use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use regex::Regex;
use tokenizers::Tokenizer;

use crate::Error;
use crate::detect::Candidate;
use crate::finding::EntityType;

const LABELS: [&str; 3] = ["person", "organization", "location"];
/// Words per window, and the words two windows share.
const WINDOW: usize = 200;
const OVERLAP: usize = 40;
/// The model's own threshold: spans below it are no candidates.
pub const MIN_SCORE: f32 = 0.3;

/// Words a span of only these is not a name.
const NOT_NAMES: &[&str] = &[
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
    "the",
    "mr",
    "mrs",
    "ms",
    "customer",
    "colleague",
    "manager",
    "agent",
    "team",
];

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(format!("NER: {message}"))
}

pub struct Ner {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    words: Regex,
    max_width: usize,
}

/// The paths of an installed model.
pub struct ModelFiles<'a> {
    pub model: &'a Path,
    pub tokenizer: &'a Path,
    pub runtime: &'a Path,
}

impl Ner {
    pub fn load(files: &ModelFiles) -> Result<Self, Error> {
        let env = crate::runtime::env(files.runtime)?;
        let session = Session::builder(env)
            .map_err(error)?
            .commit_from_file(files.model)
            .map_err(|e| error(format!("{}: {e}", files.model.display())))?;
        let tokenizer = Tokenizer::from_file(files.tokenizer)
            .map_err(|e| error(format!("{}: {e}", files.tokenizer.display())))?;
        // The tokenizer of a GLiNER model knows its prompt tokens.
        for t in ["<<ENT>>", "<<SEP>>"] {
            tokenizer
                .token_to_id(t)
                .ok_or_else(|| error(format!("the tokenizer has no `{t}`")))?;
        }
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            words: words_regex(),
            max_width: 12,
        })
    }

    /// The person candidates in `text`: byte offsets, the model's score.
    ///
    /// A text of one word (a cell, a column's name, a key) has no context to
    /// read, and the model guesses (`Name` read as a person): there, only
    /// the patterns decide, with the column's name as their context.
    pub fn candidates(&self, text: &str) -> Result<Vec<Candidate>, Error> {
        let words: Vec<(usize, usize)> = self
            .words
            .find_iter(text)
            .map(|m| (m.start(), m.end()))
            .collect();
        if words
            .iter()
            .filter(|(s, e)| text[*s..*e].chars().any(char::is_alphabetic))
            .count()
            < 2
        {
            return Ok(Vec::new());
        }
        let mut out: Vec<Candidate> = Vec::new();
        let mut start = 0;
        while start < words.len() {
            let end = (start + WINDOW).min(words.len());
            for (s, e, score) in self.window(text, &words[start..end])? {
                let c = Candidate {
                    start: s,
                    end: e,
                    entity: EntityType::new("PERSON").expect("a valid type"),
                    confidence: confidence(score),
                    detector: "ner".into(),
                };
                if can_be_name(&text[s..e]) && out.iter().all(|o| o.end <= s || o.start >= e) {
                    out.push(c);
                }
            }
            if end == words.len() {
                break;
            }
            start = end - OVERLAP;
        }
        out.sort_by_key(|c| c.start);
        Ok(out)
    }

    /// One window: (start, end, score) of each person span, greedy without
    /// overlaps, highest score first.
    fn window(
        &self,
        text: &str,
        words: &[(usize, usize)],
    ) -> Result<Vec<(usize, usize, f32)>, Error> {
        let n = words.len();
        if n == 0 {
            return Ok(Vec::new());
        }
        let inputs = self.inputs(text, words)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| error("a poisoned session"))?;
        let outputs = session
            .run(ort::inputs![
                "input_ids" => Tensor::from_array(([1usize, inputs.ids.len()], inputs.ids.clone())).map_err(error)?,
                "attention_mask" => Tensor::from_array(([1usize, inputs.ids.len()], vec![1i64; inputs.ids.len()])).map_err(error)?,
                "words_mask" => Tensor::from_array(([1usize, inputs.words_mask.len()], inputs.words_mask.clone())).map_err(error)?,
                "text_lengths" => Tensor::from_array(([1usize, 1], vec![n as i64])).map_err(error)?,
                "span_idx" => Tensor::from_array(([1usize, inputs.span_mask.len(), 2], inputs.span_idx.clone())).map_err(error)?,
                "span_mask" => Tensor::from_array(([1usize, inputs.span_mask.len()], inputs.span_mask.clone())).map_err(error)?,
            ])
            .map_err(error)?;
        let (shape, logits) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(error)?;
        let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
        decode(&dims, logits, words, self.max_width)
    }

    fn inputs(&self, text: &str, words: &[(usize, usize)]) -> Result<Inputs, Error> {
        let mut pieces: Vec<String> = Vec::new();
        for l in LABELS {
            pieces.push("<<ENT>>".into());
            pieces.push(l.into());
        }
        pieces.push("<<SEP>>".into());
        let prompt = pieces.len();
        pieces.extend(words.iter().map(|(s, e)| text[*s..*e].to_owned()));
        let enc = self
            .tokenizer
            .encode(pieces.iter().map(String::as_str).collect::<Vec<_>>(), true)
            .map_err(error)?;
        let ids: Vec<i64> = enc.get_ids().iter().map(|i| i64::from(*i)).collect();
        let word_ids = enc.get_word_ids();
        let mut words_mask = vec![0i64; ids.len()];
        let mut last = None;
        for (t, w) in word_ids.iter().enumerate() {
            if let Some(w) = *w
                && w as usize >= prompt
                && last != Some(w)
            {
                words_mask[t] = i64::from(w) - prompt as i64 + 1;
            }
            last = *w;
        }
        let n = words.len();
        let mut span_idx = Vec::with_capacity(n * self.max_width * 2);
        let mut span_mask = Vec::with_capacity(n * self.max_width);
        for i in 0..n {
            for w in 0..self.max_width {
                span_idx.push(i as i64);
                span_idx.push((i + w) as i64);
                span_mask.push(i + w < n);
            }
        }
        Ok(Inputs {
            ids,
            words_mask,
            span_idx,
            span_mask,
        })
    }
}

struct Inputs {
    ids: Vec<i64>,
    words_mask: Vec<i64>,
    span_idx: Vec<i64>,
    span_mask: Vec<bool>,
}

/// GLiNER's word splitter: words with inner `-` or `_`, else one character.
fn words_regex() -> Regex {
    Regex::new(r"\w+(?:[-_]\w+)*|\S").expect("a valid regex")
}

/// A model score above [`MIN_SCORE`] as a confidence the default threshold
/// (0.7) lets through: 0.3 -> 0.79, 1.0 -> 1.0.
fn confidence(score: f32) -> f32 {
    ((0.7 + 0.3 * score) * 1000.0).round() / 1000.0
}

/// Lowercase words inside a name.
const PARTICLES: &[&str] = &[
    "von", "van", "vom", "zu", "zur", "der", "den", "de", "del", "della", "di", "da", "du", "la",
    "le", "ten", "ter", "bin", "ibn", "al", "el", "y",
];

/// Whether a span can be a name: every word capitalised or a particle
/// (`von der`), and one word written like a name (`Kern`, not `IBAN`) that
/// is no role word. On a few words the model calls anything a person (a
/// column `name`, an id `1`), and a line that starts with `customer's IBAN`
/// once read as one.
fn can_be_name(span: &str) -> bool {
    let words: Vec<&str> = span
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 2)
        .collect();
    let capital = |w: &str| w.chars().next().is_some_and(char::is_uppercase);
    words.iter().all(|w| capital(w) || PARTICLES.contains(w))
        && words.iter().any(|w| {
            capital(w)
                && w.chars().skip(1).any(char::is_lowercase)
                && !NOT_NAMES.contains(&w.to_lowercase().as_str())
        })
}

/// The person spans of the logits `[1, words, max_width, labels]`.
fn decode(
    dims: &[usize],
    logits: &[f32],
    words: &[(usize, usize)],
    max_width: usize,
) -> Result<Vec<(usize, usize, f32)>, Error> {
    let [_, n, width, labels] = dims else {
        return Err(error(format!("unexpected logits of shape {dims:?}")));
    };
    let (n, width, labels) = (*n, (*width).min(max_width), *labels);
    let mut spans: Vec<(usize, usize, usize, f32)> = Vec::new();
    for i in 0..n.min(words.len()) {
        for w in 0..width {
            if i + w >= words.len() {
                break;
            }
            for l in 0..labels {
                let x = logits[((i * dims[2]) + w) * labels + l];
                let score = 1.0 / (1.0 + (-x).exp());
                if score >= MIN_SCORE {
                    spans.push((i, i + w, l, score));
                }
            }
        }
    }
    // Greedy, flat: the highest score first, no overlaps across labels.
    spans.sort_by(|a, b| b.3.total_cmp(&a.3));
    let mut chosen: Vec<(usize, usize, usize, f32)> = Vec::new();
    for s in spans {
        if chosen.iter().all(|c| s.1 < c.0 || s.0 > c.1) {
            chosen.push(s);
        }
    }
    Ok(chosen
        .into_iter()
        .filter(|s| LABELS[s.2] == "person")
        .map(|(a, b, _, score)| (words[a].0, words[b].1, score))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_split_as_gliner_does() {
        let text = "Beim Termin am 3.4. war auch Lena Vogt (Beispiel GmbH, Köln) anwesend.";
        let words: Vec<&str> = words_regex().find_iter(text).map(|m| m.as_str()).collect();
        assert_eq!(words.len(), 19, "{words:?}");
        assert_eq!(&words[..6], ["Beim", "Termin", "am", "3", ".", "4"]);
    }

    #[test]
    fn decoding_takes_the_best_spans_without_overlaps() {
        // Three words, max width 2, labels person/organization/location.
        let words = [(0, 4), (5, 9), (10, 14)];
        let dims = [1, 3, 2, 3];
        let mut logits = vec![-10.0f32; 3 * 2 * 3];
        let at = |i: usize, w: usize, l: usize| (i * 2 + w) * 3 + l;
        logits[at(0, 1, 0)] = 4.0; // words 0-1: person
        logits[at(1, 0, 1)] = 2.0; // word 1: organization, overlaps, lower
        logits[at(2, 0, 2)] = 3.0; // word 2: location
        let spans = decode(&dims, &logits, &words, 12).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0].0, spans[0].1), (0, 9));
    }

    #[test]
    fn role_words_are_no_names() {
        assert!(!can_be_name("Der Kunde"));
        assert!(!can_be_name("Herr Doktor"));
        assert!(!can_be_name("name"));
        assert!(!can_be_name("1"));
        assert!(!can_be_name("Der Kunde"));
        assert!(can_be_name("Lena Vogt"));
        assert!(can_be_name("Herr Kern"));
        assert!(!can_be_name("Kunde #1"));
        // A lowercase word that is no particle, or capitals only: no name.
        assert!(!can_be_name("customer's IBAN"));
        assert!(!can_be_name("IBAN"));
        assert!(!can_be_name("Agent"));
        assert!(can_be_name("Jonas Brandt's"));
        assert!(can_be_name("Ursula von der Leyen"));
        assert!(can_be_name("MEIER, Jonas"));
        assert_eq!(confidence(MIN_SCORE), 0.79);
    }

    /// The real model, when installed: VEILQ_NER_DIR (model.onnx,
    /// tokenizer.json) and VEILQ_ORT_DYLIB.
    #[test]
    fn the_model_finds_names_without_a_marking_word() {
        let (Ok(dir), Ok(dylib)) = (
            std::env::var("VEILQ_NER_DIR"),
            std::env::var("VEILQ_ORT_DYLIB"),
        ) else {
            return;
        };
        let dir = Path::new(&dir);
        let ner = Ner::load(&ModelFiles {
            model: &dir.join("model.onnx"),
            tokenizer: &dir.join("tokenizer.json"),
            runtime: Path::new(&dylib),
        })
        .unwrap();
        let text = "Beim Termin am 3.4. war auch Lena Vogt (Beispiel GmbH, Köln) anwesend.";
        let found: Vec<&str> = ner
            .candidates(text)
            .unwrap()
            .iter()
            .map(|c| &text[c.start..c.end])
            .collect();
        assert_eq!(found, ["Lena Vogt"]);
        // One word has no context: no guess.
        assert!(ner.candidates("Name").unwrap().is_empty());
        assert!(ner.candidates("Mira").unwrap().is_empty());
    }
}
