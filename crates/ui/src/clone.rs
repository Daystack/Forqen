//! The Clone dialog: pick one of your GitHub repositories, or paste a URL.
//!
//! Two tabs, matching the shape GitHub Desktop made the expected one: browse
//! what the signed-in account already has write access to, or paste a URL for
//! anything else — a colleague's fork, a non-GitHub host, a repository owned
//! by someone else entirely.
//!
//! The actual transfer is `sync::clone`, which already carries the
//! threading, progress and credential handling `fetch`/`pull`/`push` use.
//! This module is the picker in front of it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use github::models::Repository;

/// Present the clone dialog.
///
/// `on_cloned` runs once a clone finishes successfully, with the local path —
/// the caller loads it exactly as it would any repository opened by hand.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    window: &adw::ApplicationWindow,
    rt: tokio::runtime::Handle,
    on_cloned: Rc<dyn Fn(&Path)>,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Clone a Repository");
    dialog.set_content_width(560);
    dialog.set_content_height(520);

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let view_stack = adw::ViewStack::new();

    let mine_page = build_mine_tab(window, rt, &dialog, on_cloned.clone());
    view_stack.add_titled_with_icon(
        &mine_page,
        Some("mine"),
        "Your Repositories",
        "folder-remote-symbolic",
    );

    let url_page = build_url_tab(window, &dialog, on_cloned);
    view_stack.add_titled_with_icon(&url_page, Some("url"), "URL", "web-browser-symbolic");

    let switcher = adw::ViewSwitcher::builder()
        .stack(&view_stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    header.set_title_widget(Some(&switcher));

    toolbar.set_content(Some(&view_stack));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

/// Where a clone lands by default: the repository name under the user's home
/// directory. Editable — this is a starting point, not a decision made for
/// the user.
fn default_dest(name: &str) -> PathBuf {
    glib::home_dir().join(name)
}

/// A "Local path" row shared by both tabs: an entry pre-filled with a default,
/// and a "Choose…" button that lets the user pick the *parent* directory —
/// the leaf directory is always the repository's own name, git creates it,
/// and a destination that already exists and is non-empty is refused by git
/// itself with a message this dialog surfaces rather than duplicates.
fn path_row(
    window: &adw::ApplicationWindow,
    name_source: Rc<RefCell<String>>,
) -> (gtk::Box, gtk::Entry) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let entry = gtk::Entry::new();
    entry.set_hexpand(true);
    entry.set_text(&default_dest(&name_source.borrow()).to_string_lossy());

    let choose = gtk::Button::with_label("Choose…");
    {
        let entry = entry.clone();
        let window = window.clone();
        choose.connect_clicked(move |_| {
            let name_source = name_source.clone();
            let entry = entry.clone();
            let dialog = gtk::FileDialog::builder()
                .title("Clone into")
                .modal(true)
                .build();
            dialog.select_folder(
                Some(&window),
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    let Ok(folder) = result else { return };
                    let Some(parent) = folder.path() else { return };
                    entry.set_text(&parent.join(&*name_source.borrow()).to_string_lossy());
                },
            );
        });
    }

    row.append(&entry);
    row.append(&choose);
    (row, entry)
}

/// Report a clone failure the way `sync::clone` does not need to — this
/// dialog stays open on failure so the user can fix the path or URL and try
/// again, unlike a transfer against a repository that is already loaded.
fn report_error(window: &adw::ApplicationWindow, message: &str) {
    let alert = adw::AlertDialog::new(Some("Clone failed"), Some(message));
    alert.add_response("ok", "OK");
    alert.present(Some(window));
}

// ── "Your repositories" ──────────────────────────────────────────────────

enum MineMsg {
    List(Result<Vec<Repository>, String>),
}

fn build_mine_tab(
    window: &adw::ApplicationWindow,
    rt: tokio::runtime::Handle,
    dialog: &adw::Dialog,
    on_cloned: Rc<dyn Fn(&Path)>,
) -> gtk::Widget {
    let Some(client) = crate::github_client() else {
        let status = adw::StatusPage::new();
        status.set_title("Not Signed In");
        status.set_description(Some(
            "Sign in to GitHub from the account button in the toolbar, then \
             reopen this dialog to browse your repositories.",
        ));
        status.set_icon_name(Some("dialog-password-symbolic"));
        return status.upcast();
    };

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Filter your repositories…"));
    root.append(&search);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .build();
    scroll.add_css_class("card");
    root.append(&scroll);

    let status = gtk::Label::new(Some("Loading your repositories…"));
    status.add_css_class("dim-label");
    status.set_xalign(0.0);
    root.append(&status);

    let selected_name = Rc::new(RefCell::new(String::new()));
    let (path_row_box, path_entry) = path_row(window, selected_name.clone());
    root.append(&path_row_box);

    let clone_btn = gtk::Button::with_label("Clone");
    clone_btn.add_css_class("suggested-action");
    clone_btn.set_halign(gtk::Align::End);
    clone_btn.set_sensitive(false);
    root.append(&clone_btn);

    let repos: Rc<RefCell<Vec<Repository>>> = Rc::new(RefCell::new(Vec::new()));

    // `repos` is the full fetched list; `visible` is whatever the search box
    // currently shows, in the same order as the list's rows. A row's index
    // only means something against `visible` — once the search box has
    // filtered anything out, that index no longer lines up with `repos`.
    let visible: Rc<RefCell<Vec<Repository>>> = Rc::new(RefCell::new(Vec::new()));

    {
        let visible = visible.clone();
        let clone_btn = clone_btn.clone();
        let selected_name = selected_name.clone();
        let path_entry = path_entry.clone();
        list.connect_row_selected(move |_, row| {
            let repo = row
                .filter(|r| r.index() >= 0)
                .and_then(|r| visible.borrow().get(r.index() as usize).cloned());
            clone_btn.set_sensitive(repo.is_some());
            if let Some(repo) = repo {
                *selected_name.borrow_mut() = repo.name.clone();
                path_entry.set_text(&default_dest(&repo.name).to_string_lossy());
            }
        });
    }

    {
        let repos = repos.clone();
        let visible = visible.clone();
        let list = list.clone();
        search.connect_search_changed(move |s| {
            let query = s.text().to_lowercase();
            *visible.borrow_mut() = populate_repo_list(&list, &repos.borrow(), &query);
        });
    }

    {
        let window = window.clone();
        let dialog = dialog.clone();
        let visible = visible.clone();
        let list = list.clone();
        clone_btn.connect_clicked(move |_| {
            let index = list.selected_row().map(|r| r.index()).filter(|i| *i >= 0);
            let Some(repo) = index.and_then(|i| visible.borrow().get(i as usize).cloned()) else {
                return;
            };
            let url = preferred_clone_url(&repo);
            let dest = PathBuf::from(path_entry.text().as_str());
            start_clone(&window, &dialog, url, dest, on_cloned.clone());
        });
    }

    let (tx, rx) = async_channel::bounded::<MineMsg>(4);
    rt.spawn(async move {
        // 100 per page is GitHub's maximum; a second page is a rare account,
        // and paging further can follow once someone actually hits it.
        let result = client
            .my_repos(1)
            .await
            .map(|r| r.data)
            .map_err(|e| e.to_string());
        let _ = tx.send(MineMsg::List(result)).await;
    });
    glib::spawn_future_local(async move {
        if let Ok(MineMsg::List(result)) = rx.recv().await {
            match result {
                Ok(fetched) => {
                    status.set_text(&format!("{} repositories", fetched.len()));
                    *repos.borrow_mut() = fetched;
                    *visible.borrow_mut() = populate_repo_list(&list, &repos.borrow(), "");
                }
                Err(e) => status.set_text(&format!("Could not load your repositories: {e}")),
            }
        }
    });

    root.upcast()
}

/// Rebuild `list` from the repositories matching `query`, and return that
/// filtered subset in the same order the rows were added — a `ListBoxRow`'s
/// index only means something against whatever produced the rows currently
/// showing, not against the unfiltered `repos`.
fn populate_repo_list(list: &gtk::ListBox, repos: &[Repository], query: &str) -> Vec<Repository> {
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    let matches: Vec<Repository> = repos
        .iter()
        .filter(|r| r.full_name.to_lowercase().contains(query))
        .cloned()
        .collect();
    for repo in &matches {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let label = gtk::Label::new(Some(&repo.full_name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        row.append(&label);
        if repo.private {
            let badge = gtk::Label::new(Some("private"));
            badge.add_css_class("dim-label");
            badge.add_css_class("caption");
            row.append(&badge);
        }
        list.append(&row);
    }
    matches
}

/// SSH when an agent is reachable, HTTPS otherwise — the same signal
/// `remote::Remote::is_ssh` would end up inferring from the URL after the
/// fact, checked here instead so the *choice* of URL is made once, up front.
fn preferred_clone_url(repo: &Repository) -> String {
    let has_agent = std::env::var_os("SSH_AUTH_SOCK").is_some();
    let ssh_first = has_agent.then(|| repo.ssh_url.clone()).flatten();
    ssh_first
        .or_else(|| repo.clone_url.clone())
        .or_else(|| repo.ssh_url.clone())
        // Neither URL present is not a real GitHub response; fall back to
        // something that at least names the repository in the error git
        // reports, rather than panicking on a response this narrow.
        .unwrap_or_else(|| repo.full_name.clone())
}

// ── "URL" ─────────────────────────────────────────────────────────────────

fn build_url_tab(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    on_cloned: Rc<dyn Fn(&Path)>,
) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_valign(gtk::Align::Start);

    let url_label = gtk::Label::new(Some("Repository URL"));
    url_label.set_xalign(0.0);
    url_label.add_css_class("heading");
    root.append(&url_label);

    let url_entry = gtk::Entry::new();
    url_entry.set_placeholder_text(Some(
        "https://github.com/owner/repo or git@github.com:owner/repo.git",
    ));
    root.append(&url_entry);

    let path_label = gtk::Label::new(Some("Local Path"));
    path_label.set_xalign(0.0);
    path_label.add_css_class("heading");
    path_label.set_margin_top(8);
    root.append(&path_label);

    let derived_name = Rc::new(RefCell::new(String::new()));
    let (path_row_box, path_entry) = path_row(window, derived_name.clone());
    root.append(&path_row_box);

    // The path field tracks the URL until the user edits it directly — after
    // that, typing a URL must not clobber a path they already chose.
    let path_touched = Rc::new(std::cell::Cell::new(false));
    {
        let path_touched = path_touched.clone();
        path_entry.connect_changed(move |_| path_touched.set(true));
    }
    {
        let path_entry = path_entry.clone();
        let path_touched = path_touched.clone();
        let derived_name = derived_name.clone();
        url_entry.connect_changed(move |e| {
            let name = repo_name_from_url(&e.text());
            *derived_name.borrow_mut() = name.clone();
            if !path_touched.get() && !name.is_empty() {
                path_entry.set_text(&default_dest(&name).to_string_lossy());
                // set_text above fires connect_changed on path_entry too —
                // this instance is the URL-driven one, so undo the flag it
                // just set rather than let the derived update count as a
                // manual edit.
                path_touched.set(false);
            }
        });
    }

    let clone_btn = gtk::Button::with_label("Clone");
    clone_btn.add_css_class("suggested-action");
    clone_btn.set_halign(gtk::Align::End);
    clone_btn.set_margin_top(8);
    root.append(&clone_btn);

    {
        let window = window.clone();
        let dialog = dialog.clone();
        let url_entry = url_entry.clone();
        clone_btn.connect_clicked(move |_| {
            let url = url_entry.text().trim().to_string();
            if url.is_empty() {
                report_error(&window, "A repository URL is required.");
                return;
            }
            let dest_text = path_entry.text();
            if dest_text.trim().is_empty() {
                report_error(&window, "A local path is required.");
                return;
            }
            let dest = PathBuf::from(dest_text.as_str());
            start_clone(&window, &dialog, url, dest, on_cloned.clone());
        });
    }

    root.upcast()
}

/// The directory name git will use: the URL's last path segment, minus a
/// trailing `.git` — the same rule git itself applies when no destination is
/// given on the command line.
fn repo_name_from_url(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".git")
        .to_string()
}

fn start_clone(
    window: &adw::ApplicationWindow,
    dialog: &adw::Dialog,
    url: String,
    dest: PathBuf,
    on_cloned: Rc<dyn Fn(&Path)>,
) {
    if dest.exists() && dest.read_dir().is_ok_and(|mut d| d.next().is_some()) {
        report_error(
            window,
            &format!("{} already exists and is not empty.", dest.display()),
        );
        return;
    }

    dialog.close();
    let window = window.clone();
    crate::sync::clone(
        &window,
        url,
        dest,
        Rc::new(move |result| {
            if let Ok(path) = result {
                on_cloned(&path);
            }
        }),
    );
}
