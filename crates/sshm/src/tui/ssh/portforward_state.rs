//! State of the port-forward form: field layout, editing rules, validation.
//!
//! Split out of `portforward.rs` so the rules can be exercised without a
//! terminal — same shape as [`crate::tui::ssh::host_form_state`]. Rendering
//! and the event loop stay in `portforward.rs`.

use crate::models::{Tunnel, TunnelKind};

/// Field indices, in the order they are drawn.
pub mod field {
    /// Local / Remote / Dynamic selector.
    pub const KIND: usize = 0;
    pub const LOCAL_PORT: usize = 1;
    /// Hidden for `Dynamic` — SOCKS has no fixed target.
    pub const REMOTE_HOST: usize = 2;
    /// Hidden for `Dynamic`.
    pub const REMOTE_PORT: usize = 3;
    pub const LABEL: usize = 4;
    /// "Save on the host" toggle.
    pub const SAVE: usize = 5;
    /// "Restart automatically if it drops" toggle.
    pub const AUTO_RESTART: usize = 6;
    pub const START: usize = 7;
}

pub struct PortForwardForm {
    pub kind: TunnelKind,
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
    pub label: String,
    pub save: bool,
    pub auto_restart: bool,
    pub selected_field: usize,
    pub error: Option<String>,
}

impl PortForwardForm {
    pub fn new() -> Self {
        Self {
            kind: TunnelKind::Local,
            local_port: String::new(),
            remote_host: String::new(),
            remote_port: String::new(),
            label: String::new(),
            save: false,
            auto_restart: false,
            selected_field: field::KIND,
            error: None,
        }
    }

    pub fn from_existing(t: &Tunnel) -> Self {
        Self {
            kind: t.kind,
            local_port: t.local_port.to_string(),
            remote_host: t.remote_host.clone(),
            remote_port: if t.kind == TunnelKind::Dynamic {
                String::new()
            } else {
                t.remote_port.to_string()
            },
            label: t.label.clone(),
            save: true,
            auto_restart: t.auto_restart,
            selected_field: field::KIND,
            error: None,
        }
    }

    /// The field indices currently shown. A dynamic (SOCKS) forward has no
    /// remote target, so those two rows disappear.
    pub fn visible_fields(&self) -> Vec<usize> {
        match self.kind {
            TunnelKind::Dynamic => vec![
                field::KIND,
                field::LOCAL_PORT,
                field::LABEL,
                field::SAVE,
                field::AUTO_RESTART,
                field::START,
            ],
            _ => vec![
                field::KIND,
                field::LOCAL_PORT,
                field::REMOTE_HOST,
                field::REMOTE_PORT,
                field::LABEL,
                field::SAVE,
                field::AUTO_RESTART,
                field::START,
            ],
        }
    }

    pub fn next_field(&mut self) {
        let visible = self.visible_fields();
        let idx = visible.iter().position(|&f| f == self.selected_field).unwrap_or(0);
        self.selected_field = visible[(idx + 1) % visible.len()];
    }

    pub fn prev_field(&mut self) {
        let visible = self.visible_fields();
        let idx = visible.iter().position(|&f| f == self.selected_field).unwrap_or(0);
        self.selected_field = visible[(idx + visible.len() - 1) % visible.len()];
    }

    pub fn cycle_kind(&mut self, forward: bool) {
        let order = [TunnelKind::Local, TunnelKind::Remote, TunnelKind::Dynamic];
        let idx = order.iter().position(|k| *k == self.kind).unwrap_or(0);
        let next = if forward {
            (idx + 1) % order.len()
        } else {
            (idx + order.len() - 1) % order.len()
        };
        self.kind = order[next];
        // Switching to Dynamic can hide the row the cursor is on.
        if !self.visible_fields().contains(&self.selected_field) {
            self.selected_field = field::KIND;
        }
    }

    /// True when Space is a control on the current row rather than a character
    /// to type. Text rows must answer `false`, or a space in the Label field
    /// would be swallowed instead of typed.
    pub fn space_is_a_control(&self) -> bool {
        matches!(
            self.selected_field,
            field::KIND | field::SAVE | field::AUTO_RESTART
        )
    }

    /// Flip the toggle under the cursor. Returns `true` when the cursor was on
    /// a toggle row, so the caller knows whether Space still has work to do.
    pub fn toggle_selected(&mut self) -> bool {
        match self.selected_field {
            field::SAVE => {
                self.save = !self.save;
                true
            }
            field::AUTO_RESTART => {
                self.auto_restart = !self.auto_restart;
                true
            }
            _ => false,
        }
    }

    pub fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.selected_field {
            field::LOCAL_PORT => Some(&mut self.local_port),
            field::REMOTE_HOST => Some(&mut self.remote_host),
            field::REMOTE_PORT => Some(&mut self.remote_port),
            field::LABEL => Some(&mut self.label),
            _ => None,
        }
    }

    pub fn push_char(&mut self, c: char) {
        // Port rows take digits only: a typo there fails at validation with a
        // message about a number, which is confusing when you typed a letter.
        let is_port_field = matches!(self.selected_field, field::LOCAL_PORT | field::REMOTE_PORT);
        if is_port_field && !c.is_ascii_digit() {
            return;
        }
        if let Some(field) = self.active_value_mut() {
            field.push(c);
        }
    }

    pub fn pop_char(&mut self) {
        if let Some(field) = self.active_value_mut() {
            field.pop();
        }
    }

    pub fn validate(&self) -> Result<Tunnel, String> {
        let lp: u16 = self
            .local_port
            .trim()
            .parse()
            .map_err(|_| "Local port must be a number 1-65535".to_string())?;
        if lp == 0 {
            return Err("Local port must be a number 1-65535".to_string());
        }
        match self.kind {
            TunnelKind::Dynamic => Ok(Tunnel {
                label: self.label.trim().to_string(),
                kind: TunnelKind::Dynamic,
                local_port: lp,
                remote_port: 0,
                remote_host: String::new(),
                auto_restart: self.auto_restart,
            }),
            kind => {
                let rp: u16 = self
                    .remote_port
                    .trim()
                    .parse()
                    .map_err(|_| "Remote port must be a number 1-65535".to_string())?;
                if rp == 0 {
                    return Err("Remote port must be a number 1-65535".to_string());
                }
                Ok(Tunnel {
                    label: self.label.trim().to_string(),
                    kind,
                    local_port: lp,
                    remote_port: rp,
                    remote_host: self.remote_host.trim().to_string(),
                    auto_restart: self.auto_restart,
                })
            }
        }
    }
}

impl Default for PortForwardForm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> PortForwardForm {
        PortForwardForm::new()
    }

    fn filled() -> PortForwardForm {
        let mut f = form();
        f.local_port = "8080".into();
        f.remote_port = "80".into();
        f
    }

    // ---- field layout ----------------------------------------------------

    #[test]
    fn dynamic_hides_the_remote_target_rows() {
        // SOCKS has no fixed destination, so those two rows make no sense.
        let mut f = form();
        assert!(f.visible_fields().contains(&field::REMOTE_HOST));
        f.kind = TunnelKind::Dynamic;
        let visible = f.visible_fields();
        assert!(!visible.contains(&field::REMOTE_HOST));
        assert!(!visible.contains(&field::REMOTE_PORT));
        assert!(visible.contains(&field::LOCAL_PORT), "a SOCKS port is still needed");
    }

    #[test]
    fn switching_to_dynamic_moves_a_cursor_off_a_hidden_row() {
        // Otherwise the cursor sits on an invisible field and typing goes nowhere.
        let mut f = form();
        f.selected_field = field::REMOTE_PORT;
        f.cycle_kind(true); // Local -> Remote
        assert_eq!(f.selected_field, field::REMOTE_PORT, "still visible");
        f.cycle_kind(true); // Remote -> Dynamic
        assert_eq!(f.selected_field, field::KIND, "snapped back to a visible row");
    }

    #[test]
    fn navigation_skips_hidden_rows() {
        let mut f = form();
        f.kind = TunnelKind::Dynamic;
        f.selected_field = field::LOCAL_PORT;
        f.next_field();
        assert_eq!(f.selected_field, field::LABEL, "remote rows are skipped");
    }

    #[test]
    fn navigation_wraps_in_both_directions() {
        let mut f = form();
        f.selected_field = field::START;
        f.next_field();
        assert_eq!(f.selected_field, field::KIND);
        f.prev_field();
        assert_eq!(f.selected_field, field::START);
    }

    #[test]
    fn the_kind_selector_cycles_through_all_three() {
        let mut f = form();
        assert_eq!(f.kind, TunnelKind::Local);
        f.cycle_kind(true);
        assert_eq!(f.kind, TunnelKind::Remote);
        f.cycle_kind(true);
        assert_eq!(f.kind, TunnelKind::Dynamic);
        f.cycle_kind(true);
        assert_eq!(f.kind, TunnelKind::Local, "wraps");
        f.cycle_kind(false);
        assert_eq!(f.kind, TunnelKind::Dynamic, "and goes backwards");
    }

    // ---- editing ---------------------------------------------------------

    #[test]
    fn port_rows_accept_digits_only() {
        let mut f = form();
        f.selected_field = field::LOCAL_PORT;
        for c in "80a8b0".chars() {
            f.push_char(c);
        }
        assert_eq!(f.local_port, "8080", "letters are dropped at input");
    }

    #[test]
    fn text_rows_accept_anything() {
        let mut f = form();
        f.selected_field = field::LABEL;
        for c in "Postgres prod".chars() {
            f.push_char(c);
        }
        assert_eq!(f.label, "Postgres prod");
    }

    #[test]
    fn space_types_in_a_text_row_and_controls_a_toggle_row() {
        // Regression guard: making Space a plain control swallowed spaces in
        // the Label field.
        let mut f = form();
        f.selected_field = field::LABEL;
        assert!(!f.space_is_a_control(), "Space must reach the Label field");

        f.selected_field = field::SAVE;
        assert!(f.space_is_a_control());
        f.selected_field = field::AUTO_RESTART;
        assert!(f.space_is_a_control());
        f.selected_field = field::KIND;
        assert!(f.space_is_a_control());
        f.selected_field = field::LOCAL_PORT;
        assert!(!f.space_is_a_control());
    }

    #[test]
    fn toggles_are_independent() {
        let mut f = form();
        f.selected_field = field::SAVE;
        assert!(f.toggle_selected());
        assert!(f.save && !f.auto_restart);
        f.selected_field = field::AUTO_RESTART;
        assert!(f.toggle_selected());
        assert!(f.save && f.auto_restart);
        assert!(f.toggle_selected(), "flips back");
        assert!(!f.auto_restart);
    }

    #[test]
    fn toggling_a_non_toggle_row_reports_it_did_nothing() {
        let mut f = form();
        f.selected_field = field::LABEL;
        assert!(!f.toggle_selected());
        f.selected_field = field::START;
        assert!(!f.toggle_selected());
    }

    #[test]
    fn editing_is_confined_to_the_selected_row() {
        let mut f = form();
        f.selected_field = field::REMOTE_HOST;
        for c in "db.internal".chars() {
            f.push_char(c);
        }
        assert_eq!(f.remote_host, "db.internal");
        assert!(f.local_port.is_empty() && f.label.is_empty());
        f.pop_char();
        assert_eq!(f.remote_host, "db.interna");
    }

    // ---- validation ------------------------------------------------------

    #[test]
    fn a_local_forward_validates_into_a_tunnel() {
        let mut f = filled();
        f.remote_host = "  db.internal  ".into();
        f.label = "  pg  ".into();
        let t = f.validate().expect("valid");
        assert_eq!(t.kind, TunnelKind::Local);
        assert_eq!(t.local_port, 8080);
        assert_eq!(t.remote_port, 80);
        assert_eq!(t.remote_host, "db.internal", "trimmed");
        assert_eq!(t.label, "pg", "trimmed");
    }

    #[test]
    fn a_dynamic_forward_ignores_the_remote_target() {
        let mut f = filled();
        f.kind = TunnelKind::Dynamic;
        f.remote_host = "leftover".into();
        let t = f.validate().expect("valid");
        assert_eq!(t.local_port, 8080);
        assert_eq!(t.remote_port, 0);
        assert_eq!(t.remote_host, "", "a stale target must not be carried over");
    }

    #[test]
    fn a_missing_port_is_rejected_with_a_useful_message() {
        let mut f = form();
        assert!(f.validate().unwrap_err().contains("Local port"));
        f.local_port = "8080".into();
        assert!(f.validate().unwrap_err().contains("Remote port"));
    }

    #[test]
    fn port_zero_is_rejected() {
        // `0` parses as u16 but tells ssh "pick any port", which is not what
        // someone typing a port into a form means.
        let mut f = filled();
        f.local_port = "0".into();
        assert!(f.validate().is_err());
        let mut f = filled();
        f.remote_port = "0".into();
        assert!(f.validate().is_err());
    }

    #[test]
    fn a_port_above_65535_is_rejected() {
        let mut f = filled();
        f.local_port = "70000".into();
        assert!(f.validate().is_err());
    }

    #[test]
    fn the_auto_restart_choice_reaches_the_tunnel() {
        let mut f = filled();
        assert!(!f.validate().unwrap().auto_restart, "off by default");
        f.auto_restart = true;
        assert!(f.validate().unwrap().auto_restart);
    }

    // ---- round trip ------------------------------------------------------

    #[test]
    fn editing_an_existing_tunnel_loads_every_field() {
        let t = Tunnel {
            label: "pg".into(),
            kind: TunnelKind::Local,
            local_port: 15432,
            remote_port: 5432,
            remote_host: "db.internal".into(),
            auto_restart: true,
        };
        let f = PortForwardForm::from_existing(&t);
        assert_eq!(f.local_port, "15432");
        assert_eq!(f.remote_port, "5432");
        assert_eq!(f.remote_host, "db.internal");
        assert!(f.auto_restart);
        assert!(f.save, "an existing tunnel is already saved");
        assert_eq!(f.validate().unwrap(), t, "round-trips unchanged");
    }

    #[test]
    fn editing_a_dynamic_tunnel_leaves_the_remote_port_blank() {
        let t = Tunnel {
            label: "socks".into(),
            kind: TunnelKind::Dynamic,
            local_port: 1080,
            remote_port: 0,
            remote_host: String::new(),
            auto_restart: false,
        };
        let f = PortForwardForm::from_existing(&t);
        assert_eq!(f.remote_port, "", "a 0 would render as a bogus '0' in the field");
        assert_eq!(f.validate().unwrap(), t);
    }
}
