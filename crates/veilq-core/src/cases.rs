//! The case store (#34, review card points 2 to 4): what a user marked as
//! veilq's mistake, kept on the user's machine to test every later rule
//! against.
//!
//! One folder per case under [`case_dir`]: `case.json` in the format of
//! `schema/case.schema.json`, which holds no value, and, where the OS has a
//! key store, `excerpt.age`: the line, cell or picture crop around the mark,
//! encrypted with age to the store's key. The excerpt goes after
//! [`EXCERPT_DAYS`]; the case stays. Without a key the excerpt is never
//! written, and an excerpt is never written unencrypted (fail closed).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::finding::{EntityType, Location};

/// The case format's version.
pub const CASE_VERSION: u32 = 1;
/// How long an excerpt is kept, in days (the card's default).
pub const EXCERPT_DAYS: u64 = 30;
const CASE_FILE: &str = "case.json";
const EXCERPT_FILE: &str = "excerpt.age";
const TWIN_FILE: &str = "twin.txt";

/// What the user marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Personal data veilq left.
    Missed,
    /// A box veilq drew where there is no personal data.
    Extra,
}

/// One mistake a user marked, without its value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// The format's version; `1`.
    pub version: u32,
    /// Hex, unique in the store.
    pub id: String,
    /// When it was marked, in seconds since 1970 (UTC).
    pub created: u64,
    /// The veilq that made the mistake.
    pub veilq: String,
    /// SHA-256 of the policy in effect, as TOML: which rules made it.
    pub policy_sha256: String,
    /// The input's format (`text`, `csv`, `png`, ...).
    pub format: String,
    pub kind: Kind,
    /// The type the user gave a miss, or the type of the box released.
    pub entity: EntityType,
    /// The user's own word for it, where the type is not enough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Where the mark is in the input.
    pub location: Location,
    /// What the detectors proposed there and what was decided, as
    /// `--explain` records it, without values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CaseCandidate>,
    /// The encrypted excerpt, while it is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<Excerpt>,
    /// The synthetic twin, in `twin.txt`, when one reproduces the mistake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twin: Option<Twin>,
}

/// A case's synthetic twin (#34, card point 6): the excerpt with its values
/// replaced by stand-ins, on which veilq makes the same mistake, holding none
/// of the original values veilq or the user named. Kept in the clear: it is
/// the regression set every later rule is tried on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Twin {
    /// Byte offsets of the marked value's stand-in in `twin.txt`.
    pub start: usize,
    pub end: usize,
}

/// A candidate at the mark, from [`crate::Explained`], without its value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaseCandidate {
    /// Byte offsets into the excerpt's text (for a text excerpt).
    pub start: usize,
    pub end: usize,
    pub entity: String,
    pub detector: String,
    pub confidence: f32,
    pub threshold: f32,
    /// The verdict as `--explain` names it (`kept`, `below_threshold`, ...).
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lifted: bool,
}

impl From<&crate::Explained> for CaseCandidate {
    fn from(c: &crate::Explained) -> Self {
        Self {
            start: c.start,
            end: c.end,
            entity: c.entity.clone(),
            detector: c.detector.clone(),
            confidence: c.confidence,
            threshold: c.threshold,
            verdict: serde_json::to_value(c.verdict)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            features: c.features.iter().map(|f| (*f).to_owned()).collect(),
            lifted: c.lifted,
        }
    }
}

/// The excerpt of a case, in `excerpt.age`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Excerpt {
    /// `text/plain` or `image/png`.
    pub media_type: String,
    /// When it is deleted, in seconds since 1970 (UTC).
    pub expires: u64,
}

impl Case {
    /// A case to add: the store gives it its id, time and excerpt.
    pub fn new(
        kind: Kind,
        entity: EntityType,
        format: &str,
        location: Location,
        policy: &crate::Policy,
    ) -> Self {
        Self {
            version: CASE_VERSION,
            id: String::new(),
            created: 0,
            veilq: env!("CARGO_PKG_VERSION").to_owned(),
            policy_sha256: policy_sha256(policy),
            format: format.to_owned(),
            kind,
            entity,
            label: None,
            location,
            candidates: Vec::new(),
            excerpt: None,
            twin: None,
        }
    }

    /// The JSON schema of a case, as `schema/case.schema.json` holds it.
    pub fn json_schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(Case)).expect("a schema serialises")
    }
}

fn policy_sha256(policy: &crate::Policy) -> String {
    use sha2::Digest;
    let text = policy.to_toml().unwrap_or_default();
    hex(&sha2::Sha256::digest(text.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Where cases live: `VEILQ_CASE_DIR`, else the user's local data:
/// `%LOCALAPPDATA%\veilq\cases` on Windows (in an MSIX package its own
/// copy, ADR-0039), `~/Library/Application Support/veilq/cases` on macOS,
/// `$XDG_DATA_HOME/veilq/cases` (or `~/.local/share/...`) elsewhere.
pub fn case_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VEILQ_CASE_DIR") {
        return PathBuf::from(dir);
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(windows) {
        crate::models::local_data().or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".local/share")))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("veilq")
        .join("cases")
}

/// The cases in one folder.
pub struct Store {
    dir: PathBuf,
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

fn bad(m: impl std::fmt::Display) -> Error {
    Error::Case(m.to_string())
}

/// Writes `bytes` to `path` whole or not at all.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io(path))
}

/// A fresh id: hex of a hash of the time, the process and a counter.
fn new_id(now_nanos: u128) -> String {
    use sha2::Digest;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let digest = sha2::Sha256::digest(format!("{now_nanos}:{}:{n}", std::process::id()));
    hex(&digest[..8])
}

/// Seconds since 1970.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Store {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).map_err(io(&dir))?;
        Ok(Self { dir })
    }

    fn folder(&self, id: &str) -> Result<PathBuf, Error> {
        // An id names a folder: hex only, so it cannot leave the store.
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad(format!("`{id}` is not a case id")));
        }
        Ok(self.dir.join(id))
    }

    /// Adds `case` with its excerpt (`media_type`, bytes), encrypted to
    /// `key`; without a key the excerpt is dropped and only the case kept.
    /// Returns the case as stored.
    pub fn add(
        &self,
        mut case: Case,
        excerpt: Option<(&str, &[u8])>,
        key: Option<&age::x25519::Recipient>,
        now: u64,
    ) -> Result<Case, Error> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        case.id = new_id(nanos);
        case.created = now;
        let folder = self.folder(&case.id)?;
        std::fs::create_dir_all(&folder).map_err(io(&folder))?;
        case.excerpt = None;
        if let (Some((media_type, bytes)), Some(key)) = (excerpt, key) {
            let path = folder.join(EXCERPT_FILE);
            write_whole(&path, &encrypt(bytes, key)?)?;
            case.excerpt = Some(Excerpt {
                media_type: media_type.to_owned(),
                expires: now + EXCERPT_DAYS * 86_400,
            });
        }
        self.write(&case)?;
        Ok(case)
    }

    /// Gives case `id` its twin: `text`, the marked value's stand-in at
    /// `span`. Returns the case as stored.
    pub fn set_twin(
        &self,
        id: &str,
        text: &str,
        span: std::ops::Range<usize>,
    ) -> Result<Case, Error> {
        if text.get(span.clone()).is_none_or(str::is_empty) {
            return Err(bad("a twin's span is not in its text"));
        }
        let mut case = self
            .list()?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| bad(format!("no case `{id}`")))?;
        let path = self.folder(id)?.join(TWIN_FILE);
        write_whole(&path, text.as_bytes())?;
        case.twin = Some(Twin {
            start: span.start,
            end: span.end,
        });
        self.write(&case)?;
        Ok(case)
    }

    /// The twin of case `id`, if it has one.
    pub fn twin(&self, id: &str) -> Result<Option<String>, Error> {
        let path = self.folder(id)?.join(TWIN_FILE);
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&path).map(Some).map_err(io(&path))
    }

    fn write(&self, case: &Case) -> Result<(), Error> {
        let path = self.folder(&case.id)?.join(CASE_FILE);
        let json = serde_json::to_vec_pretty(case).map_err(bad)?;
        write_whole(&path, &json)
    }

    /// The case folders, and whether each holds its `case.json` (it is
    /// written last: a folder without one is a case whose writing broke).
    fn folders(&self) -> Result<Vec<(PathBuf, bool)>, Error> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.dir).map_err(io(&self.dir))? {
            let path = entry.map_err(io(&self.dir))?.path();
            if path.is_dir() {
                let complete = path.join(CASE_FILE).is_file();
                out.push((path, complete));
            }
        }
        Ok(out)
    }

    /// Every case, oldest first by the second it was made in (cases of the
    /// same second in no promised order). A case that cannot be read is an error:
    /// the store is not guessed at; a case whose writing broke is skipped
    /// (and removed by [`purge`](Self::purge)).
    pub fn list(&self) -> Result<Vec<Case>, Error> {
        let mut out = Vec::new();
        for (folder, complete) in self.folders()? {
            if !complete {
                continue;
            }
            let path = folder.join(CASE_FILE);
            let text = std::fs::read_to_string(&path).map_err(io(&path))?;
            let case: Case =
                serde_json::from_str(&text).map_err(|e| bad(format!("{}: {e}", path.display())))?;
            if case.version != CASE_VERSION {
                return Err(bad(format!(
                    "{}: version {} (this veilq reads {CASE_VERSION})",
                    path.display(),
                    case.version
                )));
            }
            out.push(case);
        }
        out.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
        Ok(out)
    }

    /// The excerpt of case `id`, decrypted with `identity`; `None` when it
    /// is gone.
    pub fn excerpt(
        &self,
        id: &str,
        identity: &age::x25519::Identity,
    ) -> Result<Option<Vec<u8>>, Error> {
        let path = self.folder(id)?.join(EXCERPT_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read(&path).map_err(io(&path))?;
        let decryptor = age::Decryptor::new(&data[..]).map_err(bad)?;
        let mut reader = decryptor
            .decrypt(std::iter::once(identity as &dyn age::Identity))
            .map_err(bad)?;
        let mut plain = Vec::new();
        reader.read_to_end(&mut plain).map_err(io(&path))?;
        Ok(Some(plain))
    }

    pub fn delete(&self, id: &str) -> Result<(), Error> {
        let folder = self.folder(id)?;
        std::fs::remove_dir_all(&folder).map_err(io(&folder))
    }

    /// Removes every case ("Delete all cases"), those whose writing broke
    /// too; returns how many folders went.
    pub fn delete_all(&self) -> Result<usize, Error> {
        let folders = self.folders()?;
        for (folder, _) in &folders {
            std::fs::remove_dir_all(folder).map_err(io(folder))?;
        }
        Ok(folders.len())
    }

    /// Deletes the excerpts whose time is up (their cases stay) and the
    /// cases whose writing broke. Returns how many excerpts went.
    pub fn purge(&self, now: u64) -> Result<usize, Error> {
        for (folder, complete) in self.folders()? {
            if !complete {
                std::fs::remove_dir_all(&folder).map_err(io(&folder))?;
            }
        }
        let mut n = 0;
        for mut case in self.list()? {
            if case.excerpt.as_ref().is_some_and(|e| e.expires <= now) {
                let path = self.folder(&case.id)?.join(EXCERPT_FILE);
                if path.exists() {
                    std::fs::remove_file(&path).map_err(io(&path))?;
                }
                case.excerpt = None;
                self.write(&case)?;
                n += 1;
            }
        }
        Ok(n)
    }
}

fn encrypt(bytes: &[u8], key: &age::x25519::Recipient) -> Result<Vec<u8>, Error> {
    let encryptor = age::Encryptor::with_recipients(std::iter::once(key as &dyn age::Recipient))
        .map_err(bad)?;
    let mut out = Vec::new();
    let mut writer = encryptor.wrap_output(&mut out).map_err(bad)?;
    writer.write_all(bytes).map_err(bad)?;
    writer.finish().map_err(bad)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Policy;

    fn case() -> Case {
        Case::new(
            Kind::Missed,
            EntityType::new("PERSON").unwrap(),
            "text",
            Location::Span {
                part: None,
                start: 10,
                end: 21,
                line: 1,
                column: 11,
            },
            &Policy::default(),
        )
    }

    const EXCERPT: &[u8] = b"Termin mit Lena Brandt am Montag";

    #[test]
    fn a_case_keeps_its_excerpt_encrypted_and_no_value_in_the_clear() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let key = age::x25519::Identity::generate();
        let stored = store
            .add(
                case(),
                Some(("text/plain", EXCERPT)),
                Some(&key.to_public()),
                1_000,
            )
            .unwrap();
        assert_eq!(
            stored.excerpt.as_ref().unwrap().expires,
            1_000 + 30 * 86_400
        );
        assert_eq!(store.list().unwrap(), vec![stored.clone()]);
        assert_eq!(
            store.excerpt(&stored.id, &key).unwrap().as_deref(),
            Some(EXCERPT)
        );
        // Nothing in the store reads as the value without the key.
        for entry in walk(dir.path()) {
            let bytes = std::fs::read(&entry).unwrap();
            assert!(
                !bytes.windows(6).any(|w| w == b"Brandt"),
                "{} holds the value",
                entry.display()
            );
        }
        let other = age::x25519::Identity::generate();
        assert!(store.excerpt(&stored.id, &other).is_err());
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn without_a_key_no_excerpt_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let stored = store
            .add(case(), Some(("text/plain", EXCERPT)), None, 1_000)
            .unwrap();
        assert!(stored.excerpt.is_none());
        assert!(!dir.path().join(&stored.id).join(EXCERPT_FILE).exists());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn an_excerpt_goes_when_its_time_is_up_and_the_case_stays() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let key = age::x25519::Identity::generate();
        let stored = store
            .add(
                case(),
                Some(("text/plain", EXCERPT)),
                Some(&key.to_public()),
                1_000,
            )
            .unwrap();
        let expires = stored.excerpt.as_ref().unwrap().expires;
        assert_eq!(store.purge(expires - 1).unwrap(), 0);
        assert_eq!(store.purge(expires).unwrap(), 1);
        assert_eq!(store.excerpt(&stored.id, &key).unwrap(), None);
        let left = store.list().unwrap();
        assert_eq!(left.len(), 1);
        assert!(left[0].excerpt.is_none());
    }

    #[test]
    fn cases_are_deleted_one_by_one_or_all() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let a = store.add(case(), None, None, 1).unwrap();
        store.add(case(), None, None, 2).unwrap();
        store.add(case(), None, None, 3).unwrap();
        assert_ne!(a.id, store.list().unwrap()[1].id);
        store.delete(&a.id).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
        assert_eq!(store.delete_all().unwrap(), 2);
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn a_case_whose_writing_broke_is_skipped_and_purged() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store.add(case(), None, None, 1).unwrap();
        // An excerpt written, the case file not: the writing broke.
        std::fs::create_dir(dir.path().join("0123abcd")).unwrap();
        std::fs::write(dir.path().join("0123abcd").join(EXCERPT_FILE), b"x").unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        store.purge(2).unwrap();
        assert!(!dir.path().join("0123abcd").exists());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn a_twin_is_kept_beside_its_case_and_goes_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let stored = store.add(case(), None, None, 1).unwrap();
        assert_eq!(store.twin(&stored.id).unwrap(), None);
        let twin = "Termin mit Jonas Keller am Montag";
        let with = store.set_twin(&stored.id, twin, 11..23).unwrap();
        assert_eq!(with.twin, Some(Twin { start: 11, end: 23 }));
        assert_eq!(store.list().unwrap()[0].twin, with.twin);
        assert_eq!(store.twin(&stored.id).unwrap().as_deref(), Some(twin));
        assert!(
            store.set_twin(&stored.id, twin, 30..40).is_err(),
            "outside the text"
        );
        assert_eq!(store.delete_all().unwrap(), 1);
        assert!(!dir.path().join(&stored.id).exists());
    }

    #[test]
    fn an_id_cannot_leave_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        for id in ["", "..", "../x", "a/b", "c:\\x"] {
            assert!(store.delete(id).is_err(), "{id}");
        }
    }

    #[test]
    fn a_case_of_another_version_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut stored = store.add(case(), None, None, 1).unwrap();
        stored.version = 2;
        store.write(&stored).unwrap();
        assert!(store.list().is_err());
    }
}
