//! The policy file, `veilq.toml` (ADR-0004).
//!
//! veilq looks for it in exactly two places, unless `--policy` names one:
//! the current directory, then the root of the git repository the current
//! directory is in. It does not search the directories in between or above:
//! a policy that applies should be one a reader of the repository finds.
//! Without a file, the built-in defaults apply. Unknown keys are errors, so
//! a misspelt key does not silently weaken a policy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::detect::{LOCALES, validate::VALIDATORS};
use crate::finding::EntityType;
use crate::models::DETECTORS;
use crate::{Action, Error};

pub const FILE_NAME: &str = "veilq.toml";
/// The locales whose built-in patterns run when a policy names none.
pub const DEFAULT_LOCALES: &[&str] = &["de", "en"];
pub const POLICY_VERSION: u32 = 1;
pub(crate) const DEFAULT_MIN_CONFIDENCE: f32 = 0.7;
const DEFAULT_PATTERN_CONFIDENCE: f32 = 0.9;
const DEFAULT_CONTEXT_BOOST: f32 = 0.3;
const DEFAULT_CONTEXT_WINDOW: usize = 48;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// The format's version; `1`.
    pub version: u32,
    /// What veilq is used for (ADR-0040): `general`, the defaults, is
    /// always one; `legal` and `development` add their packs. Between packs
    /// the stricter rule wins, and this policy's own settings come first.
    #[serde(default = "default_purposes")]
    pub purposes: Vec<String>,
    #[serde(default)]
    pub defaults: Defaults,
    /// The locales whose built-in patterns run (ADR-0010); the
    /// locale-independent ones (e-mail, IBAN, IP, cards) always run.
    #[serde(default = "default_locales")]
    pub locales: Vec<String>,
    /// Per entity type: what differs from `defaults`.
    #[serde(default)]
    pub entities: BTreeMap<EntityType, EntityRule>,
    /// The project's own patterns.
    #[serde(default)]
    pub patterns: Vec<PatternRule>,
    #[serde(default)]
    pub allow: Allow,
    /// Values that are always personal data (ADR-0044): a list of the people
    /// a text can name, inline or from files. A value here is found even
    /// where `allow` names it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<Deny>,
    #[serde(default)]
    pub detectors: Detectors,
    /// Required when any action is `pseudonymize`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pseudonymize: Option<Pseudonymize>,
    /// Who can read a mapping file (ADR-0018).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<Mapping>,
    /// Office files (ADR-0019).
    #[serde(default)]
    pub office: Office,
    /// Pictures (ADR-0023, ADR-0038).
    #[serde(default)]
    pub picture: Picture,
    /// Bodies that carry their verdict (`veilq mark`, ADR-0024).
    #[serde(default)]
    pub markers: Markers,
    /// Per entity type: what a miss and a false positive cost (ADR-0040),
    /// before the purposes' costs. Data for a learned decider; no
    /// threshold follows from them yet.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub costs: BTreeMap<EntityType, crate::purpose::Cost>,
}

fn default_purposes() -> Vec<String> {
    vec![crate::purpose::GENERAL.to_owned()]
}

/// How `veilq mark` reads a body's first line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Markers {
    /// A body whose first line starts with one of these is handed on as it
    /// is, unread and unmarked: an agent's technical note, say.
    #[serde(default)]
    pub exempt: Vec<String>,
}

/// How Office files are read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Office {
    /// Embedded objects, and images when OCR is not installed (with OCR an
    /// image is read and redacted, ADR-0023): `error` (the default) stops
    /// the run, `remove` blanks images (the report then asks for a review);
    /// other objects stay an error.
    #[serde(default)]
    pub embedded: Embedded,
}

/// How pictures are read: images, GIF frames, PDF pages, and images inside
/// Office files and mails.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Picture {
    /// Faces (ADR-0038): `redact` (the default) fills every face the face
    /// detector finds black, as `FACE` findings; a picture is then an error
    /// while the detector is not installed (`veilq models fetch face`).
    /// `keep` leaves faces as they are, for pictures that show no people
    /// or whose people may be seen.
    #[serde(default)]
    pub faces: FacePolicy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FacePolicy {
    #[default]
    Redact,
    Keep,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Embedded {
    #[default]
    Error,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    #[serde(default)]
    pub action: Action,
    /// Findings below this confidence are dropped; from 0 to 1.
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            action: Action::default(),
            min_confidence: DEFAULT_MIN_CONFIDENCE,
        }
    }
}

fn default_min_confidence() -> f32 {
    DEFAULT_MIN_CONFIDENCE
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternRule {
    /// Lower case letters, digits and `-`; the detector is `policy:<name>`
    /// (built-in: `pattern:<locale>.<name>`).
    pub name: String,
    pub entity: EntityType,
    /// A Rust `regex` pattern. With a group named `pii`, only that group is
    /// the finding (the rest is context the match needs).
    pub regex: String,
    #[serde(default = "default_pattern_confidence")]
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    /// A check the match must pass (`iban`, `luhn`, `tax_id_de`, ...). A
    /// match that fails it loses 0.3 of its confidence: below the threshold
    /// without context, above it with (ADR-0012).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validate: Option<String>,
}

impl PatternRule {
    /// Checks what serde cannot: the name, the confidences, the regex, the
    /// validator. `what` names the rule in a message.
    pub(crate) fn check(&self, what: &str) -> Result<(), String> {
        let valid_name = !self.name.is_empty()
            && self
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid_name {
            return Err(format!(
                "{what} name `{}`: lower case letters, digits and `-` only",
                self.name
            ));
        }
        check_confidence(
            &format!("{what} `{}` confidence", self.name),
            self.confidence,
        )?;
        regex::Regex::new(&self.regex).map_err(|e| format!("{what} `{}`: {e}", self.name))?;
        if let Some(c) = &self.context {
            check_confidence(&format!("{what} `{}` context.boost", self.name), c.boost)?;
            if c.words.iter().any(|w| w.trim().is_empty()) {
                return Err(format!("{what} `{}`: an empty context word", self.name));
            }
        }
        if let Some(v) = &self.validate
            && !VALIDATORS.contains(&v.as_str())
        {
            return Err(format!(
                "{what} `{}`: unknown validator `{v}` (known: {})",
                self.name,
                VALIDATORS.join(", ")
            ));
        }
        Ok(())
    }
}

fn default_pattern_confidence() -> f32 {
    DEFAULT_PATTERN_CONFIDENCE
}

fn default_locales() -> Vec<String> {
    DEFAULT_LOCALES.iter().map(|l| (*l).to_owned()).collect()
}

/// Words near a match that make it personal data: `geb.` before a date, the
/// column `email`, the key `iban` (ADR-0011).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Compared case-insensitively; a word that starts with a letter or digit
    /// must start a word in the text too.
    pub words: Vec<String>,
    /// Added to the confidence when a word is found; from 0 to 1.
    #[serde(default = "default_context_boost")]
    pub boost: f32,
    /// How many characters before the match are searched, the column's or
    /// key's name included.
    #[serde(default = "default_context_window")]
    pub window: usize,
}

fn default_context_boost() -> f32 {
    DEFAULT_CONTEXT_BOOST
}

fn default_context_window() -> usize {
    DEFAULT_CONTEXT_WINDOW
}

/// What is never personal data here: the firm's own name, court names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    /// Exact values, compared after the same normalisation as pseudonyms.
    #[serde(default)]
    pub values: Vec<String>,
    /// Rust `regex` patterns a whole finding must match.
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// Known values of one type (ADR-0044), each found as a whole word,
/// whatever its case, with the genitive `s`, `'s` and `’s`, by the detector
/// `policy:deny` at confidence 1.0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deny {
    /// The type the values are found as; `PERSON` by default.
    #[serde(default = "default_deny_entity")]
    pub entity: EntityType,
    #[serde(default)]
    pub values: Vec<String>,
    /// Plain text files, relative to the policy file that names them: UTF-8,
    /// one value per line; blank lines and lines starting with `#` are
    /// skipped. A file that cannot be read is an error.
    #[serde(default)]
    pub files: Vec<String>,
    /// A value of several words also finds each word of three letters or
    /// more alone (`Ayşe Yılmaz`: `Ayşe`, `Yılmaz`), except role words.
    #[serde(default = "default_true")]
    pub parts: bool,
    /// The values of `files`, read when the policy was loaded from its file
    /// ([`Policy::read_deny_files`]); `None` until then.
    #[serde(skip)]
    #[schemars(skip)]
    pub read: Option<Vec<String>>,
}

fn default_deny_entity() -> EntityType {
    EntityType::new("PERSON").expect("a valid type")
}

fn default_true() -> bool {
    true
}

impl Deny {
    /// Every value: inline, then from the files.
    pub fn all_values(&self) -> impl Iterator<Item = &str> {
        self.values
            .iter()
            .chain(self.read.iter().flatten())
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Detectors {
    /// Detectors without which a run fails (`ner`, `ocr`, ...); `pattern`
    /// always runs.
    #[serde(default)]
    pub require: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Pseudonymize {
    /// The environment variable that holds the key (at least 16 bytes). The
    /// key itself never goes into the policy.
    pub key_env: String,
}

/// The mapping file: what `pseudonymize` and `fake` replacements stood for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    /// age X25519 recipients (`age1...`) the mapping is encrypted to. Only
    /// public keys: whoever holds a matching identity can read it.
    pub recipients: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            version: POLICY_VERSION,
            defaults: Defaults::default(),
            locales: default_locales(),
            entities: BTreeMap::new(),
            patterns: Vec::new(),
            allow: Allow::default(),
            deny: Vec::new(),
            detectors: Detectors::default(),
            pseudonymize: None,
            mapping: None,
            office: Office::default(),
            picture: Picture::default(),
            markers: Markers::default(),
            purposes: default_purposes(),
            costs: BTreeMap::new(),
        }
    }
}

/// Where the policy of a run came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    BuiltIn,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(p) => write!(f, "{}", p.display()),
            Self::BuiltIn => f.write_str("built-in defaults"),
        }
    }
}

impl Policy {
    /// The policy for a run in `cwd`: `explicit` if given (it must exist),
    /// else `veilq.toml` in `cwd`, else in the root of `cwd`'s git
    /// repository, else the user's ([`user_file`], ADR-0028), else the
    /// built-in defaults.
    pub fn discover(explicit: Option<&Path>, cwd: &Path) -> Result<(Self, Source), Error> {
        Self::discover_in(explicit, cwd, user_file().as_deref())
    }

    fn discover_in(
        explicit: Option<&Path>,
        cwd: &Path,
        user: Option<&Path>,
    ) -> Result<(Self, Source), Error> {
        if let Some(path) = explicit {
            if !path.is_file() {
                return Err(Error::PolicyNotFound(path.to_path_buf()));
            }
            return Ok((Self::load(path)?, Source::File(path.to_path_buf())));
        }
        let mut candidates = vec![cwd.join(FILE_NAME)];
        if let Some(root) = git_root(cwd) {
            candidates.push(root.join(FILE_NAME));
        }
        candidates.extend(user.map(Path::to_path_buf));
        for path in candidates {
            if path.is_file() {
                return Ok((Self::load(&path)?, Source::File(path)));
            }
        }
        Ok((Self::default(), Source::BuiltIn))
    }

    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut policy = Self::parse(&text).map_err(|message| Error::Policy {
            path: path.to_path_buf(),
            message,
        })?;
        policy
            .read_deny_files(path.parent().unwrap_or(Path::new(".")))
            .map_err(|message| Error::Policy {
                path: path.to_path_buf(),
                message,
            })?;
        Ok(policy)
    }

    /// Reads the files of `deny`, relative to `base` (the directory of the
    /// policy file). A file that cannot be read is an error: a list that
    /// silently stays empty finds nothing (fail closed).
    pub fn read_deny_files(&mut self, base: &Path) -> Result<(), String> {
        for d in &mut self.deny {
            let mut values = Vec::new();
            for f in &d.files {
                let path = base.join(f);
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("deny file `{}`: {e}", path.display()))?;
                values.extend(
                    text.lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        .map(str::to_owned),
                );
            }
            d.read = Some(values);
        }
        Ok(())
    }

    /// The policy as a `veilq.toml` would hold it.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let policy: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != POLICY_VERSION {
            return Err(format!(
                "version {} is not supported (this veilq reads version {POLICY_VERSION})",
                self.version
            ));
        }
        check_confidence("defaults.min_confidence", self.defaults.min_confidence)?;
        for (entity, rule) in &self.entities {
            if let Some(c) = rule.min_confidence {
                check_confidence(&format!("entities.{}.min_confidence", entity.as_str()), c)?;
            }
        }
        for p in &self.purposes {
            crate::purpose::purpose(p)?;
        }
        for (entity, c) in &self.costs {
            if !(c.miss > 0.0 && c.false_positive > 0.0) {
                return Err(format!("`costs.{}` must be above zero", entity.as_str()));
            }
        }
        for l in &self.locales {
            if !LOCALES.contains(&l.as_str()) {
                return Err(format!(
                    "unknown locale `{l}` (known: {})",
                    LOCALES.join(", ")
                ));
            }
        }
        if let Some(m) = &self.mapping {
            if m.recipients.is_empty() {
                return Err("`mapping.recipients` is empty".into());
            }
            for r in &m.recipients {
                crate::mapping::check_recipient(r)?;
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for p in &self.patterns {
            p.check("pattern")?;
            if !names.insert(&p.name) {
                return Err(format!("pattern name `{}` is used twice", p.name));
            }
        }
        for r in &self.allow.patterns {
            regex::Regex::new(r).map_err(|e| format!("allow pattern `{r}`: {e}"))?;
        }
        for (i, d) in self.deny.iter().enumerate() {
            if d.values.is_empty() && d.files.is_empty() {
                return Err(format!(
                    "`deny` table {} has neither `values` nor `files`",
                    i + 1
                ));
            }
        }
        for d in &self.detectors.require {
            if !DETECTORS.contains(&d.as_str()) {
                return Err(format!(
                    "unknown detector `{d}` (known: {})",
                    DETECTORS.join(", ")
                ));
            }
        }
        let pseudonymizes = self.defaults.action == Action::Pseudonymize
            || self
                .entities
                .values()
                .any(|r| r.action == Some(Action::Pseudonymize));
        match &self.pseudonymize {
            None if pseudonymizes => {
                return Err(
                    "an action is `pseudonymize`, but `[pseudonymize] key_env` is not set".into(),
                );
            }
            Some(p) if p.key_env.trim().is_empty() => {
                return Err("`pseudonymize.key_env` is empty".into());
            }
            _ => {}
        }
        Ok(())
    }

    /// The purposes in effect: `general` first, then the policy's.
    pub fn effective_purposes(&self) -> Vec<String> {
        crate::purpose::effective(&self.purposes)
    }

    /// This policy with its purposes' rules in it (ADR-0040): per type the
    /// purposes' merged threshold and action where this policy sets none,
    /// their allow lists added, their costs where this policy names none.
    /// The purposes' patterns are not copied: the detector runs them under
    /// their own names.
    pub fn resolved(&self) -> Result<Policy, String> {
        let merged = crate::purpose::merge(&self.effective_purposes())?;
        let mut out = self.clone();
        for (entity, rule) in merged.entities {
            let own = out.entities.entry(entity).or_default();
            own.min_confidence = own.min_confidence.or(rule.min_confidence);
            own.action = own.action.or(rule.action);
        }
        for v in merged.allow.values {
            if !out.allow.values.contains(&v) {
                out.allow.values.push(v);
            }
        }
        for v in merged.allow.patterns {
            if !out.allow.patterns.contains(&v) {
                out.allow.patterns.push(v);
            }
        }
        for (entity, c) in merged.costs {
            out.costs.entry(entity).or_insert(c);
        }
        Ok(out)
    }

    /// What a mistake on `entity` costs (ADR-0040): this policy's, else its
    /// purposes', else [`crate::purpose::DEFAULT_COST`]. Call on a
    /// [`Policy::resolved`] policy to see the purposes'.
    pub fn cost(&self, entity: &EntityType) -> crate::purpose::Cost {
        self.costs
            .get(entity)
            .copied()
            .unwrap_or(crate::purpose::DEFAULT_COST)
    }

    /// The action for findings of `entity`.
    pub fn action_for(&self, entity: &EntityType) -> Action {
        self.entities
            .get(entity)
            .and_then(|r| r.action)
            .unwrap_or(self.defaults.action)
    }

    /// The JSON schema of the policy file, as `schema/policy.schema.json`
    /// holds it (TOML read as JSON).
    pub fn json_schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(Policy)).expect("a schema serialises")
    }
}

fn check_confidence(key: &str, value: f32) -> Result<(), String> {
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(format!("`{key}` must be between 0 and 1, got {value}"))
    }
}

/// The nearest directory at or above `dir` with a `.git` entry (a directory,
/// or a file in a worktree).
/// The user's own policy (ADR-0028), used where no project has one:
/// `%APPDATA%\veilq\veilq.toml` on Windows, `~/Library/Application
/// Support/veilq/veilq.toml` on macOS, else
/// `$XDG_CONFIG_HOME/veilq/veilq.toml` or `~/.config/veilq/veilq.toml`.
/// `VEILQ_USER_POLICY` names another file.
pub fn user_file() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("VEILQ_USER_POLICY") {
        return Some(PathBuf::from(p));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("veilq").join(FILE_NAME))
}

fn git_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
version = 1

[defaults]
action = "mask"
min_confidence = 0.6

[entities.PERSON]
action = "pseudonymize"
min_confidence = 0.8

[[patterns]]
name = "file-number"
entity = "FILE_NUMBER"
regex = '\b\d{4}/\d{2}\b'

[allow]
values = ["Example Law Firm"]
patterns = ['^Amtsgericht \w+$']

[detectors]
require = ["ner"]

[pseudonymize]
key_env = "VEILQ_KEY"
"#;

    #[test]
    fn a_full_policy_parses() {
        let p = Policy::parse(FULL).unwrap();
        let person = EntityType::new("PERSON").unwrap();
        assert_eq!(p.action_for(&person), Action::Pseudonymize);
        assert_eq!(
            p.action_for(&EntityType::new("EMAIL_ADDRESS").unwrap()),
            Action::Mask
        );
        assert_eq!(p.patterns[0].confidence, DEFAULT_PATTERN_CONFIDENCE);
    }

    #[test]
    fn the_minimal_policy_is_the_default() {
        assert_eq!(Policy::parse("version = 1").unwrap(), Policy::default());
    }

    #[test]
    fn mistakes_are_errors() {
        let cases = [
            ("version = 2", "version 2"),
            ("version = 1\nverison = 1", "unknown field"),
            (
                "version = 1\n[defaults]\nmin_confidence = 1.5",
                "between 0 and 1",
            ),
            (
                "version = 1\n[defaults]\naction = \"blur\"",
                "unknown variant",
            ),
            (
                "version = 1\n[entities.person]\naction = \"mask\"",
                "not an entity type",
            ),
            (
                "version = 1\n[[patterns]]\nname = \"x\"\nentity = \"X\"\nregex = \"(\"",
                "pattern `x`",
            ),
            (
                "version = 1\n[[patterns]]\nname = \"X\"\nentity = \"X\"\nregex = \"x\"",
                "lower case",
            ),
            (
                "version = 1\n[detectors]\nrequire = [\"magic\"]",
                "unknown detector",
            ),
            (
                "version = 1\n[defaults]\naction = \"pseudonymize\"",
                "key_env",
            ),
        ];
        for (text, expected) in cases {
            let err = Policy::parse(text).unwrap_err();
            assert!(err.contains(expected), "{text:?}: {err}");
        }
    }

    #[test]
    fn the_user_policy_comes_after_the_project() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let user = repo.path().join("user.toml");
        let (_, source) = Policy::discover_in(None, repo.path(), Some(&user)).unwrap();
        assert_eq!(source, Source::BuiltIn, "a user file that is not there");
        std::fs::write(&user, "version = 1").unwrap();
        let (_, source) = Policy::discover_in(None, repo.path(), Some(&user)).unwrap();
        assert_eq!(source, Source::File(user.clone()));
        std::fs::write(repo.path().join(FILE_NAME), "version = 1").unwrap();
        let (_, source) = Policy::discover_in(None, repo.path(), Some(&user)).unwrap();
        assert_eq!(source, Source::File(repo.path().join(FILE_NAME)));
    }

    #[test]
    fn a_policy_written_as_toml_reads_the_same() {
        let text = r#"
version = 1
locales = ["de"]
[defaults]
action = "fake"
min_confidence = 0.8
[entities.PHONE_NUMBER]
action = "remove"
[[patterns]]
name = "customer-no"
entity = "CUSTOMER_NUMBER"
regex = 'KD-\d{6}'
[allow]
values = ["Muster GmbH"]
"#;
        let policy = Policy::parse(text).unwrap();
        let again = Policy::parse(&policy.to_toml().unwrap()).unwrap();
        assert_eq!(policy, again);
    }

    #[test]
    fn discovery_looks_in_cwd_then_git_root_only() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".git")).unwrap();
        let sub = repo.path().join("a/b");
        std::fs::create_dir_all(&sub).unwrap();

        let (_, source) = Policy::discover_in(None, &sub, None).unwrap();
        assert_eq!(source, Source::BuiltIn);

        // A file between cwd and the root is not found.
        std::fs::write(repo.path().join("a").join(FILE_NAME), "version = 1").unwrap();
        let (_, source) = Policy::discover_in(None, &sub, None).unwrap();
        assert_eq!(source, Source::BuiltIn);

        std::fs::write(repo.path().join(FILE_NAME), "version = 1").unwrap();
        let (_, source) = Policy::discover_in(None, &sub, None).unwrap();
        assert_eq!(source, Source::File(repo.path().join(FILE_NAME)));

        std::fs::write(sub.join(FILE_NAME), "version = 1").unwrap();
        let (_, source) = Policy::discover_in(None, &sub, None).unwrap();
        assert_eq!(source, Source::File(sub.join(FILE_NAME)));

        assert!(matches!(
            Policy::discover_in(Some(&repo.path().join("missing.toml")), &sub, None),
            Err(Error::PolicyNotFound(_))
        ));
    }

    #[test]
    fn deny_files_are_read_relative_to_their_policy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("lists")).unwrap();
        std::fs::write(
            dir.path().join("lists/names.txt"),
            "# members\nAyşe Yılmaz\n\n  Mehmet  \n",
        )
        .unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(
            &path,
            "version = 1\n[[deny]]\nvalues = [\"Lena\"]\nfiles = [\"lists/names.txt\"]\n",
        )
        .unwrap();
        let policy = Policy::load(&path).unwrap();
        let values: Vec<&str> = policy.deny[0].all_values().collect();
        assert_eq!(values, ["Lena", "Ayşe Yılmaz", "Mehmet"]);
        // Written back, the file names its lists, not their values.
        let toml = policy.to_toml().unwrap();
        assert!(
            toml.contains("lists/names.txt") && !toml.contains("Mehmet"),
            "{toml}"
        );

        // A list that cannot be read fails the policy.
        std::fs::remove_file(dir.path().join("lists/names.txt")).unwrap();
        let err = Policy::load(&path).unwrap_err().to_string();
        assert!(err.contains("names.txt"), "{err}");
    }
}
