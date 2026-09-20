use crate::config::settings::AppConfig;
use crate::models::{ssh_options_to_string, tags_to_string, Database};

pub struct HostFormState {
    pub name: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub identity_file: String,
    pub proxy_jump: String,
    pub tags: String,
    pub folder: String,
    pub notes: String,
    pub remote_command: String,
    /// Raw ssh options, `;`-separated for display. Semicolon rather than comma
    /// because commas are common *inside* option values
    /// (`Ciphers=aes128-ctr,aes256-ctr`).
    pub ssh_options: String,
    pub forward_agent: bool,
    pub mosh: bool,
    pub selected_field: usize,
    pub is_edit: bool,
    pub original_name: Option<String>,
    /// Last validation error from `apply_host_form`. Rendered under the Save
    /// button until the user edits any field or presses Esc.
    pub error: Option<String>,
}

impl HostFormState {
    pub fn new_create(current_folder: Option<String>, config: &AppConfig) -> Self {
        HostFormState {
            name: String::new(),
            host: String::new(),
            port: config.default_port.to_string(),
            username: config.default_username.clone(),
            identity_file: config.default_identity_file.clone(),
            proxy_jump: String::new(),
            tags: String::new(),
            folder: current_folder.unwrap_or_default(),
            notes: String::new(),
            remote_command: String::new(),
            ssh_options: String::new(),
            forward_agent: false,
            mosh: false,
            selected_field: 0,
            is_edit: false,
            original_name: None,
            error: None,
        }
    }

    pub fn new_edit(db: &Database, name: &str) -> Self {
        if let Some(h) = db.hosts.get(name) {
            HostFormState {
                name: h.name.clone(),
                host: h.host.clone(),
                port: h.port.to_string(),
                username: h.username.clone(),
                identity_file: h.identity_file.clone().unwrap_or_default(),
                proxy_jump: h.proxy_jump.clone().unwrap_or_default(),
                tags: tags_to_string(&h.tags),
                folder: h.folder.clone().unwrap_or_default(),
                notes: h.notes.clone().unwrap_or_default(),
                remote_command: h.remote_command.clone().unwrap_or_default(),
                ssh_options: ssh_options_to_string(&h.ssh_options),
                forward_agent: h.forward_agent,
                mosh: h.mosh,
                selected_field: 0,
                is_edit: true,
                original_name: Some(h.name.clone()),
                error: None,
            }
        } else {
            HostFormState::new_create(None, &AppConfig::default())
        }
    }

    pub fn fields_count() -> usize {
        // name, host, port, username, identity_file, proxy_jump, tags, folder,
        // notes, remote_command, ssh_options, forward_agent, mosh
        13
    }

    /// Field index of the run-on-connect command row.
    pub const REMOTE_CMD_FIELD: usize = 9;
    /// Field index of the raw ssh-options row.
    pub const SSH_OPTS_FIELD: usize = 10;
    /// Field index of the ForwardAgent toggle row.
    pub const FA_FIELD: usize = 11;
    /// Field index of the mosh toggle row.
    pub const MOSH_FIELD: usize = 12;

    pub fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % (Self::fields_count() + 1);
        // +1 for Save
    }

    pub fn prev_field(&mut self) {
        if self.selected_field == 0 {
            self.selected_field = Self::fields_count();
        } else {
            self.selected_field -= 1;
        }
    }

    pub fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.selected_field {
            0 => Some(&mut self.name),
            1 => Some(&mut self.host),
            2 => Some(&mut self.port),
            3 => Some(&mut self.username),
            4 => Some(&mut self.identity_file),
            5 => Some(&mut self.proxy_jump),
            6 => Some(&mut self.tags),
            7 => Some(&mut self.folder),
            8 => Some(&mut self.notes),
            9 => Some(&mut self.remote_command),
            10 => Some(&mut self.ssh_options),
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.error = None;
        // Toggle rows: space flips the boolean, everything else is ignored.
        if self.selected_field == Self::FA_FIELD {
            if c == ' ' {
                self.forward_agent = !self.forward_agent;
            }
            return;
        }
        if self.selected_field == Self::MOSH_FIELD {
            if c == ' ' {
                self.mosh = !self.mosh;
            }
            return;
        }
        if let Some(field) = self.active_value_mut() {
            field.push(c);
        }
    }

    pub fn pop_char(&mut self) {
        self.error = None;
        if let Some(field) = self.active_value_mut() {
            field.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Host;

    fn state() -> HostFormState {
        HostFormState::new_create(None, &AppConfig::default())
    }

    #[test]
    fn every_text_field_index_is_reachable_and_distinct() {
        // Walking 0..fields_count() must hand back each text field exactly
        // once, and nothing beyond the last text row.
        let mut seen = Vec::new();
        let mut s = state();
        for i in 0..HostFormState::fields_count() {
            s.selected_field = i;
            if s.active_value_mut().is_some() {
                seen.push(i);
            }
        }
        assert_eq!(
            seen,
            (0..=HostFormState::SSH_OPTS_FIELD).collect::<Vec<_>>()
        );
    }

    #[test]
    fn toggle_rows_are_not_text_fields() {
        // The off-by-one that breaks this is invisible until you type into a
        // checkbox row and the character lands in the previous field.
        let mut s = state();
        for idx in [HostFormState::FA_FIELD, HostFormState::MOSH_FIELD] {
            s.selected_field = idx;
            assert!(
                s.active_value_mut().is_none(),
                "field {idx} must be a toggle"
            );
        }
    }

    #[test]
    fn field_indices_are_consecutive_and_within_bounds() {
        assert_eq!(
            HostFormState::SSH_OPTS_FIELD,
            HostFormState::REMOTE_CMD_FIELD + 1
        );
        assert_eq!(HostFormState::FA_FIELD, HostFormState::SSH_OPTS_FIELD + 1);
        assert_eq!(HostFormState::MOSH_FIELD, HostFormState::FA_FIELD + 1);
        assert_eq!(HostFormState::MOSH_FIELD, HostFormState::fields_count() - 1);
    }

    #[test]
    fn navigation_wraps_through_save_and_back() {
        let mut s = state();
        s.selected_field = HostFormState::fields_count() - 1;
        s.next_field();
        // One past the last field is the Save button.
        assert_eq!(s.selected_field, HostFormState::fields_count());
        s.next_field();
        assert_eq!(s.selected_field, 0);
        s.prev_field();
        assert_eq!(s.selected_field, HostFormState::fields_count());
    }

    #[test]
    fn typing_lands_in_the_selected_field() {
        let mut s = state();
        s.selected_field = HostFormState::SSH_OPTS_FIELD;
        for c in "A=1".chars() {
            s.push_char(c);
        }
        assert_eq!(s.ssh_options, "A=1");
        assert!(
            s.name.is_empty(),
            "characters must not leak into another field"
        );
        s.pop_char();
        assert_eq!(s.ssh_options, "A=");
    }

    #[test]
    fn space_toggles_a_checkbox_and_other_keys_are_ignored() {
        let mut s = state();
        s.selected_field = HostFormState::FA_FIELD;
        s.push_char('x');
        assert!(!s.forward_agent, "a non-space key must not flip the toggle");
        s.push_char(' ');
        assert!(s.forward_agent);
        s.push_char(' ');
        assert!(!s.forward_agent);

        s.selected_field = HostFormState::MOSH_FIELD;
        s.push_char(' ');
        assert!(s.mosh);
        assert!(!s.forward_agent, "toggles must be independent");
    }

    #[test]
    fn editing_an_existing_host_loads_its_ssh_options() {
        let mut db = Database::default();
        db.hosts.insert(
            "web".into(),
            Host {
                name: "web".into(),
                host: "10.0.0.5".into(),
                ssh_options: vec!["A=1".into(), "B=2".into()],
                ..Default::default()
            },
        );
        let s = HostFormState::new_edit(&db, "web");
        assert!(s.is_edit);
        assert_eq!(s.ssh_options, "A=1; B=2");
        // And the displayed form field must parse back to what was stored.
        assert_eq!(
            crate::models::parse_ssh_options(&s.ssh_options),
            vec!["A=1".to_string(), "B=2".to_string()]
        );
    }

    #[test]
    fn editing_a_missing_host_falls_back_to_a_create_form() {
        let db = Database::default();
        let s = HostFormState::new_edit(&db, "nope");
        assert!(!s.is_edit);
        assert!(s.original_name.is_none());
    }

    #[test]
    fn typing_clears_a_stale_validation_error() {
        let mut s = state();
        s.error = Some("Port 'x' is not a valid number".into());
        s.push_char('a');
        assert!(s.error.is_none());
    }
}
