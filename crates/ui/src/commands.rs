//! The command table.
//!
//! Pure data, separate from the widgets that carry out the commands. Two
//! reasons:
//!
//! * it is testable without a display, which nothing involving a `GtkButton`
//!   is in this environment;
//! * joining it against the buttons at startup turns a missing wiring into an
//!   immediate, loud failure. A command declared here with no button behind it
//!   cannot silently do nothing, which is exactly what search did for a whole
//!   commit.

/// A command: its action name, what the menu and palette call it, and the
/// accelerators bound to it.
pub type Spec = (&'static str, &'static str, &'static [&'static str]);

/// Every command activated by a toolbar or menu button.
///
/// Page-switching commands are generated from the page list instead, since
/// they are mechanical.
pub const BUTTON_COMMANDS: &[Spec] = &[
    ("open", "Open a repository", &["<Control>o"]),
    ("stashes", "Stashes", &["<Control><Shift>s"]),
    ("rebase", "Interactive rebase", &["<Control><Shift>r"]),
    ("worktrees", "Worktrees", &["<Control><Shift>w"]),
    (
        "reflog",
        "History of HEAD — undo anything",
        &["<Control><Shift>z"],
    ),
    ("blame", "Blame this file", &["<Control><Shift>b"]),
    ("search", "Search the repository", &["<Control>f"]),
    ("releases", "Releases", &[]),
    ("gists", "Gists", &[]),
    ("fetch", "Fetch all remotes", &["<Control>r"]),
    ("pull", "Pull from origin", &["<Control><Shift>p"]),
    ("push", "Push to origin", &["<Control>p"]),
    ("account", "Sign in to GitHub", &[]),
];

/// Pages, in switcher order. The index sets the `Ctrl+N` accelerator.
pub const PAGES: &[&str] = &[
    "history",
    "changes",
    "pulls",
    "issues",
    "actions",
    "inbox",
    "conflicts",
];

/// Render a GTK accelerator as something a person reads.
///
/// `<Control><Shift>p` is how GTK spells it and not how anyone says it.
pub fn pretty_accel(accel: &str) -> String {
    let mut out = accel
        .replace("<Primary>", "Ctrl+")
        .replace("<Control>", "Ctrl+")
        .replace("<Shift>", "Shift+")
        .replace("<Alt>", "Alt+");

    // The final key is a bare lowercase letter in GTK's spelling.
    if let Some(last) = out.pop() {
        out.extend(last.to_uppercase());
    }
    out
}

/// Title-case a page name for display.
pub fn page_label(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => format!("Go to {}{}", first.to_uppercase(), chars.as_str()),
        None => "Go to".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn action_names_are_unique() {
        // Two commands sharing a name means the second silently replaces the
        // first's action, and one button stops working.
        let mut seen = HashSet::new();
        for (name, _, _) in BUTTON_COMMANDS {
            assert!(seen.insert(*name), "duplicate action name: {name}");
        }
        for page in PAGES {
            assert!(
                seen.insert(page),
                "page name collides with a command: {page}"
            );
        }
    }

    #[test]
    fn every_command_has_a_label() {
        for (name, label, _) in BUTTON_COMMANDS {
            assert!(!label.trim().is_empty(), "{name} has no label");
            // The palette shows these; a name like "page-pulls" would be
            // technically fine and useless to read.
            assert!(
                label.chars().next().is_some_and(char::is_uppercase),
                "{name}'s label should read as prose: {label:?}"
            );
        }
    }

    #[test]
    fn accelerators_are_well_formed() {
        for (name, _, accels) in BUTTON_COMMANDS {
            for a in *accels {
                assert!(
                    a.starts_with('<'),
                    "{name}: an accelerator needs a modifier: {a:?}"
                );
                assert!(
                    a.ends_with(|c: char| c.is_ascii_alphanumeric()),
                    "{name}: an accelerator ends with a key: {a:?}"
                );
            }
        }
    }

    #[test]
    fn no_two_commands_claim_the_same_shortcut() {
        // A duplicate binding is resolved arbitrarily by GTK, so one of the
        // two commands becomes unreachable from the keyboard.
        let mut seen = HashSet::new();
        for (name, _, accels) in BUTTON_COMMANDS {
            for a in *accels {
                assert!(seen.insert(*a), "{name} reuses the shortcut {a}");
            }
        }
        // Pages take Ctrl+1..N; those must not collide either.
        for i in 1..=PAGES.len() {
            let accel = format!("<Control>{i}");
            assert!(
                !seen.contains(accel.as_str()),
                "a page shortcut collides with a command: {accel}"
            );
        }
    }

    #[test]
    fn accelerators_render_for_people() {
        assert_eq!(pretty_accel("<Control>o"), "Ctrl+O");
        assert_eq!(pretty_accel("<Control><Shift>p"), "Ctrl+Shift+P");
        assert_eq!(pretty_accel("<Alt>x"), "Alt+X");
        assert_eq!(pretty_accel("<Control>1"), "Ctrl+1");
        assert_eq!(pretty_accel(""), "");
    }

    #[test]
    fn page_labels_read_as_prose() {
        assert_eq!(page_label("history"), "Go to History");
        assert_eq!(page_label("pulls"), "Go to Pulls");
        assert_eq!(page_label(""), "Go to");
    }
}
