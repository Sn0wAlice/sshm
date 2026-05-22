use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::Paragraph;
use crate::tui::theme::Theme;

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

pub fn get_contextual_help(ctx: HelpContext, theme: &Theme) -> Paragraph<'static> {
    let text = match ctx {
        HelpContext::HostNav => {
            "↑↓ move │ Enter connect │ / filter │ a add │ e edit │ y clone │ d delete │ Space select │ X run-cmd │ c check │ p forward │ i identity │ f fav │ s sort │ q quit"
        }
        HelpContext::FolderNav => {
            "↑↓ move │ Enter expand/collapse │ / filter │ a add │ r rename │ d delete │ q quit"
        }
        HelpContext::FilterMode => {
            "Type to filter (fuzzy) │ Esc clear │ Enter confirm"
        }
        HelpContext::DeleteModal => {
            "←→ select │ Enter confirm │ Esc cancel"
        }
        HelpContext::SettingsTab => {
            "↑↓ navigate │ Type to edit │ Enter save │ ←→ tab │ Esc reset"
        }
        HelpContext::ThemeTab => {
            "↑↓ navigate │ Enter apply/save │ ←→ tab │ Esc reset"
        }
        HelpContext::HelpTab => {
            "↑↓ scroll │ PageUp/PageDn fast scroll │ Home top │ ←→ tab │ q quit"
        }
        HelpContext::IdentitiesTab => {
            "↑↓ move │ g generate │ p push │ a agent-add │ x agent-del │ K known-hosts │ r refresh │ ←→ tab │ q quit"
        }
        HelpContext::KlusterHeaderRuntime => {
            "↑↓ move │ Enter expand/collapse │ / filter │ r refresh │ n add cluster │ ←→ tab │ q quit"
        }
        HelpContext::KlusterHeaderCluster => {
            "↑↓ move │ Enter expand/collapse │ / filter │ r refresh │ n add │ e edit │ d delete │ ←→ tab │ q quit"
        }
        HelpContext::KlusterHeaderDockerRemote => {
            "↑↓ move │ Enter expand/collapse │ / filter │ r refresh │ n add docker remote │ d unlink │ ←→ tab │ q quit"
        }
        HelpContext::KlusterItem => {
            "↑↓ move │ Enter shell │ l logs(-f) │ / filter │ r refresh │ ←→ tab │ q quit"
        }
        HelpContext::KlusterTerminalPod => {
            "↑↓ move │ Enter shell │ l logs(-f) │ / filter │ d delete pod │ r refresh │ ←→ tab │ q quit"
        }
        HelpContext::Empty => {
            "a add host │ q quit │ ←→ tab"
        }
    };

    let spans = parse_help_spans(text, theme);
    Paragraph::new(Line::from(spans))
        .style(Style::default().bg(theme.bg))
}

fn parse_help_spans(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, segment) in text.split(" │ ").enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme.muted)));
        }
        if let Some(space_idx) = segment.find(' ') {
            let key = &segment[..space_idx];
            let desc = &segment[space_idx..];
            spans.push(Span::styled(
                key.to_string(),
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(desc.to_string(), Style::default().fg(theme.muted)));
        } else {
            spans.push(Span::styled(segment.to_string(), Style::default().fg(theme.accent)));
        }
    }
    spans
}
