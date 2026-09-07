//! Releases: what has shipped, and the files that shipped with it.
//!
//! A dialog rather than a page — releases are consulted occasionally, not
//! worked in, and the page bar is already carrying six tabs.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use github::releases::Release;

use crate::pulls::Target;

enum Msg {
    Loaded(Result<Vec<Release>, String>),
    Created(Result<(), String>),
}

pub struct ReleasesDialog {
    window: adw::Window,
    list: gtk::ListBox,
    notes: gtk::Label,
    status: gtk::Label,
    spinner: gtk::Spinner,
    open_btn: gtk::Button,
    target: Target,
    rt: tokio::runtime::Handle,
    items: Rc<RefCell<Vec<Release>>>,
    selected: Cell<Option<usize>>,
}

impl ReleasesDialog {
    pub fn present(
        parent: &impl IsA<gtk::Window>,
        target: Target,
        rt: tokio::runtime::Handle,
        suggested_tag: Option<String>,
    ) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("Releases")
            .default_width(800)
            .default_height(560)
            .build();

        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let notes = gtk::Label::new(Some("Select a release"));
        notes.set_xalign(0.0);
        notes.set_yalign(0.0);
        notes.set_wrap(true);
        notes.set_selectable(true);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let spinner = gtk::Spinner::new();

        let new_btn = gtk::Button::with_label("New release…");
        new_btn.add_css_class("suggested-action");
        let open_btn = gtk::Button::from_icon_name("web-browser-symbolic");
        open_btn.set_tooltip_text(Some("Open on GitHub"));
        open_btn.set_sensitive(false);

        let dialog = Rc::new(Self {
            window: window.clone(),
            list: list.clone(),
            notes: notes.clone(),
            status: status.clone(),
            spinner: spinner.clone(),
            open_btn: open_btn.clone(),
            target,
            rt,
            items: Rc::new(RefCell::new(Vec::new())),
            selected: Cell::new(None),
        });

        {
            let this = dialog.clone();
            list.connect_row_selected(move |_, row| {
                this.selected.set(row.map(|r| r.index() as usize));
                this.show_selected();
            });
        }
        {
            let this = dialog.clone();
            let tag = suggested_tag.clone();
            new_btn.connect_clicked(move |_| this.prompt_new(tag.clone()));
        }
        {
            let this = dialog.clone();
            open_btn.connect_clicked(move |_| this.open_web());
        }

        window.set_content(Some(&build_layout(
            &list, &notes, &status, &spinner, &new_btn, &open_btn,
        )));

        dialog.refresh();
        window.present();
    }

    fn refresh(self: &Rc<Self>) {
        self.spinner.start();
        self.status.set_text("Loading releases…");

        let target = self.target.clone();
        let (tx, rx) = async_channel::bounded::<Msg>(4);
        self.rt.spawn(async move {
            let result = target
                .client
                .releases(&target.owner, &target.repo)
                .await
                .map(|r| r.data)
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Loaded(result)).await;
        });

        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(Msg::Loaded(result)) = rx.recv().await {
                this.spinner.stop();
                match result {
                    Ok(items) => this.populate(items),
                    Err(e) => this.status.set_text(&format!("Could not load: {e}")),
                }
            }
        });
    }

    fn populate(self: &Rc<Self>, items: Vec<Release>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let drafts = items.iter().filter(|r| r.draft).count();
        self.status.set_text(&match (items.len(), drafts) {
            (0, _) => "No releases yet".to_string(),
            (n, 0) => format!("{n} releases"),
            (n, d) => format!("{n} releases · {d} draft"),
        });

        for r in &items {
            self.list.append(&row(r));
        }
        *self.items.borrow_mut() = items;

        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        }
    }

    fn show_selected(self: &Rc<Self>) {
        let Some(release) = self
            .selected
            .get()
            .and_then(|i| self.items.borrow().get(i).cloned())
        else {
            return;
        };

        self.open_btn.set_sensitive(release.html_url.is_some());

        let mut text = format!("{}\n{}\n", release.title(), release.tag_name);
        if let Some(author) = &release.author {
            text.push_str(&format!("by {}\n", author.login));
        }
        text.push('\n');

        let body = release.body.as_deref().unwrap_or("").trim();
        text.push_str(if body.is_empty() {
            "(no release notes)"
        } else {
            body
        });

        if !release.assets.is_empty() {
            text.push_str("\n\nFiles:\n");
            for a in &release.assets {
                text.push_str(&format!(
                    "  {}  ({}, {} downloads)\n",
                    a.name,
                    a.human_size(),
                    a.download_count
                ));
            }
        }

        self.notes.set_text(&text);
    }

    fn prompt_new(self: &Rc<Self>, suggested_tag: Option<String>) {
        let dialog = adw::AlertDialog::new(
            Some("New release"),
            Some(
                "The tag must already exist on the remote. Creating a release \
                 for a tag that was never pushed leaves a broken link.",
            ),
        );

        let tag = gtk::Entry::new();
        tag.set_placeholder_text(Some("Tag, e.g. v1.2.0"));
        if let Some(t) = suggested_tag {
            tag.set_text(&t);
        }

        let name = gtk::Entry::new();
        name.set_placeholder_text(Some("Title (optional — defaults to the tag)"));

        let notes = gtk::TextView::new();
        notes.set_wrap_mode(gtk::WrapMode::WordChar);
        let notes_scroll = gtk::ScrolledWindow::builder()
            .child(&notes)
            .height_request(120)
            .width_request(420)
            .build();
        notes_scroll.add_css_class("card");

        let draft = gtk::CheckButton::with_label("Save as draft");
        let prerelease = gtk::CheckButton::with_label("Mark as a prerelease");

        let boxed = gtk::Box::new(gtk::Orientation::Vertical, 8);
        boxed.append(&tag);
        boxed.append(&name);
        boxed.append(&notes_scroll);
        boxed.append(&draft);
        boxed.append(&prerelease);
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
            let buf = notes.buffer();
            let body = buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string();
            let tag_text = tag.text().to_string();
            let name_text = name.text().to_string();
            let is_draft = draft.is_active();
            let is_pre = prerelease.is_active();
            let target = this.target.clone();

            let (tx, rx) = async_channel::bounded::<Msg>(4);
            this.rt.spawn(async move {
                let result = target
                    .client
                    .create_release(
                        &target.owner,
                        &target.repo,
                        &github::releases::NewRelease {
                            tag: tag_text,
                            name: name_text,
                            body,
                            draft: is_draft,
                            prerelease: is_pre,
                        },
                    )
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Created(result)).await;
            });

            let this = this.clone();
            glib::spawn_future_local(async move {
                if let Ok(Msg::Created(result)) = rx.recv().await {
                    match result {
                        Ok(()) => this.refresh(),
                        // The common failure is a tag that does not exist on
                        // the remote, and GitHub says so explicitly.
                        Err(e) => {
                            let err = adw::AlertDialog::new(
                                Some("Could not create the release"),
                                Some(&e),
                            );
                            err.add_response("ok", "OK");
                            err.present(Some(&this.window));
                        }
                    }
                }
            });
        });

        dialog.present(Some(&self.window));
    }

    fn open_web(&self) {
        let Some(url) = self
            .selected
            .get()
            .and_then(|i| self.items.borrow().get(i).and_then(|r| r.html_url.clone()))
        else {
            return;
        };
        let launcher = gtk::UriLauncher::new(&url);
        launcher.launch(Some(&self.window), None::<&gtk::gio::Cancellable>, |_| {});
    }
}

fn row(r: &Release) -> gtk::ListBoxRow {
    let title = gtk::Label::new(Some(r.title()));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let mut meta = format!("{} · {}", r.tag_name, r.state());
    if !r.assets.is_empty() {
        meta.push_str(&format!(" · {} files", r.assets.len()));
    }

    let subtitle = gtk::Label::new(Some(&meta));
    subtitle.set_xalign(0.0);
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("caption");
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 2);
    boxed.set_margin_start(8);
    boxed.set_margin_end(8);
    boxed.set_margin_top(6);
    boxed.set_margin_bottom(6);
    boxed.append(&title);
    boxed.append(&subtitle);

    // A draft is invisible to everyone else; dimming says so without a badge.
    if r.draft {
        boxed.set_opacity(0.6);
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&boxed));
    row
}

fn build_layout(
    list: &gtk::ListBox,
    notes: &gtk::Label,
    status: &gtk::Label,
    spinner: &gtk::Spinner,
    new_btn: &gtk::Button,
    open_btn: &gtk::Button,
) -> gtk::Widget {
    let header = adw::HeaderBar::new();
    header.pack_start(new_btn);
    header.pack_end(open_btn);
    header.pack_end(spinner);

    let list_scroll = gtk::ScrolledWindow::builder()
        .child(list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .width_request(240)
        .build();

    notes.set_margin_start(12);
    notes.set_margin_end(12);
    notes.set_margin_top(12);
    notes.set_margin_bottom(12);

    let notes_scroll = gtk::ScrolledWindow::builder()
        .child(notes)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let split = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&list_scroll)
        .end_child(&notes_scroll)
        .resize_start_child(false)
        .shrink_start_child(true)
        .shrink_end_child(true)
        .position(260)
        .build();

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.set_margin_top(6);
    footer.set_margin_bottom(6);
    footer.append(status);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&split);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&footer);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    view.upcast()
}
