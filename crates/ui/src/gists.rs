//! Gists: look at the ones you have, make a new one from a file.
//!
//! Creating is the useful half. The common path is "I want to share this
//! file" — so the dialog prefills from whatever the Changes page is showing,
//! rather than starting from an empty box.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use github::gists::{Gist, NewGist};

use crate::pulls::Target;

enum Msg {
    Loaded(Result<Vec<Gist>, String>),
    Detail(Result<Box<Gist>, String>),
    Created(Result<(), String>),
}

pub struct GistsDialog {
    window: adw::Window,
    list: gtk::ListBox,
    body: gtk::Label,
    status: gtk::Label,
    spinner: gtk::Spinner,
    open_btn: gtk::Button,
    target: Target,
    rt: tokio::runtime::Handle,
    items: Rc<RefCell<Vec<Gist>>>,
    selected: Cell<Option<usize>>,
}

impl GistsDialog {
    /// `prefill` is an optional (filename, content) pair to start a new gist
    /// from — normally the file open in the Changes page.
    pub fn present(
        parent: &impl IsA<gtk::Window>,
        target: Target,
        rt: tokio::runtime::Handle,
        prefill: Option<(String, String)>,
    ) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("Gists")
            .default_width(820)
            .default_height(560)
            .build();

        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let body = gtk::Label::new(Some("Select a gist"));
        body.set_xalign(0.0);
        body.set_yalign(0.0);
        body.set_selectable(true);
        body.add_css_class("monospace");

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let spinner = gtk::Spinner::new();

        let new_btn = gtk::Button::with_label("New gist…");
        new_btn.add_css_class("suggested-action");
        let open_btn = gtk::Button::from_icon_name("web-browser-symbolic");
        open_btn.set_tooltip_text(Some("Open on GitHub"));
        open_btn.set_sensitive(false);

        let dialog = Rc::new(Self {
            window: window.clone(),
            list: list.clone(),
            body: body.clone(),
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
                this.load_detail();
            });
        }
        {
            let this = dialog.clone();
            new_btn.connect_clicked(move |_| this.prompt_new(prefill.clone()));
        }
        {
            let this = dialog.clone();
            open_btn.connect_clicked(move |_| this.open_web());
        }

        window.set_content(Some(&build_layout(
            &list, &body, &status, &spinner, &new_btn, &open_btn,
        )));

        dialog.refresh();
        window.present();
    }

    fn refresh(self: &Rc<Self>) {
        self.spinner.start();
        self.status.set_text("Loading gists…");

        let target = self.target.clone();
        let (tx, rx) = async_channel::bounded::<Msg>(4);
        self.rt.spawn(async move {
            let result = target
                .client
                .gists()
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

    fn populate(self: &Rc<Self>, items: Vec<Gist>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let secret = items.iter().filter(|g| !g.public).count();
        self.status.set_text(&match (items.len(), secret) {
            (0, _) => "No gists yet".to_string(),
            (n, 0) => format!("{n} gists"),
            (n, s) => format!("{n} gists · {s} secret"),
        });

        for g in &items {
            self.list.append(&row(g));
        }
        *self.items.borrow_mut() = items;
    }

    /// A list response carries no file contents, so the chosen gist is fetched
    /// again on its own.
    fn load_detail(self: &Rc<Self>) {
        let Some(gist) = self
            .selected
            .get()
            .and_then(|i| self.items.borrow().get(i).cloned())
        else {
            return;
        };

        self.open_btn.set_sensitive(gist.html_url.is_some());
        self.body.set_text("Loading…");

        let target = self.target.clone();
        let id = gist.id.clone();
        let (tx, rx) = async_channel::bounded::<Msg>(4);
        self.rt.spawn(async move {
            let result = target
                .client
                .gist(&id)
                .await
                .map(|r| Box::new(r.data))
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Detail(result)).await;
        });

        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(Msg::Detail(result)) = rx.recv().await {
                match result {
                    Ok(full) => this.show(&full),
                    Err(e) => this.body.set_text(&format!("Could not load: {e}")),
                }
            }
        });
    }

    fn show(&self, gist: &Gist) {
        let mut text = String::new();
        for (name, file) in &gist.files {
            text.push_str(&format!("── {name}\n"));
            text.push_str(file.content.as_deref().unwrap_or("(no content)"));
            text.push_str("\n\n");
        }
        self.body.set_text(text.trim_end());
    }

    fn prompt_new(self: &Rc<Self>, prefill: Option<(String, String)>) {
        let dialog = adw::AlertDialog::new(Some("New gist"), None);

        let description = gtk::Entry::new();
        description.set_placeholder_text(Some("Description (optional)"));

        let filename = gtk::Entry::new();
        filename.set_placeholder_text(Some("Filename — the extension sets the highlighting"));

        let content = gtk::TextView::new();
        content.set_monospace(true);
        let content_scroll = gtk::ScrolledWindow::builder()
            .child(&content)
            .height_request(200)
            .width_request(480)
            .build();
        content_scroll.add_css_class("card");

        if let Some((name, text)) = prefill {
            filename.set_text(&name);
            content.buffer().set_text(&text);
        }

        // Secret by default. A gist made by accident should not be indexed;
        // making it public is a deliberate act.
        let public = gtk::CheckButton::with_label("Public — listed on your profile and indexed");

        let boxed = gtk::Box::new(gtk::Orientation::Vertical, 8);
        boxed.append(&description);
        boxed.append(&filename);
        boxed.append(&content_scroll);
        boxed.append(&public);
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
            let buf = content.buffer();
            let new = NewGist {
                description: description.text().to_string(),
                filename: filename.text().to_string(),
                content: buf
                    .text(&buf.start_iter(), &buf.end_iter(), false)
                    .to_string(),
                public: public.is_active(),
            };

            let target = this.target.clone();
            let (tx, rx) = async_channel::bounded::<Msg>(4);
            this.rt.spawn(async move {
                let result = target
                    .client
                    .create_gist(&new)
                    .await
                    .map_err(|e| e.to_string());
                let _ = tx.send(Msg::Created(result)).await;
            });

            let this = this.clone();
            glib::spawn_future_local(async move {
                if let Ok(Msg::Created(result)) = rx.recv().await {
                    match result {
                        Ok(()) => this.refresh(),
                        Err(e) => {
                            let err =
                                adw::AlertDialog::new(Some("Could not create the gist"), Some(&e));
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
            .and_then(|i| self.items.borrow().get(i).and_then(|g| g.html_url.clone()))
        else {
            return;
        };
        let launcher = gtk::UriLauncher::new(&url);
        launcher.launch(Some(&self.window), None::<&gtk::gio::Cancellable>, |_| {});
    }
}

fn row(g: &Gist) -> gtk::ListBoxRow {
    let title = gtk::Label::new(Some(&g.title()));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let meta = gtk::Label::new(Some(&format!(
        "{} · {} file{}",
        g.visibility(),
        g.files.len(),
        if g.files.len() == 1 { "" } else { "s" }
    )));
    meta.set_xalign(0.0);
    meta.add_css_class("dim-label");
    meta.add_css_class("caption");

    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 2);
    boxed.set_margin_start(8);
    boxed.set_margin_end(8);
    boxed.set_margin_top(6);
    boxed.set_margin_bottom(6);
    boxed.append(&title);
    boxed.append(&meta);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&boxed));
    row
}

fn build_layout(
    list: &gtk::ListBox,
    body: &gtk::Label,
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

    body.set_margin_start(12);
    body.set_margin_end(12);
    body.set_margin_top(12);
    body.set_margin_bottom(12);

    let body_scroll = gtk::ScrolledWindow::builder()
        .child(body)
        .vexpand(true)
        .build();

    let split = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&list_scroll)
        .end_child(&body_scroll)
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
