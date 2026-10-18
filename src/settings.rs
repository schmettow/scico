//! Durable application settings.
//!
//! The RAG fields mirror [`RagSettings`] but are stored as plain strings, so a
//! settings file stays readable and stable across Ragrig upgrades. The
//! co-writer prompt is compiled in; an empty `system_prompt` means "use the
//! built-in one".

use std::fs;
use std::path::{Path, PathBuf};

use ragrig_gpui::{ChatProvider, MemoryChoice, PdfBackend, RagSettings};
use serde::{Deserialize, Serialize};

/// The co-writer prompt compiled into the binary.
pub const CO_WRITER_PROMPT: &str = include_str!("../assets/prompts/sciscraper-cowriter.md");

/// Everything Sciscraper remembers between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSettings {
    /// Editor workspace. Also the default RAG workspace.
    pub workspace: String,
    /// Corpus folder indexed by the agent.
    pub corpus: String,
    /// Bibliography the library pane shows.
    pub bibliography: String,
    /// Chat provider label (`Ollama` / `DeepSeek`).
    pub provider: String,
    /// Ollama chat model.
    pub ollama_model: String,
    /// DeepSeek chat model.
    pub deepseek_model: String,
    /// Sampling temperature.
    pub temperature: f64,
    /// Maximum generated tokens.
    pub max_tokens: usize,
    /// Chat context window in tokens.
    pub context_tokens: usize,
    /// Custom system prompt; empty uses [`CO_WRITER_PROMPT`].
    pub system_prompt: String,
    /// Whether answers stream.
    pub stream: bool,
    /// Embedding model.
    pub embed_model: String,
    /// Retrieval budget.
    pub top_k: usize,
    /// Cosine pre-filter.
    pub similarity_threshold: f64,
    /// Chunk size in tokens.
    pub chunk_size: usize,
    /// Chunk overlap in tokens.
    pub chunk_overlap: usize,
    /// PDF parser label.
    pub pdf_backend: String,
    /// Keep the panic-free PDF fallback.
    pub sloppy_pdf: bool,
    /// Memory strategy label.
    pub memory: String,
    /// Memory model.
    pub memory_model: String,
    /// GROBID server URL.
    pub grobid_url: String,
    /// Rename parsed PDFs.
    pub rename_pdfs: bool,
    /// Complete records from OpenAlex.
    pub openalex: bool,
    /// Resolve `@Key` mentions into attachments.
    pub resolve_citations: bool,
    /// Attach the active document to every question.
    pub document_attached: bool,
    /// Attach the bibliography to every question.
    pub library_attached: bool,
    /// Show the library pane.
    pub show_library: bool,
    /// Show the agent pane.
    pub show_agent: bool,
    /// Start in raw markdown rather than live preview.
    pub raw_mode: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        let rag = RagSettings::default();
        let provider = if std::env::var_os("DEEPSEEK_API_KEY").is_some() {
            ChatProvider::DeepSeek
        } else {
            rag.provider
        };
        Self {
            workspace: String::new(),
            corpus: String::new(),
            bibliography: String::new(),
            provider: provider.label().to_owned(),
            ollama_model: rag.ollama_model,
            deepseek_model: rag.deepseek_model,
            temperature: rag.temperature,
            max_tokens: rag.max_tokens,
            context_tokens: rag.context_tokens,
            system_prompt: String::new(),
            stream: true,
            embed_model: rag.embed_model,
            top_k: rag.top_k,
            similarity_threshold: rag.similarity_threshold,
            chunk_size: rag.chunk_size,
            chunk_overlap: rag.chunk_overlap,
            pdf_backend: rag.pdf_backend.label().to_owned(),
            sloppy_pdf: rag.sloppy_pdf,
            memory: rag.memory.label().to_owned(),
            memory_model: rag.memory_model,
            grobid_url: rag.grobid_url,
            rename_pdfs: rag.rename_pdfs,
            openalex: rag.openalex,
            resolve_citations: rag.resolve_citations,
            document_attached: true,
            library_attached: false,
            show_library: true,
            show_agent: true,
            raw_mode: false,
        }
    }
}

impl PersistedSettings {
    /// The default settings file location.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("sciscraper-gpui")
            .join("settings.json")
    }

    /// Read a settings file, falling back to defaults for a missing or
    /// unreadable file. A parse error is reported so the caller can show it.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(settings) => (settings, None),
                Err(error) => (
                    Self::default(),
                    Some(format!("cannot parse '{}': {error}", path.display())),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Self::default(), None),
            Err(error) => (
                Self::default(),
                Some(format!("cannot read '{}': {error}", path.display())),
            ),
        }
    }

    /// Write the settings file, creating its parent directory.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create '{}': {error}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("cannot serialize settings: {error}"))?;
        fs::write(path, text).map_err(|error| format!("cannot write '{}': {error}", path.display()))
    }

    /// Fill a [`RagSettings`] from the stored values, applying the built-in
    /// prompt when no custom one is stored.
    pub fn apply_to(&self, settings: &mut RagSettings) {
        if !self.workspace.trim().is_empty() {
            settings.workspace = self.workspace.trim().to_owned();
        }
        settings.bibliography = self.bibliography.trim().to_owned();
        settings.provider = ChatProvider::options()
            .iter()
            .find(|(_, label)| *label == self.provider)
            .map_or(settings.provider, |(provider, _)| *provider);
        settings.ollama_model = self.ollama_model.clone();
        settings.deepseek_model = self.deepseek_model.clone();
        settings.temperature = self.temperature;
        settings.max_tokens = self.max_tokens;
        settings.context_tokens = self.context_tokens;
        settings.system_prompt = if self.system_prompt.trim().is_empty() {
            CO_WRITER_PROMPT.to_owned()
        } else {
            self.system_prompt.clone()
        };
        settings.stream = self.stream;
        settings.embed_model = self.embed_model.clone();
        settings.top_k = self.top_k;
        settings.similarity_threshold = self.similarity_threshold;
        settings.chunk_size = self.chunk_size;
        settings.chunk_overlap = self.chunk_overlap;
        settings.pdf_backend = PdfBackend::options()
            .iter()
            .find(|(backend, label)| {
                backend.label() == self.pdf_backend || *label == self.pdf_backend
            })
            .map_or(settings.pdf_backend, |(backend, _)| *backend);
        settings.sloppy_pdf = self.sloppy_pdf;
        settings.memory = MemoryChoice::options()
            .iter()
            .find(|(memory, label)| memory.label() == self.memory || *label == self.memory)
            .map_or(settings.memory, |(memory, _)| *memory);
        settings.memory_model = self.memory_model.clone();
        settings.grobid_url = self.grobid_url.clone();
        settings.rename_pdfs = self.rename_pdfs;
        settings.openalex = self.openalex;
        settings.resolve_citations = self.resolve_citations;
    }

    /// Snapshot the editable RAG fields back into the persisted form.
    pub fn capture(&mut self, settings: &RagSettings) {
        self.workspace = settings.workspace.clone();
        self.bibliography = settings.bibliography.clone();
        self.provider = settings.provider.label().to_owned();
        self.ollama_model = settings.ollama_model.clone();
        self.deepseek_model = settings.deepseek_model.clone();
        self.temperature = settings.temperature;
        self.max_tokens = settings.max_tokens;
        self.context_tokens = settings.context_tokens;
        self.system_prompt = if settings.system_prompt.trim() == CO_WRITER_PROMPT.trim() {
            String::new()
        } else {
            settings.system_prompt.clone()
        };
        self.stream = settings.stream;
        self.embed_model = settings.embed_model.clone();
        self.top_k = settings.top_k;
        self.similarity_threshold = settings.similarity_threshold;
        self.chunk_size = settings.chunk_size;
        self.chunk_overlap = settings.chunk_overlap;
        self.pdf_backend = settings.pdf_backend.label().to_owned();
        self.sloppy_pdf = settings.sloppy_pdf;
        self.memory = settings.memory.label().to_owned();
        self.memory_model = settings.memory_model.clone();
        self.grobid_url = settings.grobid_url.clone();
        self.rename_pdfs = settings.rename_pdfs;
        self.openalex = settings.openalex;
        self.resolve_citations = settings.resolve_citations;
    }

    /// Path of the corpus folder, when configured.
    pub fn corpus_path(&self) -> Option<PathBuf> {
        let text = self.corpus.trim();
        (!text.is_empty()).then(|| PathBuf::from(text))
    }

    /// Path of the bibliography, when configured.
    pub fn bibliography_path(&self) -> Option<PathBuf> {
        let text = self.bibliography.trim();
        (!text.is_empty()).then(|| PathBuf::from(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_attach_the_document_but_not_the_library() {
        let settings = PersistedSettings::default();
        assert!(settings.document_attached);
        assert!(!settings.library_attached);
        assert!(settings.show_agent);
        assert!(!settings.raw_mode);
    }

    #[test]
    fn applying_settings_uses_the_builtin_prompt_by_default() {
        let persisted = PersistedSettings::default();
        let mut rag = RagSettings::default();
        persisted.apply_to(&mut rag);
        assert_eq!(rag.system_prompt.trim(), CO_WRITER_PROMPT.trim());
        assert!(rag.bibliography.is_empty());
    }

    #[test]
    fn capture_and_apply_round_trip() {
        let mut persisted = PersistedSettings::default();
        let mut rag = RagSettings::default();
        persisted.apply_to(&mut rag);
        rag.workspace = "/tmp/workspace".to_owned();
        rag.bibliography = "/tmp/library.bib".to_owned();
        rag.provider = ChatProvider::DeepSeek;
        rag.top_k = 42;
        rag.memory = MemoryChoice::Transcript;
        rag.system_prompt = "Custom prompt {context}".to_owned();

        persisted.capture(&rag);
        assert_eq!(persisted.provider, "DeepSeek");
        assert_eq!(persisted.top_k, 42);
        assert_eq!(persisted.memory, "Transcript");
        assert_eq!(persisted.system_prompt, "Custom prompt {context}");

        let mut restored = RagSettings::default();
        persisted.apply_to(&mut restored);
        assert_eq!(restored.workspace, "/tmp/workspace");
        assert_eq!(restored.bibliography, "/tmp/library.bib");
        assert_eq!(restored.provider, ChatProvider::DeepSeek);
        assert_eq!(restored.top_k, 42);
        assert_eq!(restored.memory, MemoryChoice::Transcript);
        assert_eq!(restored.system_prompt, "Custom prompt {context}");
    }

    #[test]
    fn a_missing_file_is_defaults_without_an_error() {
        let path = std::env::temp_dir().join(format!(
            "sciscraper-gpui-missing-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let (settings, error) = PersistedSettings::load(&path);
        assert!(error.is_none());
        assert_eq!(settings, PersistedSettings::default());
    }

    #[test]
    fn saved_settings_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "sciscraper-gpui-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let path = dir.join("settings.json");
        let settings = PersistedSettings {
            workspace: "/tmp/work".to_owned(),
            top_k: 7,
            ..PersistedSettings::default()
        };
        settings.save(&path).expect("save");
        let (loaded, error) = PersistedSettings::load(&path);
        assert!(error.is_none(), "{error:?}");
        assert_eq!(loaded.workspace, "/tmp/work");
        assert_eq!(loaded.top_k, 7);
        let _ = fs::remove_dir_all(dir);
    }
}
