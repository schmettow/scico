//! Application state: documents, workspace paths, and the RAG controller.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(editor_gpuikit)]
use gpui_kit::base::input::RopeExt as _;
#[cfg(editor_gpuikit)]
use gpui_kit::base::text::TextViewState;
use gpui_kit::component::ActiveTheme as _;
#[cfg(editor_gpuikit)]
use gpui_kit::{Bounds, Pixels};
use gpui_kit::{App, AppContext as _, Context, Entity, Focusable as _, SharedString, Window};
#[cfg(any(editor_zorite, editor_gpuikit))]
use gpui_kit::{point, px};
pub use ragrig_gpui::editor::ReplacementOutcome;
use ragrig_gpui::editor::{
    EditorSnapshot, EditorSurface, MARKER_REPLACE, Replacement, ReplacementTarget, RoundtripHost,
    SelectionSnapshot, bibtex_entry_offset, replace_at, verified_bytes,
};
use ragrig_gpui::{
    ChatProvider, GrobidService, MemoryChoice, RagChanges, RagController, RagSettings,
};
#[cfg(editor_zorite)]
use zorite_editor::EditorEvent;

use crate::document::{Document, EditorState, SyntaxStyle, read_document, title_for};
use crate::session::{SessionDocument, SessionState};
use crate::settings::{CHAT_PROMPT, CHAT_PROMPT_FILE, PersistedSettings};
use crate::theme::syntax_style;
use crate::workspace::{self, WorkspaceReport};

/// Shared editor and workspace state.
pub struct AppState {
    /// Persisted, non-RAG settings.
    pub settings: PersistedSettings,
    /// Where `settings` is written.
    pub settings_path: PathBuf,
    /// Where the crash-safe session journal is written.
    pub session_path: PathBuf,
    /// The last snapshot written to `session_path`.
    last_session: Option<SessionState>,
    /// The editable Ragrig settings.
    pub rag: Entity<RagSettings>,
    /// The configuration last handed to the worker.
    applied_rag: RagSettings,
    /// The RAG worker controller.
    pub controller: Entity<RagController>,
    /// The managed GROBID service and Docker lifecycle.
    pub grobid: Entity<GrobidService>,
    /// Open documents, in tab order.
    pub documents: Vec<Document>,
    /// Index into [`Self::documents`].
    pub active: usize,
    /// Next document identity.
    next_document: u64,
    /// The editor workspace.
    pub workspace: Option<PathBuf>,
    /// Whether [`Self::workspace`] was chosen explicitly (env, settings, or a
    /// workspace command) rather than falling back to the current directory.
    pub workspace_explicit: bool,
    /// The corpus folder indexed by the agent.
    pub corpus: Option<PathBuf>,
    /// The project-specific Ragrig data folder (store, sessions, profiles).
    pub ragrig_dir: PathBuf,
    /// The bibliography shown by the library.
    pub bibliography: Option<PathBuf>,
    /// Bumped when the bibliography document is saved or closed, so the
    /// Library section reloads its rows.
    pub references_revision: u64,
    /// Attach the active document to each question.
    pub document_attached: bool,
    /// Attach the bibliography to each question.
    pub library_attached: bool,
    /// Show the workspace pane.
    pub show_library: bool,
    /// Show the agent pane.
    pub show_agent: bool,
    /// Show the rendered preview beside the source editor.
    pub show_preview: bool,
    /// Keep the source editor and the preview scrolled together.
    pub sync_scroll: bool,
    /// Width of the workspace pane in pixels.
    pub library_width: f32,
    /// Width of the Chat pane in pixels.
    pub agent_width: f32,
    /// Empty gutter on the right of the settings surface.
    pub settings_gutter: f32,
    /// Editor font family; empty follows the theme monospace.
    pub editor_font: String,
    /// Editor font size in pixels.
    pub editor_font_size: f32,
    /// Memory strategy restored when the status bar switches Chat memory on.
    pub last_memory: MemoryChoice,
    /// Cancels older deferred scroll and preview refreshes.
    #[cfg(editor_gpuikit)]
    preview_sync_generation: u64,
    /// Monotonic token that cancels an older caret-follow animation.
    #[cfg(editor_zorite)]
    scroll_generation: u64,
    /// Monotonic token that keeps only the newest debounced session write.
    session_generation: u64,
    /// Render raw Markdown instead of the live preview.
    pub raw_mode: bool,
    /// Whether the New-profile name field is open in Settings.
    pub creating_profile: bool,
    /// Validation message for the New-profile field.
    pub profile_draft_error: Option<SharedString>,
    /// The status bar text.
    pub status: SharedString,
    style: SyntaxStyle,
}

impl AppState {
    /// Build the state, the Ragrig settings entity, and the RAG controller.
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        persisted: PersistedSettings,
        settings_path: PathBuf,
        session_path: PathBuf,
        initial_workspace: Option<PathBuf>,
    ) -> Self {
        let mut rag_values = RagSettings::default();
        persisted.apply_to(&mut rag_values);

        // A workspace is only scaffolded when it was actually configured; an
        // implicit current-directory fallback must not sprout files. An
        // explicit workspace argument outranks the environment and the saved
        // settings, because the user just named it on the command line.
        let cli_workspace = initial_workspace.filter(|path| path.is_dir());
        let requested_by_cli = cli_workspace.is_some();
        let configured_workspace = cli_workspace
            .or_else(|| std::env::var_os("SCICO_WORKSPACE").map(PathBuf::from))
            .or_else(|| non_empty_path(&persisted.workspace))
            .or_else(|| std::env::var_os("RAGRIG_WORKSPACE").map(PathBuf::from));
        let workspace_explicit = configured_workspace.is_some();
        let mut workspace_error = None;
        let workspace_report = configured_workspace
            .as_deref()
            .filter(|path| path.is_dir())
            .and_then(|path| match workspace::prepare(path, false) {
                Ok(report) => Some(report),
                Err(error) => {
                    workspace_error = Some(error);
                    None
                }
            });
        let mut workspace_status = if requested_by_cli {
            workspace_report.as_ref().map(|report| report.status(false))
        } else {
            None
        };
        let workspace = workspace_report
            .as_ref()
            .map(|report| report.root.clone())
            .or(configured_workspace)
            .or_else(|| non_empty_path(&rag_values.workspace))
            .or_else(|| std::env::current_dir().ok());

        let corpus = std::env::var_os("SCICO_CORPUS")
            .map(PathBuf::from)
            .or_else(|| {
                workspace_report
                    .as_ref()
                    .map(|report| report.root.join(workspace::CORPUS_DIR))
            })
            .or_else(|| non_empty_path(&persisted.corpus))
            .or_else(|| {
                workspace
                    .as_ref()
                    .map(|workspace| workspace.join(workspace::CORPUS_DIR))
            })
            .or_else(|| workspace.clone());
        let bibliography = std::env::var_os("SCICO_BIB")
            .map(PathBuf::from)
            .or_else(|| {
                workspace_report
                    .as_ref()
                    .map(|report| report.bibliography.clone())
            })
            .or_else(|| non_empty_path(&persisted.bibliography))
            .or_else(|| non_empty_path(&rag_values.bibliography))
            .or_else(|| {
                workspace
                    .as_ref()
                    .map(|workspace| workspace.join(".ragrig/references.bib"))
            });
        let ragrig_dir = workspace_report
            .as_ref()
            .map(|report| report.ragrig_dir.clone())
            .or_else(|| {
                workspace
                    .as_ref()
                    .map(|workspace| workspace.join(workspace::RAGRIG_DIR))
            })
            .unwrap_or_else(|| PathBuf::from(workspace::RAGRIG_DIR));

        // Rules: built-in < ~/.ragrig/config.json < <workspace>/.ragrig/config.json.
        // A workspace layer replaces the RAG fields wholesale; host-only fields
        // are restored below.
        if let Some(report) = &workspace_report
            && let Some(mut layered) = RagSettings::from_layers_over(&report.root, &rag_values)
        {
            layered.built_in_prompt = CHAT_PROMPT.to_owned();
            rag_values = layered;
        }
        if let Some(workspace) = &workspace {
            rag_values.workspace = workspace.display().to_string();
        }
        if let Some(report) = &workspace_report {
            apply_workspace_prompt(&mut rag_values, report);
        }
        // Profile bookkeeping is host state; a layered config resets it.
        rag_values.profile_name.clear();
        rag_values.refresh_profiles();
        let saved_profile = persisted.profile_name.trim().to_owned();
        if rag_values
            .profiles
            .iter()
            .any(|name| name == &saved_profile)
        {
            rag_values.profile_name = saved_profile;
        }
        if let Some(bibliography) = &bibliography {
            rag_values.bibliography = bibliography.display().to_string();
        }
        if let Some(corpus) = &corpus {
            rag_values
                .corpus_dirs
                .retain(|entry| !entry.starts_with(&format!("{}=", workspace::CORPUS_DIR)));
            rag_values
                .corpus_dirs
                .push(format!("{}={}", workspace::CORPUS_DIR, corpus.display()));
        }

        // Chat memory may be switched off; keep the strategy that was active
        // before, so the status-bar switch can restore it.
        let last_memory = if rag_values.memory != MemoryChoice::Off {
            rag_values.memory
        } else {
            match memory_choice(&persisted.memory_when_on) {
                Some(choice) if choice != MemoryChoice::Off => choice,
                _ => MemoryChoice::Rewrite,
            }
        };

        let mut status = SharedString::from("Ready");
        if let Some(error) = &workspace_error {
            status = format!("Workspace: {error}").into();
        }
        let setup = match rag_values.to_setup() {
            Ok(setup) => setup,
            Err(error) => {
                status = format!("RAG settings: {error} — using local defaults").into();
                rag_values.provider = ChatProvider::Ollama;
                rag_values
                    .to_setup()
                    .unwrap_or_else(|_| RagSettings::default().to_setup().expect("valid defaults"))
            }
        };
        let rag = cx.new(|_| rag_values);
        let applied_rag = rag.read(cx).clone();
        let grobid = cx.new(|cx| {
            GrobidService::new(rag.clone(), cx).expect("the GROBID service thread must start")
        });
        let controller = cx.new(|cx| {
            RagController::new(with_bibliography(setup, bibliography.clone()), cx)
                .expect("the RAG worker thread must start")
        });

        let style = syntax_style(cx);
        let startup_status = status.clone();
        let (restored_session, session_error) = SessionState::load(&session_path);
        let document_attached = persisted.document_attached;
        let library_attached = persisted.library_attached;
        let show_library = restored_session
            .as_ref()
            .map_or(persisted.show_library, |session| session.show_library);
        let show_agent = restored_session
            .as_ref()
            .map_or(persisted.show_agent, |session| session.show_agent);
        let show_preview = restored_session
            .as_ref()
            .map_or(persisted.show_preview, |session| session.show_preview);
        let sync_scroll = restored_session
            .as_ref()
            .map_or(persisted.sync_scroll, |session| session.sync_scroll);
        let raw_mode = restored_session
            .as_ref()
            .map_or(persisted.raw_mode, |session| session.raw_mode);
        let library_width = panel_width(
            restored_session
                .as_ref()
                .map_or(persisted.library_width, |session| session.library_width),
            persisted.library_width,
        );
        let agent_width = panel_width(
            restored_session
                .as_ref()
                .map_or(persisted.agent_width, |session| session.agent_width),
            persisted.agent_width,
        );
        let settings_gutter = gutter_width(persisted.settings_gutter);
        let editor_font = if persisted.editor_font.trim().is_empty() {
            preferred_editor_font(cx)
        } else {
            persisted.editor_font.clone()
        };
        let editor_font_size =
            if persisted.editor_font_size.is_finite() && persisted.editor_font_size > 0.0 {
                persisted
                    .editor_font_size
                    .clamp(MIN_EDITOR_FONT_SIZE, MAX_EDITOR_FONT_SIZE)
            } else {
                default_editor_font_size(cx)
            };
        let mut state = Self {
            settings: persisted,
            settings_path,
            session_path,
            last_session: restored_session.clone(),
            rag,
            applied_rag,
            controller,
            grobid,
            documents: Vec::new(),
            active: 0,
            next_document: 1,
            workspace,
            workspace_explicit,
            corpus,
            ragrig_dir,
            bibliography,
            references_revision: 0,
            document_attached,
            library_attached,
            show_library,
            show_agent,
            show_preview,
            sync_scroll,
            library_width,
            agent_width,
            settings_gutter,
            editor_font,
            editor_font_size,
            last_memory,
            #[cfg(editor_gpuikit)]
            preview_sync_generation: 0,
            #[cfg(editor_zorite)]
            scroll_generation: 0,
            session_generation: 0,
            raw_mode,
            creating_profile: false,
            profile_draft_error: None,
            status,
            style,
        };

        let restore = std::env::var_os("SCICO_FILE")
            .map(PathBuf::from)
            .filter(|path| path.is_file());
        let mut opened = false;
        if let Some(path) = restore {
            match state.open_path(&path, window, cx) {
                Ok(()) => opened = true,
                Err(error) => state.set_status(error, cx),
            }
        } else if let Some(session) = restored_session {
            state.restore_session(session, window, cx);
            opened = !state.documents.is_empty();
        }
        if requested_by_cli && let Some(report) = workspace_report.as_ref() {
            let (_, kept) = state.retain_workspace_tabs(&report.root, cx);
            state.open_workspace_files(&report.homepage, window, cx);
            opened = true;
            workspace_status = workspace_status.map(|status| with_kept_tabs(status, kept));
        }
        if !opened
            && let Some(homepage) = workspace_report
                .as_ref()
                .map(|report| report.homepage.clone())
        {
            match state.open_path(&homepage, window, cx) {
                Ok(()) => opened = true,
                Err(error) => state.set_status(error, cx),
            }
        }
        if !opened {
            state.new_document(window, cx);
        }
        if let Some(error) = session_error {
            state.set_status(error, cx);
        } else if startup_status.starts_with("RAG settings")
            || startup_status.starts_with("Workspace:")
        {
            state.set_status(startup_status, cx);
        } else if let Some(status) = workspace_status {
            state.set_status(status, cx);
        }
        state.persist_session(cx);
        if requested_by_cli {
            // Remember the directory named on the command line, so the next
            // plain launch reopens the same workspace.
            state.persist(cx);
        }

        // Persist Ragrig edits even before the user presses Apply, and keep the
        // app's bibliography/workspace in step with the settings page.
        cx.observe(&state.rag, |this, _, cx| {
            let (workspace, bibliography) = {
                let rag = this.rag.read(cx);
                (
                    non_empty_path(&rag.workspace),
                    non_empty_path(&rag.bibliography),
                )
            };
            if let Some(workspace) = workspace {
                this.workspace = Some(workspace);
            }
            if bibliography != this.bibliography {
                this.bibliography = bibliography;
            }
            this.persist(cx);
            cx.notify();
        })
        .detach();

        state
    }

    /// The style new editors start with.
    pub fn editor_style(&self) -> &SyntaxStyle {
        &self.style
    }

    /// The active document, if any.
    pub fn active_document(&self) -> Option<&Document> {
        self.documents.get(self.active)
    }

    /// Whether the workspace bibliography is open in an editor tab.
    ///
    /// GROBID rewrites the bibliography behind the editor's back, so a run
    /// must wait until the raw bibliography tab is closed.
    pub fn bibliography_open(&self) -> bool {
        let bibliography = self.bibliography.as_deref();
        self.documents
            .iter()
            .any(|document| is_bibliography_path(document.path.as_deref(), bibliography))
    }

    /// The active document mutably, if any.
    pub fn active_document_mut(&mut self) -> Option<&mut Document> {
        self.documents.get_mut(self.active)
    }

    /// A document by identity.
    pub fn document(&self, id: u64) -> Option<&Document> {
        self.documents.iter().find(|document| document.id == id)
    }

    /// A document by identity, mutably.
    pub fn document_mut(&mut self, id: u64) -> Option<&mut Document> {
        self.documents.iter_mut().find(|document| document.id == id)
    }

    /// Set the status line.
    pub fn set_status(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = status.into();
        cx.notify();
    }

    /// Create an empty document and select it.
    pub fn new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) -> u64 {
        let id = self.next_document;
        self.next_document += 1;
        let mut title = format!("Untitled-{id}.md");
        let mut suffix = 1u64;
        while self
            .documents
            .iter()
            .any(|document| document.title.as_ref() == title.as_str())
        {
            suffix += 1;
            title = format!("Untitled-{id}-{suffix}.md");
        }
        let document = Document::new(
            window,
            cx,
            &self.style,
            crate::document::DocumentInit {
                id,
                title: title.into(),
                path: None,
                text: String::new(),
                raw: self.raw_mode,
            },
        );
        #[cfg(editor_gpuikit)]
        self.subscribe_editor(&document.editor, &document.preview, id, cx);
        #[cfg(editor_zorite)]
        self.subscribe_editor(&document.editor, id, cx);
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        window.focus(
            &self.documents[self.active].editor.read(cx).focus_handle(cx),
            cx,
        );
        self.set_status("New document", cx);
        self.persist_session(cx);
        self.sync_active_preview(cx);
        id
    }

    /// Open text as an untitled buffer, or switch to an existing untitled
    /// buffer with the same title.
    ///
    /// Used to show the compiled-in prompt without writing a file.
    pub fn open_buffer(
        &mut self,
        title: impl Into<SharedString>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        let title = title.into();
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path.is_none() && document.title == title)
        {
            self.active = index;
            window.focus(&self.documents[index].editor.read(cx).focus_handle(cx), cx);
            self.sync_active_preview(cx);
            cx.notify();
            return self.documents[index].id;
        }
        let id = self.next_document;
        self.next_document += 1;
        let document = Document::new(
            window,
            cx,
            &self.style,
            crate::document::DocumentInit {
                id,
                title,
                path: None,
                text,
                raw: self.raw_mode,
            },
        );
        #[cfg(editor_gpuikit)]
        self.subscribe_editor(&document.editor, &document.preview, id, cx);
        #[cfg(editor_zorite)]
        self.subscribe_editor(&document.editor, id, cx);
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        window.focus(
            &self.documents[self.active].editor.read(cx).focus_handle(cx),
            cx,
        );
        self.persist_session(cx);
        self.sync_active_preview(cx);
        id
    }

    /// Open the effective Chat prompt in the editor.
    ///
    /// A selected prompt file opens as a normal document. Without one, the
    /// compiled-in `Chat.md` appears as an untitled buffer.
    pub fn open_chat_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match self.rag.read(cx).prompt_path() {
            Some(path) => self.open_path(&path, window, cx),
            None => {
                self.open_buffer(CHAT_PROMPT_FILE, CHAT_PROMPT.to_owned(), window, cx);
                Ok(())
            }
        }
    }

    /// Write the compiled-in prompt into the workspace, select it, and open it.
    ///
    /// An existing `Chat.md` is never overwritten: it is opened and selected
    /// as the prompt instead, so local edits survive the action.
    pub fn export_chat_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let workspace = self
            .workspace
            .clone()
            .ok_or_else(|| "choose a workspace folder first".to_owned())?;
        let path = workspace.join(CHAT_PROMPT_FILE);
        let mut created = false;
        if !path.exists() {
            fs::write(&path, CHAT_PROMPT)
                .map_err(|error| format!("cannot write '{}': {error}", path.display()))?;
            created = true;
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
        if text.trim().is_empty() {
            return Err(format!("'{}' is empty", path.display()));
        }

        self.open_path(&path, window, cx)?;
        self.rag.update(cx, |rag, cx| {
            rag.system_prompt_path = path.display().to_string();
            rag.system_prompt = text;
            cx.notify();
        });
        self.set_status(
            if created {
                format!("Exported {CHAT_PROMPT_FILE} to {}", workspace.display())
            } else {
                format!("Using existing {CHAT_PROMPT_FILE}")
            },
            cx,
        );
        Ok(path.display().to_string())
    }

    /// Keep the prompt text in step when its file is saved in the editor.
    fn refresh_prompt_from_document(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(document) = self.document(id) else {
            return;
        };
        let Some(path) = document.path.clone() else {
            return;
        };
        if self.rag.read(cx).prompt_path().as_deref() != Some(path.as_path()) {
            return;
        }
        let text = document.text(cx);
        self.rag.update(cx, |rag, cx| {
            rag.system_prompt = text;
            cx.notify();
        });
    }

    /// Rebuild every tab from a journal snapshot.
    ///
    /// File-backed buffers compare their journal text with the file on disk:
    /// a match restores cleanly, a difference reopens as an unsaved change.
    /// Untitled buffers always come back exactly as they were left.
    fn restore_session(
        &mut self,
        session: SessionState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for snapshot in session.documents {
            let id = self.next_document;
            self.next_document += 1;
            let (title, saved_text, dirty) = match &snapshot.path {
                Some(path) => {
                    let disk = read_document(path).unwrap_or_default();
                    let dirty = disk != snapshot.text;
                    (title_for(path), disk, dirty)
                }
                None => (
                    SharedString::from(if snapshot.title.trim().is_empty() {
                        format!("Untitled-{id}.md")
                    } else {
                        snapshot.title
                    }),
                    String::new(),
                    !snapshot.text.is_empty(),
                ),
            };
            let document = Document::new(
                window,
                cx,
                &self.style,
                crate::document::DocumentInit {
                    id,
                    title,
                    path: snapshot.path,
                    text: snapshot.text,
                    raw: self.raw_mode,
                },
            );
            #[cfg(editor_gpuikit)]
            self.subscribe_editor(&document.editor, &document.preview, id, cx);
            #[cfg(editor_zorite)]
            self.subscribe_editor(&document.editor, id, cx);
            document.set_cursor(snapshot.cursor, cx);
            let mut document = document;
            document.saved_text = saved_text;
            document.dirty = dirty;
            document.cursor = document.cursor_position(cx);
            self.documents.push(document);
        }

        if self.documents.is_empty() {
            self.new_document(window, cx);
            return;
        }
        self.active = session.active.min(self.documents.len() - 1);
        self.show_library = session.show_library;
        self.show_agent = session.show_agent;
        self.show_preview = session.show_preview;
        self.sync_scroll = session.sync_scroll;
        self.raw_mode = session.raw_mode;
        self.library_width = panel_width(session.library_width, self.library_width);
        self.agent_width = panel_width(session.agent_width, self.agent_width);
        if let Some(document) = self.documents.get(self.active) {
            window.focus(&document.editor.read(cx).focus_handle(cx), cx);
        }
        let active = self.active_document().map(|document| document.id);
        if let Some(id) = active {
            self.follow_caret(id, cx);
        }
    }

    /// The current window state, including buffers as they are on screen.
    fn session_snapshot(&self, cx: &App) -> SessionState {
        SessionState {
            documents: self
                .documents
                .iter()
                .map(|document| SessionDocument {
                    path: document.path.clone(),
                    title: document.title.to_string(),
                    text: document.text(cx),
                    cursor: document.editor.read(cx).cursor(),
                    dirty: document.dirty,
                })
                .collect(),
            active: self.active,
            show_library: self.show_library,
            show_agent: self.show_agent,
            show_preview: self.show_preview,
            sync_scroll: self.sync_scroll,
            raw_mode: self.raw_mode,
            library_width: self.library_width,
            agent_width: self.agent_width,
        }
    }

    /// Journal the window state atomically.
    ///
    /// Explicit actions (open, save, close, tab switch, layout) call this
    /// directly so they are durable immediately. Content edits go through
    /// `schedule_session_save`, which debounces them by 300 ms. A
    /// snapshot is skipped when nothing it records has changed, so moving the
    /// caret within one character still costs a comparison rather than a disk
    /// write.
    pub fn persist_session(&mut self, cx: &App) {
        let snapshot = self.session_snapshot(cx);
        if self.last_session.as_ref() == Some(&snapshot) {
            return;
        }
        match snapshot.save(&self.session_path) {
            Ok(()) => self.last_session = Some(snapshot),
            Err(error) => self.status = format!("Session not saved: {error}").into(),
        }
    }

    /// Debounce a session write after content edits.
    ///
    /// Each edit bumps the generation and starts a new 300 ms timer; only the
    /// newest timer writes, so a burst of keystrokes costs one journal update.
    /// Explicit actions call [`Self::persist_session`] directly instead.
    fn schedule_session_save(&mut self, cx: &mut Context<Self>) {
        self.session_generation = self.session_generation.wrapping_add(1);
        let generation = self.session_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let still_current = this
                .update(cx, |this, _| this.session_generation == generation)
                .unwrap_or(false);
            if still_current {
                let _ = this.update(cx, |this, cx| this.persist_session(cx));
            }
        })
        .detach();
    }

    /// Open a file, or switch to it when already open.
    pub fn open_path(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if path.is_dir() {
            self.open_workspace(path.to_path_buf(), window, cx)?;
            return Ok(());
        }
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path.as_deref() == Some(path))
        {
            self.active = index;
            window.focus(&self.documents[index].editor.read(cx).focus_handle(cx), cx);
            self.sync_active_preview(cx);
            cx.notify();
            return Ok(());
        }
        let text = read_document(path)?;
        let id = self.next_document;
        self.next_document += 1;
        let document = Document::new(
            window,
            cx,
            &self.style,
            crate::document::DocumentInit {
                id,
                title: title_for(path),
                path: Some(path.to_path_buf()),
                text,
                raw: self.raw_mode,
            },
        );
        #[cfg(editor_gpuikit)]
        self.subscribe_editor(&document.editor, &document.preview, id, cx);
        #[cfg(editor_zorite)]
        self.subscribe_editor(&document.editor, id, cx);
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        window.focus(
            &self.documents[self.active].editor.read(cx).focus_handle(cx),
            cx,
        );
        self.set_status(format!("Opened {}", path.display()), cx);
        self.persist_session(cx);
        self.sync_active_preview(cx);
        Ok(())
    }

    /// Ask the Library section to reload the bibliography rows.
    fn references_changed(&mut self, cx: &mut Context<Self>) {
        self.references_revision = self.references_revision.wrapping_add(1);
        cx.notify();
    }

    /// Bump the bibliography revision when `id` is the bibliography document.
    fn note_saved_document(&mut self, id: u64, cx: &mut Context<Self>) {
        let path = self
            .document(id)
            .and_then(|document| document.path.as_deref());
        if is_bibliography_path(path, self.bibliography.as_deref()) {
            self.references_changed(cx);
        }
    }

    /// Open the bibliography and place the caret on the entry for `key`.
    ///
    /// The file is focused where it already is open. A missing entry still
    /// opens the bibliography, then reports why it cannot jump.
    pub fn open_bibliography_entry(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(path) = self.bibliography.clone() else {
            return Err("No bibliography configured".to_owned());
        };
        self.open_path(&path, window, cx)?;
        let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path.as_deref() == Some(path.as_path()))
        else {
            return Err(format!("{} is not open", path.display()));
        };
        let text = self.documents[index].text(cx);
        let Some(offset) = bibtex_entry_offset(&text, key) else {
            return Err(format!("Entry @{key} is not in {}", path.display()));
        };
        let id = self.documents[index].id;
        self.documents[index].reveal(offset, window, cx);
        self.active = index;
        self.follow_caret(id, cx);
        #[cfg(editor_gpuikit)]
        self.settle_bibliography_scroll(id, cx);
        self.sync_active_preview(cx);
        self.persist_session(cx);
        cx.notify();
        Ok(())
    }

    /// Save the active document to its existing path.
    pub fn save_active(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let id = self.active_document().map(|document| document.id);
        let result = match self.active_document_mut() {
            Some(document) => document.save(cx),
            None => Err("no document is open".to_owned()),
        };
        match &result {
            Ok(()) => {
                if let Some(id) = id {
                    self.refresh_prompt_from_document(id, cx);
                    self.note_saved_document(id, cx);
                }
                let name = self
                    .active_document()
                    .map(|document| document.title.clone())
                    .unwrap_or_default();
                self.set_status(format!("Saved {name}"), cx);
                self.persist_session(cx);
            }
            Err(error) => self.set_status(error.clone(), cx),
        }
        result
    }

    /// Save a document by identity to its existing path.
    pub fn save_document(&mut self, id: u64, cx: &mut Context<Self>) -> Result<(), String> {
        let result = match self.document_mut(id) {
            Some(document) => document.save(cx),
            None => Err("that document is no longer open".to_owned()),
        };
        match &result {
            Ok(()) => {
                self.refresh_prompt_from_document(id, cx);
                self.note_saved_document(id, cx);
                self.persist_session(cx);
            }
            Err(error) => self.set_status(error.clone(), cx),
        }
        result
    }

    /// Save a document by identity under a new path.
    pub fn save_document_as(
        &mut self,
        id: u64,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let result = match self.document_mut(id) {
            Some(document) => document.save_as(path, cx),
            None => Err("that document is no longer open".to_owned()),
        };
        match &result {
            Ok(()) => {
                self.refresh_prompt_from_document(id, cx);
                self.note_saved_document(id, cx);
                let name = self
                    .document(id)
                    .map(|document| document.title.clone())
                    .unwrap_or_default();
                self.set_status(format!("Saved {name}"), cx);
                self.persist_session(cx);
            }
            Err(error) => self.set_status(error.clone(), cx),
        }
        result
    }

    /// Save the active document under a new path.
    pub fn save_active_as(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(id) = self.active_document().map(|document| document.id) else {
            return Err("no document is open".to_owned());
        };
        self.save_document_as(id, path, cx)
    }

    /// Close the active tab. A dirty document is kept until saved.
    pub fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, dirty, title)) = self
            .active_document()
            .map(|document| (document.id, document.dirty, document.title.clone()))
        else {
            return;
        };
        if dirty {
            self.set_status(
                format!("{title} has unsaved changes — save it before closing"),
                cx,
            );
            return;
        }
        self.close_document(id, window, cx);
    }

    /// Remove a document by identity, regardless of its dirty state.
    ///
    /// Callers that close a dirty document must obtain the author's consent
    /// first; [`Self::close_active`] keeps the dirty guard for the ordinary
    /// path.
    pub fn close_document(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.documents.iter().position(|document| document.id == id) else {
            return;
        };
        let closed_bibliography = is_bibliography_path(
            self.documents[index].path.as_deref(),
            self.bibliography.as_deref(),
        );
        self.documents.remove(index);
        if closed_bibliography {
            self.references_changed(cx);
        }
        if self.documents.is_empty() {
            self.active = 0;
        } else {
            if index < self.active {
                self.active -= 1;
            } else if self.active >= self.documents.len() {
                self.active = self.documents.len() - 1;
            }
            window.focus(
                &self.documents[self.active].editor.read(cx).focus_handle(cx),
                cx,
            );
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// Select the tab at `index`.
    pub fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, editor)) = self
            .documents
            .get(index)
            .map(|document| (document.id, document.focus_handle(cx)))
        else {
            return;
        };
        self.active = index;
        window.focus(&editor, cx);
        self.persist_session(cx);
        self.follow_caret(id, cx);
        self.sync_active_preview(cx);
        cx.notify();
    }

    /// Insert text at the active document's selection or caret.
    pub fn insert_at_caret(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.active_document() else {
            self.set_status("Open a document first", cx);
            return;
        };
        document.insert_text(text, window, cx);
        if let Some(document) = self.active_document_mut() {
            document.dirty = true;
        }
        self.persist_session(cx);
        cx.notify();
    }

    /// Switch live preview on or off for every editor.
    pub fn set_raw_mode(&mut self, raw: bool, cx: &mut Context<Self>) {
        if self.raw_mode == raw {
            return;
        }
        self.raw_mode = raw;
        #[cfg(editor_zorite)]
        {
            let style = self.style.clone();
            for document in &self.documents {
                document.editor.update(cx, |editor, cx| {
                    if raw {
                        editor.clear_markdown_style(cx);
                    } else {
                        editor.set_markdown_style(style.clone(), cx);
                    }
                });
            }
        }
        self.settings.raw_mode = raw;
        self.persist(cx);
        self.persist_session(cx);
        self.set_status(if raw { "Raw Markdown" } else { "Live preview" }, cx);
    }

    /// The configured editor font, or the theme's monospace family.
    pub fn editor_font_family(&self, cx: &App) -> SharedString {
        if self.editor_font.trim().is_empty() {
            cx.theme().mono_font_family.clone()
        } else {
            self.editor_font.clone().into()
        }
    }

    /// Choose the editor font family.
    pub fn set_editor_font(&mut self, font: String, cx: &mut Context<Self>) {
        if self.editor_font == font {
            return;
        }
        self.editor_font = font;
        self.settings.editor_font = self.editor_font.clone();
        self.persist(cx);
        cx.notify();
    }

    /// Set the editor font size in pixels, clamped to the supported range.
    pub fn set_editor_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = (size * 2.0).round() / 2.0;
        let size = size.clamp(MIN_EDITOR_FONT_SIZE, MAX_EDITOR_FONT_SIZE);
        if (self.editor_font_size - size).abs() < f32::EPSILON {
            return;
        }
        self.editor_font_size = size;
        self.settings.editor_font_size = size;
        self.persist(cx);
        self.set_status(format!("Editor font {size} px"), cx);
    }

    /// Zoom the editor font by `delta` pixels, as Ctrl+wheel does.
    pub fn zoom_editor_font(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.set_editor_font_size(self.editor_font_size + delta, cx);
    }

    /// Show or hide the rendered preview beside the source editor.
    pub fn set_show_preview(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_preview == show {
            return;
        }
        self.show_preview = show;
        self.settings.show_preview = show;
        self.persist(cx);
        self.persist_session(cx);
        if show {
            self.sync_active_preview(cx);
        }
        self.set_status(
            if show {
                "Preview shown"
            } else {
                "Preview hidden"
            },
            cx,
        );
    }

    /// Keep the source editor and the preview scrolled together.
    pub fn set_sync_scroll(&mut self, sync: bool, cx: &mut Context<Self>) {
        if self.sync_scroll == sync {
            return;
        }
        self.sync_scroll = sync;
        self.settings.sync_scroll = sync;
        self.persist(cx);
        self.persist_session(cx);
        if sync {
            self.sync_active_preview(cx);
        }
        self.set_status(
            if sync {
                "Scroll sync on"
            } else {
                "Scroll sync off"
            },
            cx,
        );
    }

    /// Move the preview to the source block at the top of the editor.
    ///
    /// This is the editor-to-preview half of the two-way link. It never runs
    /// while a preview-driven editor scroll is still being applied, otherwise
    /// the approximate line-to-pixel mapping could pull the preview back to a
    /// different block than the one the user chose.
    #[cfg(editor_gpuikit)]
    fn sync_preview_to_editor(&mut self, id: u64, cx: &mut Context<Self>) {
        if !self.sync_scroll || !self.show_preview {
            return;
        }
        let Some(document) = self.document(id) else {
            return;
        };
        if self.active_document().map(|document| document.id) != Some(id) {
            return;
        }
        if document.pending_editor_scroll {
            return;
        }
        let Some(block_ix) = source_top_block(document, cx) else {
            return;
        };
        let preview = document.preview.clone();
        if let Some(document) = self.document_mut(id) {
            document.preview_top = block_ix;
        }
        let (shown, count) = {
            let list = preview.read(cx).list_state();
            (list.logical_scroll_top().item_ix, list.item_count())
        };
        if shown == block_ix || count <= block_ix {
            return;
        }
        preview.update(cx, |preview, cx| {
            preview.list_state().scroll_to(gpui_kit::ListOffset {
                item_ix: block_ix,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
    }

    /// Schedule a source follow-up once the editor has laid out a new scroll
    /// position; `range_to_bounds` reads the previous frame.
    #[cfg(editor_gpuikit)]
    fn schedule_source_sync(&mut self, id: u64, cx: &mut Context<Self>) {
        self.preview_sync_generation = self.preview_sync_generation.wrapping_add(1);
        let generation = self.preview_sync_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.preview_sync_generation == generation {
                    this.sync_preview_to_editor(id, cx);
                }
            });
        })
        .detach();
    }

    /// Re-align once the preview has parsed the text an edit produced.
    #[cfg(editor_gpuikit)]
    fn schedule_preview_realign(&mut self, id: u64, cx: &mut Context<Self>) {
        self.preview_sync_generation = self.preview_sync_generation.wrapping_add(1);
        let generation = self.preview_sync_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.preview_sync_generation == generation {
                    this.sync_preview_to_editor(id, cx);
                }
            });
        })
        .detach();
    }

    /// The user scrolled the preview; put the matching source line at the top
    /// of the editor.
    ///
    /// The editor only exposes a pixel scroll offset, so the first estimate
    /// uses the wrap density learned from earlier syncs (or the viewport's own
    /// ratio). [`Self::probe_preview_sync`] then brackets the exact offset from
    /// the editor's layouts, which also covers wrapped text where the estimate
    /// is far off.
    #[cfg(editor_gpuikit)]
    fn preview_scrolled(
        &mut self,
        id: u64,
        preview: &Entity<TextViewState>,
        cx: &mut Context<Self>,
    ) {
        if !self.sync_scroll || !self.show_preview {
            return;
        }

        let (top, line, editor, current, learned) = {
            let Some(document) = self.document(id) else {
                return;
            };
            if &document.preview != preview {
                return;
            }
            let top = preview.read(cx).list_state().logical_scroll_top().item_ix;
            if document.preview_top == top {
                return;
            }
            let Some(block) = document.blocks.get(top) else {
                return;
            };
            if preview.read(cx).list_state().item_count() != document.blocks.len() {
                return;
            }
            // An edit updates the preview asynchronously; a top that moved
            // while the text is stale is not a user scroll.
            let source = document.editor.read(cx).value();
            if preview.read(cx).rendered_text().source() != source.as_ref() {
                return;
            }
            let editor = document.editor.clone();
            let current = f32::from(editor.read(cx).scroll_offset().y);
            (
                top,
                block.start_line,
                editor,
                current,
                document.preview_rows_per_line,
            )
        };

        let Some(line_height) = editor.read(cx).line_height().map(f32::from) else {
            return;
        };
        if line_height <= 0.0 {
            return;
        }
        let rows_above = (-current / line_height).max(0.0);
        let top_line = source_top_line(editor.read(cx)).unwrap_or(0);
        let viewport_ratio = if top_line > 0 && rows_above > 0.0 {
            (rows_above / top_line as f32).max(1.0)
        } else {
            1.0
        };
        let rows_per_line = learned.max(viewport_ratio);
        let estimate = -(line as f32 * rows_per_line * line_height);

        // Cancel any debounced editor-to-preview sync that was armed before
        // this user scroll: it reads the old viewport and would snap back.
        self.preview_sync_generation = self.preview_sync_generation.wrapping_add(1);
        let moved = (estimate - current).abs() > 0.5;
        if let Some(document) = self.document_mut(id) {
            document.preview_top = top;
            if moved {
                document.preview_source_offset = current;
                document.pending_editor_scroll = true;
                document.preview_scroll_target = Some(crate::document::PreviewScrollTarget {
                    block: top,
                    low: 0.0,
                    high: None,
                    attempts: 0,
                    requested: estimate,
                    observed: None,
                    waited: 0,
                });
            }
        }
        if moved {
            editor.update(cx, |editor, cx| {
                editor.set_scroll_offset(point(px(0.), px(estimate)), cx);
            });
            self.schedule_preview_probe(id, cx);
        }
    }

    /// Schedule the next look at the editor's laid-out viewport.
    #[cfg(editor_gpuikit)]
    fn schedule_preview_probe(&mut self, id: u64, cx: &mut Context<Self>) {
        let generation = self.preview_sync_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.preview_sync_generation == generation {
                    this.probe_preview_sync(id, cx);
                }
            });
        })
        .detach();
    }

    /// Narrow the bracket around the block the user chose in the preview.
    ///
    /// Called on a short timer rather than from the editor's notification:
    /// the deferred scroll offset is applied during layout, and the probe has
    /// to read the *new* layout. The mapping from scroll offset to top block is
    /// monotonic, so bisection converges even when soft wrap makes the initial
    /// pixel estimate useless.
    #[cfg(editor_gpuikit)]
    fn probe_preview_sync(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(document) = self.document(id) else {
            return;
        };
        let Some(mut target) = document.preview_scroll_target else {
            return;
        };
        let editor = document.editor.clone();
        let current = f32::from(editor.read(cx).scroll_offset().y);
        if (current - target.requested).abs() > 0.5 {
            // The deferred scroll has not reached the viewport yet.
            if target.waited < 8 {
                target.waited += 1;
                if let Some(document) = self.document_mut(id) {
                    document.preview_scroll_target = Some(target);
                }
                self.schedule_preview_probe(id, cx);
            } else if let Some(document) = self.document_mut(id) {
                document.preview_scroll_target = None;
                document.pending_editor_scroll = false;
            }
            return;
        }
        if target
            .observed
            .is_some_and(|observed| (observed - current).abs() <= 0.5)
        {
            // Waiting for the request applied above to be laid out.
            if target.waited < 8 {
                target.waited += 1;
                if let Some(document) = self.document_mut(id) {
                    document.preview_scroll_target = Some(target);
                }
                self.schedule_preview_probe(id, cx);
            } else if let Some(document) = self.document_mut(id) {
                document.preview_scroll_target = None;
                document.pending_editor_scroll = false;
            }
            return;
        }
        target.observed = Some(current);
        target.waited = 0;

        let Some(actual) = source_top_block(document, cx) else {
            if let Some(document) = self.document_mut(id) {
                document.preview_scroll_target = None;
                document.pending_editor_scroll = false;
            }
            return;
        };
        let Some(line_height) = editor.read(cx).line_height().map(f32::from) else {
            return;
        };
        if line_height <= 0.0 {
            return;
        }
        let magnitude = -current;

        if actual == target.block {
            // Remember how many display rows a line really costs here; the
            // next preview scroll then starts from a good estimate.
            let learned = document
                .blocks
                .get(target.block)
                .filter(|block| block.start_line > 0 && magnitude > 0.0)
                .map(|block| (magnitude / (block.start_line as f32 * line_height)).max(1.0));
            if let Some(document) = self.document_mut(id) {
                if let Some(learned) = learned {
                    document.preview_rows_per_line = learned;
                }
                document.preview_scroll_target = None;
                document.pending_editor_scroll = false;
            }
            return;
        }

        target.attempts += 1;
        if target.attempts > 40 {
            if let Some(document) = self.document_mut(id) {
                document.preview_scroll_target = None;
                document.pending_editor_scroll = false;
            }
            return;
        }

        let next = if actual < target.block {
            target.low = target.low.max(magnitude);
            match target.high {
                Some(high) => (target.low + high) / 2.0,
                None => (target.low * 2.0).max(target.low + line_height),
            }
        } else {
            target.high = Some(target.high.map_or(magnitude, |high| high.min(magnitude)));
            match target.low {
                0.0 => magnitude / 2.0,
                low => (low + target.high.unwrap_or(magnitude)) / 2.0,
            }
        };

        let keep_going = next.is_finite()
            && (next - magnitude).abs() > 0.25
            && target.high.is_none_or(|high| high - target.low > 0.5)
            && next < 10_000_000.0;
        if !keep_going {
            if let Some(document) = self.document_mut(id) {
                document.preview_scroll_target = None;
                document.pending_editor_scroll = false;
            }
            return;
        }

        target.requested = -next;
        target.waited = 0;
        if let Some(document) = self.document_mut(id) {
            document.preview_scroll_target = Some(target);
            document.preview_source_offset = current;
            document.pending_editor_scroll = true;
        }
        editor.update(cx, |editor, cx| {
            editor.set_scroll_offset(point(px(0.), px(-next)), cx);
        });
        self.schedule_preview_probe(id, cx);
    }

    #[cfg(editor_gpuikit)]
    fn sync_active_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.active_document().map(|document| document.id) {
            self.sync_preview_to_editor(id, cx);
        }
    }

    #[cfg(editor_zorite)]
    fn sync_active_preview(&mut self, _cx: &mut Context<Self>) {}

    /// Adopt a prepared workspace: paths, layered RAG settings, and prompt.
    pub fn adopt_workspace(&mut self, report: WorkspaceReport, cx: &mut Context<Self>) {
        self.workspace = Some(report.root.clone());
        self.workspace_explicit = true;
        self.corpus = Some(report.root.join(workspace::CORPUS_DIR));
        self.bibliography = Some(report.bibliography.clone());
        self.ragrig_dir = report.ragrig_dir.clone();

        let layered = RagSettings::from_layers_over(&report.root, &self.rag.read(cx).clone());
        let previous_profile = self.rag.read(cx).profile_name.trim().to_owned();
        self.rag.update(cx, |settings, cx| {
            if let Some(mut layered) = layered {
                layered.built_in_prompt = CHAT_PROMPT.to_owned();
                *settings = layered;
            }
            settings.workspace = report.root.display().to_string();
            settings.bibliography = report.bibliography.display().to_string();
            settings.built_in_prompt = CHAT_PROMPT.to_owned();
            settings
                .corpus_dirs
                .retain(|entry| !entry.starts_with(&format!("{}=", workspace::CORPUS_DIR)));
            settings.corpus_dirs.push(format!(
                "{}={}",
                workspace::CORPUS_DIR,
                report.root.join(workspace::CORPUS_DIR).display()
            ));
            apply_workspace_prompt(settings, &report);
            settings.profile_name.clear();
            settings.refresh_profiles();
            if !previous_profile.is_empty()
                && settings
                    .profiles
                    .iter()
                    .any(|name| name == &previous_profile)
            {
                settings.profile_name = previous_profile;
            }
            cx.notify();
        });
        self.apply_rag(cx);
    }

    fn adopt_and_open(
        &mut self,
        report: WorkspaceReport,
        created: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let status = report.status(created);
        let root = report.root.clone();
        let homepage = report.homepage.clone();
        self.adopt_workspace(report, cx);
        let (_, kept) = self.retain_workspace_tabs(&root, cx);
        self.open_workspace_files(&homepage, window, cx);
        let status = with_kept_tabs(status, kept);
        self.set_status(status.clone(), cx);
        status
    }

    /// Drop tabs that do not belong to `root` and report how many were kept.
    ///
    /// A dirty tab is kept, even when it lies outside the workspace: switching
    /// workspaces must never discard unsaved work. The active tab stays active
    /// when it survives the prune.
    pub(crate) fn retain_workspace_tabs(
        &mut self,
        root: &Path,
        cx: &mut Context<Self>,
    ) -> (usize, usize) {
        let before = self.documents.len();
        let active_id = self.active_document().map(|document| document.id);
        let mut kept_dirty = 0;
        self.documents.retain(|document| {
            let outside = match &document.path {
                Some(path) => {
                    !path.starts_with(root)
                        && !std::fs::canonicalize(path).is_ok_and(|path| path.starts_with(root))
                }
                None => true,
            };
            if outside && document.dirty {
                kept_dirty += 1;
                true
            } else {
                !outside
            }
        });
        let removed = before - self.documents.len();
        if removed > 0 {
            self.active = active_id
                .and_then(|id| self.documents.iter().position(|document| document.id == id))
                .unwrap_or_else(|| self.active.min(self.documents.len().saturating_sub(1)));
            self.persist_session(cx);
            cx.notify();
        }
        (removed, kept_dirty)
    }

    /// Open the workspace paper; the paper takes focus.
    ///
    /// The bibliography stays closed on purpose: the Library opens it on
    /// demand, so entering a workspace never leaves an editable `.bib` tab
    /// that GROBID would have to write around.
    pub(crate) fn open_workspace_files(
        &mut self,
        homepage: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.open_path(homepage, window, cx) {
            self.set_status(error, cx);
        }
    }

    /// Create a new workspace directory with the full scaffold and open it.
    pub fn new_workspace(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let report = workspace::prepare(&path, true)?;
        Ok(self.adopt_and_open(report, true, window, cx))
    }

    /// Open an existing directory as a workspace, adding missing pieces.
    pub fn open_workspace(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let report = workspace::prepare(&path, false)?;
        Ok(self.adopt_and_open(report, false, window, cx))
    }

    /// Make `path` the editor and RAG workspace.
    ///
    /// This is the path-field entry point: it scaffolds the directory but does
    /// not force a document tab, so typing a path in Settings stays cheap.
    pub fn set_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match workspace::prepare(&path, false) {
            Ok(report) => {
                let status = report.status(false);
                self.adopt_workspace(report, cx);
                self.set_status(status, cx);
            }
            Err(error) => {
                self.workspace = Some(path.clone());
                self.workspace_explicit = true;
                self.rag.update(cx, |settings, _| {
                    settings.workspace = path.display().to_string();
                });
                self.persist(cx);
                self.set_status(error, cx);
            }
        }
    }

    /// Turn the chat's conversation memory on or off and rebuild the agent.
    ///
    /// Off clears the transcript and the query rewriter. Switching it back on
    /// restores the strategy that was active before.
    pub fn set_memory_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let current = self.rag.read(cx).memory;
        let (next, last) = toggled_memory(current, self.last_memory, enabled);
        self.last_memory = last;
        if next == current {
            return;
        }
        self.rag.update(cx, |settings, cx| {
            settings.memory = next;
            cx.notify();
        });
        self.apply_rag(cx);
        self.persist_workspace_switches(cx);
        self.set_status(
            if enabled {
                format!("Chat memory on — {}", next.label())
            } else {
                "Chat memory off".to_owned()
            },
            cx,
        );
    }

    /// Turn hybrid retrieval (embeddings) on or off and rebuild the agent.
    ///
    /// Off needs no embedding model and skips vector search; answers still use
    /// explicit attachments and the bibliographic library.
    pub fn set_hybrid_retrieval(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.rag.read(cx).hybrid == enabled {
            return;
        }
        self.rag.update(cx, |settings, cx| {
            settings.hybrid = enabled;
            cx.notify();
        });
        self.apply_rag(cx);
        self.persist_workspace_switches(cx);
        self.set_status(
            if enabled {
                "Hybrid retrieval on"
            } else {
                "Hybrid retrieval off — no embeddings"
            },
            cx,
        );
    }

    /// Record the two status-bar switches in the workspace layer.
    ///
    /// This keeps them effective when `settings.json` is reset or another
    /// machine opens the workspace, without copying the whole applied
    /// configuration into `.ragrig/config.json`.
    pub(crate) fn persist_workspace_switches(&self, cx: &App) {
        let Some(workspace) = &self.workspace else {
            return;
        };
        let rag = self.rag.read(cx);
        let overlay = serde_json::json!({
            "memory": {"strategy": rag.memory.label().to_ascii_lowercase()},
            "hybrid": rag.hybrid,
        });
        let _ = workspace::write_ragrig_layer(workspace, overlay);
    }

    /// Rebuild the agent from the current settings.
    pub fn apply_rag(&mut self, cx: &mut Context<Self>) {
        match self.rag.read(cx).to_setup() {
            Ok(setup) => {
                let mut setup = with_bibliography(setup, self.bibliography.clone());
                setup.data_dir = Some(self.ragrig_dir.clone());
                self.controller
                    .update(cx, |controller, cx| controller.apply(setup, cx));
                let applied = self.rag.read(cx).clone();
                if applied.memory != MemoryChoice::Off {
                    self.last_memory = applied.memory;
                }
                self.applied_rag = applied;
                self.persist(cx);
                self.set_status("RAG agent rebuilt", cx);
            }
            Err(error) => self.set_status(format!("Settings: {error}"), cx),
        }
    }

    /// Which settings fields differ from the running configuration.
    pub fn rag_changes(&self, cx: &App) -> RagChanges {
        self.rag.read(cx).dirty_since(&self.applied_rag)
    }

    /// Record pane widths after a divider drag.
    pub fn set_panel_widths(&mut self, library: f32, agent: f32, cx: &mut Context<Self>) {
        self.library_width = panel_width(library, self.library_width);
        self.agent_width = panel_width(agent, self.agent_width);
        self.persist(cx);
        self.persist_session(cx);
        cx.notify();
    }

    /// Record the settings gutter after a divider drag.
    pub fn set_settings_gutter(&mut self, gutter: f32, cx: &mut Context<Self>) {
        self.settings_gutter = gutter_width(gutter);
        self.persist(cx);
        cx.notify();
    }

    /// Load a saved profile into the settings form.
    pub fn load_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim().to_owned();
        let result = self.rag.update(cx, |rag, cx| {
            let result = rag.load_profile(&name);
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.persist(cx);
                self.set_status(format!("Loaded profile '{name}'"), cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    /// Save the current settings under the selected profile name.
    pub fn save_profile(&mut self, cx: &mut Context<Self>) {
        let result = self.rag.update(cx, |rag, cx| {
            let result = rag.save_profile();
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.persist(cx);
                cx.notify();
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    /// Open the name field for a new profile.
    pub fn begin_new_profile(&mut self, cx: &mut Context<Self>) {
        self.creating_profile = true;
        self.profile_draft_error = None;
        cx.notify();
    }

    /// Close the New-profile field without creating anything.
    pub fn cancel_new_profile(&mut self, cx: &mut Context<Self>) {
        if self.creating_profile {
            self.creating_profile = false;
            self.profile_draft_error = None;
            cx.notify();
        }
    }

    /// Save a new profile under `name` and select it.
    ///
    /// The name is checked against the existing profiles first, so **New**
    /// never silently overwrites one; **Update Profile** remains the way to
    /// overwrite.
    pub fn create_profile(&mut self, name: impl Into<String>, cx: &mut Context<Self>) {
        let name = name.into().trim().to_owned();
        if name.is_empty() {
            self.profile_draft_error = Some("Enter a profile name first.".into());
            cx.notify();
            return;
        }
        if self
            .rag
            .read(cx)
            .profiles
            .iter()
            .any(|profile| profile == &name)
        {
            self.profile_draft_error =
                Some(format!("A profile named '{name}' already exists.").into());
            cx.notify();
            return;
        }
        let previous = self.rag.read(cx).profile_name.clone();
        let result = self.rag.update(cx, |rag, cx| {
            rag.profile_name = name.clone();
            let result = rag.save_profile();
            if result.is_err() {
                rag.profile_name = previous;
            }
            cx.notify();
            result
        });
        match result {
            Ok(()) => {
                self.persist(cx);
                self.creating_profile = false;
                self.profile_draft_error = None;
                self.set_status(format!("Created profile '{name}'"), cx);
            }
            Err(error) => {
                self.profile_draft_error = Some(error.into());
                cx.notify();
            }
        }
    }

    /// Clear a validation message while the user edits the new name.
    pub fn clear_profile_error(&mut self, cx: &mut Context<Self>) {
        if self.profile_draft_error.take().is_some() {
            cx.notify();
        }
    }

    /// Accept the GROBID source, URL, and image chosen by the service page.
    ///
    /// Discovery and a successful container start are not pending edits: the
    /// values are already in effect for the next extraction, so they join the
    /// applied snapshot and are persisted immediately.
    pub fn persist_grobid_service(&mut self, cx: &mut Context<Self>) {
        let current = self.rag.read(cx).clone();
        self.applied_rag.grobid_source = current.grobid_source;
        self.applied_rag.grobid_url = current.grobid_url;
        self.applied_rag.grobid_image = current.grobid_image;
        self.persist(cx);
    }

    /// Write the settings file and update the in-memory snapshot.
    pub fn persist(&mut self, cx: &mut Context<Self>) {
        let rag = self.rag.read(cx).clone();
        self.settings.capture(&rag);
        self.settings.corpus = self
            .corpus
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        if self.settings.bibliography.trim().is_empty() {
            self.settings.bibliography = self
                .bibliography
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
        }
        self.settings.document_attached = self.document_attached;
        self.settings.library_attached = self.library_attached;
        self.settings.show_library = self.show_library;
        self.settings.show_agent = self.show_agent;
        self.settings.show_preview = self.show_preview;
        self.settings.sync_scroll = self.sync_scroll;
        self.settings.library_width = self.library_width;
        self.settings.agent_width = self.agent_width;
        self.settings.settings_gutter = self.settings_gutter;
        self.settings.raw_mode = self.raw_mode;
        self.settings.editor_font = self.editor_font.clone();
        self.settings.editor_font_size = self.editor_font_size;
        if !self.workspace_explicit {
            // The cwd fallback is not a workspace choice; persisting it would
            // scaffold that directory on the next launch.
            self.settings.workspace.clear();
        }
        self.settings.memory_when_on = self.last_memory.label().to_owned();
        if let Err(error) = self.settings.save(&self.settings_path) {
            self.status = error.into();
        }
    }

    /// Apply the first replacement block of a finished answer.
    pub fn apply_replacement(
        &mut self,
        target: ReplacementTarget,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ReplacementOutcome {
        if text.is_empty() {
            return ReplacementOutcome::Empty;
        }
        let (title, current) = {
            let Some(document) = self.document(target.document_id) else {
                return ReplacementOutcome::Offered {
                    text: text.to_owned(),
                    reason: "the document was closed".to_owned(),
                };
            };
            (document.title.clone(), document.text(cx))
        };
        let total = current.chars().count();
        let start = target.range.start.min(total);
        let end = target.range.end.min(total).max(start);
        let Some(bytes) = verified_bytes(&current, start..end, &target.digest) else {
            return ReplacementOutcome::Offered {
                text: text.to_owned(),
                reason: "the marked passage changed while the agent was writing".to_owned(),
            };
        };
        if let Some(document) = self.document(target.document_id) {
            document.replace(bytes, text, window, cx);
        }
        if let Some(document) = self.document_mut(target.document_id) {
            document.dirty = true;
        }
        let old_chars = end.saturating_sub(start);
        let delta = text.chars().count() as isize - old_chars as isize;
        let sign = if delta >= 0 { "+" } else { "−" };
        let message = format!(
            "Replacement applied to {title}: −{old_chars} / {sign}{} chars — Ctrl+Z undoes it.",
            delta.unsigned_abs()
        );
        self.persist_session(cx);
        cx.notify();
        ReplacementOutcome::Applied { message }
    }

    /// Offer text to the active editor at the caret.
    pub fn insert_offer(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> String {
        let Some(document) = self.active_document() else {
            return "Open a document first".to_owned();
        };
        let chars = text.chars().count();
        let name = document.title.clone();
        self.insert_at_caret(text, window, cx);
        format!("Replacement inserted into {name} ({chars} chars)")
    }

    /// Open a local file with its configured viewer, or the system default.
    ///
    /// URLs are always handed to the platform opener; the viewer map only
    /// applies to local `.pdf`, `.epub`, `.html`, and `.htm` files.
    pub fn open_external(&self, path: &Path) -> Result<(), String> {
        let text = path.to_string_lossy();
        if text.contains("://") {
            return open::that(path).map_err(|error| format!("cannot open {text}: {error}"));
        }
        match self.settings.viewers.program_for(path) {
            Some(program) => std::process::Command::new(program)
                .arg(path)
                .spawn()
                .map(|_| ())
                .map_err(|error| {
                    format!(
                        "cannot launch viewer '{program}' for '{}': {error}",
                        path.display()
                    )
                }),
            None => open::that(path)
                .map_err(|error| format!("cannot open '{}': {error}", path.display())),
        }
    }

    /// Keep the caret inside the editor viewport with one direct scroll.
    ///
    /// The zorite editor auto-grows and leaves scrolling to the host. The
    /// gpuikit editor owns its viewport, so it needs no host follow at all.
    /// One frame is allowed for the keystroke's layout, then the caret bounds
    /// are read. If the scroll offset moved while waiting, the user is
    /// scrolling: abort instead of fighting the gesture. Each call bumps a
    /// generation, so key repeat retargets the follow rather than stacking
    /// scroll tasks.
    #[cfg(editor_zorite)]
    fn follow_caret(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.active_document().map(|document| document.id) != Some(id) {
            return;
        }
        let Some(document) = self.document(id) else {
            return;
        };
        let editor = document.editor.clone();
        let scroll = document.scroll.borrow().clone();
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        let generation = self.scroll_generation;
        cx.spawn(async move |this, cx| {
            let start = f32::from(scroll.offset().y);
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let still_current = this
                .update(cx, |this, _| this.scroll_generation == generation)
                .unwrap_or(false);
            if !still_current || (f32::from(scroll.offset().y) - start).abs() > 0.5 {
                return;
            }

            // A paint may still carry the previous caret row. Retry a few
            // frames before giving up; the offset must stay put throughout.
            let mut target = None;
            for attempt in 0..3u64 {
                if attempt > 0 {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    let still_current = this
                        .update(cx, |this, _| this.scroll_generation == generation)
                        .unwrap_or(false);
                    if !still_current || (f32::from(scroll.offset().y) - start).abs() > 0.5 {
                        return;
                    }
                }
                target = cx.update(|cx| {
                    let caret = editor.read(cx).caret_screen_bounds()?;
                    let viewport = scroll.bounds();
                    if viewport.size.height <= px(0.) {
                        return None;
                    }
                    let margin = px(64.);
                    let mut offset = f32::from(scroll.offset().y);
                    if caret.top() < viewport.top() + margin {
                        offset += f32::from(viewport.top() + margin - caret.top());
                    } else if caret.bottom() > viewport.bottom() - margin {
                        offset -= f32::from(caret.bottom() - (viewport.bottom() - margin));
                    }
                    let max = f32::from(scroll.max_offset().y);
                    Some(offset.clamp(-max, 0.0))
                });
                if target.is_some() {
                    break;
                }
            }
            let Some(target) = target else {
                return;
            };
            let start = f32::from(scroll.offset().y);
            if (target - start).abs() < 1.0 {
                return;
            }
            let still_current = this
                .update(cx, |this, _| this.scroll_generation == generation)
                .unwrap_or(false);
            if !still_current {
                return;
            }
            scroll.set_offset(point(px(0.), px(target)));
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    #[cfg(editor_gpuikit)]
    fn follow_caret(&mut self, _id: u64, _cx: &mut Context<Self>) {}

    /// Place a revealed bibliography key two lines below the viewport top.
    ///
    /// The editor scrolls a moved caret minimally into view during its own
    /// paint, which leaves an entry near the end of the file at the bottom
    /// edge. Once that layout exists, this recentres the viewport with a short
    /// reading margin above the entry. The retries cover the frame between the
    /// cursor move and the first paint that reports the caret's bounds.
    #[cfg(editor_gpuikit)]
    fn settle_bibliography_scroll(&mut self, id: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for _ in 0..4 {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let done = this
                    .update(cx, |this, cx| {
                        if this.active_document().map(|document| document.id) != Some(id) {
                            return true;
                        }
                        let Some(document) = this.document(id) else {
                            return true;
                        };
                        let editor = document.editor.clone();
                        let (delta, offset) = {
                            let editor = editor.read(cx);
                            let Some(line_height) = editor.line_height() else {
                                return false;
                            };
                            let caret = editor.cursor();
                            let Some(caret_bounds) = editor.range_to_bounds(&(caret..caret)) else {
                                return false;
                            };
                            (
                                bibliography_top_shift(
                                    caret_bounds,
                                    editor.input_bounds().top(),
                                    line_height,
                                ),
                                editor.scroll_offset(),
                            )
                        };
                        if delta.abs() <= px(0.5) {
                            return true;
                        }
                        let mut offset = offset;
                        offset.y -= delta;
                        editor.update(cx, |editor, cx| editor.set_scroll_offset(offset, cx));
                        false
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
    }

    fn subscribe_editor(
        &mut self,
        editor: &Entity<EditorState>,
        #[cfg(editor_gpuikit)] preview: &Entity<TextViewState>,
        id: u64,
        cx: &mut Context<Self>,
    ) {
        #[cfg(editor_zorite)]
        cx.subscribe(
            editor,
            move |this, _, event: &EditorEvent, cx| match event {
                EditorEvent::OpenLink(target) => {
                    let target = target.as_ref();
                    let path = Path::new(target);
                    let resolved = if path.is_absolute() || target.contains("://") {
                        path.to_path_buf()
                    } else {
                        this.document(id)
                            .and_then(|document| document.path.as_deref())
                            .and_then(Path::parent)
                            .map(|parent| parent.join(path))
                            .unwrap_or_else(|| path.to_path_buf())
                    };
                    match this.open_external(&resolved) {
                        Ok(()) => cx.notify(),
                        Err(error) => this.set_status(error, cx),
                    };
                }
                EditorEvent::Changed | EditorEvent::SelectionChanged => {
                    this.follow_caret(id, cx);
                }
                _ => {}
            },
        )
        .detach();

        // The editor changes its content without a `Changed` event on undo and
        // redo. Observe every notification and compare with the saved text, so
        // the dirty marker and the status bar cannot go stale. This is also the
        // only hook the gpuikit editor needs: it owns its viewport.
        cx.observe(editor, move |this, _editor, cx| {
            let Some((text, cursor)) = this
                .document(id)
                .map(|document| (document.text(cx), document.cursor_position(cx)))
            else {
                return;
            };
            let Some(document) = this.document_mut(id) else {
                return;
            };
            let dirty = text != document.saved_text;
            let chars = text.chars().count();
            let digest = *blake3::hash(text.as_bytes()).as_bytes();
            let content_changed = digest != document.content_digest;
            let dirty_changed = dirty != document.dirty;
            let cursor_moved = cursor != document.cursor;
            document.dirty = dirty;
            document.chars = chars;
            document.cursor = cursor;
            document.content_digest = digest;
            // Cursor-only movements are cheap: keep them in memory and let the
            // next edit, save, or close journal them. Writing (and fsyncing) the
            // whole session on every arrow key is what made key repeat stutter.
            if content_changed {
                #[cfg(editor_gpuikit)]
                {
                    if let Some(document) = this.document_mut(id) {
                        document.refresh_preview(&text, cx);
                        document.pending_editor_scroll = false;
                        document.preview_scroll_target = None;
                    }
                    this.schedule_preview_realign(id, cx);
                }
                this.schedule_session_save(cx);
            }
            // Hover repaints and table affordances notify the editor too; they
            // do not change anything the rest of the window shows.
            if content_changed || cursor_moved || dirty_changed {
                cx.notify();
            }
            #[cfg(editor_gpuikit)]
            {
                let offset = this
                    .document(id)
                    .map(|document| f32::from(document.editor.read(cx).scroll_offset().y))
                    .unwrap_or(0.0);
                let landed = this.document_mut(id).is_some_and(|document| {
                    if !document.pending_editor_scroll {
                        return false;
                    }
                    // The first offset change is the viewport applying the
                    // scroll the preview asked for. Consume it without feeding
                    // it back; any later change is the user.
                    if (document.preview_source_offset - offset).abs() <= 0.5 {
                        return false;
                    }
                    document.pending_editor_scroll = false;
                    document.preview_source_offset = offset;
                    true
                });
                if !landed {
                    let changed = this.document(id).is_some_and(|document| {
                        (document.preview_source_offset - offset).abs() > 0.5
                    });
                    if changed {
                        if let Some(document) = this.document_mut(id) {
                            document.preview_source_offset = offset;
                        }
                        this.schedule_source_sync(id, cx);
                    }
                }
            }
        })
        .detach();

        #[cfg(editor_gpuikit)]
        {
            // The list does not notify its entity on wheel scrolls; this is
            // the reliable signal that the user drives the preview.
            let state = cx.weak_entity();
            let preview_for_handler = preview.clone();
            preview
                .read(cx)
                .list_state()
                .set_scroll_handler(move |_, _, cx| {
                    let state = state.clone();
                    let preview = preview_for_handler.clone();
                    cx.defer(move |cx| {
                        let _ = state.update(cx, |this, cx| {
                            this.preview_scrolled(id, &preview, cx);
                        });
                    });
                });

            // Scrollbar drags bypass the list's scroll handler; their
            // notifications are the only trace of a user-driven jump. Other
            // preview notifications (parsing, remeasure) must not move the
            // editor while the user types.
            cx.observe(preview, move |this, preview, cx| {
                let dragging = preview.read(cx).list_state().is_scrollbar_dragging();
                let was_dragging = this
                    .document(id)
                    .is_some_and(|document| document.preview_dragging);
                if let Some(document) = this.document_mut(id) {
                    document.preview_dragging = dragging;
                }
                if dragging || was_dragging {
                    this.preview_scrolled(id, &preview, cx);
                }
            })
            .detach();
        }
    }
}

/// The roundtrip host backed by [`AppState`]'s active document.
///
/// The agent holds this behind [`ragrig_gpui::Roundtrip`], so it never names
/// the editor backend.
pub struct AppStateHost {
    /// The application state that owns the documents.
    pub state: Entity<AppState>,
}

impl RoundtripHost for AppStateHost {
    fn active_document(&self, cx: &App) -> Option<EditorSnapshot> {
        let state = self.state.read(cx);
        state
            .active_document()
            .map(|document| document.snapshot(cx))
    }

    fn selection(&self, window: &mut Window, cx: &mut App) -> Option<SelectionSnapshot> {
        self.state.update(cx, |state, cx| {
            let document = state.active_document()?;
            let range = document.selection(window, cx)?;
            Some(SelectionSnapshot {
                document: document.snapshot(cx),
                range,
            })
        })
    }

    fn insert(&self, text: &str, window: &mut Window, cx: &mut App) -> bool {
        self.state.update(cx, |state, cx| {
            if state.active_document().is_none() {
                state.set_status("Open a document first", cx);
                return false;
            }
            state.insert_at_caret(text, window, cx);
            true
        })
    }

    fn bibliography(&self, cx: &App) -> Option<PathBuf> {
        self.state.read(cx).bibliography.clone()
    }

    fn open_at(&self, path: &Path, offset: usize, window: &mut Window, cx: &mut App) -> bool {
        self.state.update(cx, |state, cx| {
            if state.open_path(path, window, cx).is_err() {
                return false;
            }
            let Some(index) = state
                .documents
                .iter()
                .position(|document| document.path.as_deref() == Some(path))
            else {
                return false;
            };
            let id = state.documents[index].id;
            state.documents[index].reveal(offset, window, cx);
            state.active = index;
            state.follow_caret(id, cx);
            state.sync_active_preview(cx);
            state.persist_session(cx);
            cx.notify();
            true
        })
    }

    fn edit_reference(&self, key: &str, window: &mut Window, cx: &mut App) -> Result<(), String> {
        self.state.update(cx, |state, cx| {
            state.open_bibliography_entry(key, window, cx)
        })
    }

    fn apply_replacement(
        &self,
        target: ReplacementTarget,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> ReplacementOutcome {
        self.state.update(cx, |state, cx| {
            state.apply_replacement(target, text, window, cx)
        })
    }
}

/// Format a replacement as the protocol's fenced block, e.g. for copy actions.
pub fn replacement_block(range: Range<usize>, text: &str) -> String {
    format!(
        "```{} target=\"{}..{}\"\n{}\n```",
        MARKER_REPLACE, range.start, range.end, text
    )
}

/// Rebuild a container around replacement text (used by tests and copy actions).
pub fn preview_replacement(original: &str, range: Range<usize>, replacement: &str) -> String {
    replace_at(original, range, replacement)
}

/// Attach the bibliography path to a setup when the app configured one.
fn with_bibliography(
    setup: ragrig_gpui::RagSetup,
    bibliography: Option<PathBuf>,
) -> ragrig_gpui::RagSetup {
    let mut setup = setup;
    if setup.bibliography.is_none() {
        setup.bibliography = bibliography;
    }
    setup
}

/// The memory strategy whose short label is `label`.
fn memory_choice(label: &str) -> Option<MemoryChoice> {
    MemoryChoice::options()
        .iter()
        .map(|(choice, _)| *choice)
        .find(|choice| choice.label() == label)
}

/// The strategy after a status-bar switch, plus the strategy to restore next
/// time it is switched on.
fn toggled_memory(
    current: MemoryChoice,
    last_on: MemoryChoice,
    enabled: bool,
) -> (MemoryChoice, MemoryChoice) {
    if enabled {
        let next = if current == MemoryChoice::Off {
            last_on
        } else {
            current
        };
        (next, next)
    } else {
        let last = if current == MemoryChoice::Off {
            last_on
        } else {
            current
        };
        (MemoryChoice::Off, last)
    }
}

/// Give a workspace `Chat.md` precedence. Without one, keep a prompt chosen
/// by a config layer, or fall back to the compiled-in prompt.
fn apply_workspace_prompt(settings: &mut RagSettings, report: &WorkspaceReport) {
    if let Some(prompt) = &report.prompt {
        settings.system_prompt_path = workspace::PROMPT_FILE.to_owned();
        settings.system_prompt =
            fs::read_to_string(prompt).unwrap_or_else(|_| CHAT_PROMPT.to_owned());
    } else {
        let missing = settings.system_prompt_path.trim().is_empty()
            || settings.prompt_path().is_none_or(|path| !path.is_file());
        if missing {
            settings.system_prompt_path.clear();
            settings.system_prompt = CHAT_PROMPT.to_owned();
        }
    }
}

/// How far the caret's top edge sits below the desired two-line top margin.
///
/// Subtracting this from the scroll offset moves the caret up to that margin.
#[cfg(editor_gpuikit)]
fn bibliography_top_shift(
    caret: Bounds<Pixels>,
    viewport_top: Pixels,
    line_height: Pixels,
) -> Pixels {
    caret.top() - (viewport_top + line_height + line_height)
}

/// Whether `path` is the workspace bibliography.
fn is_bibliography_path(path: Option<&Path>, bibliography: Option<&Path>) -> bool {
    let Some((path, bibliography)) = path.zip(bibliography) else {
        return false;
    };
    path == bibliography
        || std::fs::canonicalize(path)
            .ok()
            .zip(std::fs::canonicalize(bibliography).ok())
            .is_some_and(|(path, bibliography)| path == bibliography)
}

fn with_kept_tabs(status: String, kept: usize) -> String {
    if kept == 0 {
        status
    } else {
        let tabs = if kept == 1 { "tab" } else { "tabs" };
        format!("{status} · {kept} unsaved {tabs} outside the workspace kept open")
    }
}

fn non_empty_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn gutter_width(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(80.0, 900.0)
    } else {
        360.0
    }
}

fn panel_width(value: f32, fallback: f32) -> f32 {
    let fallback = if fallback.is_finite() && fallback > 0.0 {
        fallback
    } else {
        340.0
    };
    if value.is_finite() {
        value.clamp(180.0, 1_600.0)
    } else {
        fallback.clamp(180.0, 1_600.0)
    }
}

/// The supported editor font-size range, in pixels.
pub const MIN_EDITOR_FONT_SIZE: f32 = 8.0;
/// The supported editor font-size range, in pixels.
pub const MAX_EDITOR_FONT_SIZE: f32 = 48.0;

/// Monospace families that are pleasant for long-form writing, best first.
const PREFERRED_EDITOR_FONTS: &[&str] = &[
    "JetBrains Mono",
    "Fira Mono",
    "Hack",
    "Cascadia Code",
    "Source Code Pro",
    "IBM Plex Mono",
    "Inconsolata",
    "Cousine",
    "Go Mono",
    "DejaVu Sans Mono",
];

/// A preferred installed monospace family, or the theme's monospace family.
fn preferred_editor_font(cx: &App) -> String {
    let available = cx.text_system().all_font_names();
    for preferred in PREFERRED_EDITOR_FONTS {
        if let Some(name) = available
            .iter()
            .find(|name| name.eq_ignore_ascii_case(preferred))
        {
            return name.clone();
        }
    }
    cx.theme().mono_font_family.to_string()
}

/// 150% of the theme's monospace size, so the first launch is comfortable.
fn default_editor_font_size(cx: &App) -> f32 {
    (f32::from(cx.theme().mono_font_size) * 1.5).clamp(MIN_EDITOR_FONT_SIZE, MAX_EDITOR_FONT_SIZE)
}

/// Whether a family name looks like a monospace face.
pub fn is_monospace_font(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "mono",
        "code",
        "hack",
        "courier",
        "consolas",
        "menlo",
        "monaco",
        "iosevka",
        "inconsolata",
        "cousine",
        "victor",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

/// The buffer line at the top of the editor's viewport.
///
/// `range_to_bounds` maps offsets above the visible layout to the first visible
/// line, so walking up from an estimate derived from the pixel scroll offset
/// finds the first fully or partially visible logical line without needing the
/// editor's display map. The estimate and the walk are both in wrapped display
/// rows, which is exactly what makes the preview jump with paragraphs instead
/// of drifting proportionally.
#[cfg(editor_gpuikit)]
fn source_top_line(editor: &EditorState) -> Option<usize> {
    let line_height = f32::from(editor.line_height()?);
    if line_height <= 0.0 {
        return None;
    }
    let first_top = editor.range_to_bounds(&(0..1))?.top();
    let text = editor.text();
    let line_count = text.lines_len();
    if line_count == 0 {
        return None;
    }

    let estimate = (-f32::from(editor.scroll_offset().y) / line_height).max(0.0);
    let mut line = if estimate.is_finite() {
        (estimate.floor() as usize).min(line_count - 1)
    } else {
        0
    };
    loop {
        let start = text.line_start_offset(line);
        if let Some(bounds) = editor.range_to_bounds(&(start..start + 1))
            && bounds.top() <= first_top + px(0.5)
        {
            break;
        }
        if line == 0 {
            break;
        }
        line -= 1;
    }
    Some(line)
}

/// The block at the top of the editor's viewport.
#[cfg(editor_gpuikit)]
pub(crate) fn source_top_block(document: &Document, cx: &App) -> Option<usize> {
    if document.blocks.is_empty() {
        return None;
    }
    let editor = document.editor.read(cx);
    let line = source_top_line(editor)?;
    let byte = editor.text().line_start_offset(line);
    Some(document.block_for_byte(byte))
}

/// Whether the answer carries replacement blocks. Kept here so both the chat
/// pane and tests reason about an answer the same way.
pub fn replacement_from_answer(answer: &str) -> Option<(String, Vec<Replacement>)> {
    Replacement::extract(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_bibliography_path_is_recognised() {
        let bibliography = Path::new("/tmp/refs.bib");
        assert!(is_bibliography_path(Some(bibliography), Some(bibliography)));
        assert!(!is_bibliography_path(
            Some(Path::new("/tmp/paper.md")),
            Some(bibliography)
        ));
        assert!(!is_bibliography_path(None, Some(bibliography)));
        assert!(!is_bibliography_path(Some(bibliography), None));
    }

    #[cfg(editor_gpuikit)]
    #[test]
    fn a_bibliography_caret_sits_two_lines_below_the_top() {
        let caret = gpui_kit::bounds(
            gpui_kit::point(px(4.), px(120.)),
            gpui_kit::size(px(200.), px(20.)),
        );
        assert_eq!(bibliography_top_shift(caret, px(0.), px(20.)), px(80.));

        let aligned = gpui_kit::bounds(
            gpui_kit::point(px(4.), px(40.)),
            gpui_kit::size(px(200.), px(20.)),
        );
        assert_eq!(bibliography_top_shift(aligned, px(0.), px(20.)), px(0.));
    }

    #[test]
    fn replacement_blocks_round_trip() {
        let answer = "Done.\n\n```scico:v1:replace target=\"2..4\"\nbetter\n```";
        let (prose, blocks) = replacement_from_answer(answer).expect("block");
        assert_eq!(prose, "Done.");
        assert_eq!(blocks[0].target, Some(2..4));
        assert_eq!(blocks[0].body, "better");
    }

    #[test]
    fn a_replacement_preview_is_char_indexed() {
        assert_eq!(preview_replacement("aé😀z", 1..3, "X"), "aXz");
    }

    #[test]
    fn a_copy_block_names_its_target() {
        assert_eq!(
            replacement_block(2..4, "better"),
            "```scico:v1:replace target=\"2..4\"\nbetter\n```"
        );
    }

    #[test]
    fn memory_toggle_restores_the_previous_strategy() {
        let (next, last) = toggled_memory(MemoryChoice::Summary, MemoryChoice::Rewrite, false);
        assert_eq!(next, MemoryChoice::Off);
        assert_eq!(last, MemoryChoice::Summary, "the last on-strategy is kept");

        let (next, last) = toggled_memory(MemoryChoice::Off, last, true);
        assert_eq!(next, MemoryChoice::Summary, "switching on restores it");
        assert_eq!(last, MemoryChoice::Summary);

        let (next, _) = toggled_memory(MemoryChoice::Transcript, MemoryChoice::Summary, true);
        assert_eq!(next, MemoryChoice::Transcript, "switching on stays put");
    }
}
