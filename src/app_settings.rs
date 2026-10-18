//! The Sciscraper-specific settings pages.
//!
//! Ragrig supplies the model/retrieval page and the reference-ingestion group.
//! These pages own the workspace, the editor layout, and external viewers.

use std::path::PathBuf;

use gpui_kit::{
    App, Entity, ParentElement as _, SharedString, Styled as _,
    component::{
        ActiveTheme as _,
        setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    },
};

use crate::settings::ViewerKind;
use crate::state::AppState;
use ragrig_gpui::path_setting;

/// The Library page: paths, viewers, and library context.
pub fn app_library_page(state: Entity<AppState>) -> SettingPage {
    let workspace = path_setting(
        "app-workspace",
        "Choose the workspace folder",
        true,
        false,
        {
            let state = state.clone();
            move |cx| {
                state
                    .read(cx)
                    .workspace
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            }
        },
        {
            let state = state.clone();
            move |value, cx| {
                let value = value.trim().to_owned();
                if value.is_empty() {
                    return;
                }
                state.update(cx, |state, cx| {
                    state.set_workspace(PathBuf::from(value), cx);
                });
            }
        },
    );

    let corpus = path_setting(
        "app-corpus",
        "Choose the corpus folder",
        true,
        false,
        {
            let state = state.clone();
            move |cx| {
                state
                    .read(cx)
                    .corpus
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            }
        },
        {
            let state = state.clone();
            move |value, cx| {
                let value = value.trim().to_owned();
                state.update(cx, |state, cx| {
                    state.corpus = (!value.is_empty()).then(|| PathBuf::from(value));
                    state.persist(cx);
                    state.persist_session(cx);
                    cx.notify();
                });
            }
        },
    );

    let bibliography = path_setting(
        "app-bibliography",
        "Choose the bibliography file",
        false,
        true,
        {
            let state = state.clone();
            move |cx| {
                state
                    .read(cx)
                    .bibliography
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            }
        },
        {
            let state = state.clone();
            move |value, cx| {
                let value = value.trim().to_owned();
                state.update(cx, |state, cx| {
                    state.bibliography = (!value.is_empty()).then(|| PathBuf::from(value.clone()));
                    state.rag.update(cx, |rag, _| {
                        rag.bibliography = value;
                    });
                    state.persist(cx);
                    state.persist_session(cx);
                    cx.notify();
                });
            }
        },
    );

    let show_library = state_switch(
        state.clone(),
        |state| state.show_library,
        |state, value, cx| {
            state.show_library = value;
            state.persist(cx);
            state.persist_session(cx);
            cx.notify();
        },
    );
    let library_attached = state_switch(
        state.clone(),
        |state| state.library_attached,
        |state, value, cx| {
            state.library_attached = value;
            state.persist(cx);
            cx.notify();
        },
    );

    let mut viewers = SettingGroup::new().title("External viewers");
    for kind in ViewerKind::ALL {
        let key = match kind {
            ViewerKind::Pdf => "app-viewer-pdf",
            ViewerKind::Epub => "app-viewer-epub",
            ViewerKind::Html => "app-viewer-html",
        };
        viewers = viewers.item(
            SettingItem::new(kind.label(), viewer_field(state.clone(), kind, key))
                .description("Executable path or command; empty uses the system default."),
        );
    }
    viewers = viewers.footer(|_, cx| {
        gpui_kit::div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("The document path is passed as the only argument.")
    });

    SettingPage::new("Library")
        .description("Workspace, bibliography, external viewers, and library context.")
        .default_open(true)
        .group(
            SettingGroup::new()
                .title("Workspace")
                .item(
                    SettingItem::new("Workspace folder", workspace)
                        .description("Documents, .ragrig store, and sessions."),
                )
                .item(
                    SettingItem::new("Corpus folder", corpus)
                        .description("Indexed papers; Add PDFs and GROBID use this folder."),
                )
                .item(
                    SettingItem::new("Bibliography", bibliography)
                        .description("The .bib the library shows and GROBID merges into."),
                )
                .item(
                    SettingItem::new("Show library pane", show_library)
                        .description("Left pane with references and corpus actions."),
                ),
        )
        .group(viewers)
        .group(
            SettingGroup::new().title("Context").item(
                SettingItem::new("Attach library by default", library_attached)
                    .description("Ride the whole .bib with every question."),
            ),
        )
}

/// The Editor page: preview, pane visibility, and document context.
pub fn app_editor_page(state: Entity<AppState>) -> SettingPage {
    let live_preview = state_switch(
        state.clone(),
        |state| !state.raw_mode,
        |state, value, cx| state.set_raw_mode(!value, cx),
    );
    let show_sci_co = state_switch(
        state.clone(),
        |state| state.show_agent,
        |state, value, cx| {
            state.show_agent = value;
            state.persist(cx);
            state.persist_session(cx);
            cx.notify();
        },
    );
    let document_attached = state_switch(
        state.clone(),
        |state| state.document_attached,
        |state, value, cx| {
            state.document_attached = value;
            state.persist(cx);
            cx.notify();
        },
    );

    SettingPage::new("Editor")
        .description("How the editor renders and which panes stay open.")
        .default_open(false)
        .group(
            SettingGroup::new()
                .title("Editor")
                .item(
                    SettingItem::new("Live preview", live_preview)
                        .description("Off shows raw Markdown; Ctrl+Shift+E toggles it."),
                )
                .item(
                    SettingItem::new("Show SciCo", show_sci_co)
                        .description("Right pane with the RAG transcript."),
                ),
        )
        .group(
            SettingGroup::new().title("Context").item(
                SettingItem::new("Attach document by default", document_attached)
                    .description("Ride the active buffer with every question."),
            ),
        )
}

fn viewer_field(
    state: Entity<AppState>,
    kind: ViewerKind,
    key: &'static str,
) -> SettingField<SharedString> {
    path_setting(
        key,
        "Choose the viewer executable",
        false,
        true,
        {
            let state = state.clone();
            move |cx| state.read(cx).settings.viewers.program(kind).to_owned()
        },
        {
            let state = state.clone();
            move |value, cx| {
                state.update(cx, |state, cx| {
                    state.settings.viewers.set(kind, value);
                    state.persist(cx);
                    cx.notify();
                });
            }
        },
    )
}

fn state_switch<R, W>(state: Entity<AppState>, read: R, write: W) -> SettingField<bool>
where
    R: Fn(&AppState) -> bool + 'static,
    W: Fn(&mut AppState, bool, &mut gpui_kit::Context<AppState>) + 'static,
{
    SettingField::switch(
        {
            let state = state.clone();
            move |cx: &App| read(state.read(cx))
        },
        {
            let state = state.clone();
            move |value: bool, cx: &mut App| {
                state.update(cx, |state, cx| write(state, value, cx));
            }
        },
    )
}
