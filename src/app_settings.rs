//! The Sciscraper-specific settings pages.
//!
//! Ragrig supplies the model/retrieval page and the reference-ingestion group.
//! These pages own the workspace, the editor layout, external viewers, and the
//! profile/apply overview.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{
    App, Entity, FontWeight, IntoElement, ParentElement as _, SharedString, Styled as _,
    component::{
        ActiveTheme as _, Disableable as _, Sizable as _,
        button::Button,
        h_flex,
        setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage},
        v_flex,
    },
    div,
    prelude::FluentBuilder as _,
};

use crate::settings::{SCICO_PROMPT_FILE, ViewerKind};
use crate::state::AppState;
use ragrig_gpui::{
    Changes, RagChanges, RagField, RagSection, SettingRow, dirty_color, dirty_header,
    dropdown_control, number_control, path_control, path_setting, text_control,
};

/// A live view of the unsaved RAG settings.
fn changes_of(state: &Entity<AppState>) -> Changes {
    let state = state.clone();
    Rc::new(move |cx| state.read(cx).rag_changes(cx))
}

fn page_title(label: &str, dirty: bool) -> SharedString {
    if dirty {
        format!("{label} ●").into()
    } else {
        SharedString::from(label)
    }
}

/// The Overview page: profiles and the Apply row.
///
/// Apply, Profile Defaults, Update Profile, and Load profile sit together in
/// the footer so the profile workflow stays in one place.
pub fn app_overview_page(state: Entity<AppState>, changes: &RagChanges, cx: &App) -> SettingPage {
    let profile = SettingRow::new(
        "Profile",
        dropdown_control(
            "overview-profile",
            {
                let state = state.clone();
                move |cx| profile_options(&state, cx)
            },
            {
                let state = state.clone();
                move |cx| state.read(cx).rag.read(cx).profile_name.clone()
            },
            {
                let state = state.clone();
                move |value, cx| {
                    state.update(cx, |state, cx| {
                        state.rag.update(cx, |rag, cx| {
                            rag.profile_name = value;
                            cx.notify();
                        });
                        cx.notify();
                    });
                }
            },
        ),
    )
    .description("Load and Update Profile act on this profile.");

    let profile_name = SettingRow::new(
        "Profile name",
        text_control(
            "overview-profile-name",
            {
                let state = state.clone();
                move |cx| state.read(cx).rag.read(cx).profile_name.clone()
            },
            {
                let state = state.clone();
                move |value, cx| {
                    state.update(cx, |state, cx| {
                        state.rag.update(cx, |rag, cx| {
                            rag.profile_name = value;
                            cx.notify();
                        });
                        cx.notify();
                    });
                }
            },
        ),
    )
    .description("Update Profile writes the current values here; a new name creates a profile.");

    let summary = SettingItem::render({
        let state = state.clone();
        move |_, _, cx| {
            let changes = state.read(cx).rag_changes(cx);
            let mut lines = v_flex().gap_1();
            if changes.is_empty() {
                lines = lines.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("All settings are applied."),
                );
            } else {
                for section in RagSection::ALL {
                    let count = changes.count_in(section);
                    if count > 0 {
                        lines = lines.child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(dirty_color(cx))
                                .child(format!(
                                    "{} — {count} {}",
                                    section.label(),
                                    if count == 1 { "change" } else { "changes" }
                                )),
                        );
                    }
                }
            }
            lines.into_any_element()
        }
    });

    let unsaved = changes.any();
    let unsaved_count = changes.len();
    let summary_group = SettingGroup::new()
        .title(page_title("Pending changes", unsaved))
        .item(summary)
        .footer({
            let state = state.clone();
            move |_, cx| actions_row(state.clone(), cx)
        });

    let profile_group = SettingGroup::new()
        .title("Profiles")
        .item(profile.into_item())
        .item(profile_name.into_item())
        .footer({
            let state = state.clone();
            move |_, cx| {
                let status = state.read(cx).rag.read(cx).status.clone();
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(status.unwrap_or_else(|| {
                        "Profiles are JSON files in .ragrig/profiles; secrets are never written."
                            .to_owned()
                    }))
            }
        });

    let page = SettingPage::new(page_title("Overview", unsaved))
        .description("Profiles and pending changes. Apply rebuilds the agent.")
        .default_open(true)
        .title_suffix(move |_, cx| {
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(dirty_color(cx))
                .child(if unsaved {
                    format!("{unsaved_count} unsaved")
                } else {
                    String::new()
                })
        });
    let page = if unsaved {
        page.header_style(&dirty_header(cx))
    } else {
        page
    };
    page.group(profile_group).group(summary_group)
}

fn actions_row(state: Entity<AppState>, cx: &mut App) -> gpui_kit::AnyElement {
    let changes = state.read(cx).rag_changes(cx);
    let unsaved = changes.any();
    let violet = dirty_color(cx);

    let apply = {
        let state = state.clone();
        Button::new("overview-apply")
            .label("Apply")
            .small()
            .disabled(!unsaved)
            .when(unsaved, |this| {
                this.bg(violet)
                    .border_color(violet)
                    .text_color(gpui_kit::white())
            })
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| state.apply_rag(cx));
            })
    };
    let defaults = {
        let state = state.clone();
        Button::new("overview-profile-defaults")
            .label("Profile Defaults")
            .small()
            .outline()
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| {
                    state.rag.update(cx, |rag, cx| {
                        if let Err(message) = rag.reset_to_profile_defaults() {
                            rag.status = Some(message);
                        }
                        cx.notify();
                    });
                    cx.notify();
                });
            })
    };
    let update = {
        let state = state.clone();
        Button::new("overview-profile-update")
            .label("Update Profile")
            .small()
            .outline()
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| {
                    state.rag.update(cx, |rag, cx| {
                        if let Err(message) = rag.save_profile() {
                            rag.status = Some(message);
                        }
                        cx.notify();
                    });
                    state.persist(cx);
                    cx.notify();
                });
            })
    };
    let load = {
        let state = state.clone();
        Button::new("overview-profile-load")
            .label("Load profile")
            .icon(gpui_kit::assets::IconName::FolderOpen)
            .small()
            .outline()
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| {
                    state.rag.update(cx, |rag, cx| {
                        let name = rag.profile_name.trim().to_owned();
                        if name.is_empty() {
                            rag.status = Some("choose a profile first".to_owned());
                        } else if let Err(message) = rag.load_profile(&name) {
                            rag.status = Some(message);
                        }
                        cx.notify();
                    });
                    cx.notify();
                });
            })
    };

    let controller = state.read(cx).controller.clone();
    let status = state
        .read(cx)
        .rag
        .read(cx)
        .status
        .clone()
        .unwrap_or_else(|| controller.read(cx).status_line());
    let has_error = controller.read(cx).error().is_some();
    let color = if has_error {
        cx.theme().danger
    } else {
        cx.theme().muted_foreground
    };

    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(apply)
                .child(defaults)
                .child(update)
                .child(load),
        )
        .child(div().text_xs().text_color(color).child(status))
        .into_any_element()
}

fn profile_options(state: &Entity<AppState>, cx: &App) -> Vec<(SharedString, SharedString)> {
    let rag = state.read(cx).rag.read(cx);
    let mut names = rag.profiles.clone();
    let current = rag.profile_name.trim().to_owned();
    if !current.is_empty() && !names.iter().any(|name| name == &current) {
        names.push(current);
    }
    names
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| (name.as_str().into(), name.as_str().into()))
        .collect()
}

/// Host actions for the Scico prompt row in **Settings → RAG → Chat**.
///
/// The generic page owns the path field and dirty mark; these closures open the
/// effective prompt in the editor and export the compiled-in `Scico.md` into the
/// workspace.
pub fn prompt_actions(state: Entity<AppState>) -> ragrig_gpui::PromptActions {
    ragrig_gpui::PromptActions {
        built_in_name: SCICO_PROMPT_FILE,
        open: {
            let state = state.clone();
            Rc::new(move |_, window, cx| {
                state.update(cx, |state, cx| {
                    if let Err(error) = state.open_scico_prompt(window, cx) {
                        state.set_status(error, cx);
                    }
                });
            })
        },
        export: {
            let state = state.clone();
            Rc::new(move |window, cx| {
                state.update(cx, |state, cx| state.export_scico_prompt(window, cx))
            })
        },
    }
}

/// The Library page: paths, viewers, and library context.
pub fn app_library_page(state: Entity<AppState>, cx: &App) -> SettingPage {
    let now = state.read(cx).rag_changes(cx);
    let changes = changes_of(&state);
    let dirty = now.section(RagSection::Workspace) || now.section(RagSection::References);
    let title = page_title("Library", dirty);

    let workspace = path_control(
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
    let workspace_row = SettingRow::new("Workspace folder", workspace)
        .description("Documents, .ragrig store, and sessions.")
        .dirty({
            let state = state.clone();
            move |cx| state.read(cx).rag_changes(cx).contains(RagField::Workspace)
        });

    let corpus = path_control(
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
    let corpus_row = SettingRow::new("Corpus folder", corpus)
        .description("Indexed papers; Add PDFs and GROBID use this folder.");

    let bibliography = path_control(
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
    let bibliography_row = SettingRow::new("Bibliography", bibliography)
        .description("The .bib the library shows and GROBID merges into.")
        .dirty({
            let state = state.clone();
            move |cx| {
                state
                    .read(cx)
                    .rag_changes(cx)
                    .contains(RagField::Bibliography)
            }
        });

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

    let references = state.read(cx).rag.clone();

    let page = SettingPage::new(title)
        .description("Workspace, bibliography, external viewers, and library context.")
        .default_open(true);
    let page = if dirty {
        page.header_style(&dirty_header(cx))
    } else {
        page
    };
    page.group(
        SettingGroup::new()
            .title(page_title("Workspace", now.section(RagSection::Workspace)))
            .item(workspace_row.into_item())
            .item(corpus_row.into_item())
            .item(bibliography_row.into_item())
            .item(
                SettingItem::new("Show library pane", show_library)
                    .description("Left pane with references and corpus actions."),
            ),
    )
    .group(viewers)
    .group(ragrig_gpui::rag_references_group(&references, &changes))
    .group(
        SettingGroup::new().title("Context").item(
            SettingItem::new("Attach library by default", library_attached)
                .description("Ride the whole .bib with every question."),
        ),
    )
}

/// The Editor page: preview and document context.
pub fn app_editor_page(state: Entity<AppState>) -> SettingPage {
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

    let font_row = SettingRow::new(
        "Editor font",
        dropdown_control(
            "editor-font",
            {
                let state = state.clone();
                move |cx| editor_font_options(cx, state.read(cx).editor_font.as_str())
            },
            {
                let state = state.clone();
                move |cx| state.read(cx).editor_font_family(cx).to_string()
            },
            {
                let state = state.clone();
                move |value, cx| {
                    state.update(cx, |state, cx| state.set_editor_font(value, cx));
                }
            },
        ),
    )
    .description("Monospace family used by the source editor.")
    .into_item();
    let font_size_row = SettingRow::new(
        "Font size",
        number_control(
            "editor-font-size",
            NumberFieldOptions {
                min: 8.0,
                max: 48.0,
                step: 0.5,
            },
            {
                let state = state.clone();
                move |cx| state.read(cx).editor_font_size as f64
            },
            {
                let state = state.clone();
                move |value, cx| {
                    state.update(cx, |state, cx| {
                        state.set_editor_font_size(value as f32, cx);
                    });
                }
            },
        ),
    )
    .description("Pixels. Ctrl+wheel over the editor zooms; wheel up zooms out.")
    .into_item();

    let editor_group = {
        let group = SettingGroup::new().title("Editor");
        #[cfg(feature = "gpuikit")]
        let group = {
            let show_preview = state_switch(
                state.clone(),
                |state| state.show_preview,
                |state, value, cx| state.set_show_preview(value, cx),
            );
            let sync_scroll = state_switch(
                state.clone(),
                |state| state.sync_scroll,
                |state, value, cx| state.set_sync_scroll(value, cx),
            );
            group
                .item(font_row)
                .item(font_size_row)
                .item(
                    SettingItem::new("Rendered preview", show_preview).description(
                        "Show the Markdown preview beside the source; Ctrl+Shift+E toggles it.",
                    ),
                )
                .item(
                    SettingItem::new("Sync scroll", sync_scroll)
                        .description("Keep the source and the preview on the same Markdown block."),
                )
        };
        #[cfg(feature = "zorite")]
        let group = {
            let live_preview = state_switch(
                state.clone(),
                |state| !state.raw_mode,
                |state, value, cx| state.set_raw_mode(!value, cx),
            );
            group.item(font_row).item(font_size_row).item(
                SettingItem::new("Live preview", live_preview)
                    .description("Off shows raw Markdown; Ctrl+Shift+E toggles it."),
            )
        };
        group.item(
            SettingItem::new("Show Scico", show_sci_co)
                .description("Right pane with the RAG transcript."),
        )
    };

    SettingPage::new("Editor")
        .description("How the editor renders and which panes stay open.")
        .default_open(false)
        .group(editor_group)
        .group(
            SettingGroup::new().title("Context").item(
                SettingItem::new("Attach document by default", document_attached)
                    .description("Ride the active buffer with every question."),
            ),
        )
}

/// Installed monospace families, plus the current family when it is not one.
fn editor_font_options(cx: &App, current: &str) -> Vec<(SharedString, SharedString)> {
    let theme_family = cx.theme().mono_font_family.to_string();
    let mut names: Vec<String> = cx
        .text_system()
        .all_font_names()
        .into_iter()
        .filter(|name| crate::state::is_monospace_font(name))
        .collect();
    names.sort_by_key(|name| name.to_ascii_lowercase());
    names.dedup();
    if !names.iter().any(|name| name == &theme_family) {
        names.insert(0, theme_family);
    }
    if !current.trim().is_empty() && !names.iter().any(|name| name == current) {
        names.push(current.to_owned());
    }
    names
        .into_iter()
        .map(|name| (name.clone().into(), name.into()))
        .collect()
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
