//! The worktree manager.
//!
//! A dialog, like stashes: worktrees are something you set up and then leave,
//! not a place you work — the work happens in the checkout, in an editor.
//!
//! The reason it exists is pull request review. Without a worktree, looking at
//! someone else's branch means stashing, switching, unstashing and hoping;
//! with one it is a second directory that leaves the current work untouched.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;

use git::worktree::{self, Worktree};

use crate::state::AppState;

pub struct WorktreeDialog {
    window: adw::Window,
    state: AppState,
    list: gtk::ListBox,
    status: gtk::Label,
    remove_btn: gtk::Button,
    open_btn: gtk::Button,
    items: Rc<RefCell<Vec<Worktree>>>,
    selected: std::cell::Cell<Option<usize>>,
    on_change: Rc<dyn Fn()>,
}

impl WorktreeDialog {
    pub fn present(parent: &impl IsA<gtk::Window>, state: AppState, on_change: Rc<dyn Fn()>) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("Worktrees")
            .default_width(720)
            .default_height(460)
            .build();

        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let add_btn = gtk::Button::with_label("New worktree…");
        add_btn.add_css_class("suggested-action");
        let remove_btn = gtk::Button::with_label("Remove");
        remove_btn.add_css_class("destructive-action");
        remove_btn.set_sensitive(false);
        let open_btn = gtk::Button::with_label("Open folder");
        open_btn.set_sensitive(false);
        let prune_btn = gtk::Button::with_label("Prune");
        prune_btn.set_tooltip_text(Some("Forget entries whose folders are gone"));

        let dialog = Rc::new(Self {
            window: window.clone(),
            state,
            list: list.clone(),
            status: status.clone(),
            remove_btn: remove_btn.clone(),
            open_btn: open_btn.clone(),
            items: Rc::new(RefCell::new(Vec::new())),
            selected: std::cell::Cell::new(None),
            on_change,
        });

        {
            let this = dialog.clone();
            list.connect_row_selected(move |_, row| {
                let index = row.map(|r| r.index() as usize);
                this.selected.set(index);
                let removable = index
                    .and_then(|i| this.items.borrow().get(i).map(|w| w.removable()))
                    .unwrap_or(false);
                this.remove_btn.set_sensitive(removable);
                this.open_btn.set_sensitive(index.is_some());
            });
        }
        {
            let this = dialog.clone();
            add_btn.connect_clicked(move |_| this.prompt_new());
        }
        {
            let this = dialog.clone();
            remove_btn.connect_clicked(move |_| this.confirm_remove());
        }
        {
            let this = dialog.clone();
            open_btn.connect_clicked(move |_| this.open_folder());
        }
        {
            let this = dialog.clone();
            prune_btn.connect_clicked(move |_| {
                match this.state.with(|s| worktree::prune(&s.repo)) {
                    Some(Err(e)) => this.status.set_text(&e.to_string()),
                    _ => this.refresh(),
                }
            });
        }

        window.set_content(Some(&build_layout(
            &list,
            &status,
            &add_btn,
            &remove_btn,
            &open_btn,
            &prune_btn,
        )));

        dialog.refresh();
        window.present();
    }

    fn refresh(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let items = self
            .state
            .with(|s| worktree::list(&s.repo))
            .and_then(Result::ok)
            .unwrap_or_default();

        let stale = items.iter().filter(|w| w.is_prunable).count();
        self.status.set_text(&match (items.len(), stale) {
            (0, _) => "No worktrees".to_string(),
            (1, _) => "1 worktree".to_string(),
            (n, 0) => format!("{n} worktrees"),
            (n, s) => format!("{n} worktrees · {s} with a missing folder"),
        });

        for w in &items {
            self.list.append(&row(w));
        }
        *self.items.borrow_mut() = items;
        self.selected.set(None);
        self.remove_btn.set_sensitive(false);
        self.open_btn.set_sensitive(false);
    }

    /// Ask for a branch and a folder, then create.
    fn prompt_new(self: &Rc<Self>) {
        let dialog = adw::AlertDialog::new(
            Some("New worktree"),
            Some(
                "A second checkout sharing this repository's history. \
                 The current one is left untouched.",
            ),
        );

        let branch = gtk::Entry::new();
        branch.set_placeholder_text(Some("Branch name"));

        // Default alongside the repository rather than inside it: a worktree
        // nested in its own parent shows up as untracked files in the parent,
        // which is confusing and easy to commit by accident.
        let suggested = self
            .state
            .with(|s| {
                s.repo.workdir().map(|w| {
                    let name = w.file_name().map(|n| n.to_string_lossy().into_owned());
                    let parent = w.parent().map(PathBuf::from).unwrap_or_default();
                    (parent, name.unwrap_or_else(|| "repo".into()))
                })
            })
            .flatten();

        let folder = gtk::Entry::new();
        folder.set_placeholder_text(Some("Folder for the new checkout"));
        if let Some((parent, name)) = &suggested {
            folder.set_text(&parent.join(format!("{name}-worktree")).to_string_lossy());
        }

        let create = gtk::CheckButton::with_label("Create the branch if it does not exist");
        create.set_active(true);

        let boxed = gtk::Box::new(gtk::Orientation::Vertical, 8);
        boxed.append(&branch);
        boxed.append(&folder);
        boxed.append(&create);
        dialog.set_extra_child(Some(&boxed));

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("create", "Create");
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "create" {
                return;
            }
            let branch_name = branch.text().to_string();
            let path = PathBuf::from(folder.text().to_string());
            if branch_name.trim().is_empty() || path.as_os_str().is_empty() {
                this.status
                    .set_text("A branch and a folder are both needed");
                return;
            }

            match this
                .state
                .with(|s| worktree::add(&s.repo, &path, branch_name.trim(), create.is_active()))
            {
                // git's own message names the worktree already holding a
                // branch, which is more useful than anything invented here.
                Some(Err(e)) => this.report("Could not create the worktree", &e.to_string()),
                None => this.report("Could not create the worktree", "No repository open"),
                Some(Ok(())) => {
                    this.refresh();
                    (this.on_change)();
                }
            }
        });

        dialog.present(Some(&self.window));
    }

    fn confirm_remove(self: &Rc<Self>) {
        let Some(w) = self
            .selected
            .get()
            .and_then(|i| self.items.borrow().get(i).cloned())
        else {
            return;
        };

        let dialog = adw::AlertDialog::new(
            Some(&format!("Remove {}?", w.name())),
            Some(&format!(
                "The folder {} is deleted. The branch itself is kept.",
                w.path.display()
            )),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("remove", "Remove");
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "remove" {
                return;
            }
            // Never forced. git refuses a dirty worktree, and that refusal is
            // the feature: the point of a worktree is that work lives in it.
            match this
                .state
                .with(|s| worktree::remove(&s.repo, &w.path, false))
            {
                Some(Err(e)) => this.report(
                    "Could not remove the worktree",
                    &format!(
                        "{e}\n\nIf this worktree has uncommitted changes, commit or \
                         discard them there first."
                    ),
                ),
                None => this.report("Could not remove the worktree", "No repository open"),
                Some(Ok(())) => {
                    this.refresh();
                    (this.on_change)();
                }
            }
        });

        dialog.present(Some(&self.window));
    }

    fn open_folder(&self) {
        let Some(w) = self
            .selected
            .get()
            .and_then(|i| self.items.borrow().get(i).cloned())
        else {
            return;
        };
        let launcher = gtk::FileLauncher::new(Some(&gtk::gio::File::for_path(&w.path)));
        launcher.launch(Some(&self.window), None::<&gtk::gio::Cancellable>, |_| {});
    }

    fn report(&self, title: &str, message: &str) {
        let dialog = adw::AlertDialog::new(Some(title), Some(message));
        dialog.add_response("ok", "OK");
        dialog.present(Some(&self.window));
    }
}

fn row(w: &Worktree) -> gtk::ListBoxRow {
    let name = gtk::Label::new(Some(&w.name()));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

    let mut meta = match &w.branch {
        Some(b) => b.clone(),
        None => "detached HEAD".to_string(),
    };
    if w.is_main {
        meta.push_str(" · main checkout");
    }
    if w.is_locked {
        meta.push_str(" · locked");
    }
    if w.is_prunable {
        meta.push_str(" · folder missing");
    }

    let subtitle = gtk::Label::new(Some(&meta));
    subtitle.set_xalign(0.0);
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("caption");
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let path = gtk::Label::new(Some(&w.path.display().to_string()));
    path.set_xalign(0.0);
    path.add_css_class("dim-label");
    path.add_css_class("caption");
    path.add_css_class("monospace");
    path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 2);
    boxed.set_margin_start(10);
    boxed.set_margin_end(10);
    boxed.set_margin_top(8);
    boxed.set_margin_bottom(8);
    boxed.append(&name);
    boxed.append(&subtitle);
    boxed.append(&path);

    if w.is_prunable {
        boxed.set_opacity(0.55);
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&boxed));
    row
}

fn build_layout(
    list: &gtk::ListBox,
    status: &gtk::Label,
    add_btn: &gtk::Button,
    remove_btn: &gtk::Button,
    open_btn: &gtk::Button,
    prune_btn: &gtk::Button,
) -> gtk::Widget {
    let header = adw::HeaderBar::new();
    header.pack_start(add_btn);
    header.pack_end(remove_btn);
    header.pack_end(open_btn);
    header.pack_end(prune_btn);

    let scroll = gtk::ScrolledWindow::builder()
        .child(list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.set_margin_top(6);
    footer.set_margin_bottom(6);
    footer.append(status);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&scroll);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&footer);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    view.upcast()
}
