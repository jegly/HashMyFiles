use crate::verifier::{DiffEntry, FileStatus, VerifyResult};
use gtk4::prelude::*;
use gtk4::{
    Align, Button, Entry, Label, ListBox, ListBoxRow, Orientation,
    PolicyType, ScrolledWindow, SelectionMode, Separator,
};
use std::cell::RefCell;
use std::rc::Rc;

pub struct DiffViewer {
    root:        gtk4::Box,
    list_box:    ListBox,
    stats_label: Label,
    search_entry: Entry,
    all_entries: Rc<RefCell<Vec<DiffEntry>>>,
    status_filter: Rc<RefCell<Option<FileStatus>>>,
}

impl DiffViewer {
    pub fn new() -> Self {
        let root = gtk4::Box::new(Orientation::Vertical, 8);

        let stats_label = Label::new(Some(""));
        stats_label.add_css_class("monospace");
        stats_label.add_css_class("stats-label");
        stats_label.set_halign(Align::Start);

        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("🔍  Filter by path…"));
        search_entry.add_css_class("monospace");

        let filter_row = gtk4::Box::new(Orientation::Horizontal, 6);
        filter_row.set_halign(Align::Start);

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(PolicyType::Automatic);

        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::Single);
        list_box.add_css_class("diff-list");
        scroll.set_child(Some(&list_box));

        root.append(&stats_label);
        root.append(&search_entry);
        root.append(&filter_row);
        root.append(&scroll);

        let all_entries: Rc<RefCell<Vec<DiffEntry>>> = Rc::new(RefCell::new(Vec::new()));
        let status_filter: Rc<RefCell<Option<FileStatus>>> = Rc::new(RefCell::new(None));

        // Filter buttons
        let filters: &[(&str, Option<FileStatus>)] = &[
            ("All",           None),
            ("⚠️ Modified",  Some(FileStatus::Modified)),
            ("❌ Missing",   Some(FileStatus::Missing)),
            ("🆕 New",       Some(FileStatus::New)),
            ("✅ Unchanged", Some(FileStatus::Unchanged)),
        ];
        for (label, status) in filters {
            let btn = Button::with_label(label);
            btn.add_css_class("filter-btn");
            filter_row.append(&btn);

            let all_entries = all_entries.clone();
            let status_filter = status_filter.clone();
            let list_box = list_box.clone();
            let search_entry = search_entry.clone();
            let chosen = status.clone();
            btn.connect_clicked(move |_| {
                *status_filter.borrow_mut() = chosen.clone();
                let text = search_entry.text().to_string();
                render_filtered(&list_box, &all_entries.borrow(), &text,
                                status_filter.borrow().as_ref());
            });
        }

        // Wire search entry
        {
            let all_entries = all_entries.clone();
            let status_filter = status_filter.clone();
            let list_box = list_box.clone();
            search_entry.connect_changed(move |entry| {
                let text = entry.text().to_string();
                render_filtered(&list_box, &all_entries.borrow(), &text,
                                status_filter.borrow().as_ref());
            });
        }

        Self { root, list_box, stats_label, search_entry, all_entries, status_filter }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.root
    }

    pub fn populate(&self, result: &VerifyResult) {
        let s = &result.stats;
        self.stats_label.set_text(&format!(
            "Total: {}   ✅ {unchanged}   ⚠️ {modified}   ❌ {missing}   🆕 {new}   Errors: {errors}",
            s.total,
            unchanged = s.unchanged,
            modified  = s.modified,
            missing   = s.missing,
            new       = s.new_files,
            errors    = s.errors,
        ));

        // Reset filter state on new results
        *self.status_filter.borrow_mut() = None;
        *self.all_entries.borrow_mut() = result.entries.clone();
        self.search_entry.set_text("");
        render_filtered(&self.list_box, &result.entries, "", None);
    }
}

fn render_filtered(
    list_box: &ListBox,
    entries:  &[DiffEntry],
    search:   &str,
    status_filter: Option<&FileStatus>,
) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
    let search_lower = search.to_lowercase();
    for entry in entries {
        let path_str = entry.path.to_string_lossy().to_lowercase();
        let matches_search = search_lower.is_empty() || path_str.contains(&search_lower);
        let matches_status = status_filter.map(|f| &entry.status == f).unwrap_or(true);
        if matches_search && matches_status {
            list_box.append(&build_diff_row(entry));
        }
    }
}

fn build_diff_row(entry: &DiffEntry) -> ListBoxRow {
    let row = ListBoxRow::new();
    let row_box = gtk4::Box::new(Orientation::Vertical, 4);
    row_box.set_margin_top(8);
    row_box.set_margin_bottom(8);
    row_box.set_margin_start(12);
    row_box.set_margin_end(12);

    // Top line: icon  path  badge
    let top = gtk4::Box::new(Orientation::Horizontal, 8);
    let icon_label = Label::new(Some(entry.status.icon()));

    let path_label = Label::new(Some(&entry.path.to_string_lossy()));
    path_label.add_css_class("monospace");
    path_label.add_css_class(entry.status.css_class());
    path_label.set_halign(Align::Start);
    path_label.set_ellipsize(gtk4::pango::EllipsizeMode::Start);
    path_label.set_hexpand(true);

    let badge = Label::new(Some(entry.status.label()));
    badge.add_css_class("status-badge");
    badge.add_css_class(entry.status.css_class());

    top.append(&icon_label);
    top.append(&path_label);
    top.append(&badge);
    row_box.append(&top);

    // Hash detail lines
    match entry.status {
        FileStatus::Modified => {
            let hashes = gtk4::Box::new(Orientation::Horizontal, 16);
            hashes.set_margin_start(24);
            if let Some(ref old) = entry.old_hash {
                hashes.append(&hash_box("Old Hash", old, "diff-missing"));
            }
            hashes.append(&Separator::new(Orientation::Vertical));
            if let Some(ref new) = entry.new_hash {
                hashes.append(&hash_box("New Hash", new, "diff-new"));
            }
            row_box.append(&hashes);
        }
        FileStatus::Missing => {
            if let Some(ref old) = entry.old_hash {
                let lbl = inline_hash_label(&format!("Last known: {}", trunc(old)));
                row_box.append(&lbl);
            }
        }
        FileStatus::New => {
            if let Some(ref new) = entry.new_hash {
                let lbl = inline_hash_label(&format!("Hash: {}", trunc(new)));
                row_box.append(&lbl);
            }
        }
        FileStatus::Unchanged => {}
    }

    row.set_child(Some(&row_box));
    row
}

fn hash_box(title: &str, hash: &str, css: &str) -> gtk4::Box {
    let b = gtk4::Box::new(Orientation::Vertical, 2);
    let t = Label::new(Some(title));
    t.add_css_class("hash-subtitle");
    let v = Label::new(Some(&trunc(hash)));
    v.add_css_class("monospace");
    v.add_css_class(css);
    b.append(&t);
    b.append(&v);
    b
}

fn inline_hash_label(text: &str) -> Label {
    let lbl = Label::new(Some(text));
    lbl.add_css_class("monospace");
    lbl.add_css_class("hash-subtitle");
    lbl.set_halign(Align::Start);
    lbl.set_margin_start(24);
    lbl
}

fn trunc(hash: &str) -> String {
    if hash.len() > 16 {
        format!("{}…{}", &hash[..8], &hash[hash.len() - 8..])
    } else {
        hash.to_string()
    }
}
