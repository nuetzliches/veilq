//! Detectors and the models they need (ADR-0005, ADR-0022).
//!
//! A model is fetched only by `veilq models fetch` (or installed from a
//! directory with `veilq models install --from`), from a URL with a SHA-256
//! compiled into the binary; what does not match is discarded. A run never
//! downloads. A detector the policy requires whose files are not installed
//! ends the run with exit code 2.
//!
//! ONNX Runtime is code that veilq loads: it is pinned like a model, as the
//! official release archive of each platform and the library in it.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::Error;

/// The detectors a policy can name.
pub const DETECTORS: &[&str] = &["pattern", "ner", "ocr", "verifier"];

/// A file a detector needs, as compiled into the binary.
#[derive(Debug)]
pub struct PinnedFile {
    pub detector: &'static str,
    /// Where it is installed, relative to the model directory.
    pub file: &'static str,
    /// Where it is downloaded from: `base/name`.
    pub base: &'static str,
    pub name: &'static str,
    /// SHA-256 of what the URL serves.
    pub sha256: &'static str,
    /// For an archive (`.zip`, `.tgz`): the member that becomes `file`.
    pub member: Option<&'static str>,
    /// Only for this OS and architecture (`std::env::consts`); `None`: all.
    pub target: Option<(&'static str, &'static str)>,
}

impl PinnedFile {
    pub fn url(&self) -> String {
        format!("{}/{}", self.base, self.name)
    }
}

const ORT: &str = "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1";
/// The ONNX export of `urchade/gliner_multi_pii-v1` (Apache-2.0) that
/// onnx-community publishes on Hugging Face, at a fixed revision (ADR-0022):
/// veilq ships and hosts no model.
const NER_REPO: &str = "https://huggingface.co/onnx-community/gliner_multi_pii-v1/resolve/2e0397a7e8a250d76c37122232b3cbde42c8d629";
const NER_ONNX: &str = "https://huggingface.co/onnx-community/gliner_multi_pii-v1/resolve/2e0397a7e8a250d76c37122232b3cbde42c8d629/onnx";

/// PaddleOCR's detector (PP-OCRv3) and Latin recogniser (PP-OCRv5) in
/// ONNX (Apache-2.0), as `monkt/paddleocr-onnx` publishes them on Hugging
/// Face, at a fixed revision (ADR-0023).
const OCR_DET: &str = "https://huggingface.co/monkt/paddleocr-onnx/resolve/7b02d0a30a07ba2b92ad1ff5a8941ae2c633de65/detection/v3";
const OCR_LATIN: &str = "https://huggingface.co/monkt/paddleocr-onnx/resolve/7b02d0a30a07ba2b92ad1ff5a8941ae2c633de65/languages/latin";

/// The files of this release.
pub const PINNED: &[PinnedFile] = &[
    PinnedFile {
        detector: "ocr",
        file: "ocr/pp-ocrv3-det/det.onnx",
        base: OCR_DET,
        name: "det.onnx",
        sha256: "ee40e80071ba3a320d4efda75f3e22047a7d049e9bf7bcaaf9daea23fc21b935",
        member: None,
        target: None,
    },
    PinnedFile {
        detector: "ocr",
        file: "ocr/latin-pp-ocrv5-rec/rec.onnx",
        base: OCR_LATIN,
        name: "rec.onnx",
        sha256: "614ffc2d6d3902d360fad7f1b0dd455ee45e877069d14c4e51a99dc4ef144409",
        member: None,
        target: None,
    },
    PinnedFile {
        detector: "ocr",
        file: "ocr/latin-pp-ocrv5-rec/dict.txt",
        base: OCR_LATIN,
        name: "dict.txt",
        sha256: "3c0a8a79b612653c25f765271714f71281e4e955962c153e272b7b8c1d2b13ff",
        member: None,
        target: None,
    },
    PinnedFile {
        detector: "ner",
        file: "ner/gliner_multi_pii-v1-fp16/model.onnx",
        base: NER_ONNX,
        name: "model_fp16.onnx",
        sha256: "e756837e13d16ef832f6cd9fca473c2c3dd1eb232cab13e6443d958d608e797b",
        member: None,
        target: None,
    },
    PinnedFile {
        detector: "ner",
        file: "ner/gliner_multi_pii-v1-fp16/tokenizer.json",
        base: NER_REPO,
        name: "tokenizer.json",
        sha256: "914bd3c8fb7b525af9e23b60d0ec7b1248ddb2b99014efd9c02ebeb022f8cab7",
        member: None,
        target: None,
    },
    PinnedFile {
        detector: "runtime",
        file: "onnxruntime-1.31.1/onnxruntime.dll",
        base: ORT,
        name: "onnxruntime-win-x64-1.31.1.zip",
        sha256: "5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c",
        member: Some("onnxruntime-win-x64-1.31.1/lib/onnxruntime.dll"),
        target: Some(("windows", "x86_64")),
    },
    PinnedFile {
        detector: "runtime",
        file: "onnxruntime-1.31.1/libonnxruntime.so",
        base: ORT,
        name: "onnxruntime-linux-x64-1.31.1.tgz",
        sha256: "c849e36a1ef3e22d4ff217b9d1ce4a3cf5b9467b975610913ae959ad5b16bdc9",
        member: Some("onnxruntime-linux-x64-1.31.1/lib/libonnxruntime.so.1.31.1"),
        target: Some(("linux", "x86_64")),
    },
    PinnedFile {
        detector: "runtime",
        file: "onnxruntime-1.31.1/libonnxruntime.so",
        base: ORT,
        name: "onnxruntime-linux-aarch64-1.31.1.tgz",
        sha256: "5317505e2da99be65f8e2775ba3c2afd6d2961fd78aba1bd8f56159f48820c78",
        member: Some("onnxruntime-linux-aarch64-1.31.1/lib/libonnxruntime.so.1.31.1"),
        target: Some(("linux", "aarch64")),
    },
    PinnedFile {
        detector: "runtime",
        file: "onnxruntime-1.31.1/libonnxruntime.dylib",
        base: ORT,
        name: "onnxruntime-osx-arm64-1.31.1.tgz",
        sha256: "2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f",
        member: Some("onnxruntime-osx-arm64-1.31.1/lib/libonnxruntime.dylib"),
        target: Some(("macos", "aarch64")),
    },
];

/// What `detector` needs on this platform: its files and, for a model,
/// ONNX Runtime.
pub fn pinned_for(detector: &str) -> Vec<&'static PinnedFile> {
    PINNED
        .iter()
        .filter(|f| {
            f.detector == detector || (f.detector == "runtime" && matches!(detector, "ner" | "ocr"))
        })
        .filter(|f| {
            f.target.is_none_or(|(os, arch)| {
                os == std::env::consts::OS && arch == std::env::consts::ARCH
            })
        })
        .collect()
}

/// Whether every file of `detector` is in `model_dir` (and there are any).
pub fn installed(detector: &str, model_dir: &Path) -> bool {
    let files = pinned_for(detector);
    !files.is_empty() && files.iter().all(|f| model_dir.join(f.file).is_file())
}

/// Whether `detector` runs without a model (built into the binary, or an
/// endpoint the policy configures).
fn needs_no_model(detector: &str) -> bool {
    matches!(detector, "pattern" | "verifier")
}

/// Fails unless every detector in `required` can run with the models in
/// `model_dir`.
pub fn check_required(required: &[String], model_dir: &Path) -> Result<(), Error> {
    for detector in required {
        if needs_no_model(detector) {
            continue;
        }
        if !installed(detector, model_dir) {
            return Err(Error::ModelMissing {
                detector: detector.clone(),
                hint: if pinned_for(detector).is_empty() {
                    "this release pins no model for it on this platform"
                } else {
                    "run `veilq models fetch`"
                },
            });
        }
    }
    Ok(())
}

/// The installed files of OCR: detector, recogniser, dictionary, runtime.
pub fn ocr_files(model_dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf, PathBuf)> {
    if !installed("ocr", model_dir) {
        return None;
    }
    let files = pinned_for("ocr");
    let find = |suffix: &str| {
        files
            .iter()
            .find(|f| f.file.ends_with(suffix))
            .map(|f| model_dir.join(f.file))
    };
    let runtime = files
        .iter()
        .find(|f| f.member.is_some())
        .map(|f| model_dir.join(f.file))?;
    Some((
        find("det.onnx")?,
        find("rec.onnx")?,
        find("dict.txt")?,
        runtime,
    ))
}

/// The installed files of the NER detector: model, tokenizer, runtime.
pub fn ner_files(model_dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    if !installed("ner", model_dir) {
        return None;
    }
    let files = pinned_for("ner");
    let find = |suffix: &str| {
        files
            .iter()
            .find(|f| f.file.ends_with(suffix))
            .map(|f| model_dir.join(f.file))
    };
    let runtime = files
        .iter()
        .find(|f| f.member.is_some())
        .map(|f| model_dir.join(f.file))?;
    Some((find("model.onnx")?, find("tokenizer.json")?, runtime))
}

fn sha256_of(path: &Path) -> Result<String, Error> {
    let mut file = std::fs::File::open(path).map_err(|source| Error::Io {
        path: path.into(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|source| Error::Io {
            path: path.into(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Installs `f` from `download` (the file the URL serves): checks its
/// SHA-256, extracts the member of an archive, and moves the result into
/// `model_dir`. A mismatch is an error and installs nothing.
pub fn install(f: &PinnedFile, download: &Path, model_dir: &Path) -> Result<PathBuf, Error> {
    let got = sha256_of(download)?;
    if got != f.sha256 {
        return Err(Error::ModelMismatch {
            file: f.name.into(),
            expected: f.sha256.into(),
            got,
        });
    }
    let target = model_dir.join(f.file);
    let dir = target.parent().expect("a file in a directory");
    std::fs::create_dir_all(dir).map_err(|source| Error::Io {
        path: dir.into(),
        source,
    })?;
    let tmp = target.with_extension("veilq-tmp");
    let io = |source| Error::Io {
        path: tmp.clone(),
        source,
    };
    match f.member {
        None => {
            std::fs::copy(download, &tmp).map_err(io)?;
        }
        Some(member) => {
            let mut out = std::fs::File::create(&tmp).map_err(io)?;
            let archive = std::fs::File::open(download).map_err(|source| Error::Io {
                path: download.into(),
                source,
            })?;
            let found = if f.name.ends_with(".zip") {
                let mut zip =
                    zip::ZipArchive::new(archive).map_err(|e| Error::Format(e.to_string()))?;
                let mut m = zip
                    .by_name(member)
                    .map_err(|e| Error::Format(format!("{member}: {e}")))?;
                std::io::copy(&mut m, &mut out).map_err(io)?;
                true
            } else {
                let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
                let mut found = false;
                for entry in tar.entries().map_err(io)? {
                    let mut entry = entry.map_err(io)?;
                    if entry.path().map_err(io)?.to_string_lossy() == member {
                        std::io::copy(&mut entry, &mut out).map_err(io)?;
                        found = true;
                        break;
                    }
                }
                found
            };
            if !found {
                let _ = std::fs::remove_file(&tmp);
                return Err(Error::Format(format!("{} has no {member}", f.name)));
            }
        }
    }
    std::fs::rename(&tmp, &target).map_err(|source| Error::Io {
        path: target.clone(),
        source,
    })?;
    Ok(target)
}

/// Where models live: `VEILQ_MODEL_DIR`, else the user's cache directory.
pub fn model_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VEILQ_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("veilq")
        .join("models")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_required_model_that_is_missing_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_required(&["pattern".into(), "verifier".into()], dir.path()).is_ok());
        assert!(matches!(
            check_required(&["ner".into()], dir.path()),
            Err(Error::ModelMissing { detector, .. }) if detector == "ner"
        ));
    }

    #[test]
    fn urls_are_complete() {
        for f in PINNED {
            let u = f.url();
            assert!(
                u.starts_with("https://github.com/") || u.starts_with("https://huggingface.co/"),
                "{u}"
            );
            assert_eq!(f.sha256.len(), 64);
        }
    }

    #[test]
    fn a_download_that_does_not_match_installs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake.onnx");
        std::fs::write(&fake, b"not the model").unwrap();
        let f = &PINNED[0];
        assert!(matches!(
            install(f, &fake, dir.path()),
            Err(Error::ModelMismatch { .. })
        ));
        assert!(!dir.path().join(f.file).exists());
    }

    #[test]
    fn an_archive_member_is_extracted() {
        let dir = tempfile::tempdir().unwrap();
        // A zip with the member, pinned by its own hash for the test.
        let zip_path = dir.path().join("rt.zip");
        {
            use std::io::Write;
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            w.start_file(
                "rt/lib/onnxruntime.dll",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            w.write_all(b"library").unwrap();
            w.finish().unwrap();
        }
        let sha = sha256_of(&zip_path).unwrap();
        let f = PinnedFile {
            detector: "ner",
            file: "rt/onnxruntime.dll",
            base: "file:",
            name: "rt.zip",
            sha256: Box::leak(sha.into_boxed_str()),
            member: Some("rt/lib/onnxruntime.dll"),
            target: None,
        };
        let out = install(&f, &zip_path, dir.path()).unwrap();
        assert_eq!(std::fs::read(out).unwrap(), b"library");
    }
}
