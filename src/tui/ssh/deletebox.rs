use ratatui::layout::{Alignment, Rect};
use ratatui::prelude::{Line, Modifier, Span, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

pub fn show_delete_box(delete_mode: &crate::tui::app::DeleteMode, delete_button_index: usize, f: &mut ratatui::Frame, size: Rect, theme: &crate::tui::theme::Theme) {
    match &delete_mode {
        crate::tui::app::DeleteMode::None => {}
        crate::tui::app::DeleteMode::Host { name } => {
            let area = crate::tui::app::centered_rect(60, 30, size);
            let block = Block::default()
                .title(Span::styled(
                    "Confirm delete",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.fg));
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            let lines = vec![
                Line::from(format!("Delete host \"{}\" ?", name)),
                Line::from(""),
                Line::from("This action cannot be undone."),
            ];
            let msg = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(msg, inner);

            let buttons_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(3),
                width: inner.width,
                height: 3,
            };

            let delete_selected = delete_button_index == 0;
            let cancel_selected = delete_button_index == 1;

            let delete_span = if delete_selected {
                Span::styled(
                    "[ Delete ]",
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("[ Delete ]", Style::default().fg(theme.accent))
            };

            let cancel_span = if cancel_selected {
                Span::styled(
                    "[ Cancel ]",
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("[ Cancel ]")
            };

            let buttons =
                Paragraph::new(Line::from(vec![delete_span, Span::raw("   "), cancel_span]))
                    .alignment(Alignment::Center);
            f.render_widget(buttons, buttons_area);
        }
        crate::tui::app::DeleteMode::EmptyFolder { name } => {
            let area = crate::tui::app::centered_rect(60, 30, size);
            let block = Block::default()
                .title(Span::styled(
                    "Confirm delete folder",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.fg));
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            let lines = vec![
                Line::from(format!("Delete empty folder \"{}\" ?", name)),
                Line::from(""),
                Line::from("This will remove the folder only."),
            ];
            let msg = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(msg, inner);

            let buttons_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(3),
                width: inner.width,
                height: 3,
            };

            let delete_selected = delete_button_index == 0;
            let cancel_selected = delete_button_index == 1;

            let delete_span = if delete_selected {
                Span::styled(
                    "[ Delete ]",
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("[ Delete ]", Style::default().fg(theme.accent))
            };

            let cancel_span = if cancel_selected {
                Span::styled(
                    "[ Cancel ]",
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("[ Cancel ]")
            };

            let buttons =
                Paragraph::new(Line::from(vec![delete_span, Span::raw("   "), cancel_span]))
                    .alignment(Alignment::Center);
            f.render_widget(buttons, buttons_area);
        }
        crate::tui::app::DeleteMode::FolderWithHosts { name, host_count } => {
            let area = crate::tui::app::centered_rect(70, 35, size);
            let block = Block::default()
                .title(Span::styled(
                    "Confirm delete folder & hosts",
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.fg));
            let inner = block.inner(area);
            f.render_widget(Clear, area);
            f.render_widget(block, area);

            let lines = vec![
                Line::from(format!(
                    "Folder \"{}\" contains {} hosts.",
                    name, host_count
                )),
                Line::from(""),
                Line::from("What do you want to do?"),
            ];
            let msg = Paragraph::new(lines).alignment(Alignment::Center);
            f.render_widget(msg, inner);

            let buttons_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(3),
                width: inner.width,
                height: 3,
            };

            let delete_all_sel = delete_button_index == 0;
            let keep_hosts_sel = delete_button_index == 1;
            let cancel_sel = delete_button_index == 2;

            let delete_all_span = if delete_all_sel {
                Span::styled(
                    "[ Delete all ]",
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled("[ Delete all ]", Style::default().fg(theme.accent))
            };

            let keep_hosts_span = if keep_hosts_sel {
                Span::styled(
                    "[ Keep hosts ]",
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("[ Keep hosts ]")
            };

            let cancel_span = if cancel_sel {
                Span::styled(
                    "[ Cancel ]",
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("[ Cancel ]")
            };

            let buttons = Paragraph::new(Line::from(vec![
                delete_all_span,
                Span::raw("   "),
                keep_hosts_span,
                Span::raw("   "),
                cancel_span,
            ]))
            .alignment(Alignment::Center);
            f.render_widget(buttons, buttons_area);
        }
    }
}
