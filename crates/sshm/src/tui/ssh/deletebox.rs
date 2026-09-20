use ratatui::layout::Rect;

use crate::tui::ssh::modal::{render_modal, ModalButton, ModalConfig};

pub fn show_delete_box(
    delete_mode: &crate::tui::app::DeleteMode,
    delete_button_index: usize,
    f: &mut ratatui::Frame,
    size: Rect,
    theme: &crate::tui::theme::Theme,
) {
    let config = match &delete_mode {
        crate::tui::app::DeleteMode::None => return,
        crate::tui::app::DeleteMode::Host { name } => ModalConfig {
            title: crate::t!("dialog.delete_host.title"),
            body_lines: vec![
                crate::t!("dialog.delete_host.body", "name" => name),
                String::new(),
                crate::t!("dialog.irreversible"),
            ],
            buttons: vec![
                ModalButton {
                    label: crate::t!("dialog.button.delete"),
                    is_selected: delete_button_index == 0,
                },
                ModalButton {
                    label: crate::t!("dialog.button.cancel"),
                    is_selected: delete_button_index == 1,
                },
            ],
            width_percent: 60,
            height_percent: 30,
        },
        crate::tui::app::DeleteMode::EmptyFolder { name } => ModalConfig {
            title: crate::t!("dialog.delete_folder.title"),
            body_lines: vec![
                crate::t!("dialog.delete_folder.body", "name" => name),
                String::new(),
                crate::t!("dialog.delete_folder.note"),
            ],
            buttons: vec![
                ModalButton {
                    label: crate::t!("dialog.button.delete"),
                    is_selected: delete_button_index == 0,
                },
                ModalButton {
                    label: crate::t!("dialog.button.cancel"),
                    is_selected: delete_button_index == 1,
                },
            ],
            width_percent: 60,
            height_percent: 30,
        },
        crate::tui::app::DeleteMode::FolderWithHosts { name, host_count } => ModalConfig {
            title: crate::t!("dialog.delete_folder_hosts.title"),
            body_lines: vec![
                crate::t!("dialog.delete_folder_hosts.body", "name" => name, "n" => host_count),
                String::new(),
                crate::t!("dialog.delete_folder_hosts.question"),
            ],
            buttons: vec![
                ModalButton {
                    label: crate::t!("dialog.button.delete_all"),
                    is_selected: delete_button_index == 0,
                },
                ModalButton {
                    label: crate::t!("dialog.button.keep_hosts"),
                    is_selected: delete_button_index == 1,
                },
                ModalButton {
                    label: crate::t!("dialog.button.cancel"),
                    is_selected: delete_button_index == 2,
                },
            ],
            width_percent: 70,
            height_percent: 35,
        },
    };
    render_modal(f, size, &config, theme);
}
