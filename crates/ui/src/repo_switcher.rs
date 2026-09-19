//! The repository switcher: every repository forqen has opened, one click
//! to jump to any of them.
//!
//! Two mount points share this module — the start page, shown when nothing
//! is open yet, and a popover behind the repository/branch title, for
//! switching away from one that is. Both are built from the same list for
//! the same reason GitHub Desktop's are: a repository you switched to
//! yesterday belongs in the same list as one you are opening for the first
//! time, not a separate feature.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;

use git::Repo;
use github::pulls::parse_remote;

/// One entry in the switcher: where it lives, what to call it, and — when a
/// GitHub remote could be parsed — which repository it is there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownRepo {
    pub path: PathBuf,
    pub name: String,
    pub github: Option<(String, String)>,
}

/// Every repository forqen has opened that still exists on disk, classified
/// into GitHub-backed and local-only.
///
/// A thin wrapper over `classify` — kept separate so a test can drive
/// classification against real fixture paths without a GSettings schema
/// installed, which `settings::recent` needs and a test environment does not
/// have.
pub fn known_repos(prefs: Option<&gtk::gio::Settings>) -> Vec<KnownRepo> {
    classify(crate::settings::recent(prefs))
}

/// `settings::recent()` already drops a path that no longer exists; this adds
/// the rarer second case — a path that exists but no longer opens as a
/// repository (moved, corrupted, permissions changed since it was recorded).
/// Both failure modes get the same treatment: dropped rather than shown as a
/// row that errors the moment someone clicks it.
///
/// Opening each path is not batched behind a background thread the way a
/// network call would be — `Repo::open` maps the object database rather than
/// reading it, and every other call site in this codebase treats that as
/// cheap enough to do inline (`state.rs`'s own doc comments say as much).
fn classify(paths: impl IntoIterator<Item = PathBuf>) -> Vec<KnownRepo> {
    paths
        .into_iter()
        .filter_map(|path| {
            let repo = Repo::open(&path).ok()?;
            let name = path.file_name()?.to_string_lossy().into_owned();
            let github = git::remote::list(&repo).ok().and_then(|remotes| {
                remotes
                    .iter()
                    .find(|r| r.name == "origin")
                    .and_then(|r| github_owner_repo(&r.fetch_url))
                    .or_else(|| remotes.iter().find_map(|r| github_owner_repo(&r.fetch_url)))
            });
            Some(KnownRepo { path, name, github })
        })
        .collect()
}

/// `parse_remote` is deliberately host-agnostic — it also has to work for a
/// GitHub Enterprise Server remote, which forqen supports elsewhere in the
/// app, so it extracts an owner/name pair from any URL shaped like one.
/// Grouping a repository into the "GitHub" section needs the opposite check:
/// a self-hosted GitLab or Gitea remote is shaped exactly the same way and
/// must land in "Other", not be mislabeled. `.contains("github")` rather than
/// an exact match on `github.com` is what admits Enterprise Server hosts
/// (`github.example.com`) without admitting an unrelated host that merely
/// has an owner/name-shaped path.
fn github_owner_repo(url: &str) -> Option<(String, String)> {
    if !host(url)?.to_lowercase().contains("github") {
        return None;
    }
    parse_remote(url)
}

/// The host component of a git remote URL, covering both `https://host/...`
/// and the scp-like `user@host:...` form `parse_remote` also handles.
fn host(url: &str) -> Option<&str> {
    let trimmed = url.trim();
    match trimmed.split_once("://") {
        Some((_, rest)) => Some(rest.split_once('/').map_or(rest, |(h, _)| h)),
        None => trimmed.split_once('@')?.1.split_once(':').map(|(h, _)| h),
    }
}

/// Build the switcher's contents: a "GitHub" section, an "Other" section,
/// each present only when it has something in it, one click on a row calling
/// `on_activate` with that repository's path.
pub fn build_list(repos: &[KnownRepo], on_activate: Rc<dyn Fn(&Path)>) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);

    let (github, other): (Vec<_>, Vec<_>) = repos.iter().partition(|r| r.github.is_some());

    if repos.is_empty() {
        let empty = gtk::Label::new(Some("No repositories yet"));
        empty.add_css_class("dim-label");
        root.append(&empty);
        return root.upcast();
    }

    if !github.is_empty() {
        root.append(&section(
            "GitHub",
            &github,
            "applications-development-symbolic",
            &on_activate,
        ));
    }
    if !other.is_empty() {
        root.append(&section("Other", &other, "folder-symbolic", &on_activate));
    }

    root.upcast()
}

fn section(
    title: &str,
    repos: &[&KnownRepo],
    icon: &str,
    on_activate: &Rc<dyn Fn(&Path)>,
) -> gtk::Box {
    let wrap = gtk::Box::new(gtk::Orientation::Vertical, 4);

    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("caption");
    label.add_css_class("dim-label");
    wrap.append(&label);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    for repo in repos {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let image = gtk::Image::from_icon_name(icon);
        row.append(&image);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let name = gtk::Label::new(Some(&repo.name));
        name.set_xalign(0.0);
        text.append(&name);
        if let Some((owner, repo_name)) = &repo.github {
            let subtitle = gtk::Label::new(Some(&format!("{owner}/{repo_name}")));
            subtitle.set_xalign(0.0);
            subtitle.add_css_class("caption");
            subtitle.add_css_class("dim-label");
            text.append(&subtitle);
        }
        row.append(&text);

        let list_row = gtk::ListBoxRow::new();
        list_row.set_child(Some(&row));
        list_row.set_activatable(true);
        list.append(&list_row);
    }

    // Single click, unlike `wire_branch_switching`'s double-click: that guard
    // exists because a single click there is also how you select a branch to
    // *look at* without switching to it. This list has no such dual purpose —
    // every row exists only to be activated.
    let paths: Vec<PathBuf> = repos.iter().map(|r| r.path.clone()).collect();
    let on_activate = on_activate.clone();
    list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index >= 0 {
            if let Some(path) = paths.get(index as usize) {
                on_activate(path);
            }
        }
    });

    wrap.append(&list);
    wrap
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn repo_with_origin(dir: &std::path::Path, origin: Option<&str>) {
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(dir)
            .output()
            .unwrap();
        if let Some(url) = origin {
            Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["remote", "add", "origin", url])
                .output()
                .unwrap();
        }
    }

    #[test]
    fn classification_covers_github_other_and_a_broken_path() {
        let base = tempfile::tempdir().unwrap();

        let gh = base.path().join("gh-repo");
        repo_with_origin(&gh, Some("https://github.com/Daystack/Forqen.git"));

        let other = base.path().join("other-repo");
        repo_with_origin(&other, Some("https://example.invalid/o/r.git"));

        let none = base.path().join("no-remote-repo");
        repo_with_origin(&none, None);

        // Exists on disk, but is not a repository — the case
        // `settings::recent`'s own existence check cannot catch.
        let not_a_repo = base.path().join("just-a-folder");
        std::fs::create_dir(&not_a_repo).unwrap();

        let repos = classify([gh.clone(), other.clone(), none.clone(), not_a_repo.clone()]);

        assert_eq!(
            repos.len(),
            3,
            "the broken path must be dropped, not shown broken"
        );
        assert!(repos.iter().all(|r| r.path != not_a_repo));

        assert_eq!(
            repos.iter().find(|r| r.path == gh).unwrap().github.as_ref(),
            Some(&("Daystack".to_string(), "Forqen".to_string()))
        );
        assert_eq!(repos.iter().find(|r| r.path == other).unwrap().github, None);
        assert_eq!(repos.iter().find(|r| r.path == none).unwrap().github, None);
    }

    #[test]
    fn only_a_github_shaped_host_counts_as_github() {
        // parse_remote itself is host-agnostic on purpose (it also serves
        // GitHub Enterprise Server), so classification needs its own host
        // check — a self-hosted GitLab remote has the exact same
        // `host/owner/name` shape and must not be mislabeled.
        assert_eq!(
            github_owner_repo("https://github.com/Daystack/Forqen.git"),
            Some(("Daystack".to_string(), "Forqen".to_string()))
        );
        assert_eq!(
            github_owner_repo("https://github.example.com/o/r.git"),
            Some(("o".to_string(), "r".to_string())),
            "an Enterprise Server host must still count as GitHub"
        );
        assert_eq!(
            github_owner_repo("https://gitlab.example.invalid/o/r.git"),
            None,
            "a non-GitHub host must not be classified as GitHub just because \
             its URL is owner/name-shaped"
        );
    }

    #[test]
    fn the_displayed_name_is_the_directory_not_the_full_path() {
        let base = tempfile::tempdir().unwrap();
        let repo = base.path().join("my-project");
        repo_with_origin(&repo, None);

        let repos = classify([repo]);
        assert_eq!(repos[0].name, "my-project");
    }

    // `build_list` itself is not unit-tested here: every existing test module
    // in this crate is pure logic with no live GTK widgets, because CI runs
    // with no display and `gtk::Box::new` needs one. That gap is filled by
    // manual verification against the running app, the same as every other
    // widget-construction function in this codebase.
}
