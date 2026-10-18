//! The application shell: toolbar, tabs, editor, panes, and status bar.

use std::path::PathBuf;

use gpui_kit::{
    AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, actions,
    assets::IconName,
    component::{
        ActiveTheme as _, Selectable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        scroll::ScrollableElement as _,
        v_flex,
    },
    div,
    prelude::FluentBuilder as _,
    px,
};

use crate::agent::{AgentPane, AgentPaneEvent};
use crate::app_settings::app_settings_page;
use crate::library::{LibraryEvent, LibraryPane};
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
        KeyBinding::new("ctrl-,", ToggleSettings, None),
        KeyBinding::new("cmd-,", ToggleSettings, None),
    ]);
}

/// The root view.
pub struct SciscraperApp {
    state: Entity<AppState>,
    agent: Entity<AgentPane>,
    library: Entity<LibraryPane>,
    show_settings: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SciscraperApp {
    /// Build the state and both panes for a new window.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings_path = std::env::var_os("SCISCRAPER_SETTINGS")
            .map(PathBuf::from)
            .unwrap_or_else(crate::settings::PersistedSettings::default_path);
        let (persisted, load_error) = crate::settings::PersistedSettings::load(&settings_path);
        let state = cx.new(|cx| AppState::new(window, cx, persisted, settings_path));
        if let Some(error) = load_error {
            state.update(cx, |state, cx| state.set_status(error, cx));
        }
        let agent = cx.new(|cx| AgentPane::new(window, cx, state.clone()));
        let library = cx.new(|cx| LibraryPane::new(window, cx, state.clone(), agent.clone()));

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&state, |_, _, cx| cx.notify()));
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
            let editor = document.editor.clone();
            let handle = editor.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }

        let this = Self {
            state,
            agent,
            library,
            show_settings: false,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        };

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
            prompt: Some("Open Markdown documents".into()),
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
            state.close_active(cx);
            if state.documents.is_empty() {
                state.new_document(window, cx);
            }
        });
    }

    fn toggle_agent(&mut self, _: &ToggleAgent, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.show_agent = !state.show_agent;
            state.persist(cx);
            cx.notify();
        });
    }

    fn toggle_library(&mut self, _: &ToggleLibrary, _: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            state.show_library = !state.show_library;
            state.persist(cx);
            cx.notify();
        });
    }

    fn toggle_preview(&mut self, _: &TogglePreview, _: &mut Window, cx: &mut Context<Self>) {
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

    fn toolbar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (raw_mode, show_library, show_agent, title, dirty) = {
            let state = self.state.read(cx);
            let document = state.active_document();
            (
                state.raw_mode,
                state.show_library,
                state.show_agent,
                document
                    .map(|document| document.title.to_string())
                    .unwrap_or_else(|| "No document".to_owned()),
                document.is_some_and(|document| document.dirty),
            )
        };
        let theme = cx.theme();

        h_flex()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
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
                    .label(if raw_mode { "Raw" } else { "Preview" })
                    .small()
                    .selected(!raw_mode)
                    .ghost()
                    .tooltip("Toggle live preview (Ctrl+Shift+E)")
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
            .child(div().flex_1())
            .child(
                div()
                    .mr_3()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!("{title}{}", if dirty { " •" } else { "" })),
            )
            .child(
                Button::new("toolbar-library")
                    .icon(IconName::BookOpen)
                    .small()
                    .ghost()
                    .selected(show_library)
                    .tooltip("Toggle the library (Ctrl+Shift+L)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_library(&ToggleLibrary, window, cx);
                    })),
            )
            .child(
                Button::new("toolbar-agent")
                    .icon(IconName::MessagesSquare)
                    .small()
                    .ghost()
                    .selected(show_agent)
                    .tooltip("Toggle the co-writer (Ctrl+Shift+A)")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_agent(&ToggleAgent, window, cx);
                    })),
            )
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
                                    state.close_active(cx);
                                });
                            })),
                    ),
            );
        }
        bar
    }

    fn editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let editor = {
            let state = self.state.read(cx);
            state
                .active_document()
                .map(|document| document.editor.clone())
        };
        let Some(editor) = editor else {
            return div()
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
                .into_any_element();
        };
        div()
            .flex_1()
            .min_w(px(0.))
            .bg(theme.background)
            .overflow_y_scrollbar()
            .id("sciscraper-editor-scroll")
            .child(
                div()
                    .max_w(px(900.))
                    .mx_auto()
                    .px_8()
                    .py_6()
                    .text_size(px(16.))
                    .text_color(theme.foreground)
                    .child(editor),
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
        let (rag, controller) = {
            let state = self.state.read(cx);
            (state.rag.clone(), state.controller.clone())
        };
        let settings = gpui_kit::component::setting::Settings::new(ElementId::Name(
            "sciscraper-settings".into(),
        ))
        .page(ragrig_gpui::rag_settings_page(rag, controller, cx))
        .page(app_settings_page(self.state.clone()));
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
            .child(div().flex_1().min_h(px(0.)).child(settings))
    }
}

impl Render for SciscraperApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (show_library, show_agent, active_title) = {
            let state = self.state.read(cx);
            (
                state.show_library,
                state.show_agent,
                state
                    .active_document()
                    .map(|document| document.title.to_string())
                    .unwrap_or_else(|| "Sciscraper".to_owned()),
            )
        };
        window.set_window_title(&format!("Sciscraper — {active_title}"));
        let theme = cx.theme();

        let body: gpui_kit::AnyElement = if self.show_settings {
            self.settings_view(cx).into_any_element()
        } else {
            h_flex()
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .when(show_library, |this| {
                    this.child(div().w(px(340.)).h_full().child(self.library.clone()))
                })
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .h_full()
                        .child(self.toolbar(window, cx))
                        .child(self.tab_bar(cx))
                        .child(self.editor(cx))
                        .child(self.status_bar(cx)),
                )
                .when(show_agent, |this| {
                    this.child(div().w(px(430.)).h_full().child(self.agent.clone()))
                })
                .into_any_element()
        };

        div()
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
            .child(body)
    }
}

impl Focusable for SciscraperApp {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext};

    use super::*;

    fn mount(cx: &mut TestAppContext) -> (Entity<SciscraperApp>, &mut VisualTestContext) {
        cx.update(|cx| {
            ragrig_gpui::init(cx);
            zorite_editor::bind_keys(cx);
            bind_keys(cx);
        });
        cx.add_window_view(SciscraperApp::new)
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
    fn the_preview_shortcut_switches_raw_mode(cx: &mut TestAppContext) {
        let (app, cx) = mount(cx);
        cx.simulate_keystrokes("ctrl-shift-e");
        cx.update(|_, cx| {
            assert!(app.read(cx).state.read(cx).raw_mode);
        });
        cx.simulate_keystrokes("ctrl-shift-e");
        cx.update(|_, cx| {
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
        let outcome = cx.update(|_, cx| {
            let state = app.read(cx).state.clone();
            state.update(cx, |state, cx| {
                state.apply_replacement(tab_id, range.clone(), &digest, "new passage", cx)
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
        let outcome = cx.update(|_, cx| {
            let state = app.read(cx).state.clone();
            state.update(cx, |state, cx| {
                state.apply_replacement(tab_id, range, &digest, "stale text", cx)
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
}
