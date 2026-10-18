//! Theme glue for the Markdown editor.

use gpui_kit::{App, component::ActiveTheme as _, font};
use zorite_editor::SyntaxStyle;

/// Build the live-preview palette from the active GPUI Kit theme.
///
/// The editor is theme-agnostic, so every colour the host wants must be named
/// explicitly. The values here follow the component theme's semantic roles and
/// leave alert icons/property icons unset (the app ships no assets).
pub fn syntax_style(cx: &App) -> SyntaxStyle {
    let theme = cx.theme();
    SyntaxStyle {
        marker: theme.muted_foreground.opacity(0.55),
        code: theme.warning,
        code_bg: theme.muted.opacity(0.30),
        link: theme.link,
        tag: theme.success,
        quote: theme.muted_foreground,
        alert_note: theme.info,
        alert_tip: theme.success,
        alert_important: theme.accent,
        alert_warning: theme.warning,
        alert_caution: theme.danger,
        alert_icons: None,
        rule: theme.border,
        mark_bg: theme.warning.opacity(0.35),
        block_label: None,
        block_label_gen: 0,
        block_ref_count: None,
        popover_bg: theme.popover,
        popover_border: theme.border,
        popover_fg: theme.popover_foreground,
        popover_hover: theme.accent,
        popover_divider: theme.border,
        popover_danger: theme.danger,
        mono: font(theme.mono_font_family.clone()),
        property_icon: None,
    }
}
