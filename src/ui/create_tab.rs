use crate::database::{default_output_filename, HashDatabase};
use crate::hasher::HashAlgorithm;
use crate::scanner::{ScanOptions, ScanProgress, ScanResult};
use gtk4::prelude::*;
use gtk4::{
    Align, Button, CheckButton, DropDown, Entry, FileDialog, Label,
    Orientation, ProgressBar, ScrolledWindow, StringList, TextView, WrapMode,
};
use libadwaita::prelude::*;
use libadwaita::{ActionRow, EntryRow, PreferencesGroup};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub struct CreateTab {
    root: gtk4::Box,
}

impl CreateTab {
    pub fn new() -> Self {
        let root = gtk4::Box::new(Orientation::Vertical, 0);
        root.set_margin_top(16);
        root.set_margin_bottom(16);
        root.set_margin_start(16);
        root.set_margin_end(16);
        root.add_css_class("create-tab");

        // Standalone path entry — NOT inside ActionRow to avoid widget hierarchy issues
        let path_box = gtk4::Box::new(Orientation::Horizontal, 8);
        path_box.set_margin_top(4);
        path_box.set_margin_bottom(4);

        let path_label = Label::new(Some("Path:"));
        path_label.add_css_class("monospace");
        path_label.set_width_chars(6);
        path_label.set_halign(Align::Start);

        let path_entry = Entry::new();
        path_entry.set_hexpand(true);
        path_entry.add_css_class("monospace");
        path_entry.set_placeholder_text(Some(&format!("{}/documents", std::env::var("HOME").unwrap_or_else(|_| "/home/username".to_string()))));
        // Set default to user home dir
        let default_path = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
        path_entry.set_text(&default_path);

        let browse_file_btn = Button::with_label("📄 File");
        browse_file_btn.add_css_class("retro-btn");

        let browse_folder_btn = Button::with_label("📁 Folder");
        browse_folder_btn.add_css_class("retro-btn");

        path_box.append(&path_label);
        path_box.append(&path_entry);
        path_box.append(&browse_file_btn);
        path_box.append(&browse_folder_btn);

        // ── Options group
        let opts_group = PreferencesGroup::new();
        opts_group.set_title("Scan Options");

        let algo_row = ActionRow::new();
        algo_row.set_title("Hash Algorithm");
        let algo_model = StringList::new(&["SHA-256", "SHA-512", "BLAKE3"]);
        let algo_drop = DropDown::new(Some(algo_model), gtk4::Expression::NONE);
        algo_drop.set_selected(0);
        algo_drop.set_valign(Align::Center);
        algo_row.add_suffix(&algo_drop);

        let recursive_row = ActionRow::new();
        recursive_row.set_title("Recursive");
        recursive_row.set_subtitle("Scan subdirectories");
        let recursive_check = CheckButton::new();
        recursive_check.set_active(true);
        recursive_check.set_valign(Align::Center);
        recursive_row.add_suffix(&recursive_check);
        recursive_row.set_activatable_widget(Some(&recursive_check));

        let vfs_row = ActionRow::new();
        vfs_row.set_title("Skip Virtual Filesystems");
        vfs_row.set_subtitle("/proc, /sys, /dev, /run");
        let vfs_check = CheckButton::new();
        vfs_check.set_active(true);
        vfs_check.set_valign(Align::Center);
        vfs_row.add_suffix(&vfs_check);
        vfs_row.set_activatable_widget(Some(&vfs_check));

        let exclude_row = EntryRow::new();
        exclude_row.set_title("Exclude Patterns");
        exclude_row.set_tooltip_text(Some("Comma-separated globs: *.log, *.tmp, .git"));
        exclude_row.set_text("*.log, *.tmp, .git");

        opts_group.add(&algo_row);
        opts_group.add(&recursive_row);
        opts_group.add(&vfs_row);
        opts_group.add(&exclude_row);

        // ── Progress area
        let progress_box = gtk4::Box::new(Orientation::Vertical, 6);
        progress_box.set_visible(false);
        progress_box.set_margin_top(8);

        let progress_bar = ProgressBar::new();
        progress_bar.add_css_class("retro-progress");
        progress_bar.set_show_text(true);

        let status_label = Label::new(Some(""));
        status_label.add_css_class("monospace");
        status_label.add_css_class("status-label");
        status_label.set_halign(Align::Start);
        status_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        status_label.set_max_width_chars(80);

        progress_box.append(&progress_bar);
        progress_box.append(&status_label);

        // ── Log output
        let log_scroll = ScrolledWindow::new();
        log_scroll.set_vexpand(true);
        log_scroll.set_min_content_height(160);
        log_scroll.add_css_class("log-scroll");

        let log_view = TextView::new();
        log_view.set_editable(false);
        log_view.set_wrap_mode(WrapMode::Word);
        log_view.add_css_class("log-view");
        log_view.add_css_class("monospace");
        log_scroll.set_child(Some(&log_view));

        // ── Action buttons
        let btn_row = gtk4::Box::new(Orientation::Horizontal, 8);
        btn_row.set_halign(Align::End);
        btn_row.set_margin_top(8);

        let cancel_btn = Button::with_label("⏹  Cancel");
        cancel_btn.add_css_class("retro-btn");
        cancel_btn.add_css_class("destructive-action");
        cancel_btn.set_sensitive(false);

        let start_btn = Button::with_label("▶  Start Scan");
        start_btn.add_css_class("retro-btn");
        start_btn.add_css_class("suggested-action");

        btn_row.append(&cancel_btn);
        btn_row.append(&start_btn);

        let prefs_box = gtk4::Box::new(Orientation::Vertical, 12);

        // Path entry section header
        let path_section = gtk4::Box::new(Orientation::Vertical, 6);
        path_section.add_css_class("path-section");
        let path_title = Label::new(Some("Target Path"));
        path_title.add_css_class("section-title");
        path_title.set_halign(Align::Start);
        let path_hint = Label::new(Some("Type a full absolute path, or use the browse buttons"));
        path_hint.add_css_class("dim-label");
        path_hint.set_halign(Align::Start);
        path_section.append(&path_title);
        path_section.append(&path_hint);
        path_section.append(&path_box);

        prefs_box.append(&path_section);
        prefs_box.append(&opts_group);

        root.append(&prefs_box);
        root.append(&btn_row);
        root.append(&progress_box);
        root.append(&log_scroll);

        // ── State
        let cancellation: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));
        let last_db: Rc<RefCell<Option<HashDatabase>>> = Rc::new(RefCell::new(None));

        // ── Browse: 📄 File button
        {
            let path_entry = path_entry.clone();
            browse_file_btn.connect_clicked(move |btn| {
                let dialog = FileDialog::builder().title("Select File").build();
                let path_entry = path_entry.clone();
                let window = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                dialog.open(window.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            path_entry.set_text(&path.to_string_lossy());
                        }
                    }
                });
            });
        }

        // ── Browse: 📁 Folder button
        {
            let path_entry = path_entry.clone();
            browse_folder_btn.connect_clicked(move |btn| {
                let dialog = FileDialog::builder().title("Select Folder").build();
                let path_entry = path_entry.clone();
                let window = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                dialog.select_folder(window.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            path_entry.set_text(&path.to_string_lossy());
                        }
                    }
                });
            });
        }

        // ── Start Scan button
        {
            let path_entry   = path_entry.clone();
            let cancellation = cancellation.clone();
            let last_db      = last_db.clone();
            let progress_box = progress_box.clone();
            let progress_bar = progress_bar.clone();
            let status_label = status_label.clone();
            let log_view     = log_view.clone();
            let start_btn_c  = start_btn.clone();
            let cancel_btn_c = cancel_btn.clone();

            start_btn.connect_clicked(move |_| {
                // Read via buffer to bypass libadwaita::prelude trait ambiguity
                let raw = path_entry.buffer().text().to_string();
                let path_text = raw.trim().to_string();

                // Expand ~ to the real home directory
                let path_text = if path_text.starts_with("~/") {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
                    format!("{}/{}", home, &path_text[2..])
                } else if path_text == "~" {
                    std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
                } else {
                    path_text
                };

                if path_text.is_empty() {
                    append_log(&log_view, "❌ No path entered.");
                    return;
                }

                let path = PathBuf::from(&path_text);
                if !path.exists() {
                    append_log(&log_view, &format!(
                        "❌ Path does not exist: {}\n   (hint: use a full absolute path like {}/Documents)",
                        std::env::var("HOME").unwrap_or_else(|_| "/home/username".to_string()),
                        path_text));
                    return;
                }

                if crate::utils::path_needs_root(&path) {
                    if !crate::utils::show_sudo_dialog() { return; }
                }

                let algorithm = match algo_drop.selected() {
                    0 => HashAlgorithm::Sha256,
                    1 => HashAlgorithm::Sha512,
                    _ => HashAlgorithm::Blake3,
                };
                let recursive = recursive_check.is_active();
                let skip_vfs  = vfs_check.is_active();
                let exclude_patterns: Vec<String> = exclude_row.text()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                let cancelled = Arc::new(AtomicBool::new(false));
                *cancellation.borrow_mut() = Some(cancelled.clone());

                progress_box.set_visible(true);
                progress_bar.set_fraction(0.0);
                status_label.set_text("Preparing scan…");
                start_btn_c.set_sensitive(false);
                cancel_btn_c.set_sensitive(true);
                append_log(&log_view, &format!(
                    "🔍 Scanning: {} ({:?})", path.display(), algorithm));

                let (tx, rx) = mpsc::channel::<ScanMessage>();
                let tx2 = tx.clone();
                let options = ScanOptions {
                    root: path,
                    algorithm,
                    recursive,
                    exclude_patterns,
                    skip_virtual_fs: skip_vfs,
                };

                std::thread::spawn(move || {
                    let result = crate::scanner::scan_files(
                        options, cancelled,
                        move |p| { let _ = tx2.send(ScanMessage::Progress(p)); },
                    );
                    let _ = tx.send(ScanMessage::Done(result));
                });

                let progress_bar = progress_bar.clone();
                let status_label = status_label.clone();
                let log_view     = log_view.clone();
                let start_btn_c  = start_btn_c.clone();
                let cancel_btn_c = cancel_btn_c.clone();
                let last_db      = last_db.clone();

                glib::timeout_add_local(
                    std::time::Duration::from_millis(50),
                    move || {
                        loop {
                            match rx.try_recv() {
                                Ok(ScanMessage::Progress(p)) => {
                                    progress_bar.pulse();
                                    status_label.set_text(&format!(
                                        "Scanned: {} files  |  Current: {}",
                                        p.files_scanned, p.current_file));
                                }
                                Ok(ScanMessage::Done(result)) => {
                                    start_btn_c.set_sensitive(true);
                                    cancel_btn_c.set_sensitive(false);
                                    progress_bar.set_fraction(1.0);
                                    let n = result.database.entries.len();
                                    let e = result.errors.len();
                                    status_label.set_text(&format!(
                                        "✅ Complete — {} files, {} errors, {} skipped",
                                        n, e, result.skipped));
                                    for (p, err) in &result.errors {
                                        append_log(&log_view,
                                            &format!("  ⚠  {} — {}", p.display(), err));
                                    }
                                    append_log(&log_view,
                                        &format!("✅ Done: {} files, {} errors", n, e));
                                    *last_db.borrow_mut() = Some(result.database);
                                    show_save_dialog(&log_view, last_db.clone());
                                    return glib::ControlFlow::Break;
                                }
                                Err(mpsc::TryRecvError::Empty) => break,
                                Err(mpsc::TryRecvError::Disconnected) => {
                                    return glib::ControlFlow::Break;
                                }
                            }
                        }
                        glib::ControlFlow::Continue
                    },
                );
            });
        }

        // ── Cancel button
        {
            let cancellation = cancellation.clone();
            cancel_btn.connect_clicked(move |btn| {
                if let Some(c) = cancellation.borrow().as_ref() {
                    c.store(true, Ordering::Relaxed);
                }
                btn.set_sensitive(false);
            });
        }

        Self { root }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.root
    }
}

enum ScanMessage {
    Progress(ScanProgress),
    Done(ScanResult),
}

fn append_log(view: &TextView, text: &str) {
    let buf = view.buffer();
    let mut end = buf.end_iter();
    buf.insert(&mut end, &format!("{}\n", text));
    let mark = buf.create_mark(None, &buf.end_iter(), false);
    view.scroll_mark_onscreen(&mark);
}

fn show_save_dialog(log_view: &TextView, db_cell: Rc<RefCell<Option<HashDatabase>>>) {
    let db = match db_cell.borrow().clone() { Some(d) => d, None => return };
    let default_name = default_output_filename(db.algorithm);
    let dialog = FileDialog::builder()
        .title("Save Hash Database")
        .initial_name(&default_name)
        .build();
    let log_view = log_view.clone();
    dialog.save(gtk4::Window::NONE, gtk4::gio::Cancellable::NONE, move |result| {
        if let Ok(file) = result {
            if let Some(path) = file.path() {
                match db.save_txt(&path) {
                    Ok(_)  => append_log(&log_view, &format!("💾 Saved: {}", path.display())),
                    Err(e) => append_log(&log_view, &format!("❌ Save failed: {}", e)),
                }
            }
        }
    });
}
