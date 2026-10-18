//! Application state: documents, workspace paths, and the RAG controller.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui_kit::{AppContext as _, Context, Entity, Focusable as _, SharedString, Window};
use ragrig_gpui::{ChatProvider, RagController, RagSettings};
use zorite_editor::{EditorEvent, SyntaxStyle};

use crate::document::{Document, line_column, read_document, title_for};
use crate::roundtrip::{Replacement, replace_at, verified_bytes};
use crate::settings::PersistedSettings;
use crate::theme::syntax_style;

/// The outcome of trying to write an agent replacement into a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplacementOutcome {
    /// The passage matched its digest and was replaced.
    Applied {
        /// A transcript-ready summary.
        message: String,
    },
    /// The passage changed (or the tab vanished); offer the text instead.
    Offered {
        /// The replacement text.
        text: String,
        /// Why it was not applied.
        reason: String,
    },
    /// There was nothing to write.
    Empty,
}

/// Shared editor and workspace state.
pub struct AppState {
    /// Persisted, non-RAG settings.
    pub settings: PersistedSettings,
    /// Where `settings` is written.
    pub settings_path: PathBuf,
    /// The editable Ragrig settings.
    pub rag: Entity<RagSettings>,
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
        let controller = cx.new(|cx| {
            RagController::new(with_bibliography(setup, bibliography.clone()), cx)
                .expect("the RAG worker thread must start")
        });

        let style = syntax_style(cx);
        let startup_status = status.clone();
        let document_attached = persisted.document_attached;
        let library_attached = persisted.library_attached;
        let show_library = persisted.show_library;
        let show_agent = persisted.show_agent;
        let raw_mode = persisted.raw_mode;
        let mut state = Self {
            settings: persisted,
            settings_path,
            rag,
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
        } else {
            state.new_document(window, cx);
            if startup_status.starts_with("RAG settings") {
                state.set_status(startup_status, cx);
            }
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
        let title = format!("Untitled-{id}");
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
        id
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
            }
            Err(error) => self.set_status(error.clone(), cx),
        }
        result
    }

    /// Save the active document under a new path.
    pub fn save_active_as(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Result<(), String> {
        let result = match self.active_document_mut() {
            Some(document) => document.save_as(path, cx),
            None => Err("no document is open".to_owned()),
        };
        match &result {
            Ok(()) => {
                let name = self
                    .active_document()
                    .map(|document| document.title.clone())
                    .unwrap_or_default();
                self.set_status(format!("Saved {name}"), cx);
            }
            Err(error) => self.set_status(error.clone(), cx),
        }
        result
    }

    /// Close the active tab. A dirty document is kept until saved.
    pub fn close_active(&mut self, cx: &mut Context<Self>) {
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
        }
        cx.notify();
    }

    /// Select the tab at `index`.
    pub fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(document) = self.documents.get(index) {
            self.active = index;
            window.focus(&document.editor.read(cx).focus_handle(cx), cx);
            cx.notify();
        }
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
                self.persist(cx);
                self.set_status("RAG agent rebuilt", cx);
            }
            Err(error) => self.set_status(format!("Settings: {error}"), cx),
        }
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
        self.settings.raw_mode = self.raw_mode;
        if let Err(error) = self.settings.save(&self.settings_path) {
            self.status = error.into();
        }
    }

    /// Apply the first replacement block of a finished answer.
    pub fn apply_replacement(
        &mut self,
        tab_id: u64,
        range: Range<usize>,
        guard: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> ReplacementOutcome {
        if text.is_empty() {
            return ReplacementOutcome::Empty;
        }
        let (title, current) = {
            let Some(document) = self.document(tab_id) else {
                return ReplacementOutcome::Offered {
                    text: text.to_owned(),
                    reason: "the document was closed".to_owned(),
                };
            };
            (document.title.clone(), document.text(cx))
        };
        let total = current.chars().count();
        let start = range.start.min(total);
        let end = range.end.min(total).max(start);
        let Some(bytes) = verified_bytes(&current, start..end, guard) else {
            return ReplacementOutcome::Offered {
                text: text.to_owned(),
                reason: "the marked passage changed while the agent was writing".to_owned(),
            };
        };
        if let Some(document) = self.document(tab_id) {
            document.replace(bytes, text, cx);
        }
        if let Some(document) = self.document_mut(tab_id) {
            document.dirty = true;
        }
        let old_chars = end.saturating_sub(start);
        let delta = text.chars().count() as isize - old_chars as isize;
        let sign = if delta >= 0 { "+" } else { "−" };
        let message = format!(
            "Replacement applied to {title}: −{old_chars} / {sign}{} chars — Ctrl+Z undoes it.",
            delta.unsigned_abs()
        );
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

    fn subscribe_editor(
        &mut self,
        editor: &Entity<zorite_editor::EditorState>,
        id: u64,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(editor, move |this, _, event: &EditorEvent, cx| {
            if let EditorEvent::OpenLink(target) = event {
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
                match open::that(&resolved) {
                    Ok(()) => cx.notify(),
                    Err(error) => this.set_status(
                        format!("Could not open {}: {error}", resolved.display()),
                        cx,
                    ),
                };
            }
        })
        .detach();

        // The editor changes its content without a `Changed` event on undo and
        // redo. Observe every notification and compare with the saved text, so
        // the dirty marker and the status bar cannot go stale.
        cx.observe(editor, move |this, editor, cx| {
            let Some(document) = this.document_mut(id) else {
                return;
            };
            let (dirty, chars, cursor) = {
                let editor = editor.read(cx);
                let text = editor.text();
                (
                    text != document.saved_text,
                    text.chars().count(),
                    line_column(text, editor.cursor()),
                )
            };
            document.dirty = dirty;
            document.chars = chars;
            document.cursor = cursor;
            cx.notify();
        })
        .detach();
    }
}

/// Format a replacement as the protocol's fenced block, e.g. for copy actions.
pub fn replacement_block(range: Range<usize>, text: &str) -> String {
    format!(
        "```{} target=\"{}..{}\"\n{}\n```",
        crate::roundtrip::MARKER_REPLACE,
        range.start,
        range.end,
        text
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
