use crate::database::HashDatabase;
use crate::ui::diff_viewer::DiffViewer;
use crate::verifier::{verify_files, VerifyProgress, VerifyResult};
use gtk4::prelude::*;
use gtk4::{
    Align, Button, CheckButton, FileDialog, Label, Orientation, ProgressBar, ScrolledWindow,
};
use libadwaita::prelude::*;
use libadwaita::{ActionRow, PreferencesGroup};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub struct VerifyTab {
    root: gtk4::ScrolledWindow,
}

impl VerifyTab {
    pub fn new() -> Self {
        // Root is a scrolled window so the whole tab scrolls on small screens
        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);

        let root = gtk4::Box::new(Orientation::Vertical, 12);
        root.set_margin_top(16);
        root.set_margin_bottom(16);
        root.set_margin_start(16);
        root.set_margin_end(16);
        scroll.set_child(Some(&root));

        // ── Input group
        let input_group = PreferencesGroup::new();
        input_group.set_title("Inputs");

        let db_row = ActionRow::new();
        db_row.set_title("Hash Database");
        db_row.set_subtitle("Select the .txt database created by HashMyFiles");
        let db_label = Label::new(Some("(none selected)"));
        db_label.add_css_class("dim-label");
        db_label.add_css_class("monospace");
        let db_btn = Button::with_label("Select DB…");
        db_btn.add_css_class("retro-btn");
        db_btn.set_valign(Align::Center);
        db_row.add_suffix(&db_label);
        db_row.add_suffix(&db_btn);

        // Standalone path entry for target — outside ActionRow to avoid text() issues
        let dir_path_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        dir_path_box.set_margin_top(4);
        dir_path_box.set_margin_bottom(4);

        let dir_path_label = Label::new(Some("Path:"));
        dir_path_label.add_css_class("monospace");
        dir_path_label.set_width_chars(6);
        dir_path_label.set_halign(Align::Start);

        let dir_entry = gtk4::Entry::new();
        dir_entry.set_hexpand(true);
        dir_entry.add_css_class("monospace");
        dir_entry.set_placeholder_text(Some(&format!("{}/documents", std::env::var("HOME").unwrap_or_else(|_| "/home/username".to_string()))));
        let default_dir = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
        dir_entry.set_text(&default_dir);

        let dir_file_btn = Button::with_label("📄 File");
        dir_file_btn.add_css_class("retro-btn");

        let dir_btn = Button::with_label("📁 Folder");
        dir_btn.add_css_class("retro-btn");

        dir_path_box.append(&dir_path_label);
        dir_path_box.append(&dir_entry);
        dir_path_box.append(&dir_file_btn);
        dir_path_box.append(&dir_btn);

        let algo_row = ActionRow::new();
        algo_row.set_title("Algorithm");
        algo_row.set_subtitle("Auto-detected from DB header");
        let algo_label = Label::new(Some("Auto"));
        algo_label.add_css_class("dim-label");
        algo_label.add_css_class("monospace");
        algo_row.add_suffix(&algo_label);

        let vfs_row = ActionRow::new();
        vfs_row.set_title("Skip Virtual Filesystems");
        vfs_row.set_subtitle("/proc, /sys, /dev, /run");
        let vfs_check = CheckButton::new();
        vfs_check.set_active(true);
        vfs_check.set_valign(Align::Center);
        vfs_row.add_suffix(&vfs_check);
        vfs_row.set_activatable_widget(Some(&vfs_check));

        input_group.add(&db_row);

        input_group.add(&algo_row);
        input_group.add(&vfs_row);

        // ── Action buttons — placed HIGH so they're always visible
        let btn_row = gtk4::Box::new(Orientation::Horizontal, 8);
        btn_row.set_halign(Align::End);
        let cancel_btn = Button::with_label("⏹  Cancel");
        cancel_btn.add_css_class("retro-btn");
        cancel_btn.add_css_class("destructive-action");
        cancel_btn.set_sensitive(false);
        let start_btn = Button::with_label("▶  Start Verification");
        start_btn.add_css_class("retro-btn");
        start_btn.add_css_class("suggested-action");
        btn_row.append(&cancel_btn);
        btn_row.append(&start_btn);

        // ── Progress (hidden until scan starts)
        let progress_box = gtk4::Box::new(Orientation::Vertical, 6);
        progress_box.set_visible(false);
        let progress_bar = ProgressBar::new();
        progress_bar.add_css_class("retro-progress");
        progress_bar.set_show_text(true);
        let progress_label = Label::new(Some(""));
        progress_label.add_css_class("monospace");
        progress_label.add_css_class("status-label");
        progress_label.set_halign(Align::Start);
        progress_box.append(&progress_bar);
        progress_box.append(&progress_label);

        // ── Export row (hidden until results ready)
        let export_row = gtk4::Box::new(Orientation::Horizontal, 8);
        export_row.set_halign(Align::End);
        export_row.set_visible(false);
        let export_txt_btn  = Button::with_label("Export .txt");
        let export_json_btn = Button::with_label("Export .json");
        let export_csv_btn  = Button::with_label("Export .csv");
        for btn in &[&export_txt_btn, &export_json_btn, &export_csv_btn] {
            btn.add_css_class("retro-btn");
            export_row.append(*btn);
        }

        // ── Diff viewer (expands to fill remaining space)
        let diff_viewer = Rc::new(DiffViewer::new());

        // Layout order: inputs → path entry → buttons → progress → export → results
        root.append(&input_group);

        // Target path row (standalone, outside PreferencesGroup)
        let dir_target_lbl = Label::new(Some("Target Path"));
        dir_target_lbl.add_css_class("section-title");
        dir_target_lbl.set_halign(Align::Start);
        let dir_hint = Label::new(Some("Type a full absolute path, or browse"));
        dir_hint.add_css_class("dim-label");
        dir_hint.set_halign(Align::Start);
        root.append(&dir_target_lbl);
        root.append(&dir_hint);
        root.append(&dir_path_box);
        root.append(&btn_row);
        root.append(&progress_box);
        root.append(&export_row);
        root.append(diff_viewer.widget());

        // ── State
        let selected_db:  Rc<RefCell<Option<HashDatabase>>>   = Rc::new(RefCell::new(None));
        let cancellation: Rc<RefCell<Option<Arc<AtomicBool>>>> = Rc::new(RefCell::new(None));
        let last_result:  Rc<RefCell<Option<VerifyResult>>>    = Rc::new(RefCell::new(None));

        // DB browse
        {
            let selected_db = selected_db.clone();
            let db_label    = db_label.clone();
            let algo_label  = algo_label.clone();
            db_btn.connect_clicked(move |btn| {
                let dialog = FileDialog::builder().title("Select Hash Database").build();
                let filter  = gtk4::FileFilter::new();
                filter.add_pattern("*.txt");
                filter.set_name(Some("Hash databases (*.txt)"));
                let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
                filters.append(&filter);
                dialog.set_filters(Some(&filters));

                let selected_db = selected_db.clone();
                let db_label    = db_label.clone();
                let algo_label  = algo_label.clone();
                let window = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                dialog.open(window.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            match HashDatabase::load_txt(&path) {
                                Ok(db) => {
                                    let n    = db.entries.len();
                                    let algo = db.algorithm.as_str().to_string();
                                    let name = path.file_name()
                                        .map(|n| n.to_string_lossy().to_string())
                                        .unwrap_or_default();
                                    db_label.set_text(&format!("{} ({} entries)", name, n));
                                    algo_label.set_text(&format!("Auto ({})", algo));
                                    *selected_db.borrow_mut() = Some(db);
                                }
                                Err(e) => db_label.set_text(&format!("❌ Error: {}", e)),
                            }
                        }
                    }
                });
            });
        }

        // Dir file browse button
        {
            let dir_entry = dir_entry.clone();
            dir_file_btn.connect_clicked(move |btn| {
                let dialog = FileDialog::builder().title("Select File").build();
                let dir_entry = dir_entry.clone();
                let window = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                dialog.open(window.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            dir_entry.set_text(&path.to_string_lossy());
                        }
                    }
                });
            });
        }

        // Dir folder browse button
        {
            let dir_entry = dir_entry.clone();
            dir_btn.connect_clicked(move |btn| {
                let dialog = FileDialog::builder().title("Select Folder").build();
                let dir_entry = dir_entry.clone();
                let window = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
                dialog.select_folder(window.as_ref(), gtk4::gio::Cancellable::NONE, move |result| {
                    if let Ok(file) = result {
                        if let Some(path) = file.path() {
                            dir_entry.set_text(&path.to_string_lossy());
                        }
                    }
                });
            });
        }

        // Start verification
        {
            let selected_db    = selected_db.clone();
            let dir_entry      = dir_entry.clone();
            let cancellation   = cancellation.clone();
            let last_result    = last_result.clone();
            let progress_box   = progress_box.clone();
            let progress_bar   = progress_bar.clone();
            let progress_label = progress_label.clone();
            let start_btn_c    = start_btn.clone();
            let cancel_btn_c   = cancel_btn.clone();
            let export_row     = export_row.clone();
            let diff_viewer    = diff_viewer.clone();

            start_btn.connect_clicked(move |_| {
                let db = match selected_db.borrow().clone() {
                    Some(d) => d,
                    None => return,
                };
                // Read via buffer to bypass libadwaita::prelude trait ambiguity
                let raw = dir_entry.buffer().text().to_string();
                let dir_text = raw.trim().to_string();
                let dir_text = if dir_text.starts_with("~/") {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
                    format!("{}/{}", home, &dir_text[2..])
                } else if dir_text == "~" {
                    std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
                } else {
                    dir_text
                };
                if dir_text.is_empty() { return; }
                let dir = std::path::PathBuf::from(&dir_text);
                if !dir.exists() { return; }
                let skip_vfs = vfs_check.is_active();

                let cancelled = Arc::new(AtomicBool::new(false));
                *cancellation.borrow_mut() = Some(cancelled.clone());

                progress_box.set_visible(true);
                progress_bar.set_fraction(0.0);
                start_btn_c.set_sensitive(false);
                cancel_btn_c.set_sensitive(true);
                export_row.set_visible(false);

                let (tx, rx) = mpsc::channel::<VerifyMessage>();
                let tx2 = tx.clone();

                std::thread::spawn(move || {
                    let result = verify_files(
                        &db, &dir, None, skip_vfs, vec![], cancelled,
                        move |p| { let _ = tx2.send(VerifyMessage::Progress(p)); },
                    );
                    let _ = tx.send(VerifyMessage::Done(result));
                });

                let progress_bar   = progress_bar.clone();
                let progress_label = progress_label.clone();
                let start_btn_c    = start_btn_c.clone();
                let cancel_btn_c   = cancel_btn_c.clone();
                let export_row     = export_row.clone();
                let diff_viewer    = diff_viewer.clone();
                let last_result    = last_result.clone();

                glib::timeout_add_local(
                    std::time::Duration::from_millis(50),
                    move || {
                        loop {
                            match rx.try_recv() {
                                Ok(VerifyMessage::Progress(p)) => {
                                    progress_bar.pulse();
                                    progress_label.set_text(&format!(
                                        "Checked: {} files  |  Current: {}",
                                        p.files_checked, p.current_file));
                                }
                                Ok(VerifyMessage::Done(result)) => {
                                    progress_bar.set_fraction(1.0);
                                    progress_label.set_text(&format!(
                                        "✅ Done — {} unchanged  ⚠️ {} modified  \
                                         ❌ {} missing  🆕 {} new",
                                        result.stats.unchanged, result.stats.modified,
                                        result.stats.missing,   result.stats.new_files));
                                    start_btn_c.set_sensitive(true);
                                    cancel_btn_c.set_sensitive(false);
                                    export_row.set_visible(true);
                                    diff_viewer.populate(&result);
                                    *last_result.borrow_mut() = Some(result);
                                    return glib::ControlFlow::Break;
                                }
                                Err(mpsc::TryRecvError::Empty)        => break,
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

        // Cancel
        {
            let cancellation = cancellation.clone();
            cancel_btn.connect_clicked(move |btn| {
                if let Some(c) = cancellation.borrow().as_ref() {
                    c.store(true, Ordering::Relaxed);
                }
                btn.set_sensitive(false);
            });
        }

        // Export buttons
        {
            let r = last_result.clone();
            export_txt_btn.connect_clicked(move |btn| { save_result_dialog(btn, &r, "txt"); });
        }
        {
            let r = last_result.clone();
            export_json_btn.connect_clicked(move |btn| { save_result_dialog(btn, &r, "json"); });
        }
        {
            let r = last_result.clone();
            export_csv_btn.connect_clicked(move |btn| { save_result_dialog(btn, &r, "csv"); });
        }

        // Return the ScrolledWindow as the tab widget
        Self { root: scroll }
    }

    pub fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.root
    }
}

enum VerifyMessage {
    Progress(VerifyProgress),
    Done(VerifyResult),
}

fn save_result_dialog(
    btn: &Button,
    result_cell: &Rc<RefCell<Option<VerifyResult>>>,
    ext: &'static str,
) {
    let result   = match result_cell.borrow().clone() { Some(r) => r, None => return };
    let now      = chrono::Local::now();
    let filename = format!("verify_results_{}.{}", now.format("%Y%m%d_%H%M%S"), ext);
    let dialog   = FileDialog::builder().title("Export Results").initial_name(&filename).build();
    let window   = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
    dialog.save(window.as_ref(), gtk4::gio::Cancellable::NONE, move |res| {
        if let Ok(file) = res {
            if let Some(path) = file.path() {
                let outcome = match ext {
                    "json" => result.export_json(&path),
                    "csv"  => result.export_csv(&path),
                    _      => result.export_txt(&path),
                };
                if let Err(e) = outcome { eprintln!("Export error: {}", e); }
            }
        }
    });
}
