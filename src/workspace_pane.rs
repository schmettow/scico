//! The workspace super container.
//!
//! A workspace is the persistence and synchronization layer of a Scico
//! project: the paper, corpus, bibliography, Ragrig store, sessions, and the
//! `.scico/workspace.toml` descriptor live in one folder. The [`LibraryPane`]
//! is one pipeline over that workspace — the corpus, its index, and the
//! references — and further pipelines can join it later.

use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Window,
    assets::IconName,
    component::{
        ActiveTheme as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        h_flex, v_flex,
    },
    div,
};
use ragrig_gpui::prelude::*;

use crate::{
    library::LibraryPane,
    state::AppState,
    workspace::{WORKSPACE_DIR, WORKSPACE_FILE},
};

/// What the workspace panel asks its host to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceEvent {
    /// Create a new workspace folder.
    NewWorkspace,
    /// Adopt an existing workspace folder.
    OpenWorkspace,
    /// Show the settings surface.
    OpenSettings,
}

/// The project identity shown above the Library pipeline.
struct WorkspaceSummary {
    name: SharedString,
    path: SharedString,
    status: SharedString,
    initialized: bool,
}

/// The left dock: workspace persistence and synchronization around its
/// pipelines.
pub struct WorkspacePane {
    state: Entity<AppState>,
    library: Entity<LibraryPane>,
}

impl WorkspacePane {
    /// Build the workspace shell around an already-built library pipeline.
    pub fn new(state: Entity<AppState>, library: Entity<LibraryPane>) -> Self {
        Self { state, library }
    }

    fn workspace_summary(&self, cx: &App) -> WorkspaceSummary {
        let state = self.state.read(cx);
        let workspace = state.workspace.clone();
        let explicit = state.workspace_explicit;
        let name = workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No workspace".to_owned());
        let path = workspace
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Create or open a workspace folder".to_owned());
        let initialized = workspace
            .as_ref()
            .is_some_and(|path| path.join(WORKSPACE_DIR).join(WORKSPACE_FILE).is_file());
        let status = if initialized {
            format!("Persisted in {WORKSPACE_DIR}/{WORKSPACE_FILE}")
        } else if workspace.is_some() && explicit {
            format!("Open the workspace once to create {WORKSPACE_DIR}/{WORKSPACE_FILE}")
        } else if workspace.is_some() {
            "Current folder — open a workspace to pin it".to_owned()
        } else {
            "Create or open a workspace to begin".to_owned()
        };
        WorkspaceSummary {
            name: name.into(),
            path: path.into(),
            status: status.into(),
            initialized,
        }
    }
}

impl Render for WorkspacePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let project = self.workspace_summary(cx);
        let status_color = if project.initialized {
            theme.success
        } else {
            theme.muted_foreground
        };

        let new_workspace = Button::new("workspace-new")
            .label("New workspace")
            .icon(IconName::FolderPlus)
            .small()
            .outline()
            .tooltip("Create a workspace folder (Ctrl+Shift+N)")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(WorkspaceEvent::NewWorkspace)));
        let open_workspace = Button::new("workspace-open")
            .label("Open workspace")
            .icon(IconName::Folder)
            .small()
            .outline()
            .tooltip("Adopt an existing workspace folder (Ctrl+Shift+O)")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(WorkspaceEvent::OpenWorkspace)));
        let settings = Button::new("workspace-settings")
            .icon(IconName::Settings)
            .small()
            .ghost()
            .tooltip("Workspace settings")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(WorkspaceEvent::OpenSettings)));

        let controls = h_flex()
            .flex_wrap()
            .gap_1()
            .child(new_workspace)
            .child(open_workspace)
            .child(div().flex_1())
            .child(settings);

        let summary = v_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(project.name),
            )
            .child(
                div()
                    .truncate()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(project.path),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Paper, corpus, bibliography, index, and sessions in one folder."),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(status_color)
                    .child(project.status),
            );

        let workspace_tools = v_flex().gap_2().child(controls).child(summary);

        let library_header = v_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Library"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("One pipeline over the workspace: corpus, index, and references."),
            );

        SidePanel::new("workspace-panel", "Workspace")
            .subtitle("Persistence and synchronization")
            .toolbar(workspace_tools)
            .child(library_header)
            .child(self.library.clone())
    }
}

impl EventEmitter<WorkspaceEvent> for WorkspacePane {}

impl Focusable for WorkspacePane {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.library.read(cx).focus_handle(cx)
    }
}
