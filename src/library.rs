//! The reference and corpus pane.

use std::path::{Path, PathBuf};

use gpui_kit::{
    AnyElement, AppContext as _, ClipboardItem, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _,
    PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window,
    assets::IconName,
    component::{
        ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::{Input, InputEvent, InputState},
        v_flex,
    },
    div,
    prelude::FluentBuilder as _,
    px,
};
use ragrig_gpui::prelude::*;

use crate::agent::AgentPane;
use crate::state::AppState;

/// What the library asks the host to do beyond updating shared state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryEvent {
    /// The user asked for the settings surface.
    OpenSettings,
}

/// The left dock: bibliography rows, corpus actions, and status.
pub struct LibraryPane {
    state: Entity<AppState>,
    agent: Entity<AgentPane>,
    entries: Vec<ReferenceEntry>,
    bibliography: Option<PathBuf>,
    report: Option<ReferenceReport>,
    filter: Entity<InputState>,
    selected: ReferenceSelection,
    status: SharedString,
    index_after_references: bool,
    last_paths: (Option<PathBuf>, Option<PathBuf>),
    references_revision: u64,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl LibraryPane {
    /// Create the pane and request the bibliography.
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: Entity<AppState>,
        agent: Entity<AgentPane>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter references…"));
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                cx.notify();
            }
        }));
        let controller = state.read(cx).controller.clone();
        subscriptions.push(cx.subscribe(&controller, |this, _, event: &RagEvent, cx| {
            this.on_rag_event(event.clone(), cx);
        }));
        subscriptions.push(cx.observe(&state, |this, state, cx| {
            let revision = state.read(cx).references_revision;
            let paths_changed = this.sync_paths(cx);
            if revision != this.references_revision {
                this.references_revision = revision;
                if !paths_changed {
                    this.refresh_references(cx);
                }
            }
            cx.notify();
        }));

        let (corpus, bibliography, references_revision) = {
            let state = state.read(cx);
            (
                state.corpus.clone(),
                state.bibliography.clone(),
                state.references_revision,
            )
        };
        let last_paths = (corpus.clone(), bibliography.clone());
        let mut this = Self {
            state,
            agent,
            entries: Vec::new(),
            bibliography,
            report: None,
            filter,
            selected: ReferenceSelection::default(),
            status: "Choose a corpus folder, then Index.".into(),
            index_after_references: false,
            last_paths,
            references_revision,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.refresh_references(cx);
        this
    }

    fn folder(&self, cx: &Context<Self>) -> PathBuf {
        let state = self.state.read(cx);
        state
            .corpus
            .clone()
            .or_else(|| state.workspace.clone())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn set_status(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = status.into();
        cx.notify();
    }

    fn sync_paths(&mut self, cx: &mut Context<Self>) -> bool {
        let (corpus, bibliography) = {
            let state = self.state.read(cx);
            (state.corpus.clone(), state.bibliography.clone())
        };
        let next = (corpus, bibliography);
        if next != self.last_paths {
            self.last_paths = next.clone();
            self.bibliography = next.1;
            self.refresh_references(cx);
            return true;
        }
        false
    }

    fn refresh_references(&mut self, cx: &mut Context<Self>) {
        let folder = self.folder(cx);
        let controller = self.state.read(cx).controller.clone();
        controller.update(cx, |controller, cx| {
            controller.refresh_references(folder, cx)
        });
        self.set_status("Reloading references…", cx);
    }

    fn index_corpus(&mut self, cx: &mut Context<Self>) {
        let folder = self.folder(cx);
        if !folder.is_dir() {
            self.set_status(format!("'{}' is not a folder", folder.display()), cx);
            return;
        }
        let name = folder
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("corpus")
            .to_owned();
        let controller = self.state.read(cx).controller.clone();
        match controller.update(cx, |controller, cx| {
            controller.sync_corpus(name.clone(), folder.clone(), cx)
        }) {
            Ok(_) => self.set_status(format!("Indexing {}…", folder.display()), cx),
            Err(error) => self.set_status(error, cx),
        }
        self.state.update(cx, |state, cx| {
            state.corpus = Some(folder.clone());
            let entry = format!("{name}={}", folder.display());
            state.rag.update(cx, |settings, _| {
                settings.corpus_dirs.retain(|existing| {
                    existing
                        .split_once('=')
                        .map(|(existing_name, _)| existing_name != name)
                        .unwrap_or(true)
                });
                settings.corpus_dirs.push(entry);
            });
            state.persist(cx);
        });
    }

    fn extract_references(&mut self, index_after: bool, cx: &mut Context<Self>) {
        let folder = self.folder(cx);
        if !folder.is_dir() {
            self.set_status(format!("'{}' is not a folder", folder.display()), cx);
            return;
        }
        let options = self.state.read(cx).rag.read(cx).reference_options();
        let controller = self.state.read(cx).controller.clone();
        match controller.update(cx, |controller, cx| {
            controller.extract_references(folder, options, cx)
        }) {
            Ok(_) => {
                self.index_after_references = index_after;
                self.set_status("Parsing references with GROBID…", cx);
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn add_pdfs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(corpus) = self.state.read(cx).corpus.clone() else {
            self.set_status("Choose a corpus folder first", cx);
            return;
        };
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add PDFs to the library".into()),
        });
        let this = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let corpus = corpus.clone();
            let result = cx
                .background_spawn(async move { copy_pdfs(&paths, &corpus) })
                .await;
            let _ = this.update_in(cx, |this, _window, cx| {
                this.on_pdfs_copied(result, cx);
            });
        })
        .detach();
    }

    fn on_pdfs_copied(
        &mut self,
        result: Result<(Vec<PathBuf>, Vec<String>), String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok((copied, errors)) => {
                let mut message = if copied.is_empty() {
                    "Nothing new to add".to_owned()
                } else {
                    format!("Copied {} PDF(s) into the corpus", copied.len())
                };
                if !errors.is_empty() {
                    message.push_str(&format!(" · {} skipped", errors.len()));
                }
                self.set_status(message, cx);
                if !copied.is_empty() {
                    self.extract_references(true, cx);
                }
            }
            Err(error) => self.set_status(error, cx),
        }
    }

    fn on_rag_event(&mut self, event: RagEvent, cx: &mut Context<Self>) {
        match event {
            RagEvent::ReferencesLoaded {
                entries,
                bibliography,
            } => {
                self.selected.retain_existing(&entries);
                self.entries = entries;
                self.bibliography = Some(bibliography.clone());
                self.set_status(
                    format!(
                        "{} references · {}",
                        self.entries.len(),
                        bibliography.display()
                    ),
                    cx,
                );
            }
            RagEvent::ReferencesProgress { progress, .. } => {
                self.set_status(progress.summary(), cx);
            }
            RagEvent::ReferencesFinished { report, .. } => {
                self.report = Some(report.clone());
                self.set_status(report.summary(), cx);
                if self.index_after_references {
                    self.index_after_references = false;
                    self.index_corpus(cx);
                }
            }
            RagEvent::ReferencesFailed { message, .. } => {
                self.set_status(message.clone(), cx);
                if self.index_after_references {
                    self.index_after_references = false;
                    self.set_status(format!("{message} — indexing the PDFs anyway"), cx);
                    self.index_corpus(cx);
                }
            }
            RagEvent::ReferencesStopped { .. } => {
                self.set_status("Reference extraction stopped", cx);
                if self.index_after_references {
                    self.index_after_references = false;
                    self.index_corpus(cx);
                }
            }
            RagEvent::SyncProgress { progress, .. } => self.set_status(progress.summary(), cx),
            RagEvent::SyncFinished { summary, .. } => {
                self.set_status(
                    format!(
                        "Indexed {} document(s), {} chunk(s)",
                        summary.documents, summary.chunks
                    ),
                    cx,
                );
            }
            RagEvent::SyncStopped { .. } => self.set_status("Indexing stopped", cx),
            RagEvent::SyncFailed { message, .. } => self.set_status(message, cx),
            RagEvent::AgentFailed { message } => self.set_status(message, cx),
            RagEvent::AgentReady(_) => {
                // An Apply in Settings may have changed the bibliography; the
                // worker now resolves against the new file.
                self.refresh_references(cx);
            }
            RagEvent::Notice(message) if !message.is_empty() => {
                self.set_status(message, cx);
            }
            _ => {}
        }
    }

    fn insert_citation(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let selection = self.selected.entries(&self.entries);
        let text = if selection.len() > 1 && selection.iter().any(|entry| entry.key == key) {
            let keys = selection
                .iter()
                .map(|entry| format!("@{}", entry.key))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{keys}]")
        } else {
            format!("@{key}")
        };
        let inserted = self
            .agent
            .update(cx, |agent, cx| agent.insert_text(&text, window, cx));
        if inserted {
            self.set_status(format!("Inserted {text}"), cx);
        }
    }

    /// Insert every selected reference as one citation at the caret.
    fn insert_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries: Vec<ReferenceEntry> = self
            .selected
            .entries(&self.entries)
            .into_iter()
            .cloned()
            .collect();
        if entries.is_empty() {
            return;
        }
        let keys = entries
            .iter()
            .map(|entry| format!("@{}", entry.key))
            .collect::<Vec<_>>();
        let text = if keys.len() == 1 {
            keys[0].clone()
        } else {
            format!("[{}]", keys.join(", "))
        };
        let inserted = self
            .agent
            .update(cx, |agent, cx| agent.insert_text(&text, window, cx));
        if inserted {
            self.set_status(format!("Inserted {text}"), cx);
        }
    }

    fn toggle_selection(&mut self, key: String, cx: &mut Context<Self>) {
        self.selected.toggle(key);
        cx.notify();
    }

    fn attach_selected(&mut self, cx: &mut Context<Self>) {
        let entries: Vec<ReferenceEntry> = self
            .selected
            .entries(&self.entries)
            .into_iter()
            .cloned()
            .collect();
        let mut files = Vec::new();
        for entry in &entries {
            if let Some(path) = &entry.path {
                files.push((format!("@{}", entry.key), path.clone()));
            }
        }
        let count = files.len();
        self.agent
            .update(cx, |agent, cx| agent.attach_files(files, cx));
        self.selected.clear();
        if count == 0 {
            self.set_status("The selected rows have no PDF", cx);
        } else {
            self.set_status(
                format!("Attached {count} paper(s) to the next question"),
                cx,
            );
        }
    }
}

impl LibraryPane {
    /// Open the bibliography at this key, or focus it where already open.
    fn edit_in_bibtex(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.clone();
        let result = state.update(cx, |state, cx| {
            state.open_bibliography_entry(&key, window, cx)
        });
        match result {
            Ok(()) => self.set_status(format!("Editing @{key} in the bibliography"), cx),
            Err(error) => self.set_status(error, cx),
        }
    }
}

impl Render for LibraryPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let controller = self.state.read(cx).controller.clone();
        let (ingesting, busy, extracting, progress) = {
            let controller = controller.read(cx);
            (
                controller.is_ingesting(),
                controller.is_busy(),
                controller.is_extracting_references(),
                controller.reference_progress().cloned(),
            )
        };
        let folder = self.folder(cx);
        let filter = self.filter.read(cx).value().trim().to_lowercase();
        let selected_entries: Vec<ReferenceEntry> = self
            .selected
            .entries(&self.entries)
            .into_iter()
            .cloned()
            .collect();

        let add_pdfs = Button::new("library-add-pdfs")
            .label("Add PDFs")
            .icon(IconName::FolderPlus)
            .small()
            .outline()
            .disabled(extracting)
            .tooltip("Copy PDFs into the corpus, parse them, and index them")
            .on_click(cx.listener(|this, _, window, cx| this.add_pdfs(window, cx)));
        let grobid = Button::new("library-grobid")
            .label("GROBID")
            .icon(IconName::BookOpen)
            .small()
            .outline()
            .disabled(extracting)
            .tooltip("Parse this folder's PDFs into the bibliography")
            .on_click(cx.listener(|this, _, _, cx| this.extract_references(false, cx)));
        let index = Button::new("library-index")
            .label(if ingesting { "Stop" } else { "Index" })
            .icon(if ingesting {
                IconName::Square
            } else {
                IconName::FolderOpen
            })
            .small()
            .primary()
            .disabled(busy && !ingesting)
            .tooltip("Incrementally index the corpus folder")
            .on_click(cx.listener(|this, _, _, cx| {
                let controller = this.state.read(cx).controller.clone();
                if controller.read(cx).is_ingesting() {
                    controller.update(cx, |controller, cx| controller.stop_sync(cx));
                    this.set_status("Stopping indexing…", cx);
                } else {
                    this.index_corpus(cx);
                }
            }));
        let refresh = Button::new("library-refresh")
            .icon(IconName::RefreshCw)
            .small()
            .ghost()
            .tooltip("Reload the bibliography")
            .on_click(cx.listener(|this, _, _, cx| this.refresh_references(cx)));
        let settings = Button::new("library-settings")
            .icon(IconName::Settings)
            .small()
            .ghost()
            .tooltip("Settings")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(LibraryEvent::OpenSettings)));

        let mut body = v_flex().gap_2();
        body = body.child(
            h_flex()
                .gap_1()
                .child(add_pdfs)
                .child(grobid)
                .child(index)
                .child(refresh)
                .child(div().flex_1())
                .child(settings),
        );
        body = body.child(
            div().text_xs().text_color(theme.muted_foreground).child(
                folder
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| folder.display().to_string()),
            ),
        );
        body = body.child(
            Input::new(&self.filter)
                .small()
                .bordered(true)
                .cleanable(true),
        );

        if let Some(progress) = progress {
            body = body.child(
                v_flex()
                    .gap_1()
                    .p_2()
                    .rounded(theme.radius)
                    .bg(theme.muted.opacity(0.35))
                    .child(div().text_sm().child(progress.summary())),
            );
        }
        if let Some(report) = &self.report {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("Last run: {}", report.summary())),
            );
            for failure in report.failed.iter().take(5) {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(failure.clone()),
                );
            }
        }

        if !selected_entries.is_empty() {
            let this = cx.entity();
            body = body.child(ReferenceSelectionBar::new(selected_entries).on_action(
                move |action, entries, window, cx| match action {
                    ReferenceAction::Insert => {
                        this.update(cx, |this, cx| this.insert_selected(window, cx));
                    }
                    ReferenceAction::CopyKeys | ReferenceAction::CopyPaths => {
                        this.update(cx, |this, cx| {
                            let count = entries.len();
                            this.set_status(format!("Copied {count} reference value(s)"), cx);
                        });
                    }
                    ReferenceAction::Attach => {
                        this.update(cx, |this, cx| this.attach_selected(cx));
                    }
                    ReferenceAction::Clear => {
                        this.update(cx, |this, cx| {
                            this.selected.clear();
                            cx.notify();
                        });
                    }
                },
            ));
        }

        if self.entries.is_empty() {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if self.bibliography.is_some() {
                        "No references yet. Press GROBID to parse this folder's PDFs, or add PDFs above."
                    } else {
                        "No bibliography configured. Choose a workspace in Settings."
                    }),
            );
        } else {
            let mut shown = 0usize;
            for (index, entry) in self.entries.iter().enumerate() {
                let haystack = format!(
                    "{} {} {} {} {}",
                    entry.key, entry.authors, entry.year, entry.title, entry.source
                )
                .to_lowercase();
                if !filter.is_empty() && !haystack.contains(&filter) {
                    continue;
                }
                shown += 1;
                let key = entry.key.clone();
                let row_key = key.clone();
                let edit_key = key.clone();
                let is_selected = self.selected.contains(&key);
                let path = entry.path.clone();
                let title = entry.display_title().to_owned();
                let meta = entry.meta_line();
                let key_label = entry.copy_key_text();

                let copy_key = {
                    let key = key.clone();
                    Button::new(ElementId::Name(format!("library-copy-key-{index}").into()))
                        .icon(IconName::Copy)
                        .xsmall()
                        .ghost()
                        .tooltip("Copy @key")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            cx.write_to_clipboard(ClipboardItem::new_string(format!("@{key}")));
                            this.set_status(format!("Copied @{key}"), cx);
                        }))
                };
                let copy_path = match path.clone() {
                    Some(path) => {
                        Button::new(ElementId::Name(format!("library-copy-path-{index}").into()))
                            .icon(IconName::FileText)
                            .xsmall()
                            .ghost()
                            .tooltip("Copy path")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    path.display().to_string(),
                                ));
                                this.set_status(format!("Copied {}", path.display()), cx);
                            }))
                    }
                    None => {
                        Button::new(ElementId::Name(format!("library-copy-path-{index}").into()))
                            .icon(IconName::FileText)
                            .xsmall()
                            .ghost()
                            .disabled(true)
                            .tooltip("No PDF path")
                    }
                };
                let open_paper = match path {
                    Some(path) => {
                        Button::new(ElementId::Name(format!("library-open-{index}").into()))
                            .icon(IconName::FolderOpen)
                            .xsmall()
                            .ghost()
                            .tooltip("Open with the configured viewer")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                let result = this.state.read(cx).open_external(&path);
                                match result {
                                    Ok(()) => {
                                        this.set_status(format!("Opened {}", path.display()), cx)
                                    }
                                    Err(error) => this.set_status(error, cx),
                                }
                            }))
                    }
                    None => Button::new(ElementId::Name(format!("library-open-{index}").into()))
                        .icon(IconName::FolderOpen)
                        .xsmall()
                        .ghost()
                        .disabled(true)
                        .tooltip("No PDF path"),
                };
                let edit_bib =
                    Button::new(ElementId::Name(format!("library-edit-bib-{index}").into()))
                        .icon(IconName::Pencil)
                        .xsmall()
                        .ghost()
                        .tooltip("Edit in BibTeX")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.edit_in_bibtex(edit_key.clone(), window, cx);
                        }));
                let select = Button::new(ElementId::Name(format!("library-select-{index}").into()))
                    .icon(if is_selected {
                        IconName::Check
                    } else {
                        IconName::Plus
                    })
                    .xsmall()
                    .ghost()
                    .selected(is_selected)
                    .tooltip(if is_selected { "Deselect" } else { "Select" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_selection(key.clone(), cx);
                    }));

                let row = h_flex()
                    .id(ElementId::Name(format!("library-row-{index}").into()))
                    .group("library-row")
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(if is_selected {
                        theme.primary.opacity(0.55)
                    } else {
                        theme.border
                    })
                    .when(is_selected, |row| row.bg(theme.primary.opacity(0.10)))
                    .hover(|row| row.bg(theme.muted.opacity(0.28)))
                    .cursor_pointer()
                    .child(select)
                    .child(
                        div()
                            .w(px(3.))
                            .h(px(30.))
                            .flex_none()
                            .rounded_full()
                            .bg(theme.primary.opacity(if is_selected { 1.0 } else { 0.6 })),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_xs()
                                            .text_color(theme.primary)
                                            .child(key_label),
                                    )
                                    .when(!meta.is_empty(), |row| {
                                        row.child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(meta),
                                        )
                                    }),
                            )
                            .child(div().min_w_0().truncate().text_sm().child(title)),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .opacity(0.45)
                            .group_hover("library-row", |row| row.opacity(1.0))
                            .child(edit_bib)
                            .child(copy_key)
                            .child(copy_path)
                            .child(open_paper),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_citation(row_key.clone(), window, cx);
                    }));
                body = body.child(row);
            }
            if !filter.is_empty() {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("{shown} of {} references", self.entries.len())),
                );
            }
        }

        body = body.child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(self.status.clone()),
        );

        let footer: AnyElement = div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child("Click a row to insert @key; + selects for bulk actions.")
            .into_any_element();

        SidePanel::new("library-panel", "Library")
            .subtitle("References and corpus")
            .child(body)
            .footer(footer)
    }
}

impl EventEmitter<LibraryEvent> for LibraryPane {}

impl Focusable for LibraryPane {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Copy PDFs into `corpus`, skipping identical names. Returns the copies and
/// human-readable skip reasons.
fn copy_pdfs(paths: &[PathBuf], corpus: &Path) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    std::fs::create_dir_all(corpus)
        .map_err(|error| format!("cannot create '{}': {error}", corpus.display()))?;
    let mut copied = Vec::new();
    let mut errors = Vec::new();
    for source in paths {
        if !source.is_file() {
            errors.push(format!("{}: not a file", source.display()));
            continue;
        }
        if source
            .extension()
            .and_then(|extension| extension.to_str())
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("pdf"))
        {
            errors.push(format!("{}: only PDFs are ingested", source.display()));
            continue;
        }
        let Some(name) = source.file_name() else {
            errors.push(format!("{}: no file name", source.display()));
            continue;
        };
        let mut target = corpus.join(name);
        if target.exists() {
            let same = std::fs::metadata(&target)
                .ok()
                .zip(std::fs::metadata(source).ok())
                .is_some_and(|(target, source)| target.len() == source.len());
            if same {
                errors.push(format!("{}: already in the corpus", name.to_string_lossy()));
                continue;
            }
            let stem = Path::new(name)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "paper".to_owned());
            let mut number = 2u32;
            loop {
                let candidate = corpus.join(format!("{stem}-{number}.pdf"));
                if !candidate.exists() {
                    target = candidate;
                    break;
                }
                number += 1;
            }
        }
        match std::fs::copy(source, &target) {
            Ok(_) => copied.push(target),
            Err(error) => errors.push(format!("{}: {error}", source.display())),
        }
    }
    Ok((copied, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pdfs_are_copied_and_name_clashes_get_a_suffix() {
        let dir = std::env::temp_dir().join(format!(
            "sciscraper-gpui-copy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let corpus = dir.join("corpus");
        let source = dir.join("source");
        std::fs::create_dir_all(&source).expect("source dir");
        let paper = source.join("paper.pdf");
        std::fs::write(&paper, b"%PDF-1").expect("paper");
        let note = source.join("note.txt");
        std::fs::write(&note, b"text").expect("note");

        let (copied, errors) = copy_pdfs(&[paper.clone(), note], &corpus).expect("copy");
        assert_eq!(copied.len(), 1);
        assert_eq!(errors.len(), 1);
        assert!(corpus.join("paper.pdf").is_file());

        // A different paper with the same name gets a suffix instead of
        // overwriting the first.
        std::fs::write(&paper, b"%PDF-2-longer").expect("rewrite");
        let (copied, _) = copy_pdfs(&[paper], &corpus).expect("copy again");
        assert_eq!(copied.len(), 1);
        assert!(corpus.join("paper-2.pdf").is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
