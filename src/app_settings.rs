//! The Scico-specific settings pages.
//!
//! Ragrig supplies the ingestion, chat, memory, and retrieval groups. These
//! pages split them between the Workspace sheet (folders, editor, parsing,
//! embeddings, references, profiles) and the Scico sheet (chat, memory,
//! retrieval, context). Both sheets share one Apply action.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{
    App, AppContext as _, Entity, IntoElement, ParentElement as _, SharedString, Styled as _,
    Subscription,
    assets::IconName,
    component::{
        ActiveTheme as _, Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::{Input, InputEvent, InputState},
        setting::{NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage},
        v_flex,
    },
    div,
    prelude::FluentBuilder as _,
    px,
};

use crate::settings::{CHAT_PROMPT_FILE, ViewerKind};
use crate::state::AppState;
use ragrig_gpui::{
    Changes, ChatProvider, Control, EmbedProvider, RagChanges, RagField, RagSection, RagSettings,
    ServiceKind, ServiceSource, SettingRow, control, docker_group, dropdown_control,
    llamacpp_group, number_control, ollama_group, path_control_home, path_setting, rag_chat_group,
    rag_corpora_group, rag_embedding_group, rag_memory_group, rag_parsing_group,
    rag_reference_processing_group, rag_retrieval_group,
};

/// Pages inside the Workspace settings sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceSettingsPage {
    /// Workspace, corpus, bibliography, and pane layout.
    Workspace = 0,
    /// Embedding provider, model, hybrid switch, and local service.
    Embedding = 1,
    /// Parser, chunking, and folder corpora.
    Parsing = 2,
    /// GROBID and reference processing.
    References = 3,
    /// Profile management.
    Profiles = 4,
    /// Editor and external viewers.
    Editor = 5,
}

/// Pages inside the Scico settings sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScicoSettingsPage {
    /// Provider, model, generation, and prompt.
    Chat = 0,
    /// Conversation memory.
    Memory = 1,
    /// Top-k and similarity threshold.
    Retrieval = 2,
    /// Chat context and pane visibility.
    Editor = 3,
}

impl WorkspaceSettingsPage {
    /// Every page, in sidebar order.
    pub(crate) const ALL: [Self; 6] = [
        Self::Workspace,
        Self::Embedding,
        Self::Parsing,
        Self::References,
        Self::Profiles,
        Self::Editor,
    ];

    /// Stable page index for the `Settings` sidebar.
    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

impl ScicoSettingsPage {
    /// Every page, in sidebar order.
    pub(crate) const ALL: [Self; 4] = [Self::Chat, Self::Memory, Self::Retrieval, Self::Editor];

    /// Stable page index for the `Settings` sidebar.
    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// The name field shown by **New**.
struct ProfileNameInput {
    input: Entity<InputState>,
    _subscription: Subscription,
}

fn page_title(label: &str, dirty: bool) -> SharedString {
    if dirty {
        format!("{label} ●").into()
    } else {
        SharedString::from(label)
    }
}

/// The idle hint under the profile picker.
fn profile_footer() -> String {
    let env_file = crate::env::env_file_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "the Scico env file".to_owned());
    format!(
        "Profiles are JSON files in .ragrig/profiles; secrets are never written. \
         Provider keys come from the environment or {env_file}."
    )
}

/// Build the Workspace settings pages.
pub(crate) fn workspace_settings_pages(
    state: Entity<AppState>,
    services: Entity<ragrig_gpui::ExternalServices>,
    changes: &Changes,
    cx: &App,
) -> Vec<SettingPage> {
    let rag = state.read(cx).rag.clone();
    let controller = state.read(cx).controller.clone();
    let now = state.read(cx).rag_changes(cx);

    let workspace = SettingPage::new(page_title("Workspace", now.section(RagSection::Workspace)))
        .description("Folders, persistence, and pane visibility.")
        .group(workspace_layout_group(state.clone()));

    let embedding_dirty =
        now.section(RagSection::Embedding) || embedding_service_dirty(&rag, &now, cx);
    let embedding = SettingPage::new(page_title("Embedding", embedding_dirty))
        .description("How the workspace index is embedded. Follows the chat provider until you choose another backend.")
        .group(rag_embedding_group(&rag, changes, Some(&controller), cx))
        .groups(embedding_service_groups(&rag, &services, changes, cx));

    let parsing_dirty = now.section(RagSection::Parsing) || now.section(RagSection::Corpora);
    let parsing = SettingPage::new(page_title("Parsing", parsing_dirty))
        .description("PDF backend, chunking, and extra folder corpora.")
        .group(rag_parsing_group(&rag, changes, cx))
        .group(rag_corpora_group(&rag, changes, cx));

    let mut reference_groups = vec![rag_reference_processing_group(&rag, changes, cx)];
    reference_groups.extend(reference_service_groups(&rag, &services, changes, cx));
    let references_dirty = now.section(RagSection::References) || now.section(RagSection::Grobid);
    let references = SettingPage::new(page_title("References", references_dirty))
        .description("GROBID parsing and the bibliography record pipeline.")
        .groups(reference_groups);

    let profiles = SettingPage::new("Profiles")
        .description("Load, create, save, and reset workspace profiles.")
        .group(profiles_group(state.clone()));

    let editor = SettingPage::new("Editor")
        .description("Editor rendering, external viewers, and pane layout.")
        .group(editor_group(state.clone()))
        .group(viewers_group(state));

    let pages = vec![workspace, embedding, parsing, references, profiles, editor];
    debug_assert_eq!(pages.len(), WorkspaceSettingsPage::ALL.len());
    pages
}

/// Build the Scico settings pages.
pub(crate) fn scico_settings_pages(
    state: Entity<AppState>,
    services: Entity<ragrig_gpui::ExternalServices>,
    changes: &Changes,
    cx: &App,
) -> Vec<SettingPage> {
    let rag = state.read(cx).rag.clone();
    let now = state.read(cx).rag_changes(cx);
    let prompt = prompt_actions(state.clone());

    let chat_dirty = now.section(RagSection::Chat) || chat_service_dirty(&rag, &now, cx);
    let chat = SettingPage::new(page_title("Chat", chat_dirty))
        .description("The model that writes, its generation limits, and the system prompt.")
        .group(rag_chat_group(&rag, changes, Some(&prompt), cx))
        .groups(chat_service_groups(&rag, &services, changes, cx));

    let memory = SettingPage::new(page_title("Memory", now.section(RagSection::Memory)))
        .description("Conversation memory and the small rewriting model.")
        .group(rag_memory_group(&rag, changes, cx));

    let retrieval = SettingPage::new(page_title("Retrieval", now.section(RagSection::Retrieval)))
        .description("How many chunks Scico reads and how similar they must be.")
        .group(rag_retrieval_group(&rag, changes, cx));

    let editor = SettingPage::new("Context")
        .description("Which parts of the workspace ride along with a question.")
        .group(chat_context_group(state));

    let pages = vec![chat, memory, retrieval, editor];
    debug_assert_eq!(pages.len(), ScicoSettingsPage::ALL.len());
    pages
}

fn workspace_layout_group(state: Entity<AppState>) -> SettingGroup {
    let workspace = path_control_home(
        "app-workspace",
        "Choose the workspace folder",
        true,
        false,
        px(510.),
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
        .description("Paper, corpus/, bibliography, .ragrig, and .scico descriptor.")
        .dirty({
            let state = state.clone();
            move |cx| state.read(cx).rag_changes(cx).contains(RagField::Workspace)
        });

    let corpus = path_control_home(
        "app-corpus",
        "Choose the corpus folder",
        true,
        false,
        px(510.),
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
        .description("Source documents; workspaces default to <workspace>/corpus.");

    let bibliography = path_control_home(
        "app-bibliography",
        "Choose the bibliography file",
        false,
        true,
        px(510.),
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

    SettingGroup::new()
        .title("Workspace layout")
        .item(workspace_row.into_item())
        .item(corpus_row.into_item())
        .item(bibliography_row.into_item())
        .item(
            SettingItem::new("Show workspace pane", show_library)
                .description("Left pane with the workspace, references, and corpus actions."),
        )
}

fn profiles_group(state: Entity<AppState>) -> SettingGroup {
    let profile = SettingRow::new("Profile", profile_control(state.clone())).description(
        "Choosing a profile loads it; Update Profile saves to it. New asks for a name.",
    );
    let actions = SettingItem::render({
        let state = state.clone();
        move |_, _, cx| profile_actions(state.clone(), cx)
    });

    SettingGroup::new()
        .title("Profiles")
        .item(profile.into_item())
        .item(actions)
        .footer(|_, cx| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(profile_footer())
        })
}

fn profile_actions(state: Entity<AppState>, _cx: &mut App) -> gpui_kit::AnyElement {
    let defaults = {
        let state = state.clone();
        Button::new("workspace-profile-defaults")
            .label("Profile Defaults")
            .small()
            .outline()
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| {
                    state.rag.update(cx, |rag, cx| {
                        let uses_profile = rag
                            .profiles
                            .iter()
                            .any(|profile| profile == rag.profile_name.trim());
                        if let Err(message) = rag.reset_to_profile_defaults() {
                            rag.status = Some(message);
                        } else if !uses_profile {
                            crate::settings::apply_standard_rag_settings(rag);
                        }
                        cx.notify();
                    });
                    cx.notify();
                });
            })
    };
    let update = {
        let state = state.clone();
        Button::new("workspace-profile-update")
            .label("Update Profile")
            .small()
            .outline()
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| state.save_profile(cx));
            })
    };
    let setup = {
        let state = state.clone();
        Button::new("workspace-setup-guide")
            .label("Setup guide")
            .small()
            .outline()
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| state.restart_onboarding(cx));
            })
    };

    h_flex()
        .flex_wrap()
        .gap_2()
        .items_center()
        .child(defaults)
        .child(update)
        .child(setup)
        .into_any_element()
}

fn editor_group(state: Entity<AppState>) -> SettingGroup {
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

    let group = SettingGroup::new().title("Editor");
    #[cfg(editor_gpuikit)]
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
    #[cfg(editor_zorite)]
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
    #[cfg(editor_block)]
    let group = group.item(font_row).item(font_size_row);
    group
}

fn viewers_group(state: Entity<AppState>) -> SettingGroup {
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
    viewers.footer(|_, cx| {
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child("The document path is passed as the only argument.")
    })
}

fn chat_context_group(state: Entity<AppState>) -> SettingGroup {
    let show_chat = state_switch(
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
    let library_attached = state_switch(
        state.clone(),
        |state| state.library_attached,
        |state, value, cx| {
            state.library_attached = value;
            state.persist(cx);
            cx.notify();
        },
    );

    SettingGroup::new()
        .title("Context")
        .item(
            SettingItem::new("Show Scico", show_chat)
                .description("Right pane with the RAG transcript."),
        )
        .item(
            SettingItem::new("Attach document by default", document_attached)
                .description("Ride the active buffer with every question."),
        )
        .item(
            SettingItem::new("Attach library by default", library_attached)
                .description("Ride the whole .bib with every question."),
        )
}

fn embedding_service_groups(
    settings: &Entity<RagSettings>,
    services: &Entity<ragrig_gpui::ExternalServices>,
    changes: &Changes,
    cx: &App,
) -> Vec<SettingGroup> {
    let provider = settings.read(cx).embed_provider;
    let kind = match provider {
        EmbedProvider::Ollama => Some(ServiceKind::Ollama),
        EmbedProvider::LlamaCpp => Some(ServiceKind::LlamaCpp),
        _ => None,
    };
    let Some(kind) = kind else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    if docker_needed(settings, &[kind], cx) {
        groups.push(docker_group(settings.clone(), services.clone()));
    }
    groups.push(match kind {
        ServiceKind::Ollama => {
            ollama_group(settings.clone(), services.clone(), changes.clone(), cx)
        }
        ServiceKind::LlamaCpp => {
            llamacpp_group(settings.clone(), services.clone(), changes.clone(), cx)
        }
        ServiceKind::Grobid => unreachable!("the embedder cannot use GROBID"),
    });
    groups
}

fn chat_service_groups(
    settings: &Entity<RagSettings>,
    services: &Entity<ragrig_gpui::ExternalServices>,
    changes: &Changes,
    cx: &App,
) -> Vec<SettingGroup> {
    let provider = settings.read(cx).provider;
    let kind = match provider {
        ChatProvider::Ollama => Some(ServiceKind::Ollama),
        ChatProvider::LlamaCpp => Some(ServiceKind::LlamaCpp),
        _ => None,
    };
    let Some(kind) = kind else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    if docker_needed(settings, &[kind], cx) {
        groups.push(docker_group(settings.clone(), services.clone()));
    }
    groups.push(match kind {
        ServiceKind::Ollama => {
            ollama_group(settings.clone(), services.clone(), changes.clone(), cx)
        }
        ServiceKind::LlamaCpp => {
            llamacpp_group(settings.clone(), services.clone(), changes.clone(), cx)
        }
        ServiceKind::Grobid => unreachable!("the chat model cannot use GROBID"),
    });
    groups
}

fn reference_service_groups(
    settings: &Entity<RagSettings>,
    services: &Entity<ragrig_gpui::ExternalServices>,
    changes: &Changes,
    cx: &App,
) -> Vec<SettingGroup> {
    let mut groups = Vec::new();
    if docker_needed(settings, &[ServiceKind::Grobid], cx) {
        groups.push(docker_group(settings.clone(), services.clone()));
    }
    groups.push(ragrig_gpui::grobid_group(
        settings.clone(),
        services.clone(),
        changes.clone(),
        cx,
    ));
    groups
}

fn docker_needed(settings: &Entity<RagSettings>, kinds: &[ServiceKind], cx: &App) -> bool {
    let settings = settings.read(cx);
    kinds.iter().any(|kind| {
        matches!(
            settings.service_source(*kind),
            ServiceSource::Managed | ServiceSource::ExistingContainer
        )
    })
}

fn embedding_service_dirty(settings: &Entity<RagSettings>, now: &RagChanges, cx: &App) -> bool {
    match settings.read(cx).embed_provider {
        EmbedProvider::Ollama => service_dirty(now, ServiceKind::Ollama),
        EmbedProvider::LlamaCpp => service_dirty(now, ServiceKind::LlamaCpp),
        _ => false,
    }
}

fn chat_service_dirty(settings: &Entity<RagSettings>, now: &RagChanges, cx: &App) -> bool {
    match settings.read(cx).provider {
        ChatProvider::Ollama => service_dirty(now, ServiceKind::Ollama),
        ChatProvider::LlamaCpp => service_dirty(now, ServiceKind::LlamaCpp),
        _ => false,
    }
}

fn service_dirty(changes: &RagChanges, kind: ServiceKind) -> bool {
    let fields: &[RagField] = match kind {
        ServiceKind::Grobid => &[
            RagField::GrobidSource,
            RagField::GrobidUrl,
            RagField::GrobidImage,
            RagField::GrobidAutoStart,
            RagField::GrobidContainer,
        ],
        ServiceKind::Ollama => &[
            RagField::OllamaSource,
            RagField::OllamaImage,
            RagField::OllamaAutoStart,
            RagField::OllamaContainer,
        ],
        ServiceKind::LlamaCpp => &[
            RagField::LlamaCppSource,
            RagField::LlamaCppImage,
            RagField::LlamaCppModel,
            RagField::LlamaCppArgs,
            RagField::LlamaCppAutoStart,
            RagField::LlamaCppContainer,
        ],
    };
    fields.iter().any(|field| changes.contains(*field))
}

/// The profile picker and **New** in one row.
///
/// **New** swaps the picker for a name field with **Create** and **Cancel**.
/// The name is checked against existing profiles before the settings are
/// snapshotted, so creating never overwrites one.
fn profile_control(state: Entity<AppState>) -> Control {
    let picker = dropdown_control(
        "workspace-profile",
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
                state.update(cx, |state, cx| state.load_profile(&value, cx));
            }
        },
    );
    control(move |options, window, cx| {
        let (creating, error) = {
            let state = state.read(cx);
            (state.creating_profile, state.profile_draft_error.clone())
        };
        let draft =
            window.use_keyed_state(SharedString::from("workspace-profile-new-input"), cx, {
                let state = state.clone();
                move |window, cx| {
                    let input =
                        cx.new(|cx| InputState::new(window, cx).placeholder("Profile name"));
                    let subscription = cx.subscribe(
                        &input,
                        move |_, input, event: &InputEvent, cx| match event {
                            InputEvent::Change => {
                                state.update(cx, |state, cx| state.clear_profile_error(cx));
                            }
                            InputEvent::PressEnter { .. } if state.read(cx).creating_profile => {
                                let name = input.read(cx).value().to_string();
                                state.update(cx, |state, cx| state.create_profile(name, cx));
                            }
                            _ => {}
                        },
                    );
                    ProfileNameInput {
                        input,
                        _subscription: subscription,
                    }
                }
            });
        let input = draft.read(cx).input.clone();

        let mut row = h_flex().flex_wrap().gap_2().items_center();

        if creating {
            let state_for_create = state.clone();
            let input_for_create = input.clone();
            let create = Button::new("workspace-profile-create")
                .label("Create")
                .small()
                .primary()
                .disabled(options.is_disabled())
                .on_click(move |_, _, cx| {
                    let name = input_for_create.read(cx).value().to_string();
                    state_for_create.update(cx, |state, cx| state.create_profile(name, cx));
                });
            let state_for_cancel = state.clone();
            let cancel = Button::new("workspace-profile-cancel")
                .label("Cancel")
                .small()
                .outline()
                .on_click(move |_, _, cx| {
                    state_for_cancel.update(cx, |state, cx| state.cancel_new_profile(cx));
                });
            row = row
                .child(
                    div().w(px(180.)).child(
                        Input::new(&input)
                            .with_size(options.size())
                            .disabled(options.is_disabled())
                            .w_full(),
                    ),
                )
                .child(create)
                .child(cancel);
        } else {
            let state_for_new = state.clone();
            let input_for_new = input.clone();
            let new = Button::new("workspace-profile-new")
                .label("New")
                .icon(IconName::Plus)
                .small()
                .outline()
                .tooltip("Create a profile from the current settings")
                .on_click(move |_, window, cx| {
                    state_for_new.update(cx, |state, cx| state.begin_new_profile(cx));
                    input_for_new.update(cx, |input, cx| {
                        input.set_value("", window, cx);
                        input.focus(window, cx);
                    });
                });
            row = row.child(picker(options, window, cx)).child(new);
        }

        v_flex()
            .gap_1()
            .child(row)
            .when_some(error.filter(|_| creating), |this, error| {
                this.child(div().text_xs().text_color(cx.theme().danger).child(error))
            })
            .into_any_element()
    })
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

/// Host actions for the Scico prompt row in **Settings → Scico → Chat**.
///
/// The generic page owns the path field and dirty mark; these closures open the
/// effective prompt in the editor and export the compiled-in `Chat.md` into the
/// workspace.
pub fn prompt_actions(state: Entity<AppState>) -> ragrig_gpui::PromptActions {
    ragrig_gpui::PromptActions {
        built_in_name: CHAT_PROMPT_FILE,
        open: {
            let state = state.clone();
            Rc::new(move |_, window, cx| {
                state.update(cx, |state, cx| {
                    if let Err(error) = state.open_chat_prompt(window, cx) {
                        state.set_status(error, cx);
                    }
                });
            })
        },
        export: {
            let state = state.clone();
            Rc::new(move |window, cx| {
                state.update(cx, |state, cx| state.export_chat_prompt(window, cx))
            })
        },
    }
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

#[cfg(test)]
mod tests {
    use super::{ScicoSettingsPage, WorkspaceSettingsPage, profile_footer};

    #[test]
    fn the_profile_footer_names_the_environment_file() {
        let footer = profile_footer();

        assert!(footer.contains(".ragrig/profiles"));
        assert!(footer.contains("secrets are never written"));
        assert!(footer.contains("scico") && footer.contains("env"));
    }

    #[test]
    fn the_settings_page_order_is_stable() {
        assert_eq!(WorkspaceSettingsPage::Workspace.index(), 0);
        assert_eq!(WorkspaceSettingsPage::Embedding.index(), 1);
        assert_eq!(WorkspaceSettingsPage::Parsing.index(), 2);
        assert_eq!(WorkspaceSettingsPage::References.index(), 3);
        assert_eq!(WorkspaceSettingsPage::Profiles.index(), 4);
        assert_eq!(WorkspaceSettingsPage::Editor.index(), 5);
        assert_eq!(ScicoSettingsPage::Chat.index(), 0);
        assert_eq!(ScicoSettingsPage::Memory.index(), 1);
        assert_eq!(ScicoSettingsPage::Retrieval.index(), 2);
        assert_eq!(ScicoSettingsPage::Editor.index(), 3);
    }
}
