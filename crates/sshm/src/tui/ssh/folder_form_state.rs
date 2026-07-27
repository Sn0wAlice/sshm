pub struct FolderFormState {
    pub name: String,
    pub original_name: String,
    pub selected_field: usize,
    pub error: Option<String>,
}

impl FolderFormState {
    pub fn new_rename(name: &str) -> Self {
        FolderFormState {
            name: name.to_string(),
            original_name: name.to_string(),
            selected_field: 0,
            error: None,
        }
    }

    pub fn fields_count() -> usize {
        1
    }

    pub fn next_field(&mut self) {
        self.selected_field = (self.selected_field + 1) % (Self::fields_count() + 1);
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