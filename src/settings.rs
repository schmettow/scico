//! Durable application settings.
//!
//! The RAG fields mirror [`RagSettings`] but are stored as plain strings, so a
//! settings file stays readable and stable across Ragrig upgrades. The Chat
//! prompt is compiled in as `Chat.md`; an empty `system_prompt_path` means
//! "use the built-in one".

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ragrig_gpui::{ChatProvider, GrobidSource, MemoryChoice, PdfBackend, RagSettings};
use serde::{Deserialize, Serialize};

/// The Chat prompt compiled into the binary.
pub const CHAT_PROMPT: &str = include_str!("../assets/prompts/Chat.md");

/// The file name used when the prompt is exported into a workspace.
pub const CHAT_PROMPT_FILE: &str = "Chat.md";

/// A file kind with an assignable external viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerKind {
    /// `.pdf` files.
    Pdf,
    /// `.epub` files.
    Epub,
    /// `.html` and `.htm` files.
    Html,
}

impl ViewerKind {
    /// Every assignable viewer, in settings order.
    pub const ALL: [Self; 3] = [Self::Pdf, Self::Epub, Self::Html];

    /// The label shown in settings.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF viewer",
            Self::Epub => "EPUB viewer",
            Self::Html => "HTML viewer",
        }
    }

    /// Extensions this viewer handles, without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Pdf => &["pdf"],
            Self::Epub => &["epub"],
            Self::Html => &["html", "htm"],
        }
    }
}

/// External programs used instead of the platform default when opening files.
///
/// An empty program means "use the system default". These are executable paths
/// or command names, not shell snippets; the document path is passed as the
/// only argument.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalViewers {
    /// Program for `.pdf` files.
    pub pdf: String,
    /// Program for `.epub` files.
    pub epub: String,
    /// Program for `.html`/`.htm` files.
    pub html: String,
}

impl ExternalViewers {
    /// The configured program for one kind.
    pub fn program(&self, kind: ViewerKind) -> &str {
        let value = match kind {
            ViewerKind::Pdf => &self.pdf,
            ViewerKind::Epub => &self.epub,
            ViewerKind::Html => &self.html,
        };
        value.trim()
    }

    /// Assign (or clear) the program for one kind.
    pub fn set(&mut self, kind: ViewerKind, value: String) {
        let value = value.trim().to_owned();
        match kind {
            ViewerKind::Pdf => self.pdf = value,
            ViewerKind::Epub => self.epub = value,
            ViewerKind::Html => self.html = value,
        }
    }

    /// The program handling `path`, if one is configured.
    pub fn program_for(&self, path: &Path) -> Option<&str> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        let kind = ViewerKind::ALL
            .into_iter()
            .find(|kind| kind.extensions().contains(&extension.as_str()))?;
        let program = self.program(kind);
        (!program.is_empty()).then_some(program)
    }
}

/// Everything Scico remembers between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSettings {
    /// Editor workspace. Also the default RAG workspace.
    pub workspace: String,
    /// Corpus folder indexed by the agent.
    pub corpus: String,
    /// Bibliography the library pane shows.
    pub bibliography: String,
    /// Chat provider label (`Ollama` / `DeepSeek` / `OpenAI` / `Claude` /
    /// `Mistral` / `OpenRouter`).
    pub provider: String,
    /// Ollama chat model.
    pub ollama_model: String,
    /// DeepSeek chat model.
    pub deepseek_model: String,
    /// Models for the cloud providers without a legacy field, keyed by
    /// provider label.
    pub provider_models: BTreeMap<String, String>,
    /// Sampling temperature.
    pub temperature: f64,
    /// Maximum generated tokens.
    pub max_tokens: usize,
    /// Chat context window in tokens.
    pub context_tokens: usize,
    /// Markdown prompt file overriding [`CHAT_PROMPT`]; empty uses the
    /// built-in prompt.
    pub system_prompt_path: String,
    /// Whether answers stream.
    pub stream: bool,
    /// Embedding model.
    pub embed_model: String,
    /// Hybrid retrieval (BM25 + embeddings). Off needs no embedding model.
    pub hybrid: bool,
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
    /// Strategy restored when Chat memory is switched back on.
    pub memory_when_on: String,
    /// Memory model.
    pub memory_model: String,
    /// Where the GROBID server comes from.
    pub grobid_source: String,
    /// GROBID server URL.
    pub grobid_url: String,
    /// Docker image for the managed GROBID container.
    pub grobid_image: String,
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
    /// Show the rendered preview beside the source editor.
    pub show_preview: bool,
    /// Keep the source editor and the preview scrolled together.
    pub sync_scroll: bool,
    /// Width of the library pane in pixels.
    pub library_width: f32,
    /// Width of the Chat pane in pixels.
    pub agent_width: f32,
    /// Empty gutter on the right of the settings surface, in pixels.
    pub settings_gutter: f32,
    /// Start in raw markdown rather than live preview.
    pub raw_mode: bool,
    /// Editor font family; empty follows the theme's monospace family.
    pub editor_font: String,
    /// Editor font size in pixels; zero follows 150% of the theme mono size.
    pub editor_font_size: f32,
    /// External programs for PDF, EPUB, and HTML files.
    pub viewers: ExternalViewers,
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
            ollama_model: rag.model_for(ChatProvider::Ollama).to_owned(),
            deepseek_model: rag.model_for(ChatProvider::DeepSeek).to_owned(),
            provider_models: BTreeMap::new(),
            temperature: rag.temperature,
            max_tokens: rag.max_tokens,
            context_tokens: rag.context_tokens,
            system_prompt_path: String::new(),
            stream: true,
            embed_model: rag.embed_model,
            hybrid: rag.hybrid,
            top_k: rag.top_k,
            similarity_threshold: rag.similarity_threshold,
            chunk_size: rag.chunk_size,
            chunk_overlap: rag.chunk_overlap,
            pdf_backend: rag.pdf_backend.label().to_owned(),
            sloppy_pdf: rag.sloppy_pdf,
            memory: rag.memory.label().to_owned(),
            memory_when_on: rag.memory.label().to_owned(),
            memory_model: rag.memory_model,
            grobid_source: rag.grobid_source.label().to_owned(),
            grobid_url: rag.grobid_url,
            grobid_image: rag.grobid_image,
            rename_pdfs: rag.rename_pdfs,
            openalex: rag.openalex,
            resolve_citations: rag.resolve_citations,
            document_attached: true,
            library_attached: false,
            show_library: true,
            show_agent: true,
            show_preview: true,
            sync_scroll: true,
            library_width: 340.0,
            agent_width: 430.0,
            settings_gutter: 360.0,
            raw_mode: false,
            editor_font: String::new(),
            editor_font_size: 0.0,
            viewers: ExternalViewers::default(),
        }
    }
}

impl PersistedSettings {
    /// The default settings file location.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("scico")
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
        settings.provider = ChatProvider::from_label(&self.provider).unwrap_or(settings.provider);
        if !self.ollama_model.trim().is_empty() {
            settings.set_model_for(ChatProvider::Ollama, self.ollama_model.clone());
        }
        if !self.deepseek_model.trim().is_empty() {
            settings.set_model_for(ChatProvider::DeepSeek, self.deepseek_model.clone());
        }
        for (label, model) in &self.provider_models {
            if model.trim().is_empty() {
                continue;
            }
            if let Some(provider) = ChatProvider::from_label(label) {
                settings.set_model_for(provider, model.clone());
            }
        }
        settings.temperature = self.temperature;
        settings.max_tokens = self.max_tokens;
        settings.context_tokens = self.context_tokens;
        settings.built_in_prompt = CHAT_PROMPT.to_owned();
        settings.system_prompt_path = self.system_prompt_path.trim().to_owned();
        settings.system_prompt = match settings.prompt_path() {
            Some(path) => fs::read_to_string(&path)
                .ok()
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| CHAT_PROMPT.to_owned()),
            None => CHAT_PROMPT.to_owned(),
        };
        settings.stream = self.stream;
        settings.embed_model = self.embed_model.clone();
        settings.hybrid = self.hybrid;
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
        settings.grobid_source =
            GrobidSource::from_label(&self.grobid_source).unwrap_or(settings.grobid_source);
        settings.grobid_url = self.grobid_url.clone();
        settings.grobid_image = self.grobid_image.clone();
        settings.rename_pdfs = self.rename_pdfs;
        settings.openalex = self.openalex;
        settings.resolve_citations = self.resolve_citations;
    }

    /// Snapshot the editable RAG fields back into the persisted form.
    pub fn capture(&mut self, settings: &RagSettings) {
        self.workspace = settings.workspace.clone();
        self.bibliography = settings.bibliography.clone();
        self.provider = settings.provider.label().to_owned();
        self.ollama_model = settings.model_for(ChatProvider::Ollama).to_owned();
        self.deepseek_model = settings.model_for(ChatProvider::DeepSeek).to_owned();
        self.provider_models = ChatProvider::options()
            .iter()
            .filter(|(provider, _)| {
                provider.env_var().is_some()
                    && *provider != ChatProvider::DeepSeek
                    && !settings.model_for(*provider).trim().is_empty()
                    && settings.model_for(*provider) != provider.default_model()
            })
            .map(|(provider, _)| {
                (
                    provider.label().to_owned(),
                    settings.model_for(*provider).to_owned(),
                )
            })
            .collect();
        self.temperature = settings.temperature;
        self.max_tokens = settings.max_tokens;
        self.context_tokens = settings.context_tokens;
        self.system_prompt_path = settings.system_prompt_path.clone();
        self.stream = settings.stream;
        self.embed_model = settings.embed_model.clone();
        self.hybrid = settings.hybrid;
        self.top_k = settings.top_k;
        self.similarity_threshold = settings.similarity_threshold;
        self.chunk_size = settings.chunk_size;
        self.chunk_overlap = settings.chunk_overlap;
        self.pdf_backend = settings.pdf_backend.label().to_owned();
        self.sloppy_pdf = settings.sloppy_pdf;
        self.memory = settings.memory.label().to_owned();
        self.memory_model = settings.memory_model.clone();
        self.grobid_source = settings.grobid_source.label().to_owned();
        self.grobid_url = settings.grobid_url.clone();
        self.grobid_image = settings.grobid_image.clone();
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
    fn external_viewers_match_extensions_and_fall_back_to_the_system() {
        let mut viewers = ExternalViewers::default();
        assert!(viewers.program_for(Path::new("paper.pdf")).is_none());
        viewers.set(ViewerKind::Pdf, "  /usr/bin/zathura ".to_owned());
        viewers.set(ViewerKind::Html, "firefox".to_owned());
        assert_eq!(
            viewers.program_for(Path::new("paper.PDF")),
            Some("/usr/bin/zathura")
        );
        assert_eq!(viewers.program_for(Path::new("page.htm")), Some("firefox"));
        assert!(viewers.program_for(Path::new("notes.md")).is_none());
    }

    #[test]
    fn retrieval_and_memory_switches_round_trip() {
        let mut persisted = PersistedSettings::default();
        assert!(persisted.hybrid);

        let mut settings = RagSettings::default();
        persisted.apply_to(&mut settings);
        assert!(settings.hybrid);

        settings.hybrid = false;
        settings.memory = MemoryChoice::Off;
        persisted.capture(&settings);
        assert!(!persisted.hybrid);
        assert_eq!(persisted.memory, "Off");

        let mut restored = RagSettings::default();
        persisted.apply_to(&mut restored);
        assert!(!restored.hybrid);
        assert_eq!(restored.memory, MemoryChoice::Off);
    }

    #[test]
    fn defaults_attach_the_document_but_not_the_library() {
        let settings = PersistedSettings::default();
        assert!(settings.document_attached);
        assert!(!settings.library_attached);
        assert!(settings.show_agent);
        assert!(!settings.raw_mode);
        assert_eq!(settings.library_width, 340.0);
        assert_eq!(settings.agent_width, 430.0);
    }

    #[test]
    fn applying_settings_uses_the_builtin_prompt_by_default() {
        let persisted = PersistedSettings::default();
        let mut rag = RagSettings::default();
        persisted.apply_to(&mut rag);
        assert_eq!(rag.system_prompt.trim(), CHAT_PROMPT.trim());
        assert_eq!(rag.built_in_prompt.trim(), CHAT_PROMPT.trim());
        assert!(rag.system_prompt_path.is_empty());
        assert!(rag.bibliography.is_empty());
    }

    #[test]
    fn a_custom_prompt_file_replaces_the_builtin_prompt() {
        let dir = std::env::temp_dir().join(format!(
            "scico-prompt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("prompt dir");
        let path = dir.join("Chat.md");
        std::fs::write(&path, "Custom Chat prompt").expect("prompt file");

        let persisted = PersistedSettings {
            system_prompt_path: path.display().to_string(),
            ..PersistedSettings::default()
        };
        let mut rag = RagSettings::default();
        persisted.apply_to(&mut rag);

        assert_eq!(rag.system_prompt, "Custom Chat prompt");
        assert_eq!(rag.built_in_prompt.trim(), CHAT_PROMPT.trim());
        assert_eq!(
            rag.to_setup().expect("setup").system_prompt.as_deref(),
            Some("Custom Chat prompt")
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn capture_and_apply_round_trip() {
        let mut persisted = PersistedSettings::default();
        let mut rag = RagSettings::default();
        persisted.apply_to(&mut rag);
        rag.workspace = "/tmp/workspace".to_owned();
        rag.bibliography = "/tmp/library.bib".to_owned();
        rag.provider = ChatProvider::DeepSeek;
        rag.set_model_for(ChatProvider::OpenAi, "gpt-4o-mini");
        rag.top_k = 42;
        rag.memory = MemoryChoice::Transcript;
        rag.system_prompt_path = "/tmp/Chat.md".to_owned();
        rag.grobid_source = GrobidSource::Managed;
        rag.grobid_url = "http://127.0.0.1:8070".to_owned();
        rag.grobid_image = "grobid/grobid:0.9.1-crf".to_owned();

        persisted.capture(&rag);
        assert_eq!(persisted.provider, "DeepSeek");
        assert_eq!(persisted.top_k, 42);
        assert_eq!(persisted.memory, "Transcript");
        assert_eq!(persisted.system_prompt_path, "/tmp/Chat.md");
        assert_eq!(persisted.grobid_source, "Docker container");

        let mut restored = RagSettings::default();
        persisted.apply_to(&mut restored);
        assert_eq!(restored.workspace, "/tmp/workspace");
        assert_eq!(restored.bibliography, "/tmp/library.bib");
        assert_eq!(restored.provider, ChatProvider::DeepSeek);
        assert_eq!(restored.model_for(ChatProvider::OpenAi), "gpt-4o-mini");
        assert_eq!(restored.top_k, 42);
        assert_eq!(restored.memory, MemoryChoice::Transcript);
        assert_eq!(restored.system_prompt_path, "/tmp/Chat.md");
        assert_eq!(restored.system_prompt.trim(), CHAT_PROMPT.trim());
        assert_eq!(restored.grobid_source, GrobidSource::Managed);
        assert_eq!(restored.grobid_url, "http://127.0.0.1:8070");
        assert_eq!(restored.grobid_image, "grobid/grobid:0.9.1-crf");
    }

    #[test]
    fn a_missing_file_is_defaults_without_an_error() {
        let path = std::env::temp_dir().join(format!(
            "scico-missing-{}-{}.json",
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
            "scico-settings-{}-{}",
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
            viewers: ExternalViewers {
                pdf: "zathura".to_owned(),
                ..ExternalViewers::default()
            },
            ..PersistedSettings::default()
        };
        settings.save(&path).expect("save");
        let (loaded, error) = PersistedSettings::load(&path);
        assert!(error.is_none(), "{error:?}");
        assert_eq!(loaded.workspace, "/tmp/work");
        assert_eq!(loaded.top_k, 7);
        assert_eq!(loaded.viewers.pdf, "zathura");
        let _ = fs::remove_dir_all(dir);
    }
}
