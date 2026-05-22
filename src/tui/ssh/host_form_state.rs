use crate::config::settings::AppConfig;
use crate::models::{tags_to_string, Database};

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
        // notes, forward_agent, mosh
        11
    }

    /// Field index of the ForwardAgent toggle row.
    pub const FA_FIELD: usize = 9;
    /// Field index of the mosh toggle row.
    pub const MOSH_FIELD: usize = 10;

    pub fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % (Self::fields_count() + 1); // +1 for Save
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
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.error = None;
        // Toggle rows: space flips the boolean, everything else is ignored.
        if self.selected_field == Self::FA_FIELD {
            if c == ' ' { self.forward_agent = !self.forward_agent; }
            return;
        }
        if self.selected_field == Self::MOSH_FIELD {
            if c == ' ' { self.mosh = !self.mosh; }
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
