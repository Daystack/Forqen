//! The reflog browser — "undo anything".
//!
//! git already records every position HEAD has held. What is missing in every
//! client is a way to *read* it without knowing it exists, so a bad reset or a
//! rebase that ate a commit becomes a search-engine problem instead of a
//! button. This is that button.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use git::reflog::{self, Entry};

use crate::state::AppState;

pub struct ReflogDialog {
    window: adw::Window,
    state: AppState,
    list: gtk::ListBox,
    status: gtk::Label,
    restore_btn: gtk::Button,
    only_recovery: gtk::CheckButton,
    entries: Rc<RefCell<Vec<Entry>>>,
    selected: Cell<Option<usize>>,
    on_change: Rc<dyn Fn()>,
}

impl ReflogDialog {
    pub fn present(parent: &impl IsA<gtk::Window>, state: AppState, on_change: Rc<dyn Fn()>) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("History of HEAD")
            .default_width(760)
            .default_height(520)
            .build();

        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let restore_btn = gtk::Button::with_label("Restore this state");
        restore_btn.add_css_class("destructive-action");
        restore_btn.set_sensitive(false);

        // A reflog is mostly noise — every checkout and commit appears. The
        // filter defaults on, because someone opening this is looking for the
        // moment before something went wrong, not a complete log.
        let only_recovery = gtk::CheckButton::with_label("Only show resets, rebases and merges");
        only_recovery.set_active(true);

        let dialog = Rc::new(Self {
            window: window.clone(),
            state,
            list: list.clone(),
            status: status.clone(),
            restore_btn: restore_btn.clone(),
            only_recovery: only_recovery.clone(),
            entries: Rc::new(RefCell::new(Vec::new())),
            selected: Cell::new(None),
            on_change,
        });

        {
            let this = dialog.clone();
            list.connect_row_selected(move |_, row| {
                this.selected.set(row.map(|r| r.index() as usize));
                this.restore_btn.set_sensitive(row.is_some());
            });
        }
        {
            let this = dialog.clone();
            only_recovery.connect_toggled(move |_| this.refresh());
        }
        {
            let this = dialog.clone();
            restore_btn.connect_clicked(move |_| this.confirm_restore());
        }

        window.set_content(Some(&build_layout(
            &list,
            &status,
            &restore_btn,
            &only_recovery,
        )));

        dialog.refresh();
        window.present();
    }

    fn refresh(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let all = self
            .state
            .with(|s| reflog::read(&s.repo, "HEAD", 200))
            .and_then(Result::ok)
            .unwrap_or_default();

        let filtered: Vec<Entry> = if self.only_recovery.is_active() {
            all.iter()
                .filter(|e| e.is_recovery_point())
                .cloned()
                .collect()
        } else {
            all.clone()
        };

        self.status.set_text(&match (all.len(), filtered.len()) {
            (0, _) => "Nothing recorded yet".to_string(),
            (total, shown) if shown < total => {
                format!("{shown} of {total} entries")
            }
            (total, _) => format!("{total} entries"),
        });

        for e in &filtered {
            self.list.append(&row(e));
        }
        *self.entries.borrow_mut() = filtered;
        self.selected.set(None);
        self.restore_btn.set_sensitive(false);
    }

    fn confirm_restore(self: &Rc<Self>) {
        let Some(entry) = self
            .selected
            .get()
            .and_then(|i| self.entries.borrow().get(i).cloned())
        else {
            return;
        };

        let dialog = adw::AlertDialog::new(
            Some("Restore this state?"),
            Some(&format!(
                "HEAD moves to {} and the working tree is reset to match it. \
                 Uncommitted changes are lost.\n\n\
                 This is itself recorded, so it can be undone from here too.",
                entry.id.short()
            )),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("restore", "Restore");
        dialog.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "restore" {
                return;
            }
            match this.state.with(|s| reflog::restore(&s.repo, entry.id)) {
                Some(Err(e)) => {
                    let err =
                        adw::AlertDialog::new(Some("Could not restore"), Some(&e.to_string()));
                    err.add_response("ok", "OK");
                    err.present(Some(&this.window));
                }
                None => {}
                Some(Ok(())) => {
                    this.refresh();
                    (this.on_change)();
                }
            }
        });

        dialog.present(Some(&self.window));
    }
}

fn row(e: &Entry) -> gtk::ListBoxRow {
    let headline = gtk::Label::new(Some(&if e.detail.is_empty() {
        e.action.clone()
    } else {
        format!("{}: {}", e.action, e.detail)
    }));
    headline.set_xalign(0.0);
    headline.set_hexpand(true);
    headline.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let when = gtk::Label::new(Some(&e.when));
    when.add_css_class("dim-label");
    when.add_css_class("caption");

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.append(&headline);
    top.append(&when);

    let meta = gtk::Label::new(Some(&format!("{} · {}", e.selector, e.id.short())));
    meta.set_xalign(0.0);
    meta.add_css_class("dim-label");
    meta.add_css_class("caption");
    meta.add_css_class("monospace");

    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 2);
    boxed.set_margin_start(10);
    boxed.set_margin_end(10);
    boxed.set_margin_top(8);
    boxed.set_margin_bottom(8);
    boxed.append(&top);
    boxed.append(&meta);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&boxed));
    row
}

fn build_layout(
    list: &gtk::ListBox,
    status: &gtk::Label,
    restore_btn: &gtk::Button,
    only_recovery: &gtk::CheckButton,
) -> gtk::Widget {
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(
        "History of HEAD",
        "Every position this branch has held",
    )));
    header.pack_end(restore_btn);

    let scroll = gtk::ScrolledWindow::builder()
        .child(list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.set_margin_top(6);
    footer.set_margin_bottom(6);
    status.set_hexpand(true);
    footer.append(status);
    footer.append(only_recovery);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&scroll);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&footer);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    view.upcast()
}
