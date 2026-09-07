//! The command palette.
//!
//! Every command in forqen is a `GAction` before it is a button, so the palette
//! needs no separate dispatch path — it activates the same action the toolbar
//! does, and an action that is disabled is disabled here too.
//!
//! Matching is subsequence-based rather than substring: typing `icr` should
//! find "Interactive rebase", which is the entire point of a palette. Scoring
//! favours matches at word starts, so `pr` ranks "Pull requests" above
//! "Interactive rebase" even though both contain the letters in order.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

/// One entry in the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// The `app.` action name, without the prefix.
    pub action: String,
    pub label: String,
    /// Rendered accelerator, e.g. `Ctrl+Shift+P`. Empty when unbound.
    pub accel: String,
}

/// Score `query` against `text`, or `None` when it does not match at all.
///
/// Higher is better. The rules, in order of weight:
///
/// * every character of the query must appear in order — a subsequence, so
///   `icr` matches "Interactive rebase";
/// * a character matching the start of a word scores far more than one in the
///   middle, which is what makes initials rank above incidental letters;
/// * consecutive matches score more than scattered ones;
/// * shorter targets win ties, so an exact short command beats a long one that
///   merely contains it.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return Some(0);
    }

    let t: Vec<char> = text.to_lowercase().chars().collect();
    let raw: Vec<char> = text.chars().collect();

    let mut total = 0;
    let mut qi = 0;
    let mut last_match: Option<usize> = None;

    for (i, ch) in t.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        if *ch != q[qi] {
            continue;
        }

        // A word start is the first character, or one following a separator.
        let starts_word = i == 0
            || matches!(raw.get(i - 1), Some(p) if !p.is_alphanumeric())
            || matches!(raw.get(i - 1), Some(p) if p.is_lowercase() && raw[i].is_uppercase());

        total += if starts_word { 10 } else { 1 };
        if last_match == Some(i.wrapping_sub(1)) {
            total += 5;
        }

        last_match = Some(i);
        qi += 1;
    }

    if qi < q.len() {
        return None;
    }

    // Prefer shorter targets on equal evidence.
    Some(total - (t.len() as i32) / 8)
}

/// Rank commands against a query, best first, dropping non-matches.
pub fn rank(query: &str, commands: &[Command]) -> Vec<Command> {
    let mut scored: Vec<(i32, &Command)> = commands
        .iter()
        .filter_map(|c| score(query, &c.label).map(|s| (s, c)))
        .collect();

    // Stable by label so equal scores do not shuffle between keystrokes —
    // a list that reorders under the cursor is how the wrong thing gets run.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
    scored.into_iter().map(|(_, c)| c.clone()).collect()
}

pub struct Palette;

impl Palette {
    pub fn present(parent: &impl IsA<gtk::Window>, app: adw::Application, commands: Vec<Command>) {
        let window = adw::Window::builder()
            .transient_for(parent)
            .modal(true)
            .title("Commands")
            .default_width(560)
            .default_height(420)
            .build();

        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Type a command…"));

        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let shown: Rc<RefCell<Vec<Command>>> = Rc::new(RefCell::new(Vec::new()));

        let repopulate = {
            let list = list.clone();
            let shown = shown.clone();
            let commands = commands.clone();
            let app = app.clone();
            Rc::new(move |query: &str| {
                while let Some(child) = list.first_child() {
                    list.remove(&child);
                }

                let matches = rank(query, &commands);
                for c in &matches {
                    // A command whose action is disabled is shown dimmed rather
                    // than hidden: "why is Push missing" is a worse question
                    // than "why is Push greyed out".
                    let enabled = app
                        .lookup_action(&c.action)
                        .map(|a| a.is_enabled())
                        .unwrap_or(false);
                    list.append(&row(c, enabled));
                }
                *shown.borrow_mut() = matches;

                if let Some(first) = list.row_at_index(0) {
                    list.select_row(Some(&first));
                }
            })
        };

        repopulate("");

        {
            let repopulate = repopulate.clone();
            entry.connect_search_changed(move |e| repopulate(&e.text()));
        }

        let activate = {
            let list = list.clone();
            let shown = shown.clone();
            let app = app.clone();
            let window = window.clone();
            Rc::new(move || {
                let Some(row) = list.selected_row() else {
                    return;
                };
                let Some(command) = shown.borrow().get(row.index() as usize).cloned() else {
                    return;
                };
                // Close first: several commands open dialogs of their own, and
                // stacking one on a palette that is about to vanish leaves the
                // new dialog parented to a dying window.
                window.close();
                if let Some(action) = app.lookup_action(&command.action) {
                    action.activate(None);
                }
            })
        };

        {
            let activate = activate.clone();
            entry.connect_activate(move |_| activate());
        }
        {
            let activate = activate.clone();
            list.connect_row_activated(move |_, _| activate());
        }

        // Up/Down from the entry move the selection without leaving the field,
        // so the whole interaction is one uninterrupted piece of typing.
        let keys = gtk::EventControllerKey::new();
        {
            let list = list.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                let delta = match key {
                    gtk::gdk::Key::Down => 1,
                    gtk::gdk::Key::Up => -1,
                    _ => return glib::Propagation::Proceed,
                };
                let current = list.selected_row().map(|r| r.index()).unwrap_or(0);
                if let Some(next) = list.row_at_index((current + delta).max(0)) {
                    list.select_row(Some(&next));
                }
                glib::Propagation::Stop
            });
        }
        entry.add_controller(keys);

        window.set_content(Some(&build_layout(&entry, &list)));
        window.present();
        entry.grab_focus();
    }
}

fn row(c: &Command, enabled: bool) -> gtk::ListBoxRow {
    let label = gtk::Label::new(Some(&c.label));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    let accel = gtk::Label::new(Some(&c.accel));
    accel.add_css_class("dim-label");
    accel.add_css_class("caption");
    accel.add_css_class("monospace");

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row_box.set_margin_start(10);
    row_box.set_margin_end(10);
    row_box.set_margin_top(8);
    row_box.set_margin_bottom(8);
    row_box.append(&label);
    row_box.append(&accel);

    if !enabled {
        row_box.set_opacity(0.45);
    }

    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&row_box));
    row.set_activatable(enabled);
    row
}

fn build_layout(entry: &gtk::SearchEntry, list: &gtk::ListBox) -> gtk::Widget {
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
    scroll.set_margin_bottom(12);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(entry);
    content.append(&scroll);
    content.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds() -> Vec<Command> {
        [
            ("open", "Open a repository", "Ctrl+O"),
            ("push", "Push to origin", "Ctrl+P"),
            ("pull", "Pull from origin", "Ctrl+Shift+P"),
            ("rebase", "Interactive rebase", "Ctrl+Shift+R"),
            ("stashes", "Stashes", "Ctrl+Shift+S"),
            ("page-pulls", "Pull requests", ""),
            ("blame", "Blame this file", "Ctrl+Shift+B"),
        ]
        .into_iter()
        .map(|(a, l, k)| Command {
            action: a.into(),
            label: l.into(),
            accel: k.into(),
        })
        .collect()
    }

    #[test]
    fn initials_find_a_multi_word_command() {
        // The whole point of a palette: `ir` should reach "Interactive rebase".
        let hit = rank("ir", &cmds());
        assert_eq!(hit[0].label, "Interactive rebase");
    }

    #[test]
    fn a_scattered_subsequence_still_matches() {
        assert!(score("icr", "Interactive rebase").is_some());
        assert!(score("xyz", "Interactive rebase").is_none());
    }

    #[test]
    fn word_starts_outrank_letters_buried_mid_word() {
        // "Pull requests" matches p and r at two word starts; "Repository
        // options" matches the p inside "Repository" and a later r, both
        // mid-word. Same letters, same order, very different usefulness.
        let word_start = score("pr", "Pull requests").unwrap();
        let buried = score("pr", "Repository options").unwrap();
        assert!(
            word_start > buried,
            "initials should win: {word_start} vs {buried}"
        );
        assert_eq!(rank("pr", &cmds())[0].label, "Pull requests");
    }

    #[test]
    fn a_target_missing_a_query_letter_does_not_match() {
        // "Interactive rebase" has no `p` at all.
        assert_eq!(score("pr", "Interactive rebase"), None);
    }

    #[test]
    fn consecutive_characters_beat_scattered_ones() {
        let together = score("sta", "Stashes").unwrap();
        let apart = score("sta", "Set the amend").unwrap();
        assert!(together > apart, "{together} vs {apart}");
    }

    #[test]
    fn an_empty_query_keeps_everything_in_a_stable_order() {
        let all = rank("", &cmds());
        assert_eq!(all.len(), cmds().len());
        // Stable ordering matters: a list that reshuffles between keystrokes
        // is how the wrong command gets run.
        assert_eq!(all, rank("", &cmds()));
    }

    #[test]
    fn matching_ignores_case_and_spaces_in_the_query() {
        assert!(score("PULL", "Pull from origin").is_some());
        assert!(score("p r", "Pull requests").is_some());
    }

    #[test]
    fn a_query_matching_nothing_returns_nothing() {
        assert!(rank("zzzzq", &cmds()).is_empty());
    }

    #[test]
    fn ties_are_broken_by_label_not_by_input_order() {
        let mut a = cmds();
        a.reverse();
        let ranked_forward = rank("p", &cmds());
        let ranked_reverse = rank("p", &a);
        assert_eq!(
            ranked_forward, ranked_reverse,
            "ranking must not depend on the order commands were registered"
        );
    }
}
