//! Blame — who last changed each line, and which pull request brought it.
//!
//! The question blame actually gets asked is not "who wrote this" — the name
//! is rarely the point, and often the person has left — but "why". The answer
//! lives in the pull request discussion, so selecting a line looks up the pull
//! requests that introduced its commit.
//!
//! Rows are a `ColumnView` rather than a text view with a gutter: a line is a
//! row, so selecting one is unambiguous and the commit behind it is exact.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::glib;

use git::blame::BlameLine;

use crate::state::AppState;

/// How to reach GitHub for the PR lookup, when there is a GitHub remote.
#[derive(Clone)]
pub struct Lookup {
    pub owner: String,
    pub repo: String,
    pub client: Arc<github::Client>,
}

pub struct BlameDialog {
    list: gtk::ListBox,
    status: gtk::Label,
    origin: gtk::Label,
    lines: Rc<RefCell<Vec<BlameLine>>>,
    lookup: Option<Lookup>,
    rt: tokio::runtime::Handle,
}

impl BlameDialog {
    pub fn present(
        parent: &impl IsA<gtk::Window>,
        state: AppState,
        path: &str,
        lookup: Option<Lookup>,
        rt: tokio::runtime::Handle,
    ) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title(format!("Blame — {path}"))
            .default_width(900)
            .default_height(600)
            .build();

        let list = gtk::ListBox::new();
        list.add_css_class("monospace");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        // Where the selected line came from. Starts with an instruction rather
        // than blank, so the pane explains itself before anything is clicked.
        let origin = gtk::Label::new(Some("Select a line to see what introduced it"));
        origin.set_xalign(0.0);
        origin.set_wrap(true);
        origin.set_selectable(true);

        let dialog = Rc::new(Self {
            list: list.clone(),
            status: status.clone(),
            origin: origin.clone(),
            lines: Rc::new(RefCell::new(Vec::new())),
            lookup,
            rt,
        });

        {
            let this = dialog.clone();
            list.connect_row_selected(move |_, row| {
                if let Some(row) = row {
                    this.select(row.index() as usize);
                }
            });
        }

        window.set_content(Some(&build_layout(&list, &status, &origin)));

        dialog.load(&state, path);
        window.present();
    }

    fn load(self: &Rc<Self>, state: &AppState, path: &str) {
        let result = state.with(|s| git::blame::blame(&s.repo, std::path::Path::new(path)));

        match result {
            Some(Ok(lines)) => {
                let authors: std::collections::HashSet<&str> =
                    lines.iter().map(|l| l.author.as_str()).collect();
                self.status.set_text(&format!(
                    "{} lines · {} contributors",
                    lines.len(),
                    authors.len()
                ));
                for l in &lines {
                    self.list.append(&row(l));
                }
                *self.lines.borrow_mut() = lines;
            }
            Some(Err(e)) => self.status.set_text(&e.to_string()),
            None => self.status.set_text("No repository open"),
        }
    }

    fn select(self: &Rc<Self>, index: usize) {
        let Some(line) = self.lines.borrow().get(index).cloned() else {
            return;
        };

        if line.uncommitted {
            self.origin
                .set_text("This line has not been committed yet.");
            return;
        }

        let when = format_time(line.time);
        self.origin.set_text(&format!(
            "{}\n{} · {} · {}",
            line.summary,
            line.author,
            when,
            line.id.short()
        ));

        let Some(lookup) = self.lookup.clone() else {
            return;
        };
        let sha = line.id.to_hex();
        let summary = line.summary.clone();
        let author = line.author.clone();
        let short = line.id.short();

        let (tx, rx) =
            async_channel::bounded::<Result<Vec<github::pulls::AssociatedPull>, String>>(1);
        self.rt.spawn(async move {
            let result = lookup
                .client
                .pulls_for_commit(&lookup.owner, &lookup.repo, &sha)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(result).await;
        });

        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(result) = rx.recv().await else { return };
            let base = format!("{summary}\n{author} · {when} · {short}");
            match result {
                Ok(pulls) if !pulls.is_empty() => {
                    let list = pulls
                        .iter()
                        .map(|p| format!("#{} {}", p.number, p.title))
                        .collect::<Vec<_>>()
                        .join("\n");
                    this.origin
                        .set_text(&format!("{base}\n\nIntroduced by:\n{list}"));
                }
                // A commit pushed straight to a branch belongs to no pull
                // request. Saying so is more useful than leaving the pane
                // looking like it is still loading.
                Ok(_) => this
                    .origin
                    .set_text(&format!("{base}\n\nNot part of any pull request")),
                Err(e) => this
                    .origin
                    .set_text(&format!("{base}\n\nCould not look up: {e}")),
            }
        });
    }
}

/// Render a unix timestamp as a date.
///
/// Days since the epoch converted with the civil-from-days algorithm rather
/// than pulling in `chrono` for one label — the whole dependency for a single
/// `YYYY-MM-DD` is not worth its compile time.
pub fn format_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn row(l: &BlameLine) -> gtk::ListBoxRow {
    let no = gtk::Label::new(Some(&l.line_no.to_string()));
    no.set_xalign(1.0);
    no.set_width_chars(5);
    no.add_css_class("dim-label");

    let sha = gtk::Label::new(Some(&if l.uncommitted {
        "uncommitted".to_string()
    } else {
        l.id.short()
    }));
    sha.set_xalign(0.0);
    sha.set_width_chars(12);
    sha.add_css_class("dim-label");

    let author = gtk::Label::new(Some(&l.author));
    author.set_xalign(0.0);
    author.set_width_chars(14);
    author.set_ellipsize(gtk::pango::EllipsizeMode::End);
    author.add_css_class("dim-label");

    let content = gtk::Label::new(Some(&l.content));
    content.set_xalign(0.0);
    content.set_hexpand(true);
    // A source line is a unit; wrapping one makes the line numbers stop
    // lining up with what they number.
    content.set_single_line_mode(true);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_margin_start(6);
    row_box.set_margin_end(6);
    row_box.set_margin_top(1);
    row_box.set_margin_bottom(1);
    row_box.append(&no);
    row_box.append(&sha);
    row_box.append(&author);
    row_box.append(&content);

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row
}

fn build_layout(list: &gtk::ListBox, status: &gtk::Label, origin: &gtk::Label) -> gtk::Widget {
    let header = adw::HeaderBar::new();

    let scroll = gtk::ScrolledWindow::builder()
        .child(list)
        .vexpand(true)
        .build();

    origin.set_margin_start(12);
    origin.set_margin_end(12);
    origin.set_margin_top(8);
    origin.set_margin_bottom(8);

    let origin_scroll = gtk::ScrolledWindow::builder()
        .child(origin)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .height_request(110)
        .build();

    let panes = gtk::Paned::builder()
        .orientation(gtk::Orientation::Vertical)
        .start_child(&scroll)
        .end_child(&origin_scroll)
        .resize_start_child(true)
        .shrink_end_child(true)
        .build();

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.set_margin_top(4);
    footer.set_margin_bottom(4);
    footer.append(status);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&panes);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&footer);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&content));
    view.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_render_as_dates() {
        assert_eq!(format_time(0), "1970-01-01");
        assert_eq!(format_time(1_600_000_000), "2020-09-13");
        assert_eq!(format_time(1_700_000_000), "2023-11-14");
    }

    #[test]
    fn leap_days_are_handled() {
        // 2024-02-29 — the case a hand-rolled date conversion gets wrong.
        assert_eq!(format_time(1_709_164_800), "2024-02-29");
        assert_eq!(format_time(1_709_251_200), "2024-03-01");
    }

    #[test]
    fn a_time_before_the_epoch_does_not_wrap_round() {
        // git accepts negative timestamps, and a rewritten history can carry
        // them; flooring division is what keeps this from landing in 1969-12-32.
        assert_eq!(format_time(-1), "1969-12-31");
        assert_eq!(format_time(-86_400), "1969-12-31");
    }
}
