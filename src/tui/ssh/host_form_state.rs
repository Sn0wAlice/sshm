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
    pub selected_field: usize,
    pub is_edit: bool,
    pub original_name: Option<String>,
}

impl HostFormState {
    pub fn new_create(current_folder: Option<String>) -> Self {
        HostFormState {
            name: String::new(),
            host: String::new(),
            port: "22".to_string(),
            username: "root".to_string(),
            identity_file: String::new(),
            proxy_jump: String::new(),
            tags: String::new(),
            folder: current_folder.unwrap_or_default(),
            selected_field: 0,
            is_edit: false,
            original_name: None,
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
                selected_field: 0,
                is_edit: true,
                original_name: Some(h.name.clone()),
            }
        } else {
            HostFormState::new_create(None)
        }
    }

    pub fn fields_count() -> usize {
        8 // name, host, port, username, identity_file, proxy_jump, tags, folder
    }

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
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        if let Some(field) = self.active_value_mut() {
            field.push(c);
        }
    }

    pub fn pop_char(&mut self) {
        if let Some(field) = self.active_value_mut() {
            field.pop();
        }
    }
}