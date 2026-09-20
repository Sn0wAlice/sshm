use crate::tui::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

#[derive(Clone, Copy)]
pub enum HelpContext {
    HostNav,
    FolderNav,
    FilterMode,
    DeleteModal,
    SettingsTab,
    ThemeTab,
    HelpTab,
    IdentitiesTab,
    /// Selection is on a Docker / Incus-local / Incus-remote section header
    /// (no edit/delete — those daemons aren't user-managed entries).
    KlusterHeaderRuntime,
    /// Selection is on a saved k8s/k3s cluster header (full CRUD available).
    KlusterHeaderCluster,
    /// Selection is on a saved Docker remote header (delete = unlink, no edit).
    KlusterHeaderDockerRemote,
    /// Selection is on a container / pod / instance — shell + logs apply.
    KlusterItem,
    /// Selection is on a Succeeded/Failed pod — additionally allows `d` to
    /// delete it (`kubectl delete pod`).
    KlusterTerminalPod,
    Empty,
}

/// The full ` │ `-separated shortcut string for a context. This is the source
/// of truth for both the (possibly truncated) footer bar and the `h` popup.
/// Translation-key suffix for a context. Used for both the shortcut string
/// (`help.<suffix>`) and the popup title (`help.title.<suffix>`).
fn context_key(ctx: HelpContext) -> &'static str {
    match ctx {
        HelpContext::HostNav => "host_nav",
        HelpContext::FolderNav => "folder_nav",
        HelpContext::FilterMode => "filter_mode",
        HelpContext::DeleteModal => "delete_modal",
        HelpContext::SettingsTab => "settings_tab",
        HelpContext::ThemeTab => "theme_tab",
        HelpContext::HelpTab => "help_tab",
        HelpContext::IdentitiesTab => "identities_tab",
        HelpContext::KlusterHeaderRuntime => "kluster_header_runtime",
        HelpContext::KlusterHeaderCluster => "kluster_header_cluster",
        HelpContext::KlusterHeaderDockerRemote => "kluster_header_docker_remote",
        HelpContext::KlusterItem => "kluster_item",
        HelpContext::KlusterTerminalPod => "kluster_terminal_pod",
        HelpContext::Empty => "empty",
    }
}

/// The full ` │ `-separated shortcut string for a context. This is the source
/// of truth for both the (possibly truncated) footer bar and the `h` popup.
///
/// Localized: the key letters are fixed, the descriptions after them are not.
fn help_text_for(ctx: HelpContext) -> String {
    crate::t!(&format!("help.{}", context_key(ctx)))
}

/// Short human label for a context, used as the popup title.
fn context_label(ctx: HelpContext) -> String {
    crate::t!(&format!("help.title.{}", context_key(ctx)))
}

/// Build the contextual help bar, fitted to `width` display columns.
///
/// The bar is a single line, so it never wraps: only the whole segments that
/// fit are kept (cut on ` │ ` boundaries, never mid-word), and a trailing `…`
/// is appended when one or more segments had to be dropped. Widen the terminal
/// — or press `h` for the full popup — and the hidden shortcuts reappear.
pub fn get_contextual_help(ctx: HelpContext, theme: &Theme, width: u16) -> Paragraph<'static> {
    let text = help_text_for(ctx);
    let spans = build_help_spans(&text, theme, width);
    Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.bg))
}

/// Render the full contextual help as a centered popup overlay — i.e. every
/// shortcut for the current context, including the ones the footer had to
/// truncate. Sized to its content and clamped to the screen.
pub fn draw_help_popup(f: &mut Frame, ctx: HelpContext, theme: &Theme) {
    let area = f.area();
    let text = help_text_for(ctx);
    let segments: Vec<&str> = text.split(" │ ").collect();

    // One shortcut per line: key (bold accent) padded, then description.
    let mut lines: Vec<Line> = Vec::new();
    for segment in &segments {
        match segment.split_once(' ') {
            Some((key, desc)) => lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<13}", key),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(desc.to_string(), Style::default().fg(theme.fg)),
            ])),
            None => lines.push(Line::from(Span::styled(
                format!("  {}", segment),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ))),
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Esc / h      close this popup",
        Style::default().fg(theme.muted),
    )));

    // Size to content, clamped to the available screen.
    let content_w = lines.iter().map(|l| l.width()).max().unwrap_or(20) as u16;
    let w = (content_w + 4).min(area.width.max(1));
    let h = (lines.len() as u16 + 2).min(area.height.max(1));
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let rect = Rect {
        x,
        y,
        width: w,
        height: h,
    };

    let block = Block::default()
        .title(format!(" Help — {} ", context_label(ctx)))
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme.bg).fg(theme.fg));

    f.render_widget(Clear, rect);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

/// Width (in display columns) of one ` │ ` segment separator.
const SEP_WIDTH: usize = 3;

/// Fit as many ` │ `-separated segments of `text` as possible into `width`
/// columns, appending a `…` marker when some had to be dropped.
fn build_help_spans(text: &str, theme: &Theme, width: u16) -> Vec<Span<'static>> {
    let segments: Vec<&str> = text.split(" │ ").collect();
    let budget = width as usize;

    let (spans, all_fit) = fit_segments(&segments, theme, budget);
    if all_fit {
        return spans;
    }
    // Some segments were dropped — re-fit leaving room for the " …" marker.
    let (mut spans, _) = fit_segments(&segments, theme, budget.saturating_sub(2));
    spans.push(Span::styled(" …", Style::default().fg(theme.muted)));
    spans
}

/// Greedily lay out `segments` within `budget` columns. Returns the styled
/// spans plus whether *every* segment fit.
fn fit_segments(segments: &[&str], theme: &Theme, budget: usize) -> (Vec<Span<'static>>, bool) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for (i, segment) in segments.iter().enumerate() {
        let sep_w = if i > 0 { SEP_WIDTH } else { 0 };
        let seg_w = segment.chars().count();
        if used + sep_w + seg_w > budget {
            return (spans, false);
        }
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme.muted)));
        }
        if let Some(space_idx) = segment.find(' ') {
            let key = &segment[..space_idx];
            let desc = &segment[space_idx..];
            spans.push(Span::styled(
                key.to_string(),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                desc.to_string(),
                Style::default().fg(theme.muted),
            ));
        } else {
            spans.push(Span::styled(
                segment.to_string(),
                Style::default().fg(theme.accent),
            ));
        }
        used += sep_w + seg_w;
    }
    (spans, true)
}
