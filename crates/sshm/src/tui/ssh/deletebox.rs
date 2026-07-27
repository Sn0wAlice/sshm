use ratatui::layout::Rect;

use crate::tui::ssh::modal::{ModalButton, ModalConfig, render_modal};

pub fn show_delete_box(delete_mode: &crate::tui::app::DeleteMode, delete_button_index: usize, f: &mut ratatui::Frame, size: Rect, theme: &crate::tui::theme::Theme) {
    let config = match &delete_mode {
        crate::tui::app::DeleteMode::None => return,
        crate::tui::app::DeleteMode::Host { name } => ModalConfig {
            title: "Confirm delete".into(),
            body_lines: vec![
                format!("Delete host \"{}\" ?", name),
                String::new(),
                "This action cannot be undone.".into(),
            ],
            buttons: vec![
                ModalButton { label: "Delete".into(), is_selected: delete_button_index == 0 },
                ModalButton { label: "Cancel".into(), is_selected: delete_button_index == 1 },
            ],
            width_percent: 60,
            height_percent: 30,
        },
        crate::tui::app::DeleteMode::EmptyFolder { name } => ModalConfig {
            title: "Confirm delete folder".into(),
            body_lines: vec![
                format!("Delete empty folder \"{}\" ?", name),
                String::new(),
                "This will remove the folder only.".into(),
            ],
            buttons: vec![
                ModalButton { label: "Delete".into(), is_selected: delete_button_index == 0 },
                ModalButton { label: "Cancel".into(), is_selected: delete_button_index == 1 },
            ],
            width_percent: 60,
            height_percent: 30,
        },
        crate::tui::app::DeleteMode::FolderWithHosts { name, host_count } => ModalConfig {
            title: "Confirm delete folder & hosts".into(),
            body_lines: vec![
                format!("Folder \"{}\" contains {} hosts.", name, host_count),
                String::new(),
                "What do you want to do?".into(),
            ],
            buttons: vec![
                ModalButton { label: "Delete all".into(), is_selected: delete_button_index == 0 },
                ModalButton { label: "Keep hosts".into(), is_selected: delete_button_index == 1 },
                ModalButton { label: "Cancel".into(), is_selected: delete_button_index == 2 },
            ],
            width_percent: 70,
            height_percent: 35,
        },
    };
    render_modal(f, size, &config, theme);
}
