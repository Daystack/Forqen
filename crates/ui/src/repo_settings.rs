//! Repository settings, read-only.
//!
//! Answers the question people actually bring to a settings screen from a git
//! client: "why was my push rejected". Changing protection or access is left
//! to the web, where the consequences are spelled out.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use github::settings::{Collaborator, Protection, RepoSettings};

use crate::pulls::Target;

type Loaded = (Box<RepoSettings>, Vec<Collaborator>, Protection);

pub struct RepoSettingsDialog {
    window: adw::Window,
    content: gtk::Box,
    status: gtk::Label,
    spinner: gtk::Spinner,
}

impl RepoSettingsDialog {
    pub fn present(parent: &impl IsA<gtk::Window>, target: Target, rt: tokio::runtime::Handle) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("Repository settings")
            .default_width(560)
            .default_height(560)
            .build();

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(16);
        content.set_margin_bottom(16);

        let status = gtk::Label::new(Some("Loading…"));
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let spinner = gtk::Spinner::new();
        spinner.start();

        let dialog = Rc::new(Self {
            window: window.clone(),
            content: content.clone(),
            status: status.clone(),
            spinner: spinner.clone(),
        });

        let header = adw::HeaderBar::new();
        header.pack_end(&spinner);

        let scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.set_margin_start(16);
        footer.set_margin_end(16);
        footer.set_margin_bottom(8);
        footer.append(&status);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.append(&scroll);
        outer.append(&footer);

        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&outer));
        window.set_content(Some(&view));

        dialog.load(target, rt);
        window.present();
    }

    fn load(self: &Rc<Self>, target: Target, rt: tokio::runtime::Handle) {
        let (tx, rx) = async_channel::bounded::<Result<Loaded, String>>(1);

        rt.spawn(async move {
            let settings = target
                .client
                .repo_settings(&target.owner, &target.repo)
                .await
                .map(|r| r.data);

            let result = match settings {
                Err(e) => Err(e.to_string()),
                Ok(settings) => {
                    // Collaborators and protection are best-effort: a
                    // read-only token cannot see either, and that is a fact
                    // about the token rather than a failure of the screen.
                    let collaborators = target
                        .client
                        .collaborators(&target.owner, &target.repo)
                        .await
                        .unwrap_or_default();

                    let branch = settings
                        .default_branch
                        .clone()
                        .unwrap_or_else(|| "main".into());
                    let protection = target
                        .client
                        .branch_protection(&target.owner, &target.repo, &branch)
                        .await
                        .unwrap_or(Protection {
                            branch,
                            protected: None,
                            ..Default::default()
                        });

                    Ok((Box::new(settings), collaborators, protection))
                }
            };
            let _ = tx.send(result).await;
        });

        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(result) = rx.recv().await else { return };
            this.spinner.stop();
            match result {
                Ok((settings, collaborators, protection)) => {
                    this.show(&settings, &collaborators, &protection)
                }
                Err(e) => this.status.set_text(&format!("Could not load: {e}")),
            }
        });
    }

    fn show(&self, s: &RepoSettings, collaborators: &[Collaborator], p: &Protection) {
        self.window.set_title(Some(&s.full_name));
        self.status.set_text(&format!(
            "{} · {}",
            s.visibility
                .as_deref()
                .unwrap_or(if s.private { "private" } else { "public" }),
            s.permissions
                .as_ref()
                .map(|x| format!("your access: {}", x.role()))
                .unwrap_or_else(|| "access unknown".into())
        ));

        let mut rows: Vec<(String, String)> = vec![
            ("Repository".into(), s.full_name.clone()),
            (
                "Description".into(),
                s.description.clone().unwrap_or_else(|| "—".into()),
            ),
            (
                "Default branch".into(),
                s.default_branch.clone().unwrap_or_else(|| "—".into()),
            ),
            (
                "Licence".into(),
                s.license
                    .as_ref()
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| "none".into()),
            ),
        ];

        if s.archived {
            // Archived means pushes are rejected outright, which is exactly
            // the "why was my push rejected" case this screen exists for.
            rows.push((
                "Archived".into(),
                "yes — this repository is read-only".into(),
            ));
        }

        self.content.append(&section("Repository", &rows));

        let protection_rows = match p.protected {
            None => vec![(
                p.branch.clone(),
                "unknown — either unprotected, or your token cannot read it".into(),
            )],
            Some(_) => {
                let mut v = vec![(p.branch.clone(), "protected".to_string())];
                if let Some(n) = p.required_reviews {
                    v.push(("Required approvals".into(), n.to_string()));
                }
                if !p.required_checks.is_empty() {
                    v.push(("Required checks".into(), p.required_checks.join(", ")));
                }
                v.push((
                    "Applies to admins".into(),
                    if p.enforces_admins { "yes" } else { "no" }.into(),
                ));
                v
            }
        };
        self.content
            .append(&section("Branch protection", &protection_rows));

        let collab_rows: Vec<(String, String)> = if collaborators.is_empty() {
            vec![(
                "—".into(),
                "not listed — needs push access to read".to_string(),
            )]
        } else {
            collaborators
                .iter()
                .map(|c| {
                    (
                        c.login.clone(),
                        c.permissions
                            .as_ref()
                            .map(|p| p.role().to_string())
                            .unwrap_or_else(|| "unknown".into()),
                    )
                })
                .collect()
        };
        self.content.append(&section("Collaborators", &collab_rows));

        let note = gtk::Label::new(Some(
            "Read-only. Changing protection or access is done on github.com, \
             where the consequences are spelled out.",
        ));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("dim-label");
        note.add_css_class("caption");
        self.content.append(&note);
    }
}

fn section(title: &str, rows: &[(String, String)]) -> gtk::Widget {
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    for (key, value) in rows {
        let k = gtk::Label::new(Some(key));
        k.set_xalign(0.0);
        k.set_width_chars(18);
        k.add_css_class("dim-label");

        let v = gtk::Label::new(Some(value));
        v.set_xalign(0.0);
        v.set_hexpand(true);
        v.set_wrap(true);
        v.set_selectable(true);

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row_box.set_margin_start(10);
        row_box.set_margin_end(10);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);
        row_box.append(&k);
        row_box.append(&v);

        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&row_box));
        list.append(&row);
    }

    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 6);
    boxed.append(&heading);
    boxed.append(&list);
    boxed.upcast()
}
