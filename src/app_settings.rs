//! The Sciscraper-specific settings page.
//!
//! Ragrig supplies the model/retrieval part; this page owns the workspace,
//! corpus, bibliography, pane visibility, and the editor's live-preview switch.

use std::path::PathBuf;

use gpui_kit::{
    App, Entity, ParentElement as _, Styled as _,
    component::{
        ActiveTheme as _, Sizable as _,
        button::ButtonVariants as _,
        setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    },
};
use ragrig_gpui::path_setting;

use crate::state::AppState;

/// Build the app page that sits beside Ragrig's settings page.
pub fn app_settings_page(state: Entity<AppState>) -> SettingPage {
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

    let show_library = SettingField::switch(
        {
            let state = state.clone();
            move |cx: &App| state.read(cx).show_library
        },
        {
            let state = state.clone();
            move |value: bool, cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.show_library = value;
                    state.persist(cx);
                    state.persist_session(cx);
                    cx.notify();
                });
            }
        },
    );
    let show_agent = SettingField::switch(
        {
            let state = state.clone();
            move |cx: &App| state.read(cx).show_agent
        },
        {
            let state = state.clone();
            move |value: bool, cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.show_agent = value;
                    state.persist(cx);
                    state.persist_session(cx);
                    cx.notify();
                });
            }
        },
    );
    let document_attached = SettingField::switch(
        {
            let state = state.clone();
            move |cx: &App| state.read(cx).document_attached
        },
        {
            let state = state.clone();
            move |value: bool, cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.document_attached = value;
                    state.persist(cx);
                    cx.notify();
                });
            }
        },
    );
    let library_attached = SettingField::switch(
        {
            let state = state.clone();
            move |cx: &App| state.read(cx).library_attached
        },
        {
            let state = state.clone();
            move |value: bool, cx: &mut App| {
                state.update(cx, |state, cx| {
                    state.library_attached = value;
                    state.persist(cx);
                    cx.notify();
                });
            }
        },
    );
    let live_preview = SettingField::switch(
        {
            let state = state.clone();
            move |cx: &App| !state.read(cx).raw_mode
        },
        {
            let state = state.clone();
            move |value: bool, cx: &mut App| {
                state.update(cx, |state, cx| state.set_raw_mode(!value, cx));
            }
        },
    );

    let apply = SettingItem::render({
        let state = state.clone();
        move |_, _, cx| {
            let button = gpui_kit::component::button::Button::new("sciscraper-settings-apply")
                .label("Apply to agent")
                .icon(gpui_kit::assets::IconName::Play)
                .primary()
                .small()
                .on_click({
                    let state = state.clone();
                    move |_, _, cx| {
                        state.update(cx, |state, cx| state.apply_rag(cx));
                    }
                });
            gpui_kit::component::h_flex()
                .gap_3()
                .items_center()
                .child(button)
                .child(
                    gpui_kit::div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(state.read(cx).status.clone()),
                )
        }
    });

    SettingPage::new("Sciscraper")
        .description("Workspace, editor, and what rides along with a question.")
        .default_open(false)
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
                ),
        )
        .group(
            SettingGroup::new()
                .title("Editor")
                .item(
                    SettingItem::new("Live preview", live_preview)
                        .description("Off shows raw Markdown; Ctrl+Shift+E toggles it."),
                )
                .item(
                    SettingItem::new("Show library", show_library)
                        .description("Left pane with references and corpus actions."),
                )
                .item(
                    SettingItem::new("Show co-writer", show_agent)
                        .description("Right pane with the RAG transcript."),
                ),
        )
        .group(
            SettingGroup::new()
                .title("Context")
                .item(
                    SettingItem::new("Attach document by default", document_attached)
                        .description("Ride the active buffer with every question."),
                )
                .item(
                    SettingItem::new("Attach library by default", library_attached)
                        .description("Ride the whole .bib with every question."),
                ),
        )
        .group(SettingGroup::new().title("Apply").item(apply))
}
