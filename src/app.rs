//! The application shell: toolbar, tabs, editor, panes, and status bar.

use std::path::PathBuf;
use std::rc::Rc;

#[cfg(feature = "zorite")]
use gpui_kit::ScrollHandle;
#[cfg(feature = "zorite")]
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, PathPromptOptions,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    actions,
    assets::IconName,
    component::{
        ActiveTheme as _, ResizableState, Selectable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        h_flex, h_resizable, resizable_panel, v_flex,
    },
    div,
    prelude::FluentBuilder as _,
    px,
};

use crate::agent::{AgentPane, AgentPaneEvent};
use crate::app_settings::{app_editor_page, app_library_page, app_overview_page};
use crate::library::{LibraryEvent, LibraryPane};
use crate::session::SessionState;
use crate::state::AppState;

actions!(
    sciscraper,
    [
        NewDocument,
        OpenFile,
        OpenFolder,
        SaveFile,
        SaveFileAs,
        CloseTab,
        ToggleAgent,
        ToggleLibrary,
        TogglePreview,
        ToggleSettings,
        FocusSelection,
        EditSelection,
        CancelClose,
        EditorPageUp,
        EditorPageDown,
        EditorSelectPageUp,
        EditorSelectPageDown,
    ]
);

/// Bind the application's global shortcuts.
pub fn bind_keys(cx: &mut gpui_kit::App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NewDocument, None),
        KeyBinding::new("cmd-n", NewDocument, None),
        KeyBinding::new("ctrl-o", OpenFile, None),
        KeyBinding::new("cmd-o", OpenFile, None),
        KeyBinding::new("ctrl-shift-o", OpenFolder, None),
        KeyBinding::new("cmd-shift-o", OpenFolder, None),
        KeyBinding::new("ctrl-s", SaveFile, None),
        KeyBinding::new("cmd-s", SaveFile, None),
        KeyBinding::new("ctrl-shift-s", SaveFileAs, None),
        KeyBinding::new("cmd-shift-s", SaveFileAs, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-shift-a", ToggleAgent, None),
        KeyBinding::new("cmd-shift-a", ToggleAgent, None),
        KeyBinding::new("ctrl-shift-l", ToggleLibrary, None),
        KeyBinding::new("cmd-shift-l", ToggleLibrary, None),
        KeyBinding::new("ctrl-shift-e", TogglePreview, None),
        KeyBinding::new("cmd-shift-e", TogglePreview, None),
        KeyBinding::new("ctrl-shift-p", FocusSelection, None),
        KeyBinding::new("cmd-shift-p", FocusSelection, None),
        KeyBinding::new("ctrl-shift-d", EditSelection, None),
        KeyBinding::new("cmd-shift-d", EditSelection, None),
        KeyBinding::new("ctrl-,", ToggleSettings, None),
        KeyBinding::new("cmd-,", ToggleSettings, None),
        KeyBinding::new("escape", CancelClose, None),
    ]);

    // The gpui-kit editor owns its keymap, including word jumps and page
    // movement. The zorite backend needs the conventional Linux bindings and
    // the host-side page actions because it does not install them itself.
    #[cfg(feature = "zorite")]
    cx.bind_keys([
        KeyBinding::new("ctrl-left", zorite_editor::WordLeft, Some("Editor")),
        KeyBinding::new("ctrl-right", zorite_editor::WordRight, Some("Editor")),
        KeyBinding::new(
            "ctrl-shift-left",
            zorite_editor::SelectWordLeft,
            Some("Editor"),
        ),
        KeyBinding::new(
            "ctrl-shift-right",
            zorite_editor::SelectWordRight,
            Some("Editor"),
        ),
        KeyBinding::new("pageup", EditorPageUp, Some("Editor")),
        KeyBinding::new("pagedown", EditorPageDown, Some("Editor")),
        KeyBinding::new("shift-pageup", EditorSelectPageUp, Some("Editor")),
        KeyBinding::new("shift-pagedown", EditorSelectPageDown, Some("Editor")),
    ]);
}

/// The root view.
pub struct SciscraperApp {
    state: Entity<AppState>,
    agent: Entity<AgentPane>,
    library: Entity<LibraryPane>,
    show_settings: bool,
    close_prompt: bool,
    allow_close: bool,
    resizable: Entity<ResizableState>,
    settings_resizable: Entity<ResizableState>,
    last_panes: (bool, bool),
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SciscraperApp {
    /// Build the state and both panes for a new window.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings_path = std::env::var_os("SCISCRAPER_SETTINGS")
            .map(PathBuf::from)
            .unwrap_or_else(crate::settings::PersistedSettings::default_path);
        let session_path = std::env::var_os("SCISCRAPER_SESSION")
            .map(PathBuf::from)
            .unwrap_or_else(|| SessionState::path_for_settings(&settings_path));
        Self::new_with_paths(window, cx, settings_path, session_path)
    }

    /// Build a window with explicit persistence paths (used by tests).
    pub(crate) fn new_with_paths(
        window: &mut Window,
        cx: &mut Context<Self>,
        settings_path: PathBuf,
        session_path: PathBuf,
    ) -> Self {
        let (persisted, load_error) = crate::settings::PersistedSettings::load(&settings_path);
        let state = cx.new(|cx| AppState::new(window, cx, persisted, settings_path, session_path));
        if let Some(error) = load_error {
            state.update(cx, |state, cx| state.set_status(error, cx));
        }
        let agent = cx.new(|cx| AgentPane::new(window, cx, state.clone()));
        let library = cx.new(|cx| LibraryPane::new(window, cx, state.clone(), agent.clone()));
        let initial_panes = {
            let state = state.read(cx);
            (state.show_library, state.show_agent)
        };

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&state, |this, state, cx| {
            let panes = {
                let state = state.read(cx);
                (state.show_library, state.show_agent)
            };
            if panes != this.last_panes {
                this.last_panes = panes;
                // A panel count change remaps the old slots; start clean so a
                // hidden pane comes back at its recorded width.
                this.resizable = cx.new(|_| ResizableState::default());
            }
            cx.notify();
        }));
        subscriptions.push(cx.subscribe(
            &agent,
            |this, _, event: &AgentPaneEvent, cx| match event {
                AgentPaneEvent::Status(status) => {
                    this.state
                        .update(cx, |state, cx| state.set_status(status.clone(), cx));
                }
                AgentPaneEvent::OpenSettings => {
                    this.show_settings = true;
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe(&library, |this, _, event: &LibraryEvent, cx| {
            if matches!(event, LibraryEvent::OpenSettings) {
                this.show_settings = true;
                cx.notify();
            }
        }));

        // The panes announce themselves as soon as the worker builds; ask for
        // the session list and store snapshot explicitly so a restored window
        // is populated even before the user touches anything.
        let controller = state.read(cx).controller.clone();
        controller.update(cx, |controller, cx| {
            controller.refresh_sessions(cx);
            controller.refresh_library(cx);
        });

        if let Some(document) = state.read(cx).active_document() {
            let handle = document.focus_handle(cx);
            window.focus(&handle, cx);
        }

        let this = Self {
            state,
            agent,
            library,
            show_settings: false,
            close_prompt: false,
            allow_close: false,
            resizable: cx.new(|_| ResizableState::default()),
            settings_resizable: cx.new(|_| ResizableState::default()),
            last_panes: initial_panes,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };

        // Intercept the window manager's close request so unsaved buffers can
        // be saved (or deliberately discarded) before the window goes away.
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.on_close_requested(window, cx))
                .unwrap_or(true)
        });

        if let Ok(prompt) = std::env::var("SCISCRAPER_ASK")
            && !prompt.trim().is_empty()
        {
            let agent = this.agent.clone();
            cx.defer(move |cx| {
                agent.update(cx, |agent, cx| agent.ask(prompt, cx));
            });
        }
        this
    }

    fn new_document(&mut self, _: &NewDocument, window: &mut Window, cx: &mut Context<Self>) {
        self.state
            .update(cx, |state, cx| state.new_document(window, cx));
    }

    fn open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open Markdown or Quarto documents".into()),
        });
        let state = self.state.clone();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let _ = state.update_in(cx, |state, window, cx| {
                for path in paths {
                    if let Err(error) = state.open_path(&path, window, cx) {
                        state.set_status(error, cx);
                    }
                }
            });
        })
        .detach();
    }

    fn open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open a workspace folder".into()),
        });
        let state = self.state.clone();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = state.update_in(cx, |state, _window, cx| {
                state.set_workspace(path, cx);
            });
        })
        .detach();
    }

    fn save_file(&mut self, _: &SaveFile, window: &mut Window, cx: &mut Context<Self>) {
        let needs_path = self
            .state
            .read(cx)
            .active_document()
            .is_some_and(|document| document.path.is_none());
        if needs_path {
            self.save_file_as(&SaveFileAs, window, cx);
            return;
        }
        self.state.update(cx, |state, cx| {
            if let Err(error) = state.save_active(cx) {
                state.set_status(error, cx);
            }
        });
    }

    fn save_file_as(&mut self, _: &SaveFileAs, window: &mut Window, cx: &mut Context<Self>) {
        let directory = self
            .state
            .read(cx)
            .workspace
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let suggested = self
            .state
            .read(cx)
            .active_document()
            .map(|document| document.title.to_string())
            .unwrap_or_else(|| "paper.md".to_owned());
        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
        let state = self.state.clone();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            let _ = state.update_in(cx, |state, _window, cx| {
                if let Err(error) = state.save_active_as(path, cx) {
                    state.set_status(error, cx);
                }
            });
        })
        .detach();
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.close_active(window, cx);
            if state.documents.is_empty() {
                state.new_document(window, cx);
            }
        });
    }

    fn editor_page_up(&mut self, _: &EditorPageUp, window: &mut Window, cx: &mut Context<Self>) {
        self.editor_page(true, false, window, cx);
    }

    fn editor_page_down(
        &mut self,
        _: &EditorPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_page(false, false, window, cx);
    }

    fn editor_select_page_up(
        &mut self,
        _: &EditorSelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_page(true, true, window, cx);
    }

    fn editor_select_page_down(
        &mut self,
        _: &EditorSelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor_page(false, true, window, cx);
    }

    /// Move or extend the caret by one viewport using the editor's own
    /// goal-column movement. The gpuikit editor binds page movement itself.
    #[cfg(feature = "zorite")]
    fn editor_page(&mut self, up: bool, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, scroll)) = self
            .state
            .read(cx)
            .active_document()
            .map(|document| (document.editor.clone(), document.scroll.borrow().clone()))
        else {
            return;
        };
        let viewport = {
            let height = scroll.bounds().size.height;
            if height > px(0.) {
                height
            } else {
                window.viewport_size().height
            }
        };
        let rows = editor.read(cx).row_layout();
        let page = if rows.is_empty() {
            (f32::from(viewport) / (16.0 * zorite_editor::LINE_HEIGHT_RATIO)).floor() as usize
        } else {
            rows.iter().filter(|(top, _)| *top < viewport).count()
        }
        .clamp(1, 50);
        for _ in 0..page {
            match (up, select) {
                (true, false) => window.dispatch_action(Box::new(zorite_editor::Up), cx),
                (true, true) => window.dispatch_action(Box::new(zorite_editor::SelectUp), cx),
                (false, false) => window.dispatch_action(Box::new(zorite_editor::Down), cx),
                (false, true) => window.dispatch_action(Box::new(zorite_editor::SelectDown), cx),
            }
        }
    }

    #[cfg(feature = "gpuikit")]
    fn editor_page(
        &mut self,
        _up: bool,
        _select: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn toggle_agent(&mut self, _: &ToggleAgent, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.show_agent = !state.show_agent;
            state.persist(cx);
            state.persist_session(cx);
            cx.notify();
        });
    }

    fn toggle_library(&mut self, _: &ToggleLibrary, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.show_library = !state.show_library;
            state.persist(cx);
            state.persist_session(cx);
            cx.notify();
        });
    }

    fn toggle_preview(&mut self, _: &TogglePreview, _: &mut Window, cx: &mut Context<Self>) {
        #[cfg(feature = "gpuikit")]
        self.state.update(cx, |state, cx| {
            state.set_show_preview(!state.show_preview, cx);
        });
        #[cfg(feature = "zorite")]
        self.state.update(cx, |state, cx| {
            let raw = !state.raw_mode;
            state.set_raw_mode(raw, cx);
        });
    }

    fn toggle_settings(&mut self, _: &ToggleSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.show_settings = !self.show_settings;
        cx.notify();
    }

    fn focus_selection(&mut self, _: &FocusSelection, window: &mut Window, cx: &mut Context<Self>) {
        self.agent
            .update(cx, |agent, cx| agent.focus_selection(window, cx));
    }

    fn edit_selection(&mut self, _: &EditSelection, window: &mut Window, cx: &mut Context<Self>) {
        self.agent
            .update(cx, |agent, cx| agent.edit_selection(window, cx));
    }

    fn cancel_close(&mut self, _: &CancelClose, _: &mut Window, cx: &mut Context<Self>) {
        if self.close_prompt {
            self.close_prompt = false;
            cx.notify();
        }
    }

    fn dirty_count(&self, cx: &App) -> usize {
        self.state
            .read(cx)
            .documents
            .iter()
            .filter(|document| document.dirty)
            .count()
    }

    /// Called by the window manager before the window closes.
    ///
    /// `false` cancels the close and puts the unsaved-changes dialog on screen;
    /// `true` lets the window go away after flushing the session journal.
    fn on_close_requested(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.state.update(cx, |state, cx| state.persist_session(cx));
        if self.allow_close || self.dirty_count(cx) == 0 {
            return true;
        }
        if !self.close_prompt {
            self.close_prompt = true;
            cx.notify();
        }
        false
    }

    fn finish_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.persist_session(cx));
        self.allow_close = true;
        self.close_prompt = false;
        window.remove_window();
    }

    /// Save every dirty buffer, then close. Untitled buffers ask for a path one
    /// at a time; cancelling any of those dialogs leaves the window open.
    fn save_all_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.clone();
        let mut failure = None;
        let untitled = state.update(cx, |state, cx| {
            let dirty: Vec<u64> = state
                .documents
                .iter()
                .filter(|document| document.dirty)
                .map(|document| document.id)
                .collect();
            for id in dirty {
                if state
                    .document(id)
                    .is_some_and(|document| document.path.is_some())
                    && let Err(error) = state.save_document(id, cx)
                {
                    failure = Some(error);
                    return None;
                }
            }
            state
                .documents
                .iter()
                .find(|document| document.dirty)
                .map(|document| (document.id, document.title.to_string()))
        });
        if let Some(error) = failure {
            state.update(cx, |state, cx| state.set_status(error, cx));
            return;
        }
        let Some((id, title)) = untitled else {
            self.finish_close(window, cx);
            return;
        };
        let directory = state
            .read(cx)
            .workspace
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(&directory, Some(&title));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            let result = state.update_in(cx, |state, _window, cx| {
                state.save_document_as(id, path, cx)
            });
            match result {
                Ok(Ok(())) => {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.save_all_and_close(window, cx);
                    });
                }
                Ok(Err(error)) => {
                    state.update(cx, |state, cx| state.set_status(error, cx));
                }
                Err(_) => {}
            }
        })
        .detach();
    }

    fn close_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (count, names) = {
            let state = self.state.read(cx);
            let dirty: Vec<&str> = state
                .documents
                .iter()
                .filter(|document| document.dirty)
                .map(|document| document.title.as_ref())
                .collect();
            (dirty.len(), dirty.join(", "))
        };
        let summary = if count == 1 {
            format!("“{names}” has unsaved changes.")
        } else {
            format!("{count} documents have unsaved changes: {names}")
        };
        div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .occlude()
            .bg(theme.background.opacity(0.55))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(480.))
                    .p_5()
                    .gap_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .child(div().text_lg().child("Save changes before closing?"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(summary),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        "Unsaved text is also journaled and will be restored on the next launch.",
                    ))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("close-cancel")
                                    .label("Cancel")
                                    .small()
                                    .outline()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_prompt = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("close-discard")
                                    .label("Discard and close")
                                    .small()
                                    .outline()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.finish_close(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("close-save-all")
                                    .label("Save all and close")
                                    .small()
                                    .primary()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_all_and_close(window, cx);
                                    })),
                            ),
                    ),
            )
    }

    fn dock(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (show_library, show_agent, library_width, agent_width) = {
            let state = self.state.read(cx);
            (
                state.show_library,
                state.show_agent,
                state.library_width,
                state.agent_width,
            )
        };
        let resize_state = self.state.clone();
        let mut dock = h_resizable(ElementId::Name("sciscraper-dock".into()))
            .with_state(&self.resizable)
            .on_resize(move |resizable, _window, cx| {
                let sizes: Vec<f32> = resizable
                    .read(cx)
                    .sizes()
                    .iter()
                    .map(|size| size.as_f32())
                    .collect();
                let (show_library, show_agent, fallback_library, fallback_agent) = {
                    let state = resize_state.read(cx);
                    (
                        state.show_library,
                        state.show_agent,
                        state.library_width,
                        state.agent_width,
                    )
                };
                let library = if show_library {
                    sizes.first().copied().unwrap_or(fallback_library)
                } else {
                    fallback_library
                };
                let agent_index = if show_library { 2 } else { 1 };
                let agent = if show_agent {
                    sizes.get(agent_index).copied().unwrap_or(fallback_agent)
                } else {
                    fallback_agent
                };
                resize_state.update(cx, |state, cx| {
                    state.set_panel_widths(library, agent, cx);
                });
            });
        if show_library {
            dock = dock.child(
                resizable_panel()
                    .size(px(library_width))
                    .size_range(px(200.)..px(720.))
                    .child(self.library.clone()),
            );
        }
        dock = dock.child(
            resizable_panel().child(
                v_flex()
                    .size_full()
                    .min_w(px(0.))
                    .child(self.toolbar(window, cx))
                    .child(self.tab_bar(cx))
                    .child(self.editor(window, cx))
                    .child(self.status_bar(cx)),
            ),
        );
        if show_agent {
            dock = dock.child(
                resizable_panel()
                    .size(px(agent_width))
                    .size_range(px(280.)..px(900.))
                    .child(self.agent.clone()),
            );
        }
        dock
    }

    fn toolbar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (preview_on, show_library, show_agent) = {
            let state = self.state.read(cx);
            #[cfg(feature = "gpuikit")]
            let preview_on = state.show_preview;
            #[cfg(feature = "zorite")]
            let preview_on = !state.raw_mode;
            (preview_on, state.show_library, state.show_agent)
        };
        let theme = cx.theme();
        #[cfg(feature = "gpuikit")]
        let preview_label = if preview_on { "Preview" } else { "Preview off" };
        #[cfg(feature = "zorite")]
        let preview_label = if preview_on { "Preview" } else { "Raw" };

        h_flex()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .child(
                Button::new("toolbar-library")
                    .label("Library")
                    .icon(if show_library {
                        IconName::PanelLeftClose
                    } else {
                        IconName::PanelLeftOpen
                    })
                    .small()
                    .ghost()
                    .selected(show_library)
                    .tooltip(if show_library {
                        "Hide the library (Ctrl+Shift+L)"
                    } else {
                        "Show the library (Ctrl+Shift+L)"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_library(&ToggleLibrary, window, cx);
                    })),
            )
            .child(div().mr_3().text_color(theme.primary).child("Sciscraper"))
            .child(
                Button::new("toolbar-new")
                    .icon(IconName::Plus)
                    .small()
                    .ghost()
                    .tooltip("New document (Ctrl+N)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.state
                            .update(cx, |state, cx| state.new_document(window, cx));
                    })),
            )
            .child(
                Button::new("toolbar-open")
                    .icon(IconName::FolderOpen)
                    .small()
                    .ghost()
                    .tooltip("Open (Ctrl+O)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_file(&OpenFile, window, cx);
                    })),
            )
            .child(
                Button::new("toolbar-folder")
                    .icon(IconName::Folder)
                    .small()
                    .ghost()
                    .tooltip("Open workspace folder (Ctrl+Shift+O)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_folder(&OpenFolder, window, cx);
                    })),
            )
            .child(
                Button::new("toolbar-save")
                    .icon(IconName::Save)
                    .small()
                    .ghost()
                    .tooltip("Save (Ctrl+S)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_file(&SaveFile, window, cx);
                    })),
            )
            .child(div().w(px(1.)).h(px(20.)).mx_2().bg(theme.border))
            .child(
                Button::new("toolbar-preview")
                    .label(preview_label)
                    .small()
                    .selected(preview_on)
                    .ghost()
                    .when(cfg!(feature = "gpuikit"), |button| {
                        button.icon(if preview_on {
                            IconName::PanelRightClose
                        } else {
                            IconName::PanelRightOpen
                        })
                    })
                    .tooltip(if cfg!(feature = "gpuikit") {
                        if preview_on {
                            "Hide the rendered preview (Ctrl+Shift+E)"
                        } else {
                            "Show the rendered preview (Ctrl+Shift+E)"
                        }
                    } else if preview_on {
                        "Switch to raw Markdown (Ctrl+Shift+E)"
                    } else {
                        "Switch to the live preview (Ctrl+Shift+E)"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_preview(&TogglePreview, window, cx);
                    })),
            )
            .child(
                Button::new("toolbar-focus")
                    .label("Ask about selection")
                    .icon(IconName::Sparkles)
                    .small()
                    .ghost()
                    .tooltip("Select a passage, then attach it to the next question (Ctrl+Shift+P)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.focus_selection(&FocusSelection, window, cx);
                    })),
            )
            .child(
                Button::new("toolbar-edit-selection")
                    .label("Edit selection")
                    .icon(IconName::Pencil)
                    .small()
                    .ghost()
                    .tooltip(
                        "Mark a passage, then write instructions; the answer replaces it (Ctrl+Shift+D)",
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.edit_selection(&EditSelection, window, cx);
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new("toolbar-settings")
                    .icon(IconName::Settings)
                    .small()
                    .ghost()
                    .tooltip("Settings (Ctrl+,)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_settings(&ToggleSettings, window, cx);
                    })),
            )
            .child(
                Button::new("toolbar-agent")
                    .label("Scico")
                    .icon(if show_agent {
                        IconName::PanelRightClose
                    } else {
                        IconName::PanelRightOpen
                    })
                    .small()
                    .ghost()
                    .selected(show_agent)
                    .tooltip(if show_agent {
                        "Hide Scico (Ctrl+Shift+A)"
                    } else {
                        "Show Scico (Ctrl+Shift+A)"
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_agent(&ToggleAgent, window, cx);
                    })),
            )
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tabs: Vec<(usize, SharedString, bool, bool)> = {
            let state = self.state.read(cx);
            state
                .documents
                .iter()
                .enumerate()
                .map(|(index, document)| {
                    (
                        index,
                        document.title.clone(),
                        document.dirty,
                        index == state.active,
                    )
                })
                .collect()
        };
        let mut bar = h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.tab_bar);
        if tabs.is_empty() {
            bar = bar.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No document open"),
            );
        }
        for (index, title, dirty, active) in tabs {
            let close_id = ElementId::Name(format!("tab-close-{index}").into());
            bar = bar.child(
                h_flex()
                    .id(ElementId::Name(format!("tab-{index}").into()))
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(if active { theme.border } else { theme.tab_bar })
                    .bg(if active {
                        theme.background
                    } else {
                        theme.tab_bar
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.state
                            .update(cx, |state, cx| state.select_tab(index, window, cx));
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(if active {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(format!("{title}{}", if dirty { " •" } else { "" })),
                    )
                    .child(
                        Button::new(close_id)
                            .icon(IconName::X)
                            .xsmall()
                            .ghost()
                            .tooltip("Close")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, cx| {
                                    if state.active != index {
                                        state.select_tab(index, window, cx);
                                    }
                                    state.close_active(window, cx);
                                });
                            })),
                    ),
            );
        }
        bar
    }

    fn editor(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(feature = "zorite")]
        {
            self.editor_zorite(window, cx)
        }
        #[cfg(feature = "gpuikit")]
        {
            let _ = window;
            self.editor_gpuikit(cx)
        }
    }

    fn empty_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_lg()
                            .text_color(theme.muted_foreground)
                            .child("No document open"),
                    )
                    .child(
                        Button::new("empty-new")
                            .label("New document")
                            .icon(IconName::Plus)
                            .primary()
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.state
                                    .update(cx, |state, cx| state.new_document(window, cx));
                            })),
                    ),
            )
            .into_any_element()
    }

    #[cfg(feature = "zorite")]
    fn editor_zorite(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let editor = {
            let state = self.state.read(cx);
            state.active_document().map(|document| {
                (
                    document.editor.clone(),
                    document.scroll_id.clone(),
                    document.scroll.clone(),
                    state.editor_font_family(cx),
                    state.editor_font_size,
                )
            })
        };
        let Some((editor, scroll_id, scroll_cell, font_family, font_size)) = editor else {
            return self.empty_editor(cx);
        };
        let scroll = window
            .use_keyed_state(scroll_id.clone(), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        *scroll_cell.borrow_mut() = scroll.clone();
        div()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .relative()
            .bg(theme.background)
            .child(
                div()
                    .id(scroll_id)
                    .size_full()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(
                        |this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                            if !event.modifiers.control && !event.modifiers.platform {
                                return;
                            }
                            let lines = match event.delta {
                                gpui_kit::ScrollDelta::Lines(delta) => delta.y,
                                gpui_kit::ScrollDelta::Pixels(delta) => f32::from(delta.y) / 20.0,
                            };
                            if lines.abs() < f32::EPSILON {
                                return;
                            }
                            let step = if lines > 0.0 { -1.0 } else { 1.0 };
                            this.state
                                .update(cx, |state, cx| state.zoom_editor_font(step, cx));
                            cx.stop_propagation();
                        },
                    ))
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .flex_none()
                            .min_h_full()
                            .max_w(px(900.))
                            .mx_auto()
                            .px_8()
                            .py_6()
                            .font_family(font_family)
                            .text_size(px(font_size))
                            .text_color(theme.foreground)
                            .child(editor),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(0.))
                    .size_full()
                    .child(Scrollbar::vertical(&scroll).viewport_from_layout()),
            )
            .into_any_element()
    }

    #[cfg(feature = "gpuikit")]
    fn editor_gpuikit(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let active = {
            let state = self.state.read(cx);
            state.active_document().map(|document| {
                let base = document
                    .path
                    .as_deref()
                    .and_then(std::path::Path::parent)
                    .map(std::path::Path::to_path_buf)
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("."));
                (
                    document.editor.clone(),
                    document.preview.clone(),
                    state.editor_font_family(cx),
                    state.editor_font_size,
                    state.show_preview,
                    state.sync_scroll,
                    base,
                )
            })
        };
        let Some((editor, preview, font_family, font_size, show_preview, sync_scroll, base)) =
            active
        else {
            return self.empty_editor(cx);
        };
        let source = div()
            .id("editor-source")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .on_scroll_wheel(
                cx.listener(|this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                    if !event.modifiers.control && !event.modifiers.platform {
                        return;
                    }
                    let lines = match event.delta {
                        gpui_kit::ScrollDelta::Lines(delta) => delta.y,
                        gpui_kit::ScrollDelta::Pixels(delta) => f32::from(delta.y) / 20.0,
                    };
                    if lines.abs() < f32::EPSILON {
                        return;
                    }
                    let step = if lines > 0.0 { -1.0 } else { 1.0 };
                    this.state
                        .update(cx, |state, cx| state.zoom_editor_font(step, cx));
                    cx.stop_propagation();
                }),
            )
            .child(
                gpui_kit::component::input::Editor::new(&editor)
                    .bordered(false)
                    .font_family(font_family)
                    .text_size(px(font_size))
                    .h_full(),
            );
        let body = if !show_preview {
            source.into_any_element()
        } else {
            let preview_header = h_flex()
                .flex_none()
                .items_center()
                .justify_between()
                .px_3()
                .py_1()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.title_bar)
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Preview"),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("preview-sync")
                                .icon(if sync_scroll {
                                    IconName::Link
                                } else {
                                    IconName::Link2Off
                                })
                                .xsmall()
                                .ghost()
                                .selected(sync_scroll)
                                .tooltip(if sync_scroll {
                                    "Scroll sync on: the preview follows the source block"
                                } else {
                                    "Scroll sync off"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.state.update(cx, |state, cx| {
                                        state.set_sync_scroll(!state.sync_scroll, cx);
                                    });
                                })),
                        )
                        .child(
                            Button::new("preview-collapse")
                                .icon(IconName::PanelRightClose)
                                .xsmall()
                                .ghost()
                                .tooltip("Collapse the preview (Ctrl+Shift+E)")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_preview(&TogglePreview, window, cx);
                                })),
                        ),
                );

            h_flex()
                .size_full()
                .child(source)
                .child(div().w(px(1.)).h_full().bg(theme.border))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .child(preview_header)
                        .child(
                            div().flex_1().min_h(px(0.)).child(
                                gpui_kit::base::TextView::new(&preview)
                                    .scrollable(true)
                                    .image_source(crate::document::preview_image_source(base)),
                            ),
                        ),
                )
                .into_any_element()
        };
        div()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .relative()
            .bg(theme.background)
            .child(
                div()
                    .size_full()
                    .px_4()
                    .py_4()
                    .text_size(px(font_size))
                    .text_color(theme.foreground)
                    .child(body),
            )
            .into_any_element()
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (path, cursor, chars, status, controller_status, workspace) = {
            let state = self.state.read(cx);
            let controller_status = state.controller.read(cx).status_line();
            let (path, cursor, chars) = state
                .active_document()
                .map(|document| {
                    (
                        document
                            .path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "Untitled".to_owned()),
                        format!("{}:{}", document.cursor.0 + 1, document.cursor.1 + 1),
                        document.chars,
                    )
                })
                .unwrap_or_else(|| ("—".to_owned(), "—".to_owned(), 0));
            let workspace = state
                .workspace
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "no workspace".to_owned());
            (
                path,
                cursor,
                chars,
                state.status.to_string(),
                controller_status,
                workspace,
            )
        };
        h_flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .bg(theme.title_bar)
            .child(div().truncate().child(path))
            .child(div().child(cursor))
            .child(div().child(format!("{chars} chars")))
            .child(div().flex_1())
            .child(
                div()
                    .max_w(px(520.))
                    .truncate()
                    .child(format!("{status} · {controller_status}")),
            )
            .child(div().max_w(px(280.)).truncate().child(workspace))
    }

    fn settings_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rag = self.state.read(cx).rag.clone();
        let changes: ragrig_gpui::Changes = {
            let state = self.state.clone();
            Rc::new(move |cx: &App| state.read(cx).rag_changes(cx))
        };
        let now = self.state.read(cx).rag_changes(cx);
        let settings = gpui_kit::component::setting::Settings::new(ElementId::Name(
            "sciscraper-settings".into(),
        ))
        .page(app_overview_page(self.state.clone(), &now, cx))
        .page(app_library_page(self.state.clone(), cx))
        .page(app_editor_page(self.state.clone()))
        .page(ragrig_gpui::rag_model_page(rag, changes, cx));
        let gutter = self.state.read(cx).settings_gutter;
        let resize_state = self.state.clone();
        let layout = h_resizable(ElementId::Name("sciscraper-settings-layout".into()))
            .with_state(&self.settings_resizable)
            .on_resize(move |resizable, _window, cx| {
                let gutter = resizable
                    .read(cx)
                    .sizes()
                    .get(1)
                    .map(|size| size.as_f32())
                    .unwrap_or(360.0);
                resize_state.update(cx, |state, cx| state.set_settings_gutter(gutter, cx));
            })
            .child(resizable_panel().child(settings))
            .child(
                resizable_panel()
                    .size(px(gutter))
                    .size_range(px(80.)..px(900.))
                    .flex_none()
                    .child(div().size_full().bg(theme.muted.opacity(0.06))),
            );
        v_flex()
            .size_full()
            .bg(theme.background)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Button::new("settings-back")
                            .label("Back to editor")
                            .icon(IconName::ArrowLeft)
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_settings = false;
                                cx.notify();
                            })),
                    )
                    .child(div().text_lg().child("Settings")),
            )
            .child(div().flex_1().min_h(px(0.)).child(layout))
    }
}

impl Render for SciscraperApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_title = self
            .state
            .read(cx)
            .active_document()
            .map(|document| document.title.to_string())
            .unwrap_or_else(|| "Sciscraper".to_owned());
        window.set_window_title(&format!("Sciscraper — {active_title}"));
        let theme = cx.theme();

        let body: gpui_kit::AnyElement = if self.show_settings {
            self.settings_view(cx).into_any_element()
        } else {
            div()
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .child(self.dock(window, cx))
                .into_any_element()
        };

        div()
            .relative()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::new_document))
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::open_folder))
            .on_action(cx.listener(Self::save_file))
            .on_action(cx.listener(Self::save_file_as))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::toggle_agent))
            .on_action(cx.listener(Self::toggle_library))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::focus_selection))
            .on_action(cx.listener(Self::edit_selection))
            .on_action(cx.listener(Self::cancel_close))
            .on_action(cx.listener(Self::editor_page_up))
            .on_action(cx.listener(Self::editor_page_down))
            .on_action(cx.listener(Self::editor_select_page_up))
            .on_action(cx.listener(Self::editor_select_page_down))
            .child(body)
            .when(self.close_prompt, |this| this.child(self.close_dialog(cx)))
    }
}

impl Focusable for SciscraperApp {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use gpui_kit::{TestAppContext, VisualTestContext};

    use super::*;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    /// A scratch persistence directory, unique per test and per window.
    fn scratch_paths() -> (PathBuf, PathBuf) {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sciscraper-gpui-app-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        (dir.join("settings.json"), dir.join("session.json"))
    }

    fn mount(cx: &mut TestAppContext) -> (Entity<SciscraperApp>, &mut VisualTestContext) {
        cx.update(|cx| {
            ragrig_gpui::init(cx);
            #[cfg(feature = "zorite")]
            zorite_editor::bind_keys(cx);
            bind_keys(cx);
        });
        let (settings, session) = scratch_paths();
        cx.add_window_view(move |window, cx| {
            SciscraperApp::new_with_paths(window, cx, settings, session)
        })
    }

    fn mount_at(
        cx: &mut TestAppContext,
        settings: PathBuf,
        session: PathBuf,
    ) -> (Entity<SciscraperApp>, &mut VisualTestContext) {
        cx.update(|cx| {
            ragrig_gpui::init(cx);
            #[cfg(feature = "zorite")]
            zorite_editor::bind_keys(cx);
            bind_keys(cx);
        });
        cx.add_window_view(move |window, cx| {
            SciscraperApp::new_with_paths(window, cx, settings, session)
        })
    }

    fn type_into_active(cx: &mut VisualTestContext, app: &Entity<SciscraperApp>, text: &str) {
        let handle = cx.update(|_, cx| {
            app.read(cx)
                .state
                .read(cx)
                .active_document()
                .expect("a document")
                .focus_handle(cx)
        });
        cx.update(|window, cx| {
            window.focus(&handle, cx);
        });
        cx.simulate_input(text);
    }

    #[gpui_kit::test]
    fn the_shell_mounts_with_an_editor_and_both_panes(cx: &mut TestAppContext) {
        let (app, _cx) = mount(cx);
        cx.update(|cx| {
            let state = app.read(cx).state.read(cx);
            assert_eq!(state.documents.len(), 1, "one scratch document");
            assert!(state.active_document().is_some());
            assert!(state.show_library);
            assert!(state.show_agent);
        });
    }

    #[gpui_kit::test]
    fn the_library_inserts_through_the_roundtrip_contract(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let agent = cx.update(|_, cx| app.read(cx).agent.clone());
        let inserted = cx.update(|window, cx| {
            agent.update(cx, |agent, cx| agent.insert_text("@Kano2025", window, cx))
        });
        assert!(inserted, "the active document accepts the citation");
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            assert_eq!(
                state.active_document().expect("a document").text(cx),
                "@Kano2025"
            );
        });
    }

    #[gpui_kit::test]
    fn a_new_document_starts_clean(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.state
                    .update(cx, |state, cx| state.new_document(window, cx));
                let state = app.state.read(cx);
                assert_eq!(state.documents.len(), 2);
                assert!(
                    state
                        .active_document()
                        .is_some_and(|document| !document.dirty)
                );
            });
        });
    }

    #[gpui_kit::test]
    fn typing_reaches_the_editor_and_marks_it_dirty(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let editor = cx.update(|_, cx| {
            app.read(cx)
                .state
                .read(cx)
                .active_document()
                .expect("a document")
                .editor
                .clone()
        });
        cx.update(|window, cx| {
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_input("roundtrip");
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            let document = state.active_document().expect("a document");
            assert_eq!(document.text(cx), "roundtrip");
            assert!(document.dirty);
        });
    }

    #[gpui_kit::test]
    fn select_all_and_focus_selection_captures_the_passage(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let editor = cx.update(|_, cx| {
            app.read(cx)
                .state
                .read(cx)
                .active_document()
                .expect("a document")
                .editor
                .clone()
        });
        cx.update(|window, cx| {
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_input("mark this passage");
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_keystrokes("ctrl-shift-p");
        cx.update(|_, cx| {
            let chars = app.read(cx).agent.read(cx).pending_selection_chars();
            assert_eq!(chars, Some("mark this passage".chars().count()));
        });
    }

    #[gpui_kit::test]
    fn the_preview_shortcut_toggles_the_preview(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        cx.simulate_keystrokes("ctrl-shift-e");
        cx.update(|_, cx| {
            #[cfg(feature = "gpuikit")]
            assert!(!app.read(cx).state.read(cx).show_preview);
            #[cfg(feature = "zorite")]
            assert!(app.read(cx).state.read(cx).raw_mode);
        });
        cx.simulate_keystrokes("ctrl-shift-e");
        cx.update(|_, cx| {
            #[cfg(feature = "gpuikit")]
            assert!(app.read(cx).state.read(cx).show_preview);
            #[cfg(feature = "zorite")]
            assert!(!app.read(cx).state.read(cx).raw_mode);
        });
    }

    #[gpui_kit::test]
    fn the_settings_shortcut_opens_the_settings_surface(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        cx.simulate_keystrokes("ctrl-,");
        cx.update(|_, cx| assert!(app.read(cx).show_settings));
    }

    #[gpui_kit::test]
    fn undo_returns_the_buffer_to_clean(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let editor = cx.update(|_, cx| {
            app.read(cx)
                .state
                .read(cx)
                .active_document()
                .expect("a document")
                .editor
                .clone()
        });
        cx.update(|window, cx| {
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_input("draft");
        cx.update(|_, cx| {
            assert!(
                app.read(cx)
                    .state
                    .read(cx)
                    .active_document()
                    .expect("doc")
                    .dirty
            );
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            let document = state.active_document().expect("a document");
            assert_eq!(document.text(cx), "");
            assert!(!document.dirty, "undo must clear the dirty marker");
            assert_eq!(document.chars, 0);
        });
    }

    #[gpui_kit::test]
    fn a_guarded_replacement_replaces_the_marked_passage(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let editor = cx.update(|_, cx| {
            app.read(cx)
                .state
                .read(cx)
                .active_document()
                .expect("a document")
                .editor
                .clone()
        });
        cx.update(|window, cx| {
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_input("old passage");
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_keystrokes("ctrl-shift-p");

        let (tab_id, range, digest) = cx.update(|_, cx| {
            app.read(cx)
                .agent
                .read(cx)
                .pending_selection_guard()
                .expect("a captured selection")
        });
        let outcome = cx.update(|window, cx| {
            let state = app.read(cx).state.clone();
            state.update(cx, |state, cx| {
                state.apply_replacement(
                    ragrig_gpui::editor::ReplacementTarget {
                        document_id: tab_id,
                        range: range.clone(),
                        digest: digest.clone(),
                    },
                    "new passage",
                    window,
                    cx,
                )
            })
        });
        assert!(matches!(
            outcome,
            crate::state::ReplacementOutcome::Applied { .. }
        ));
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            let document = state.active_document().expect("a document");
            assert_eq!(document.text(cx), "new passage");
            assert!(document.dirty);
        });

        // A stale digest is offered, never written.
        let outcome = cx.update(|window, cx| {
            let state = app.read(cx).state.clone();
            state.update(cx, |state, cx| {
                state.apply_replacement(
                    ragrig_gpui::editor::ReplacementTarget {
                        document_id: tab_id,
                        range,
                        digest,
                    },
                    "stale text",
                    window,
                    cx,
                )
            })
        });
        assert!(matches!(
            outcome,
            crate::state::ReplacementOutcome::Offered { .. }
        ));
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            assert_eq!(
                state.active_document().expect("a document").text(cx),
                "new passage"
            );
        });

        // The write-back is one undo step for the editor.
        cx.update(|window, cx| {
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_keystrokes("ctrl-z");
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            assert_eq!(
                state.active_document().expect("a document").text(cx),
                "old passage"
            );
        });
    }

    #[gpui_kit::test]
    fn ctrl_arrows_move_by_word(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        type_into_active(cx, &app, "one two three");
        cx.simulate_keystrokes("ctrl-left");
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            assert_eq!(
                state.active_document().expect("doc").cursor_position(cx),
                (0, 8)
            );
        });
        cx.simulate_keystrokes("ctrl-right");
        cx.update(|_, cx| {
            let state = app.read(cx).state.read(cx);
            assert_eq!(
                state.active_document().expect("doc").cursor_position(cx),
                (0, 13)
            );
        });
    }

    #[cfg(feature = "zorite")]
    #[gpui_kit::test]
    fn page_keys_move_by_a_viewport(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let editor = cx.update(|_, cx| {
            app.read(cx)
                .state
                .read(cx)
                .active_document()
                .expect("a document")
                .editor
                .clone()
        });
        let text = (0..40)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_text(text, cx);
                editor.set_cursor(0, cx);
            });
        });
        cx.run_until_parked();

        let line = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                app.read(cx)
                    .state
                    .read(cx)
                    .active_document()
                    .expect("a document")
                    .cursor_position(cx)
                    .0
            })
        };

        assert_eq!(line(cx), 0);
        cx.simulate_keystrokes("pagedown");
        cx.run_until_parked();
        let after = line(cx);
        assert!(
            after > 1,
            "pagedown must move more than one line (0 -> {after})"
        );

        cx.simulate_keystrokes("pageup");
        cx.run_until_parked();
        let back = line(cx);
        assert!(back < after, "pageup must move back ({after} -> {back})");
    }

    #[gpui_kit::test]
    fn several_selections_can_be_attached_and_edit_mode_arms(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        type_into_active(cx, &app, "alpha beta gamma");

        // Mark "gamma", collapse, mark "beta".
        cx.simulate_keystrokes("ctrl-shift-left");
        cx.simulate_keystrokes("ctrl-shift-p");
        cx.simulate_keystrokes("left");
        cx.simulate_keystrokes("ctrl-left");
        cx.simulate_keystrokes("ctrl-shift-right");
        cx.simulate_keystrokes("ctrl-shift-p");

        let agent = cx.update(|_, cx| app.read(cx).agent.clone());
        cx.update(|_, cx| {
            assert_eq!(agent.read(cx).pending_selection_count(), 2);
            assert_eq!(agent.read(cx).pending_selection_chars(), Some(9));
        });

        cx.simulate_keystrokes("ctrl-shift-d");
        cx.update(|_, cx| {
            assert!(agent.read(cx).edit_target_is_set());
        });
    }

    #[gpui_kit::test]
    fn hiding_and_showing_the_agent_round_trips(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        cx.simulate_keystrokes("ctrl-shift-a");
        cx.update(|_, cx| {
            assert!(!app.read(cx).state.read(cx).show_agent);
        });
        // The toolbar toggle remains available while the pane is hidden.
        cx.simulate_keystrokes("ctrl-shift-a");
        cx.update(|_, cx| {
            assert!(app.read(cx).state.read(cx).show_agent);
        });
    }

    #[cfg(feature = "gpuikit")]
    #[cfg(feature = "gpuikit")]
    #[gpui_kit::test]
    fn ctrl_wheel_zooms_the_editor_font(cx: &mut TestAppContext) {
        use gpui_kit::InputEvent as _;

        let (app, cx) = mount(cx);
        let state = cx.update(|_, cx| app.read(cx).state.clone());
        let editor = cx.update(|_, cx| {
            state
                .read(cx)
                .active_document()
                .expect("a document")
                .editor
                .clone()
        });
        let position = cx.update(|_, cx| editor.read(cx).input_bounds().center());
        let size = |cx: &mut VisualTestContext| cx.update(|_, cx| state.read(cx).editor_font_size);
        let before = size(cx);

        cx.update(|window, cx| {
            window.dispatch_event(
                gpui_kit::ScrollWheelEvent {
                    position,
                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-40.))),
                    modifiers: gpui_kit::Modifiers {
                        control: true,
                        ..Default::default()
                    },
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        });
        cx.run_until_parked();
        assert!(size(cx) > before, "wheel down must zoom in");
        assert_eq!(
            cx.update(|_, cx| editor.read(cx).scroll_offset()),
            gpui_kit::Point::default(),
            "Ctrl+wheel must zoom, not scroll"
        );

        cx.update(|window, cx| {
            window.dispatch_event(
                gpui_kit::ScrollWheelEvent {
                    position,
                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(40.))),
                    modifiers: gpui_kit::Modifiers {
                        control: true,
                        ..Default::default()
                    },
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        });
        cx.run_until_parked();
        assert!(size(cx) < before + 1.0, "wheel up must zoom out");
    }

    #[gpui_kit::test]
    fn editor_font_defaults_are_comfortable(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let state = cx.update(|_, cx| app.read(cx).state.clone());
        cx.update(|_, cx| {
            let state = state.read(cx);
            assert!(!state.editor_font.is_empty());
            assert!(
                state.editor_font_size >= crate::state::MIN_EDITOR_FONT_SIZE,
                "the default follows 150% of the theme mono size"
            );
        });
        cx.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.set_editor_font_size(1.0, cx);
                assert_eq!(state.editor_font_size, crate::state::MIN_EDITOR_FONT_SIZE);
                state.set_editor_font_size(999.0, cx);
                assert_eq!(state.editor_font_size, crate::state::MAX_EDITOR_FONT_SIZE);
            });
        });
    }

    #[cfg(feature = "gpuikit")]
    #[gpui_kit::test]
    fn documents_choose_a_language_from_their_path(cx: &mut TestAppContext) {
        let (settings, session) = scratch_paths();
        let directory = settings.parent().expect("scratch dir").to_path_buf();
        std::fs::write(directory.join("paper.md"), "# Paper").expect("markdown");
        std::fs::write(directory.join("references.bib"), "@article{key}").expect("bibtex");
        let (app, cx) = mount_at(cx, settings, session);
        let state = cx.update(|_, cx| app.read(cx).state.clone());

        for (name, expected) in [("paper.md", "markdown"), ("references.bib", "bibtex")] {
            cx.update(|window, cx| {
                state.update(cx, |state, cx| {
                    state
                        .open_path(&directory.join(name), window, cx)
                        .expect("open");
                });
            });
            let language = cx.update(|_, cx| {
                state
                    .read(cx)
                    .active_document()
                    .expect("a document")
                    .editor
                    .read(cx)
                    .language_name()
            });
            assert_eq!(language.as_ref(), expected, "{name}");
        }
    }

    #[gpui_kit::test]
    fn closing_with_unsaved_work_asks_first(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        type_into_active(cx, &app, "unsaved draft");
        let allowed =
            cx.update(|window, cx| app.update(cx, |app, cx| app.on_close_requested(window, cx)));
        assert!(!allowed, "the window must stay open");
        cx.update(|_, cx| {
            assert!(app.read(cx).close_prompt);
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| app.cancel_close(&CancelClose, window, cx));
            assert!(!app.read(cx).close_prompt);
        });
    }

    #[gpui_kit::test]
    fn unsaved_text_comes_back_from_the_session(cx: &mut TestAppContext) {
        let (settings, session) = scratch_paths();
        let (app, cx) = mount_at(cx, settings.clone(), session.clone());
        let state = cx.update(|_, cx| app.read(cx).state.clone());
        cx.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.show_agent = false;
                state.set_show_preview(false, cx);
                state.set_sync_scroll(false, cx);
                state.set_panel_widths(480.0, 430.0, cx);
            });
        });
        type_into_active(cx, &app, "recovered draft");
        // The journal debounces edits; a real window would close (flush) before
        // the next launch. Flush here to model that boundary.
        cx.update(|_, cx| {
            state.update(cx, |state, cx| state.persist_session(cx));
        });

        // A second window on the same persistence paths is the next launch.
        let (restored, cx) = mount_at(cx, settings, session);
        cx.update(|_, cx| {
            let state = restored.read(cx).state.read(cx);
            let document = state.active_document().expect("a document");
            assert_eq!(document.text(cx), "recovered draft");
            assert!(document.dirty, "recovered text is unsaved");
            assert!(!state.show_agent);
            assert!(!state.show_preview);
            assert!(!state.sync_scroll);
            assert_eq!(state.library_width, 480.0);
        });
    }

    #[gpui_kit::test]
    fn pane_widths_are_clamped(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let state = cx.update(|_, cx| app.read(cx).state.clone());
        cx.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.set_panel_widths(500.0, 9_000.0, cx);
            });
        });
        cx.update(|_, cx| {
            let state = state.read(cx);
            assert_eq!(state.library_width, 500.0);
            assert_eq!(state.agent_width, 1_600.0);
        });
    }

    #[cfg(feature = "gpuikit")]
    #[gpui_kit::test]
    fn scroll_sync_follows_the_editor(cx: &mut TestAppContext) {
        use gpui_kit::InputEvent as _;

        let (app, cx) = mount(cx);
        let state = cx.update(|_, cx| app.read(cx).state.clone());
        let (editor, preview) = cx.update(|_, cx| {
            let document = state.read(cx).active_document().expect("a document");
            (document.editor.clone(), document.preview.clone())
        });
        let text = (0..400)
            .map(|line| format!("## Section {line}\n\nParagraph {line}.\n\n"))
            .collect::<String>();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            state.update(cx, |state, _| state.preview_sync_suppressed_until = None);
        });

        let position = cx.update(|_, cx| editor.read(cx).input_bounds().center());
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui_kit::ScrollWheelEvent {
                    position,
                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-1_200.))),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();

        let top = cx.update(|_, cx| preview.read(cx).list_state().logical_scroll_top().item_ix);
        assert!(
            top > 0,
            "the preview must jump to the source block at the viewport top"
        );
    }

    #[cfg(feature = "gpuikit")]
    #[gpui_kit::test]
    fn block_sync_survives_soft_wrapping(cx: &mut TestAppContext) {
        use gpui_kit::InputEvent as _;

        let (app, cx) = mount(cx);
        let state = cx.update(|_, cx| app.read(cx).state.clone());
        let (editor, preview) = cx.update(|_, cx| {
            let document = state.read(cx).active_document().expect("a document");
            (document.editor.clone(), document.preview.clone())
        });
        let body = "word ".repeat(400);
        let text = (0..30)
            .map(|index| format!("Paragraph {index}. {body}\n\n"))
            .collect::<String>();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            state.update(cx, |state, _| state.preview_sync_suppressed_until = None);
        });

        let position = cx.update(|_, cx| editor.read(cx).input_bounds().center());
        cx.update(|window, cx| {
            window.dispatch_event(
                gpui_kit::ScrollWheelEvent {
                    position,
                    delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-3_000.))),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();

        let top = cx.update(|_, cx| preview.read(cx).list_state().logical_scroll_top().item_ix);
        assert!(top > 0, "wrapped paragraphs must still advance the preview");
    }

    #[cfg(feature = "gpuikit")]
    #[gpui_kit::test]
    fn scroll_sync_moves_the_editor_with_the_preview(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        let state = cx.update(|_, cx| app.read(cx).state.clone());
        let (editor, preview) = cx.update(|_, cx| {
            let document = state.read(cx).active_document().expect("a document");
            (document.editor.clone(), document.preview.clone())
        });
        let text = (0..400)
            .map(|line| format!("## Section {line}\n\nParagraph {line}.\n\n"))
            .collect::<String>();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            state.update(cx, |state, _| state.preview_sync_suppressed_until = None);
        });

        let count = cx.update(|_, cx| preview.read(cx).list_state().item_count());
        assert!(count > 50, "the preview must have parsed its blocks");
        preview.update(cx, |preview, cx| {
            preview.list_state().scroll_to(gpui_kit::ListOffset {
                item_ix: 50,
                offset_in_item: px(0.),
            });
            cx.notify();
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();

        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(
                editor.scroll_offset().y < px(0.),
                "the editor must follow a scrolled preview, got {:?}",
                editor.scroll_offset()
            );
        });
    }

    #[gpui_kit::test]
    fn a_file_backed_buffer_recovers_its_unsaved_tail(cx: &mut TestAppContext) {
        let (settings, session) = scratch_paths();
        let paper = settings.parent().expect("scratch dir").join("paper.md");
        std::fs::write(&paper, "saved text").expect("paper");
        let (app, cx) = mount_at(cx, settings.clone(), session.clone());
        cx.update(|window, cx| {
            let state = app.read(cx).state.clone();
            state.update(cx, |state, cx| {
                state.open_path(&paper, window, cx).expect("open");
            });
        });
        cx.update(|_, cx| {
            let state = app.read(cx).state.clone();
            state.update(cx, |state, cx| {
                if let Some(document) = state.active_document() {
                    let end = document.text(cx).len();
                    document.set_cursor(end, cx);
                }
            });
        });
        type_into_active(cx, &app, " edited");
        // The journal debounces edits; flush at the simulated close boundary.
        cx.update(|_, cx| {
            let state = app.read(cx).state.clone();
            state.update(cx, |state, cx| state.persist_session(cx));
        });

        let (restored, cx) = mount_at(cx, settings, session);
        cx.update(|_, cx| {
            let state = restored.read(cx).state.read(cx);
            let document = state.active_document().expect("a document");
            assert_eq!(document.path.as_deref(), Some(paper.as_path()));
            assert_eq!(document.text(cx), "saved text edited");
            assert!(document.dirty);
        });
    }
}
