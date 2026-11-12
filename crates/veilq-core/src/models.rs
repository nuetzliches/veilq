//! Detectors and the models they need (ADR-0005, ADR-0022).
//!
//! A model is fetched only by `veilq models fetch` (or installed from a
//! directory with `veilq models install --from`), from a URL with a SHA-256
//! compiled into the binary; what does not match is discarded. A run never
//! downloads. A detector the policy requires whose files are not installed
//! ends the run with exit code 2.
//!
//! ONNX Runtime and PDFium are code that veilq loads: they are pinned like a
//! model, as a release archive of each platform and the library in it.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::Error;

/// The detectors a policy can name.
pub const DETECTORS: &[&str] = &["pattern", "ner", "ocr", "face", "verifier"];

/// What `veilq models` fetches: the detectors with a model, and `pdf`
/// (PDFium, which renders a PDF's pages for OCR; ADR-0023).
pub const COMPONENTS: &[&str] = &["ner", "ocr", "face", "pdf"];

/// The detectors that run in ONNX Runtime.
const ON_RUNTIME: &[&str] = &["ner", "ocr", "face"];

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

/// YuNet 2023mar (MIT), the face detector of the OpenCV Zoo, as `opencv`
/// publishes it on Hugging Face, at a fixed revision (ADR-0038).
const FACE: &str = "https://huggingface.co/opencv/face_detection_yunet/resolve/3cc26e7f1014a5ee5d74a42acee58bafc9d0a310";

/// PDFium (BSD-3-Clause, with the licences of its parts in the archive), as
/// `bblanchon/pdfium-binaries` (MIT) builds it, at the version whose API
/// pdfium-render binds (ADR-0023).
const PDFIUM: &str = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/8125";

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
        detector: "face",
        file: "face/yunet-2023mar/model.onnx",
        base: FACE,
        name: "face_detection_yunet_2023mar.onnx",
        sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
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
    PinnedFile {
        detector: "pdf",
        file: "pdfium-8125/pdfium.dll",
        base: PDFIUM,
        name: "pdfium-win-x64.tgz",
        sha256: "9a964261760cff52570aabf038ab346d3a5034dd8b30d0d5fe8861235d356b8b",
        member: Some("bin/pdfium.dll"),
        target: Some(("windows", "x86_64")),
    },
    PinnedFile {
        detector: "pdf",
        file: "pdfium-8125/libpdfium.so",
        base: PDFIUM,
        name: "pdfium-linux-x64.tgz",
        sha256: "e1c3ce6e270cb5e0c20816b379ad9f2c1309972697c505f508de92bf5e7d6659",
        member: Some("lib/libpdfium.so"),
        target: Some(("linux", "x86_64")),
    },
    PinnedFile {
        detector: "pdf",
        file: "pdfium-8125/libpdfium.so",
        base: PDFIUM,
        name: "pdfium-linux-arm64.tgz",
        sha256: "8b8818f2ec57d1b213d952ca6d7f0ffeca9d0110487d0e20ad83ba918bc92efa",
        member: Some("lib/libpdfium.so"),
        target: Some(("linux", "aarch64")),
    },
    PinnedFile {
        detector: "pdf",
        file: "pdfium-8125/libpdfium.dylib",
        base: PDFIUM,
        name: "pdfium-mac-arm64.tgz",
        sha256: "4939bd935ac83ddc09a3a4f038bc43a19cea1660316a3667f26cb07a070e139d",
        member: Some("lib/libpdfium.dylib"),
        target: Some(("macos", "aarch64")),
    },
];

/// What `detector` needs on this platform: its files and, for a model,
/// ONNX Runtime.
pub fn pinned_for(detector: &str) -> Vec<&'static PinnedFile> {
    PINNED
        .iter()
        .filter(|f| {
            f.detector == detector || (f.detector == "runtime" && ON_RUNTIME.contains(&detector))
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

/// The installed files of the face detector: model, runtime.
pub fn face_files(model_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    if !installed("face", model_dir) {
        return None;
    }
    let files = pinned_for("face");
    let model = files.iter().find(|f| f.detector == "face")?;
    let runtime = files.iter().find(|f| f.detector == "runtime")?;
    Some((model_dir.join(model.file), model_dir.join(runtime.file)))
}

/// The installed PDFium library.
pub fn pdf_library(model_dir: &Path) -> Option<PathBuf> {
    if !installed("pdf", model_dir) {
        return None;
    }
    pinned_for("pdf").first().map(|f| model_dir.join(f.file))
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

/// The SHA-256 of a file, as lower-case hex.
pub fn sha256_of(path: &Path) -> Result<String, Error> {
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
            if !unpack(download, f.name, member, &tmp)? {
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

/// Writes `member` of the archive `archive` (a `.zip` by its `name`, else a
/// `.tar.gz`) to `out`; `false` when the archive has no such member.
pub fn unpack(archive: &Path, name: &str, member: &str, out: &Path) -> Result<bool, Error> {
    let io = |source| Error::Io {
        path: out.to_path_buf(),
        source,
    };
    let mut target = std::fs::File::create(out).map_err(io)?;
    let file = std::fs::File::open(archive).map_err(|source| Error::Io {
        path: archive.into(),
        source,
    })?;
    if name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Format(e.to_string()))?;
        let Ok(mut m) = zip.by_name(member) else {
            return Ok(false);
        };
        std::io::copy(&mut m, &mut target).map_err(io)?;
        return Ok(true);
    }
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(io)? {
        let mut entry = entry.map_err(io)?;
        if entry.path().map_err(io)?.to_string_lossy() == member {
            std::io::copy(&mut entry, &mut target).map_err(io)?;
            return Ok(true);
        }
    }
    Ok(false)
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
