

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, ListState};

use crate::tui::theme;

#[derive(Clone, Debug)]
struct FileEntry {
    name: String,
    is_dir: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum PanelFocus {
    Local,
    Remote,
}

enum Mode {
    Normal,
    Filter,
}

#[derive(Debug)]
struct PanelState {
    cwd: PathBuf,
    entries: Vec<FileEntry>,
    selected: usize,
}

impl PanelState {
    fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            entries: Vec::new(),
            selected: 0,
        }
    }

    fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1]);

    horizontal[1]
}

fn read_local_dir(path: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        let file_name = entry
            .file_name()
            .to_string_lossy()
            .to_string();
        let is_dir = meta.is_dir();
        entries.push(FileEntry {
            name: file_name,
            is_dir,
        });
    }
    // Sort directories first, then files, both alphabetically
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
    Ok(entries)
}

fn ssh_list_remote_dir(
    user: &str,
    host: &str,
    port: u16,
    identity: Option<&str>,
    remote_path: &str,
) -> io::Result<Vec<FileEntry>> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-p").arg(port.to_string());

    if let Some(id) = identity {
        if !id.is_empty() {
            cmd.arg("-i").arg(id);
        }
    }

    let target = format!("{}@{}", user, host);
    // `ls -p -1` : one entry per line, `/` suffix for dirs
    let remote_cmd = format!("LC_ALL=C ls -p -1 -- {}", shell_escape(remote_path));
    cmd.arg(target).arg(remote_cmd);

    let output = cmd.output()?;
    if !output.status.success() {
        // On error, return empty list instead of failing hard
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let is_dir = line.ends_with('/');
        let name = if is_dir {
            line.trim_end_matches('/').to_string()
        } else {
            line.to_string()
        };
        entries.push(FileEntry { name, is_dir });
    }

    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    Ok(entries)
}

fn shell_escape(path: &str) -> String {
    // Very small and naive shell escaping for paths
    // Wrap in single quotes and escape existing ones.
    if path.is_empty() {
        "''".to_string()
    } else {
        let escaped = path.replace("'", "'\\''");
        format!("'{}'", escaped)
    }
}

fn join_remote_path(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{}", name)
    } else if base.ends_with('/') {
        format!("{}{}", base, name)
    } else {
        format!("{}/{}", base, name)
    }
}

fn parent_remote_path(path: &str) -> String {
    if path == "/" {
        "/".to_string()
    } else {
        match Path::new(path).parent() {
            Some(p) => {
                let s = p.to_string_lossy().to_string();
                if s.is_empty() { "/".to_string() } else { s }
            }
            None => "/".to_string(),
        }
    }
}

fn ssh_remote_file_size(
    user: &str,
    host: &str,
    port: u16,
    identity: Option<&str>,
    remote_path: &str,
) -> io::Result<Option<u64>> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-p").arg(port.to_string());

    if let Some(id) = identity {
        if !id.is_empty() {
            cmd.arg("-i").arg(id);
        }
    }

    let target = format!("{}@{}", user, host);
    let escaped = shell_escape(remote_path);

    let remote_cmd = format!(
        "LC_ALL=C stat -c %s -- {p} 2>/dev/null || stat -f %z -- {p} 2>/dev/null",
        p = escaped
    );
    cmd.arg(target).arg(remote_cmd);

    let output = cmd.output()?;
    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(val) = trimmed.parse::<u64>() {
            return Ok(Some(val));
        }
    }
    Ok(None)
}

fn download_remote_file(
    user: &str,
    host: &str,
    port: u16,
    identity: Option<&str>,
    remote_path: &str,
    local_target: &Path,
) -> io::Result<()> {
    let mut cmd = Command::new("scp");
    // -q to silence progress / banners
    cmd.arg("-q");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.arg("-P").arg(port.to_string());
    if let Some(id) = identity {
        if !id.is_empty() {
            cmd.arg("-i").arg(id);
        }
    }

    let remote_spec = format!("{}@{}:{}", user, host, remote_path);
    cmd.arg(remote_spec).arg(local_target);

    let status = cmd.status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("scp failed with status: {:?}", status.code()),
        ));
    }

    Ok(())
}

fn ssh_mkdir_remote(
    user: &str,
    host: &str,
    port: u16,
    identity: Option<&str>,
    remote_dir: &str,
) -> io::Result<()> {
    let mut cmd = Command::new("ssh");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.arg("-p").arg(port.to_string());

    if let Some(id) = identity {
        if !id.is_empty() {
            cmd.arg("-i").arg(id);
        }
    }

    let target = format!("{}@{}", user, host);
    let escaped = shell_escape(remote_dir);
    let remote_cmd = format!("mkdir -p {}", escaped);
    cmd.arg(target).arg(remote_cmd);

    let status = cmd.status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("ssh mkdir -p failed for {}", remote_dir),
        ));
    }

    Ok(())
}

fn upload_local_file(
    user: &str,
    host: &str,
    port: u16,
    identity: Option<&str>,
    local_path: &Path,
    remote_target: &str,
) -> io::Result<()> {
    let mut cmd = Command::new("scp");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.arg("-q");
    cmd.arg("-P").arg(port.to_string());
    if let Some(id) = identity {
        if !id.is_empty() {
            cmd.arg("-i").arg(id);
        }
    }

    let remote_spec = format!("{}@{}:{}", user, host, remote_target);
    cmd.arg(local_path).arg(remote_spec);

    let status = cmd.status()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("scp upload failed with status: {:?}", status.code()),
        ));
    }

    Ok(())
}

fn upload_local_folder(
    user: &str,
    host: &str,
    port: u16,
    identity: Option<&str>,
    local_root: &Path,
    remote_root: &str,
) -> io::Result<()> {
    // Ensure the root directory exists on remote
    ssh_mkdir_remote(user, host, port, identity, remote_root)?;

    // Stack-based DFS over local directories
    let mut stack: Vec<(PathBuf, String)> = Vec::new();
    stack.push((local_root.to_path_buf(), remote_root.to_string()));

    while let Some((local_dir, remote_dir)) = stack.pop() {
        for entry in fs::read_dir(&local_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry
                .file_name()
                .to_string_lossy()
                .to_string();
            let remote_path = join_remote_path(&remote_dir, &name);

            if path.is_dir() {
                // Create remote subdir and recurse
                ssh_mkdir_remote(user, host, port, identity, &remote_path)?;
                stack.push((path, remote_path));
            } else {
                // Upload single file
                upload_local_file(user, host, port, identity, &path, &remote_path)?;
            }
        }
    }

    Ok(())
}

fn unique_local_path(dir: &Path, file_name: &str) -> PathBuf {
    // Split into base and suffix (keep multi-part extensions like .tar.gz as suffix)
    let (base, suffix) = if let Some(pos) = file_name.find('.') {
        let (b, s) = file_name.split_at(pos);
        (b.to_string(), s.to_string())
    } else {
        (file_name.to_string(), String::new())
    };

    let mut candidate = dir.join(format!("{}{}", base, suffix));
    if !candidate.exists() {
        return candidate;
    }

    let mut n = 1;
    loop {
        let name = format!("{} ({}){}", base, n, suffix);
        candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

#[derive(Clone, Debug)]
struct DownloadJob {
    id: u64,
    file_name: String,
    local_path: PathBuf,
    remote_path: String,
    total_size: Option<u64>,
}

#[derive(Clone, Debug)]
struct ActiveDownload {
    id: u64,
    file_name: String,
    local_path: PathBuf,
    remote_path: String,
    total_size: Option<u64>,
    current_size: u64,
    done_at: Option<std::time::Instant>,
}

#[derive(Debug, Clone)]
struct DownloadProgressState {
    folder_label: String,
    files_done: usize,
    files_total: usize,
    done_bytes: u64,
    total_bytes: u64,
}

#[derive(Debug)]
enum DownloadEvent {
    Completed {
        id: u64,
        file_name: String,
        local_path: PathBuf,
        result: io::Result<()>,
    },
    Progress {
        folder_label: String,
        files_done: usize,
        files_total: usize,
        done_bytes: u64,
        total_bytes: u64,
    },
}

#[derive(Debug, Clone)]
struct UploadProgressState {
    label: String,
    files_done: usize,
    files_total: usize,
    done_bytes: u64,
    total_bytes: u64,
}

#[derive(Debug)]
enum SftpEvent {
    UploadCompleted {
        file_name: String,
    },
    UploadProgress {
        label: String,
        files_done: usize,
        files_total: usize,
        done_bytes: u64,
        total_bytes: u64,
    },
}

/// Run the dual-pane SFTP-like browser.
///
/// Left panel: local filesystem starting at $HOME.
/// Right panel: remote filesystem via ssh/ls starting at `/`.
///
/// - Tab switches focus between panels
/// - Enter opens directories
/// - Backspace goes to parent directory
/// - On remote panel: `d` downloads selected file into current local directory
/// - `q` quits the browser and returns to SSHM
pub fn run_sftp_ui(user: &str, host: &str, port: u16, identity: Option<&str>) -> io::Result<()> {
    let theme = theme::load();
    let theme = theme.clone();

    // Owned copies for threaded downloads
    let user_owned = user.to_string();
    let host_owned = host.to_string();
    let identity_owned = identity.map(|s| s.to_string());

    // Download manager state
    let (dl_tx, dl_rx) = mpsc::channel::<DownloadEvent>();
    let (ul_tx, ul_rx) = mpsc::channel::<SftpEvent>();
    let mut next_download_id: u64 = 1;
    let mut active_downloads: Vec<ActiveDownload> = Vec::new();
    let mut pending_downloads: VecDeque<DownloadJob> = VecDeque::new();
    const MAX_PARALLEL_DOWNLOADS: usize = 3;

    let local_start = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    // Try to start in /home/<user> — fallback to "/" if unreadable
    let home_path = format!("/home/{}", user);
    let remote_start = if ssh_list_remote_dir(user, host, port, identity, &home_path)
        .unwrap_or_default()
        .len() > 0
    {
        home_path
    } else {
        "/".to_string()
    };

    let mut local_panel = PanelState::new(local_start);
    let mut remote_panel = PanelState::new(PathBuf::from(&remote_start));

    local_panel.entries = read_local_dir(&local_panel.cwd).unwrap_or_default();
    if local_panel.cwd.parent().is_some() {
        local_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
    }
    remote_panel.entries = ssh_list_remote_dir(user, host, port, identity, &remote_start)?;
    if remote_panel.cwd.to_string_lossy() != "/" {
        remote_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
    }

    let mut focus = PanelFocus::Remote; // Default: remote side active
    let mut message: Option<String> = None;
    let mut mode = Mode::Normal;
    let mut filter_input = String::new();
    let mut upload_progress: Option<UploadProgressState> = None;
    let mut download_progress: Option<DownloadProgressState> = None;

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    'outer: loop {
        // Handle upload events (progress + completion)
        while let Ok(ev) = ul_rx.try_recv() {
            match ev {
                SftpEvent::UploadCompleted { file_name: _ } => {
                    // Refresh remote panel after upload
                    let current = remote_panel.cwd.to_string_lossy().to_string();
                    if let Ok(mut list) = ssh_list_remote_dir(user, host, port, identity, &current) {
                        if current != "/" {
                            list.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                        }
                        remote_panel.entries = list;
                        remote_panel.selected = 0; // reset to top
                    }
                    upload_progress = None;
                    message = Some(String::from("Upload completed ✓"));
                }
                SftpEvent::UploadProgress {
                    label,
                    files_done,
                    files_total,
                    done_bytes,
                    total_bytes,
                } => {
                    upload_progress = Some(UploadProgressState {
                        label,
                        files_done,
                        files_total,
                        done_bytes,
                        total_bytes,
                    });
                }
            }
        }
        // Handle completed downloads and spawn new ones
        // Drain completed events
        while let Ok(ev) = dl_rx.try_recv() {
            match ev {
                DownloadEvent::Completed { id, file_name, local_path, result } => {
                    download_progress = None;
                    // Remove from active_downloads
                    active_downloads.retain(|d| d.id != id);
                    match result {
                        Ok(()) => {
                            // Refresh local panel after download
                            match read_local_dir(&local_panel.cwd) {
                                Ok(list) => {
                                    local_panel.entries = list;
                                    if local_panel.cwd.parent().is_some() {
                                        local_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!(
                                        "Downloaded but local refresh failed: {}",
                                        e
                                    ));
                                }
                            }
                            message = Some(format!("Downloaded {} ✓", file_name));
                        }
                        Err(e) => {
                            message = Some(format!("Download error for {}: {}", file_name, e));
                        }
                    }
                }
                DownloadEvent::Progress {
                    folder_label,
                    files_done,
                    files_total,
                    done_bytes,
                    total_bytes,
                } => {
                    download_progress = Some(DownloadProgressState {
                        folder_label,
                        files_done,
                        files_total,
                        done_bytes,
                        total_bytes,
                    });
                }
            }
        }
        // Update current_size for active downloads based on local file metadata
        for d in active_downloads.iter_mut() {
            if let Ok(meta) = fs::metadata(&d.local_path) {
                d.current_size = meta.len();
            }
        }
        // Spawn new downloads up to MAX_PARALLEL_DOWNLOADS
        while active_downloads.len() < MAX_PARALLEL_DOWNLOADS {
            if let Some(job) = pending_downloads.pop_front() {
                let tx = dl_tx.clone();
                let user_cl = user_owned.clone();
                let host_cl = host_owned.clone();
                let id_cl = identity_owned.clone();
                let job_cl = job.clone();
                active_downloads.push(ActiveDownload {
                    id: job.id,
                    file_name: job.file_name.clone(),
                    local_path: job.local_path.clone(),
                    remote_path: job.remote_path.clone(),
                    total_size: job.total_size,
                    current_size: 0,
                    done_at: None,
                });
                thread::spawn(move || {
                    let res = download_remote_file(
                        &user_cl,
                        &host_cl,
                        port,
                        id_cl.as_deref(),
                        &job_cl.remote_path,
                        &job_cl.local_path,
                    );
                    let _ = tx.send(DownloadEvent::Completed {
                        id: job_cl.id,
                        file_name: job_cl.file_name.clone(),
                        local_path: job_cl.local_path.clone(),
                        result: res,
                    });
                });
            } else {
                break;
            }
        }
        let upload_progress_snapshot = upload_progress.clone();
        let download_progress_snapshot = download_progress.clone();
        terminal.draw(|f| {
            let size = f.size();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(2),
                ])
                .split(size);

            let panels = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(chunks[0]);

            // theme is already captured and cloned above

            // Local panel title
            let local_title = format!(
                "Local: {}",
                local_panel
                    .cwd
                    .to_string_lossy()
                    .to_string()
            );

            let local_items: Vec<ListItem> = local_panel
                .entries
                .iter()
                .map(|e| {
                    let icon = if e.is_dir { "📁" } else { "📄" };
                    ListItem::new(format!("{} {}", icon, e.name))
                })
                .collect();

            let local_block = Block::default()
                .title(local_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.fg));

            let mut local_state = ListState::default();
            if !local_panel.entries.is_empty() {
                local_state.select(Some(local_panel.selected.min(local_panel.entries.len() - 1)));
            }

            let local_list = List::new(local_items).block(local_block).highlight_style(
                if focus == PanelFocus::Local {
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            );

            f.render_stateful_widget(local_list, panels[0], &mut local_state);

            // Remote panel
            let remote_title = format!(
                "Remote: {}@{}:{} - {}",
                user,
                host,
                port,
                remote_panel.cwd.to_string_lossy()
            );

            let remote_items: Vec<ListItem> = remote_panel
                .entries
                .iter()
                .map(|e| {
                    let icon = if e.is_dir { "📁" } else { "📄" };
                    ListItem::new(format!("{} {}", icon, e.name))
                })
                .collect();

            let remote_block = Block::default()
                .title(remote_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg).fg(theme.fg));

            let mut remote_state = ListState::default();
            if !remote_panel.entries.is_empty() {
                remote_state
                    .select(Some(remote_panel.selected.min(remote_panel.entries.len() - 1)));
            }

            let remote_list = List::new(remote_items).block(remote_block).highlight_style(
                if focus == PanelFocus::Remote {
                    Style::default()
                        .bg(theme.accent)
                        .fg(theme.bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            );

            f.render_stateful_widget(remote_list, panels[1], &mut remote_state);

            // --- New two-line footer with download info ---
            let help_text = match mode {
                Mode::Normal => {
                    match focus {
                        PanelFocus::Local => {
                            "Local — Enter: open directory • Backspace: parent • u: upload • /: filter • Tab: switch panel • q: quit"
                        }
                        PanelFocus::Remote => {
                            "Remote — Enter: open directory • Backspace: parent • d: download • /: filter • Tab: switch panel • q: quit"
                        }
                    }
                }
                Mode::Filter => {
                    // Filtering overrides help text fully
                    Box::leak(format!("Filter: {}", filter_input).into_boxed_str())
                }
            };

            let active_count = active_downloads.len();
            let queued_count = pending_downloads.len();

            // Layout for footer (two lines)
            let footer_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(chunks[1]);

            let footer_width = footer_chunks[1].width.saturating_sub(15) as usize;

            // First line: help/message text
            let footer_text_top = Paragraph::new(help_text)
                .style(Style::default().bg(theme.bg).fg(theme.fg));

            // Second line: download status (most recently started)
            let footer_text_bottom = if active_count > 0 {
                // Show the most recently started active single-file download
                let d = active_downloads.last().unwrap();
                let queued_display = if queued_count > 0 {
                    format!(" ({} queued)", queued_count)
                } else {
                    String::new()
                };

                if let Some(total) = d.total_size {
                    if total > 0 {
                        let current = d.current_size.min(total);
                        let percentage = (current.saturating_mul(100) / total) as usize;
                        let filled = (percentage * footer_width / 100).min(footer_width);
                        let empty = footer_width.saturating_sub(filled);
                        let bar = format!(
                            "{}% {}{}",
                            percentage,
                            "▓".repeat(filled),
                            "░".repeat(empty)
                        );
                        Paragraph::new(format!(
                            "Downloading {} {}{}",
                            d.file_name,
                            bar,
                            queued_display
                        ))
                        .style(Style::default().bg(theme.bg).fg(theme.accent))
                    } else {
                        Paragraph::new(format!("Downloading {}{}", d.file_name, queued_display))
                            .style(Style::default().bg(theme.bg).fg(theme.accent))
                    }
                } else {
                    // No known total size, show simple status without bar
                    Paragraph::new(format!("Downloading {}{}", d.file_name, queued_display))
                        .style(Style::default().bg(theme.bg).fg(theme.accent))
                }
            } else if let Some(p) = &download_progress_snapshot {
                let total = if p.total_bytes > 0 { p.total_bytes } else { 1 };
                let current = p.done_bytes.min(total);
                let percentage = (current.saturating_mul(100) / total) as usize;
                let filled = (percentage * footer_width / 100).min(footer_width);
                let empty = footer_width.saturating_sub(filled);
                let bar = format!(
                    "{}% {}{}",
                    percentage,
                    "▓".repeat(filled),
                    "░".repeat(empty)
                );
                Paragraph::new(format!(
                    "{} — {} ({}/{})",
                    p.folder_label,
                    bar,
                    p.files_done,
                    p.files_total
                ))
                .style(Style::default().bg(theme.bg).fg(theme.accent))
            } else if let Some(p) = &upload_progress_snapshot {
                let total = if p.total_bytes > 0 { p.total_bytes } else { 1 };
                let current = p.done_bytes.min(total);
                let percentage = (current.saturating_mul(100) / total) as usize;
                let filled = (percentage * footer_width / 100).min(footer_width);
                let empty = footer_width.saturating_sub(filled);
                let bar = format!(
                    "{}% {}{}",
                    percentage,
                    "▓".repeat(filled),
                    "░".repeat(empty)
                );
                Paragraph::new(format!(
                    "{} — {} ({}/{})",
                    p.label,
                    bar,
                    p.files_done,
                    p.files_total
                ))
                .style(Style::default().bg(theme.bg).fg(theme.accent))
            } else {
                Paragraph::new("")
                    .style(Style::default().bg(theme.bg).fg(theme.fg))
            };

            f.render_widget(footer_text_top, footer_chunks[0]);
            f.render_widget(footer_text_bottom, footer_chunks[1]);
        })?;

        if event::poll(Duration::from_millis(150))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // First, handle filter editing keys (active only in filter mode)
                    if let Mode::Filter = mode {
                        match key.code {
                            KeyCode::Esc => {
                                // Exit filter mode and restore full listing
                                mode = Mode::Normal;
                                filter_input.clear();
                                // Re-read current dirs
                                if focus == PanelFocus::Local {
                                    if let Ok(list) = read_local_dir(&local_panel.cwd) {
                                        local_panel.entries = list;
                                        if local_panel.cwd.parent().is_some() {
                                            local_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                        }
                                        local_panel.selected = 0;
                                    }
                                } else {
                                    let current = remote_panel.cwd.to_string_lossy().to_string();
                                    if let Ok(list) = ssh_list_remote_dir(user, host, port, identity, &current) {
                                        remote_panel.entries = list;
                                        if remote_panel.cwd.to_string_lossy() != "/" {
                                            remote_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                        }
                                        remote_panel.selected = 0;
                                    }
                                }
                                // Esc in filter mode is fully handled here
                                continue;
                            }
                            KeyCode::Char(c) => {
                                // Add to filter and update entries
                                filter_input.push(c);
                            }
                            KeyCode::Backspace => {
                                filter_input.pop();
                            }
                            _ => {
                                // Non-filter-editing keys (arrows, enter, d, etc.) fall through
                            }
                        }

                        // For Char/Backspace we recalc filtered entries and stay in filter mode.
                        match key.code {
                            KeyCode::Char(_) | KeyCode::Backspace => {
                                let filter = filter_input.clone();
                                if focus == PanelFocus::Local {
                                    let base_list = read_local_dir(&local_panel.cwd).unwrap_or_default();
                                    let mut filtered = apply_filter(&base_list, &filter);
                                    if local_panel.cwd.parent().is_some() {
                                        filtered.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                    }
                                    local_panel.entries = filtered;
                                    local_panel.selected = 0;
                                } else {
                                    let current = remote_panel.cwd.to_string_lossy().to_string();
                                    if let Ok(list) = ssh_list_remote_dir(user, host, port, identity, &current) {
                                        let mut filtered = apply_filter(&list, &filter);
                                        if remote_panel.cwd.to_string_lossy() != "/" {
                                            filtered.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                        }
                                        remote_panel.entries = filtered;
                                        remote_panel.selected = 0;
                                    }
                                }
                                // After editing the filter, we do not handle the key further.
                                continue;
                            }
                            _ => {
                                // If it's another key (navigation, enter, d...), we let it be handled below.
                            }
                        }
                    }

                    // Global key handling (works in both Normal and Filter modes, except Esc in filter which is handled above)
                    match key.code {
                        KeyCode::Char('q') => {
                            break 'outer;
                        }
                        KeyCode::Esc => {
                            // In normal mode, Esc behaves like quit; in filter mode it's already handled above
                            if let Mode::Normal = mode {
                                break 'outer;
                            }
                        }
                        KeyCode::Tab => {
                            focus = match focus {
                                PanelFocus::Local => PanelFocus::Remote,
                                PanelFocus::Remote => PanelFocus::Local,
                            };
                        }
                        KeyCode::Up => {
                            let panel = match focus {
                                PanelFocus::Local => &mut local_panel,
                                PanelFocus::Remote => &mut remote_panel,
                            };
                            if panel.selected > 0 {
                                panel.selected -= 1;
                            }
                        }
                        KeyCode::Down => {
                            let panel = match focus {
                                PanelFocus::Local => &mut local_panel,
                                PanelFocus::Remote => &mut remote_panel,
                            };
                            if !panel.entries.is_empty() {
                                panel.selected = (panel.selected + 1).min(panel.entries.len() - 1);
                            }
                        }
                        KeyCode::Char('/') => {
                            mode = Mode::Filter;
                            filter_input.clear();
                            message = None;
                        }
                        KeyCode::Enter => {
                            match focus {
                                PanelFocus::Local => {
                                    if let Some(entry) = local_panel.selected_entry() {
                                        if entry.name == ".." {
                                            if let Some(parent) = local_panel.cwd.parent() {
                                                let parent = parent.to_path_buf();
                                                if let Ok(list) = read_local_dir(&parent) {
                                                    local_panel.cwd = parent;
                                                    local_panel.entries = list;
                                                    if local_panel.cwd.parent().is_some() {
                                                        local_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                                    }
                                                    local_panel.selected = 0;
                                                    message = None;
                                                }
                                            }
                                        } else if entry.is_dir {
                                            let new_cwd = local_panel.cwd.join(&entry.name);
                                            match read_local_dir(&new_cwd) {
                                                Ok(list) => {
                                                    local_panel.cwd = new_cwd;
                                                    local_panel.entries = list;
                                                    if local_panel.cwd.parent().is_some() {
                                                        local_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                                    }
                                                    local_panel.selected = 0;
                                                    message = None;
                                                }
                                                Err(e) => {
                                                    message = Some(format!("Local read error: {}", e));
                                                }
                                            }
                                        }
                                    }
                                }
                                PanelFocus::Remote => {
                                    if let Some(entry) = remote_panel.selected_entry() {
                                        if entry.name == ".." {
                                            let current = remote_panel.cwd.to_string_lossy().to_string();
                                            let parent = parent_remote_path(&current);
                                            if let Ok(list) = ssh_list_remote_dir(user, host, port, identity, &parent) {
                                                remote_panel.cwd = PathBuf::from(&parent);
                                                remote_panel.entries = list;
                                                if remote_panel.cwd.to_string_lossy() != "/" {
                                                    remote_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                                }
                                                remote_panel.selected = 0;
                                                message = None;
                                            }
                                        } else if entry.is_dir {
                                            let new_path = join_remote_path(
                                                &remote_panel.cwd.to_string_lossy(),
                                                &entry.name,
                                            );
                                            match ssh_list_remote_dir(
                                                user,
                                                host,
                                                port,
                                                identity,
                                                &new_path,
                                            ) {
                                                Ok(list) => {
                                                    remote_panel.cwd = PathBuf::from(&new_path);
                                                    remote_panel.entries = list;
                                                    if remote_panel.cwd.to_string_lossy() != "/" {
                                                        remote_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                                    }
                                                    remote_panel.selected = 0;
                                                    message = None;
                                                }
                                                Err(e) => {
                                                    message = Some(format!(
                                                        "Remote read error: {}",
                                                        e
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // F-Enter-B behavior: whenever Enter navigates, exit filter mode and clear filter
                            mode = Mode::Normal;
                            filter_input.clear();
                        }
                        KeyCode::Backspace => {
                            match focus {
                                PanelFocus::Local => {
                                    if let Some(parent) = local_panel.cwd.parent() {
                                        let parent = parent.to_path_buf();
                                        match read_local_dir(&parent) {
                                            Ok(list) => {
                                                local_panel.cwd = parent;
                                                local_panel.entries = list;
                                                if local_panel.cwd.parent().is_some() {
                                                    local_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                                }
                                                local_panel.selected = 0;
                                                message = None;
                                            }
                                            Err(e) => {
                                                message = Some(format!("Local read error: {}", e));
                                            }
                                        }
                                    }
                                }
                                PanelFocus::Remote => {
                                    let current = remote_panel.cwd.to_string_lossy().to_string();
                                    let parent = parent_remote_path(&current);
                                    match ssh_list_remote_dir(user, host, port, identity, &parent) {
                                        Ok(list) => {
                                            remote_panel.cwd = PathBuf::from(&parent);
                                            remote_panel.entries = list;
                                            if remote_panel.cwd.to_string_lossy() != "/" {
                                                remote_panel.entries.insert(0, FileEntry { name: "..".to_string(), is_dir: true });
                                            }
                                            remote_panel.selected = 0;
                                            message = None;
                                        }
                                        Err(e) => {
                                            message = Some(format!("Remote read error: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('u') => {
                            if focus == PanelFocus::Local {
                                if let Some(entry) = local_panel.selected_entry() {
                                    if entry.name == ".." {
                                        // Do nothing on parent pseudo-entry
                                    } else {
                                        let name = entry.name.clone();
                                        let name_for_thread = name.clone();
                                        let is_dir = entry.is_dir;
                                        let local_root = local_panel.cwd.clone();
                                        let remote_cwd = remote_panel.cwd.to_string_lossy().to_string();
                                        let user_cl = user.to_string();
                                        let host_cl = host.to_string();
                                        let id_cl = identity.map(|s| s.to_string());
                                        let ul_tx_clone = ul_tx.clone();

                                        thread::spawn(move || {
                                            use std::path::Path as StdPath;

                                            if is_dir {
                                                let local_path = local_root.join(&name_for_thread);
                                                let remote_root = join_remote_path(&remote_cwd, &name_for_thread);

                                                // First pass: collect all files and their sizes
                                                let mut files: Vec<(PathBuf, String, u64)> = Vec::new();
                                                let mut stack: Vec<(PathBuf, String)> = Vec::new();
                                                stack.push((local_path.clone(), remote_root.clone()));

                                                while let Some((l_dir, r_dir)) = stack.pop() {
                                                    if let Ok(read_dir) = fs::read_dir(&l_dir) {
                                                        for entry in read_dir {
                                                            if let Ok(entry) = entry {
                                                                let path = entry.path();
                                                                let name = entry.file_name().to_string_lossy().to_string();
                                                                let remote_path = join_remote_path(&r_dir, &name);
                                                                if path.is_dir() {
                                                                    stack.push((path, remote_path));
                                                                } else {
                                                                    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                                                                    files.push((path, remote_path, size));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }

                                                let total_files = files.len();
                                                let total_bytes: u64 = files.iter().map(|(_, _, s)| *s).sum();
                                                let mut done_files: usize = 0;
                                                let mut done_bytes: u64 = 0;

                                                // Ensure root dir exists
                                                let _ = ssh_mkdir_remote(&user_cl, &host_cl, port, id_cl.as_deref(), &remote_root);

                                                for (local_file, remote_file, size) in files {
                                                    // Ensure parent directory exists
                                                    if let Some(parent) = StdPath::new(&remote_file).parent() {
                                                        let parent_str = parent.to_string_lossy().to_string();
                                                        let _ = ssh_mkdir_remote(&user_cl, &host_cl, port, id_cl.as_deref(), &parent_str);
                                                    }

                                                    if upload_local_file(
                                                        &user_cl,
                                                        &host_cl,
                                                        port,
                                                        id_cl.as_deref(),
                                                        &local_file,
                                                        &remote_file,
                                                    ).is_ok() {
                                                        done_files += 1;
                                                        done_bytes = done_bytes.saturating_add(size);
                                                        let _ = ul_tx_clone.send(SftpEvent::UploadProgress {
                                                            label: format!("Folder: {}", name_for_thread),
                                                            files_done: done_files,
                                                            files_total: total_files,
                                                            done_bytes,
                                                            total_bytes,
                                                        });
                                                    } else {
                                                        break;
                                                    }
                                                }

                                                let _ = ul_tx_clone.send(SftpEvent::UploadCompleted { file_name: name_for_thread.clone() });
                                            } else {
                                                let local_path = local_root.join(&name_for_thread);
                                                let remote_target = join_remote_path(&remote_cwd, &name_for_thread);
                                                let size = fs::metadata(&local_path).map(|m| m.len()).unwrap_or(0);

                                                if upload_local_file(
                                                    &user_cl,
                                                    &host_cl,
                                                    port,
                                                    id_cl.as_deref(),
                                                    &local_path,
                                                    &remote_target,
                                                ).is_ok() {
                                                    let _ = ul_tx_clone.send(SftpEvent::UploadProgress {
                                                        label: format!("File: {}", name_for_thread),
                                                        files_done: 1,
                                                        files_total: 1,
                                                        done_bytes: size,
                                                        total_bytes: size,
                                                    });
                                                    let _ = ul_tx_clone.send(SftpEvent::UploadCompleted { file_name: name_for_thread.clone() });
                                                }
                                            }
                                        });

                                        message = Some(format!("Uploading '{}' in background…", name));
                                    }
                                }
                            }
                        }
                        KeyCode::Char('d') => {
                            if focus == PanelFocus::Remote {
                                if let Some(entry) = remote_panel.selected_entry() {
                                    if !entry.is_dir {
                                        let remote_full = join_remote_path(
                                            &remote_panel.cwd.to_string_lossy(),
                                            &entry.name,
                                        );
                                        let local_target = unique_local_path(&local_panel.cwd, &entry.name);
                                        let total_size = ssh_remote_file_size(
                                            user,
                                            host,
                                            port,
                                            identity,
                                            &remote_full,
                                        ).unwrap_or(None);

                                        let job = DownloadJob {
                                            id: next_download_id,
                                            file_name: entry.name.clone(),
                                            local_path: local_target,
                                            remote_path: remote_full,
                                            total_size,
                                        };
                                        next_download_id += 1;
                                        pending_downloads.push_back(job);
                                        message = Some("Queued download".to_string());
                                    } else {
                                        // Download folder: queue a background scan job and download all files in folder
                                        let folder_name = entry.name.clone();
                                        let remote_root = join_remote_path(
                                            &remote_panel.cwd.to_string_lossy(),
                                            &entry.name,
                                        );
                                        let local_root = unique_local_path(&local_panel.cwd, &entry.name);
                                        let user_cl2 = user.to_string();
                                        let host_cl2 = host.to_string();
                                        let id_cl2 = identity.map(|s| s.to_string());
                                        let tx_clone2 = dl_tx.clone();
                                        let local_root_clone = local_root.clone();

                                        thread::spawn(move || {
                                            use std::path::Path as StdPath;

                                            let mut stack = Vec::new();
                                            stack.push(remote_root.clone());
                                            let mut files = Vec::new();
                                            // BFS scan: collect only files with their sizes
                                            while let Some(dir) = stack.pop() {
                                                if let Ok(list) = ssh_list_remote_dir(
                                                    &user_cl2,
                                                    &host_cl2,
                                                    port,
                                                    id_cl2.as_deref(),
                                                    &dir
                                                ) {
                                                    for e in list {
                                                        if e.name == ".." {
                                                            continue;
                                                        }
                                                        let full = join_remote_path(&dir, &e.name);
                                                        if e.is_dir {
                                                            stack.push(full);
                                                        } else {
                                                            if let Ok(size_opt) = ssh_remote_file_size(
                                                                &user_cl2,
                                                                &host_cl2,
                                                                port,
                                                                id_cl2.as_deref(),
                                                                &full
                                                            ) {
                                                                let size = size_opt.unwrap_or(0);
                                                                files.push((full, size));
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                            let files_total = files.len();
                                            let total_bytes: u64 = files.iter().map(|(_, s)| s).sum();
                                            let _ = tx_clone2.send(DownloadEvent::Progress {
                                                folder_label: format!("Folder: {}", folder_name),
                                                files_done: 0,
                                                files_total,
                                                done_bytes: 0,
                                                total_bytes,
                                            });

                                            // Ensure the local root directory exists
                                            if let Err(e) = fs::create_dir_all(&local_root_clone) {
                                                let _ = tx_clone2.send(DownloadEvent::Completed {
                                                    id: 0,
                                                    file_name: format!("Folder: {}", folder_name),
                                                    local_path: local_root_clone.clone(),
                                                    result: Err(e),
                                                });
                                                return;
                                            }

                                            let mut done_files: usize = 0;
                                            let mut done_bytes: u64 = 0;

                                            for (remote_file, size) in files {
                                                // Compute relative path from remote_root to this file
                                                let rel = match StdPath::new(&remote_file).strip_prefix(&remote_root) {
                                                    Ok(p) => p.to_path_buf(),
                                                    Err(_) => StdPath::new(&remote_file).file_name()
                                                        .map(|n| StdPath::new(n).to_path_buf())
                                                        .unwrap_or_else(|| StdPath::new("").to_path_buf()),
                                                };
                                                let local_file_path = if rel.as_os_str().is_empty() {
                                                    local_root_clone.join(
                                                        StdPath::new(&remote_file)
                                                            .file_name()
                                                            .unwrap_or_default(),
                                                    )
                                                } else {
                                                    local_root_clone.join(&rel)
                                                };

                                                if let Some(parent) = local_file_path.parent() {
                                                    let _ = fs::create_dir_all(parent);
                                                }

                                                let res = download_remote_file(
                                                    &user_cl2,
                                                    &host_cl2,
                                                    port,
                                                    id_cl2.as_deref(),
                                                    &remote_file,
                                                    &local_file_path,
                                                );

                                                match res {
                                                    Ok(()) => {
                                                        done_files += 1;
                                                        done_bytes = done_bytes.saturating_add(size);
                                                        let _ = tx_clone2.send(DownloadEvent::Progress {
                                                            folder_label: format!("Folder: {}", folder_name),
                                                            files_done: done_files,
                                                            files_total,
                                                            done_bytes,
                                                            total_bytes,
                                                        });
                                                    }
                                                    Err(e) => {
                                                        let _ = tx_clone2.send(DownloadEvent::Completed {
                                                            id: 0,
                                                            file_name: format!("Folder: {}", folder_name),
                                                            local_path: local_root_clone.clone(),
                                                            result: Err(e),
                                                        });
                                                        return;
                                                    }
                                                }
                                            }

                                            let _ = tx_clone2.send(DownloadEvent::Completed {
                                                id: 0,
                                                file_name: format!("Folder: {}", folder_name),
                                                local_path: local_root_clone.clone(),
                                                result: Ok(()),
                                            });
                                        });

                                        message = Some(format!("Scanning folder '{}'…", entry.name));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}
fn apply_filter(entries: &[FileEntry], filter: &str) -> Vec<FileEntry> {
    if filter.is_empty() { return entries.to_vec(); }
    entries.iter()
        .filter(|e| e.name.to_lowercase().contains(&filter.to_lowercase()))
        .cloned()
        .collect()
}