//! Application state: documents, workspace paths, and the RAG controller.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, SharedString, Window, point, px,
};
pub use ragrig_gpui::editor::ReplacementOutcome;
use ragrig_gpui::editor::{
    EditorSnapshot, EditorSurface, MARKER_REPLACE, Replacement, ReplacementTarget, RoundtripHost,
    SelectionSnapshot, replace_at, verified_bytes,
};
use ragrig_gpui::{ChatProvider, RagChanges, RagController, RagSettings};
use zorite_editor::{EditorEvent, SyntaxStyle};

use crate::document::{Document, line_column, read_document, title_for};
use crate::session::{SessionDocument, SessionState};
use crate::settings::PersistedSettings;
use crate::theme::syntax_style;

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
    /// Open documents, in tab order.
    pub documents: Vec<Document>,
    /// Index into [`Self::documents`].
    pub active: usize,
    /// Next document identity.
    next_document: u64,
    /// The editor workspace.
    pub workspace: Option<PathBuf>,
    /// The corpus folder indexed by the agent.
    pub corpus: Option<PathBuf>,
    /// The bibliography shown by the library.
    pub bibliography: Option<PathBuf>,
    /// Attach the active document to each question.
    pub document_attached: bool,
    /// Attach the bibliography to each question.
    pub library_attached: bool,
    /// Show the library pane.
    pub show_library: bool,
    /// Show the agent pane.
    pub show_agent: bool,
    /// Width of the library pane in pixels.
    pub library_width: f32,
    /// Width of the SciCo pane in pixels.
    pub agent_width: f32,
    /// Empty gutter on the right of the settings surface.
    pub settings_gutter: f32,
    /// Monotonic token that cancels an older caret-follow animation.
    scroll_generation: u64,
    /// Monotonic token that keeps only the newest debounced session write.
    session_generation: u64,
    /// Render raw Markdown instead of the live preview.
    pub raw_mode: bool,
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
    ) -> Self {
        let mut rag_values = RagSettings::default();
        persisted.apply_to(&mut rag_values);

        let workspace = std::env::var_os("SCISCRAPER_WORKSPACE")
            .map(PathBuf::from)
            .or_else(|| non_empty_path(&persisted.workspace))
            .or_else(|| non_empty_path(&rag_values.workspace))
            .or_else(|| std::env::current_dir().ok());
        let corpus = std::env::var_os("SCISCRAPER_CORPUS")
            .map(PathBuf::from)
            .or_else(|| non_empty_path(&persisted.corpus))
            .or_else(|| {
                workspace
                    .as_ref()
                    .map(|workspace| workspace.join(".ragrig/library"))
            })
            .or_else(|| workspace.clone());
        let bibliography = std::env::var_os("SCISCRAPER_BIB")
            .map(PathBuf::from)
            .or_else(|| non_empty_path(&persisted.bibliography))
            .or_else(|| non_empty_path(&rag_values.bibliography))
            .or_else(|| {
                workspace
                    .as_ref()
                    .map(|workspace| workspace.join(".ragrig/references.bib"))
            });

        if let Some(workspace) = &workspace {
            rag_values.workspace = workspace.display().to_string();
        }
        if let Some(bibliography) = &bibliography {
            rag_values.bibliography = bibliography.display().to_string();
        }

        let mut status = SharedString::from("Ready");
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
        let mut state = Self {
            settings: persisted,
            settings_path,
            session_path,
            last_session: restored_session.clone(),
            rag,
            applied_rag,
            controller,
            documents: Vec::new(),
            active: 0,
            next_document: 1,
            workspace,
            corpus,
            bibliography,
            document_attached,
            library_attached,
            show_library,
            show_agent,
            library_width,
            agent_width,
            settings_gutter,
            scroll_generation: 0,
            session_generation: 0,
            raw_mode,
            status,
            style,
        };

        let restore = std::env::var_os("SCISCRAPER_FILE")
            .map(PathBuf::from)
            .filter(|path| path.is_file());
        if let Some(path) = restore {
            if let Err(error) = state.open_path(&path, window, cx) {
                state.set_status(error, cx);
            }
        } else if let Some(session) = restored_session {
            state.restore_session(session, window, cx);
        } else {
            state.new_document(window, cx);
        }
        if let Some(error) = session_error {
            state.set_status(error, cx);
        } else if startup_status.starts_with("RAG settings") {
            state.set_status(startup_status, cx);
        }
        state.persist_session(cx);

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
        let mut title = format!("Untitled-{id}");
        let mut suffix = 1u64;
        while self
            .documents
            .iter()
            .any(|document| document.title.as_ref() == title.as_str())
        {
            suffix += 1;
            title = format!("Untitled-{id}-{suffix}");
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
        self.subscribe_editor(&document.editor, id, cx);
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        window.focus(
            &self.documents[self.active].editor.read(cx).focus_handle(cx),
            cx,
        );
        self.set_status("New document", cx);
        self.persist_session(cx);
        id
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
                        format!("Untitled-{id}")
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
            self.subscribe_editor(&document.editor, id, cx);
            document
                .editor
                .update(cx, |editor, cx| editor.set_cursor(snapshot.cursor, cx));
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
            raw_mode: self.raw_mode,
            library_width: self.library_width,
            agent_width: self.agent_width,
        }
    }

    /// Journal the window state atomically.
    ///
    /// Explicit actions (open, save, close, tab switch, layout) call this
    /// directly so they are durable immediately. Content edits go through
    /// [`Self::schedule_session_save`], which debounces them by 300 ms. A
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
            self.set_workspace(path.to_path_buf(), cx);
            return Ok(());
        }
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path.as_deref() == Some(path))
        {
            self.active = index;
            window.focus(&self.documents[index].editor.read(cx).focus_handle(cx), cx);
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
        self.subscribe_editor(&document.editor, id, cx);
        self.documents.push(document);
        self.active = self.documents.len() - 1;
        window.focus(
            &self.documents[self.active].editor.read(cx).focus_handle(cx),
            cx,
        );
        self.set_status(format!("Opened {}", path.display()), cx);
        self.persist_session(cx);
        Ok(())
    }

    /// Save the active document to its existing path.
    pub fn save_active(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let result = match self.active_document_mut() {
            Some(document) => document.save(cx),
            None => Err("no document is open".to_owned()),
        };
        match &result {
            Ok(()) => {
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
            Ok(()) => self.persist_session(cx),
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
        let Some(document) = self.active_document() else {
            return;
        };
        if document.dirty {
            let title = document.title.clone();
            self.set_status(
                format!("{title} has unsaved changes — save it before closing"),
                cx,
            );
            return;
        }
        self.documents.remove(self.active);
        if self.documents.is_empty() {
            self.active = 0;
            // The caller supplies the window in the normal path; a closed last
            // tab leaves an empty state rather than creating an editor here.
        } else {
            self.active = self.active.min(self.documents.len() - 1);
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
            .map(|document| (document.id, document.editor.clone()))
        else {
            return;
        };
        self.active = index;
        window.focus(&editor.read(cx).focus_handle(cx), cx);
        self.persist_session(cx);
        self.follow_caret(id, cx);
        cx.notify();
    }

    /// Insert text at the active document's selection or caret.
    pub fn insert_at_caret(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.active_document() else {
            self.set_status("Open a document first", cx);
            return;
        };
        document.insert(text, window, cx);
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
        self.settings.raw_mode = raw;
        self.persist(cx);
        self.persist_session(cx);
        self.set_status(if raw { "Raw Markdown" } else { "Live preview" }, cx);
    }

    /// Make `path` the editor and RAG workspace.
    ///
    /// The path is stored even while it does not exist yet, so the settings
    /// input can be typed freely; derived folders and the agent rebuild wait
    /// until it is a real directory.
    pub fn set_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let is_dir = path.is_dir();
        self.workspace = Some(path.clone());
        if is_dir {
            self.corpus = Some(path.join(".ragrig/library"));
            self.bibliography = Some(path.join(".ragrig/references.bib"));
        }
        let bibliography = self
            .bibliography
            .as_ref()
            .map(|bibliography| bibliography.display().to_string());
        let workspace = path.display().to_string();
        self.rag.update(cx, |settings, _| {
            settings.workspace = workspace;
            if let Some(bibliography) = &bibliography {
                settings.bibliography = bibliography.clone();
            }
        });
        if is_dir {
            self.apply_rag(cx);
            self.set_status(format!("Workspace {}", path.display()), cx);
        } else {
            self.persist(cx);
            self.set_status(format!("'{}' is not a folder yet", path.display()), cx);
        }
    }

    /// Rebuild the agent from the current settings.
    pub fn apply_rag(&mut self, cx: &mut Context<Self>) {
        match self.rag.read(cx).to_setup() {
            Ok(setup) => {
                let setup = with_bibliography(setup, self.bibliography.clone());
                self.controller
                    .update(cx, |controller, cx| controller.apply(setup, cx));
                self.applied_rag = self.rag.read(cx).clone();
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
        self.settings.library_width = self.library_width;
        self.settings.agent_width = self.agent_width;
        self.settings.settings_gutter = self.settings_gutter;
        self.settings.raw_mode = self.raw_mode;
        if let Err(error) = self.settings.save(&self.settings_path) {
            self.status = error.into();
        }
    }

    /// Apply the first replacement block of a finished answer.
    pub fn apply_replacement(
        &mut self,
        target: ReplacementTarget,
        text: &str,
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
            document.replace(bytes, text, cx);
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
    /// The editor auto-grows; this is the host scroll view. One frame is
    /// allowed for the keystroke's layout, then the caret bounds are read. If
    /// the scroll offset moved while waiting, the user is scrolling: abort
    /// instead of fighting the gesture. Each call bumps a generation, so key
    /// repeat retargets the follow rather than stacking scroll tasks.
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

    fn subscribe_editor(
        &mut self,
        editor: &Entity<zorite_editor::EditorState>,
        id: u64,
        cx: &mut Context<Self>,
    ) {
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
        // the dirty marker and the status bar cannot go stale.
        cx.observe(editor, move |this, editor, cx| {
            let Some(document) = this.document_mut(id) else {
                return;
            };
            let (dirty, chars, cursor, digest) = {
                let editor = editor.read(cx);
                let text = editor.text();
                (
                    text != document.saved_text,
                    text.chars().count(),
                    line_column(text, editor.cursor()),
                    *blake3::hash(text.as_bytes()).as_bytes(),
                )
            };
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
                this.schedule_session_save(cx);
            }
            // Hover repaints and table affordances notify the editor too; they
            // do not change anything the rest of the window shows.
            if content_changed || cursor_moved || dirty_changed {
                cx.notify();
            }
        })
        .detach();
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

    fn apply_replacement(
        &self,
        target: ReplacementTarget,
        text: &str,
        cx: &mut App,
    ) -> ReplacementOutcome {
        self.state
            .update(cx, |state, cx| state.apply_replacement(target, text, cx))
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

/// Whether the answer carries replacement blocks. Kept here so both the chat
/// pane and tests reason about an answer the same way.
pub fn replacement_from_answer(answer: &str) -> Option<(String, Vec<Replacement>)> {
    Replacement::extract(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_blocks_round_trip() {
        let answer = "Done.\n\n```sciscraper:v1:replace target=\"2..4\"\nbetter\n```";
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
            "```sciscraper:v1:replace target=\"2..4\"\nbetter\n```"
        );
    }
}
