//! Repository search.
//!
//! Three groups rather than one ranked list — file contents, file names,
//! commit messages. They answer different questions ("where is this string",
//! "where is this file", "when did this change"), and blending them into one
//! relevance order buries whichever the user actually meant.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use git::search::{Hit, Kind};

use crate::state::AppState;

pub struct SearchDialog {
    list: gtk::ListBox,
    status: gtk::Label,
    state: AppState,
    hits: Rc<RefCell<Vec<Hit>>>,
    /// Called with a file path when a result is chosen.
    on_open: Rc<dyn Fn(&str)>,
    window: adw::Window,
}

impl SearchDialog {
    pub fn present(parent: &impl IsA<gtk::Window>, state: AppState, on_open: Rc<dyn Fn(&str)>) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("Search")
            .default_width(760)
            .default_height(520)
            .build();

        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Search files, contents and commit messages…"));

        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let status = gtk::Label::new(Some("Type to search"));
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let dialog = Rc::new(Self {
            list: list.clone(),
            status: status.clone(),
            state,
            hits: Rc::new(RefCell::new(Vec::new())),
            on_open,
            window: window.clone(),
        });

        {
            // Debounced: `git grep` over a large repository on every keystroke
            // is a process spawn per character, and the results for a
            // half-typed word are never what was wanted anyway.
            let this = dialog.clone();
            let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
            entry.connect_search_changed(move |e| {
                if let Some(id) = pending.borrow_mut().take() {
                    id.remove();
                }
                let query = e.text().to_string();
                let this = this.clone();
                let pending_ = pending.clone();
                let id = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(180),
                    move || {
                        pending_.borrow_mut().take();
                        this.run(&query);
                    },
                );
                *pending.borrow_mut() = Some(id);
            });
        }
        {
            let this = dialog.clone();
            list.connect_row_activated(move |_, row| {
                let Some(hit) = this.hits.borrow().get(row.index() as usize).cloned() else {
                    return;
                };
                // A commit hit has no path to open; the others do.
                if hit.path.is_empty() {
                    return;
                }
                this.window.close();
                (this.on_open)(&hit.path);
            });
        }

        window.set_content(Some(&build_layout(&entry, &list, &status)));
        window.present();
        entry.grab_focus();
    }

    fn run(self: &Rc<Self>, query: &str) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        if query.trim().is_empty() {
            self.status.set_text("Type to search");
            self.hits.borrow_mut().clear();
            return;
        }

        let result = self
            .state
            .with(|s| git::search::search(&s.repo, query, 100));
        let hits = match result {
            Some(Ok(h)) => h,
            Some(Err(e)) => {
                self.status.set_text(&e.to_string());
                return;
            }
            None => {
                self.status.set_text("No repository open");
                return;
            }
        };

        let counts = |k: Kind| hits.iter().filter(|h| h.kind == k).count();
        self.status.set_text(&if hits.is_empty() {
            "No matches".to_string()
        } else {
            format!(
                "{} in files · {} file names · {} commits",
                counts(Kind::Content),
                counts(Kind::Path),
                counts(Kind::Message)
            )
        });

        let mut last_kind: Option<Kind> = None;
        for h in &hits {
            if last_kind != Some(h.kind) {
                self.list.append(&heading(h.kind));
                last_kind = Some(h.kind);
            }
            self.list.append(&row(h));
        }

        // Headings occupy rows too, so the index the activation handler sees
        // would not line up with `hits`. Storing a parallel vector with a
        // placeholder per heading keeps them in step.
        let mut aligned = Vec::new();
        let mut last_kind: Option<Kind> = None;
        for h in &hits {
            if last_kind != Some(h.kind) {
                aligned.push(Hit {
                    kind: h.kind,
                    path: String::new(),
                    line: None,
                    text: String::new(),
                    commit: None,
                });
                last_kind = Some(h.kind);
            }
            aligned.push(h.clone());
        }
        *self.hits.borrow_mut() = aligned;
    }
}

fn heading(kind: Kind) -> gtk::ListBoxRow {
    let label = gtk::Label::new(Some(match kind {
        Kind::Content => "In files",
        Kind::Path => "File names",
        Kind::Message => "Commit messages",
    }));
    label.set_xalign(0.0);
    label.add_css_class("heading");
    label.add_css_class("dim-label");
    label.set_margin_start(10);
    label.set_margin_top(8);
    label.set_margin_bottom(4);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));
    row.set_selectable(false);
    row.set_activatable(false);
    row
}

fn row(h: &Hit) -> gtk::ListBoxRow {
    let primary = gtk::Label::new(Some(&h.text));
    primary.set_xalign(0.0);
    primary.set_hexpand(true);
    primary.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if h.kind == Kind::Content {
        primary.add_css_class("monospace");
    }

    let secondary_text = match h.kind {
        Kind::Content => match h.line {
            Some(n) => format!("{}:{n}", h.path),
            None => h.path.clone(),
        },
        Kind::Path => String::new(),
        Kind::Message => h.commit.map(|c| c.short()).unwrap_or_default(),
    };

    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 2);
    boxed.set_margin_start(10);
    boxed.set_margin_end(10);
    boxed.set_margin_top(6);
    boxed.set_margin_bottom(6);
    boxed.append(&primary);

    if !secondary_text.is_empty() {
        let secondary = gtk::Label::new(Some(&secondary_text));
        secondary.set_xalign(0.0);
        secondary.add_css_class("dim-label");
        secondary.add_css_class("caption");
        secondary.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        boxed.append(&secondary);
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&boxed));
    row
}

fn build_layout(entry: &gtk::SearchEntry, list: &gtk::ListBox, status: &gtk::Label) -> gtk::Widget {
    entry.set_margin_start(12);
    entry.set_margin_end(12);
    entry.set_margin_top(12);
    entry.set_margin_bottom(6);

    let scroll = gtk::ScrolledWindow::builder()
        .child(list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    scroll.set_margin_start(12);
    scroll.set_margin_end(12);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.set_margin_top(6);
    footer.set_margin_bottom(8);
    footer.append(status);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(entry);
    content.append(&scroll);
    content.append(&footer);
    content.upcast()
}
