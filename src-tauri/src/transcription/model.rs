//! Whisper model registry + Model Manager (download & cache).
//!
//! whisper.cpp ships GGML models in a handful of sizes (PRD §4.5):
//! `tiny`, `base`, `small`, `medium`, `large-v3`. v1.0 defaults to **small**
//! (good speed/size trade-off; `base`/`tiny` are faster, `medium`/`large-v3`
//! are more accurate for Traditional Chinese but heavier).
//!
//! Models are **downloaded on first use** from the canonical Hugging Face
//! `ggml-org/whisper.cpp` repo into a cache directory and reused thereafter.
//! Downloading is async (`reqwest` streaming) with a progress callback so the
//! UI can show a determinate bar.
//!
//! The registry itself ([`ModelInfo`], [`registry`], [`lookup`], [`default_model`])
//! is **pure** and fully unit-tested. The network download is integration-only
//! and is noted as unverifiable in this environment.

use std::path::{Path, PathBuf};

use super::{Result, TranscriptionError};

/// Static metadata describing one whisper GGML model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelInfo {
    /// Stable id used in settings / API (e.g. `"small"`, `"large-v3"`).
    pub id: &'static str,
    /// Human-friendly label for the settings UI.
    pub label: &'static str,
    /// On-disk file name inside the cache dir (e.g. `ggml-small.bin`).
    pub file_name: &'static str,
    /// Approximate download size in mebibytes — drives the "this will download
    /// N MB" confirmation and the progress-bar fallback when the server sends
    /// no `Content-Length`.
    pub approx_size_mb: u32,
    /// Relative path under the model host (Hugging Face) repo.
    pub remote_path: &'static str,
}

impl ModelInfo {
    /// Absolute path this model would occupy inside `cache_dir`.
    pub fn path_in(&self, cache_dir: &Path) -> PathBuf {
        cache_dir.join(self.file_name)
    }

    /// Full HTTPS URL to download this model from. Fine-tuned models live in
    /// their own Hugging Face repos, so a `remote_path` that is itself a full
    /// URL (has a scheme) is used as-is; otherwise it is joined onto the default
    /// whisper.cpp host.
    pub fn download_url(&self) -> String {
        if self.remote_path.starts_with("http") {
            self.remote_path.to_string()
        } else {
            format!("{MODEL_HOST}/{}", self.remote_path)
        }
    }
}

/// Base URL of the canonical whisper.cpp GGML model host. `resolve/main` is the
/// raw-blob endpoint on Hugging Face.
pub const MODEL_HOST: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// The id used when the user has not chosen a model. PRD §4.5: default small.
pub const DEFAULT_MODEL_ID: &str = "small";

/// The full v1.0 model registry, in ascending size/accuracy order.
const REGISTRY: &[ModelInfo] = &[
    ModelInfo {
        id: "tiny",
        label: "Tiny (fastest, lowest accuracy)",
        file_name: "ggml-tiny.bin",
        approx_size_mb: 75,
        remote_path: "ggml-tiny.bin",
    },
    ModelInfo {
        id: "base",
        label: "Base (fast)",
        file_name: "ggml-base.bin",
        approx_size_mb: 142,
        remote_path: "ggml-base.bin",
    },
    ModelInfo {
        id: "small",
        label: "Small (default — balanced)",
        file_name: "ggml-small.bin",
        approx_size_mb: 466,
        remote_path: "ggml-small.bin",
    },
    ModelInfo {
        id: "medium",
        label: "Medium (slower, more accurate)",
        file_name: "ggml-medium.bin",
        approx_size_mb: 1500,
        remote_path: "ggml-medium.bin",
    },
    // Chinese fine-tune of large-v3-turbo (BELLE). Pre-converted GGML, big
    // Mandarin accuracy gain over vanilla turbo/small. Default for this app.
    // NOTE: outputs Simplified Chinese — OpenCC s2twp post-processing (a later
    // step) is needed for Traditional output.
    ModelInfo {
        id: "belle-turbo-zh",
        label: "中文 Turbo (Belle, 推薦)",
        file_name: "ggml-belle-large-v3-turbo-zh.bin",
        approx_size_mb: 1623,
        remote_path:
            "https://huggingface.co/BELLE-2/Belle-whisper-large-v3-turbo-zh-ggml/resolve/main/ggml-model.bin",
    },
    ModelInfo {
        id: "large-v3",
        label: "Large v3 (best accuracy, slowest)",
        file_name: "ggml-large-v3.bin",
        approx_size_mb: 2950,
        remote_path: "ggml-large-v3.bin",
    },
];

/// All known models, ascending by size.
pub fn registry() -> &'static [ModelInfo] {
    REGISTRY
}

/// Look up a model by id. Accepts a couple of common aliases (`large` →
/// `large-v3`) so older settings values keep working.
pub fn lookup(id: &str) -> Result<&'static ModelInfo> {
    let canonical = match id {
        // Normalize aliases people are likely to have stored.
        "large" | "large-v3" => "large-v3",
        other => other,
    };
    REGISTRY
        .iter()
        .find(|m| m.id == canonical)
        .ok_or_else(|| TranscriptionError::UnknownModel(id.to_string()))
}

/// The default model (`small`). Infallible — the id is a compile-time constant
/// guaranteed to be in the registry (asserted by a unit test).
pub fn default_model() -> &'static ModelInfo {
    lookup(DEFAULT_MODEL_ID).expect("default model id must exist in the registry")
}

/// Progress of an in-flight model download, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    /// `None` when the server did not advertise a `Content-Length`.
    pub total_bytes: Option<u64>,
}

impl DownloadProgress {
    /// Completion in `[0.0, 1.0]`, or `None` if the total size is unknown.
    pub fn fraction(&self) -> Option<f32> {
        match self.total_bytes {
            Some(total) if total > 0 => {
                Some((self.downloaded_bytes as f32 / total as f32).clamp(0.0, 1.0))
            }
            _ => None,
        }
    }
}

/// Manages where models live on disk and fetches them on first use.
#[derive(Debug, Clone)]
pub struct ModelManager {
    cache_dir: PathBuf,
}

impl ModelManager {
    /// Create a manager rooted at `cache_dir` (e.g.
    /// `~/.cache/MeetingRecordApp/models`). The directory is created lazily on
    /// first download.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        ModelManager {
            cache_dir: cache_dir.into(),
        }
    }

    /// The cache directory this manager writes to.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Where the given model would live on disk.
    pub fn model_path(&self, model: &ModelInfo) -> PathBuf {
        model.path_in(&self.cache_dir)
    }

    /// Whether the model file already exists in the cache.
    pub fn is_cached(&self, model: &ModelInfo) -> bool {
        self.model_path(model).is_file()
    }

    /// Ensure the model is present locally, downloading it on first use, and
    /// return its on-disk path.
    ///
    /// `on_progress` is invoked periodically while bytes stream in so callers
    /// can drive a progress bar. If the file is already cached, the closure is
    /// not called and the cached path is returned immediately.
    ///
    /// The download is atomic: bytes are written to a `*.partial` sibling and
    /// renamed into place only on success, so an interrupted download never
    /// leaves a corrupt model that `is_cached` would treat as valid.
    #[cfg(feature = "whisper")]
    pub async fn ensure_model(
        &self,
        model: &ModelInfo,
        mut on_progress: impl FnMut(DownloadProgress),
    ) -> Result<PathBuf> {
        let dest = self.model_path(model);
        if dest.is_file() {
            return Ok(dest);
        }

        std::fs::create_dir_all(&self.cache_dir)?;

        let url = model.download_url();
        let response = reqwest::get(&url)
            .await
            .map_err(|e| TranscriptionError::Download(format!("GET {url}: {e}")))?
            .error_for_status()
            .map_err(|e| TranscriptionError::Download(format!("GET {url}: {e}")))?;

        let total_bytes = response.content_length();
        let tmp = dest.with_extension("partial");
        let mut file = std::fs::File::create(&tmp)?;
        let mut downloaded: u64 = 0;

        // `Response::chunk()` streams the body chunk-by-chunk without pulling in
        // `futures_util::StreamExt`, so we get determinate progress with no
        // extra dependency.
        use std::io::Write;
        let mut response = response;
        loop {
            let chunk = response
                .chunk()
                .await
                .map_err(|e| TranscriptionError::Download(format!("stream {url}: {e}")))?;
            let Some(chunk) = chunk else { break };
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            on_progress(DownloadProgress {
                downloaded_bytes: downloaded,
                total_bytes,
            });
        }
        file.flush()?;
        drop(file);

        // Atomic publish.
        std::fs::rename(&tmp, &dest)?;
        Ok(dest)
    }

    /// Return the cached path, erroring if the model is not present. Used by the
    /// transcriber to fail fast with a clear message rather than block on a
    /// download it cannot perform (e.g. offline).
    pub fn require_cached(&self, model: &ModelInfo) -> Result<PathBuf> {
        let path = self.model_path(model);
        if path.is_file() {
            Ok(path)
        } else {
            Err(TranscriptionError::ModelMissing {
                id: model.id.to_string(),
                path,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn default_is_small_and_in_registry() {
        assert_eq!(DEFAULT_MODEL_ID, "small");
        assert_eq!(default_model().id, "small");
        // The default id constant must always resolve.
        assert!(lookup(DEFAULT_MODEL_ID).is_ok());
    }

    #[test]
    fn registry_lists_expected_models() {
        let ids: Vec<&str> = registry().iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            vec!["tiny", "base", "small", "medium", "belle-turbo-zh", "large-v3"]
        );
    }

    #[test]
    fn belle_model_uses_its_own_full_url() {
        let belle = lookup("belle-turbo-zh").unwrap();
        assert!(belle.download_url().starts_with("https://huggingface.co/BELLE-2/"));
        // Standard models still join onto the default host.
        assert_eq!(
            lookup("small").unwrap().download_url(),
            format!("{MODEL_HOST}/ggml-small.bin")
        );
    }

    #[test]
    fn lookup_each_id_roundtrips() {
        for m in registry() {
            assert_eq!(lookup(m.id).unwrap().id, m.id);
        }
    }

    #[test]
    fn lookup_large_alias_maps_to_v3() {
        assert_eq!(lookup("large").unwrap().id, "large-v3");
        assert_eq!(lookup("large-v3").unwrap().id, "large-v3");
    }

    #[test]
    fn lookup_unknown_errors_with_original_id() {
        let err = lookup("ginormous").unwrap_err();
        match err {
            TranscriptionError::UnknownModel(id) => assert_eq!(id, "ginormous"),
            other => panic!("expected UnknownModel, got {other:?}"),
        }
    }

    #[test]
    fn registry_is_ascending_by_size() {
        let sizes: Vec<u32> = registry().iter().map(|m| m.approx_size_mb).collect();
        let mut sorted = sizes.clone();
        sorted.sort_unstable();
        assert_eq!(sizes, sorted, "registry should be ordered small→large");
    }

    #[test]
    fn download_url_is_well_formed() {
        let small = lookup("small").unwrap();
        assert_eq!(
            small.download_url(),
            format!("{MODEL_HOST}/ggml-small.bin")
        );
        assert!(small.download_url().starts_with("https://"));
    }

    #[test]
    fn path_in_uses_file_name() {
        let m = lookup("base").unwrap();
        let p = m.path_in(Path::new("/tmp/models"));
        assert_eq!(p, Path::new("/tmp/models/ggml-base.bin"));
    }

    #[test]
    fn manager_paths_and_cache_check() {
        let mgr = ModelManager::new("/tmp/mra-models");
        let m = lookup("tiny").unwrap();
        assert_eq!(mgr.cache_dir(), Path::new("/tmp/mra-models"));
        assert_eq!(
            mgr.model_path(m),
            Path::new("/tmp/mra-models/ggml-tiny.bin")
        );
        // Almost certainly absent in the test env.
        assert!(!mgr.is_cached(m));
    }

    #[test]
    fn require_cached_reports_missing_path() {
        let mgr = ModelManager::new("/definitely/not/here");
        let m = lookup("small").unwrap();
        match mgr.require_cached(m).unwrap_err() {
            TranscriptionError::ModelMissing { id, path } => {
                assert_eq!(id, "small");
                assert_eq!(path, Path::new("/definitely/not/here/ggml-small.bin"));
            }
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    #[test]
    fn download_progress_fraction() {
        let p = DownloadProgress {
            downloaded_bytes: 50,
            total_bytes: Some(100),
        };
        assert_eq!(p.fraction(), Some(0.5));

        let unknown = DownloadProgress {
            downloaded_bytes: 50,
            total_bytes: None,
        };
        assert_eq!(unknown.fraction(), None);

        // Over-count clamps to 1.0 rather than exceeding it.
        let over = DownloadProgress {
            downloaded_bytes: 150,
            total_bytes: Some(100),
        };
        assert_eq!(over.fraction(), Some(1.0));

        let zero_total = DownloadProgress {
            downloaded_bytes: 0,
            total_bytes: Some(0),
        };
        assert_eq!(zero_total.fraction(), None);
    }
}
