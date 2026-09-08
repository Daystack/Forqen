//! Preferences: theme, typeface, density.
//!
//! Every choice applies immediately rather than on close. A theme picker that
//! waits for an OK button asks the user to imagine the result; applying live
//! lets them see it and change their mind, which is the entire reason the
//! setting exists.

use std::rc::Rc;

use adw::prelude::*;

use crate::theme::{Density, Font, Theme};

/// Read the current selection from settings, falling back to the shipped
/// defaults when there is no settings backend at all.
pub fn current(prefs: Option<&gtk::gio::Settings>) -> (Theme, Font, Density) {
    let Some(p) = prefs else {
        return (Theme::default(), Font::default(), Density::default());
    };
    (
        Theme::from_id(&p.string("theme")),
        Font::from_id(&p.string("ui-font")),
        Density::from_id(&p.string("density")),
    )
}

/// Present the preferences dialog.
///
/// `on_change` re-applies the stylesheet; it runs on every selection so the
/// window redraws under the dialog while it is still open.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    prefs: Option<gtk::gio::Settings>,
    on_change: Rc<dyn Fn()>,
) {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Preferences");

    let page = adw::PreferencesPage::new();
    page.set_title("Appearance");
    page.set_icon_name(Some("applications-graphics-symbolic"));

    let (theme_now, font_now, density_now) = current(prefs.as_ref());

    // ── theme ────────────────────────────────────────────────────────────
    let theme_group = adw::PreferencesGroup::new();
    theme_group.set_title("Theme");
    theme_group.set_description(Some(
        "Each is a complete system — palette, corner radius and rules — not a \
         recolouring of the same one.",
    ));

    let theme_labels: Vec<&str> = Theme::ALL.iter().map(|t| t.label()).collect();
    let theme_row = adw::ComboRow::new();
    theme_row.set_title("Theme");
    theme_row.set_model(Some(&gtk::StringList::new(&theme_labels)));
    theme_row.set_selected(Theme::ALL.iter().position(|t| *t == theme_now).unwrap_or(0) as u32);
    theme_row.set_subtitle(theme_now.description());
    theme_group.add(&theme_row);

    // ── typeface ─────────────────────────────────────────────────────────
    let font_group = adw::PreferencesGroup::new();
    font_group.set_title("Typeface");
    font_group.set_description(Some(
        "IBM Plex ships with forqen. Others appear only when your system has \
         them — a missing family would fall back without saying so.",
    ));

    // Offer only what will actually render. A dropdown entry that silently
    // does nothing is worse than one that is absent.
    let available: Vec<Font> = Font::ALL.into_iter().filter(|f| f.is_available()).collect();
    let font_labels: Vec<&str> = available.iter().map(|f| f.label()).collect();
    let font_row = adw::ComboRow::new();
    font_row.set_title("Interface font");
    font_row.set_model(Some(&gtk::StringList::new(&font_labels)));
    font_row.set_selected(available.iter().position(|f| *f == font_now).unwrap_or(0) as u32);
    font_row.set_subtitle(font_now.description());
    font_group.add(&font_row);

    // Name what is missing rather than leaving a shorter list unexplained.
    let missing: Vec<&str> = Font::ALL
        .into_iter()
        .filter(|f| !f.is_available())
        .map(|f| f.label())
        .collect();
    if !missing.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("Not installed");
        row.set_subtitle(&format!(
            "{} — install the family to enable it here.",
            missing.join(", ")
        ));
        row.set_sensitive(false);
        font_group.add(&row);
    }

    // ── density ──────────────────────────────────────────────────────────
    let density_group = adw::PreferencesGroup::new();
    density_group.set_title("Density");
    density_group.set_description(Some("Applies to every list at once."));

    let density_labels: Vec<&str> = Density::ALL.iter().map(|d| d.label()).collect();
    let density_row = adw::ComboRow::new();
    density_row.set_title("Row spacing");
    density_row.set_model(Some(&gtk::StringList::new(&density_labels)));
    density_row.set_selected(
        Density::ALL
            .iter()
            .position(|d| *d == density_now)
            .unwrap_or(1) as u32,
    );
    density_group.add(&density_row);

    // ── wiring ───────────────────────────────────────────────────────────
    {
        let prefs = prefs.clone();
        let on_change = on_change.clone();
        let row = theme_row.clone();
        theme_row.connect_selected_notify(move |r| {
            let theme = Theme::ALL[r.selected().min(2) as usize];
            row.set_subtitle(theme.description());
            if let Some(p) = &prefs {
                p.set_string("theme", theme.id()).ok();
            }
            on_change();
        });
    }
    {
        let prefs = prefs.clone();
        let on_change = on_change.clone();
        let available = available.clone();
        let row = font_row.clone();
        font_row.connect_selected_notify(move |r| {
            let Some(font) = available.get(r.selected() as usize).copied() else {
                return;
            };
            row.set_subtitle(font.description());
            if let Some(p) = &prefs {
                p.set_string("ui-font", font.id()).ok();
            }
            on_change();
        });
    }
    {
        let prefs = prefs.clone();
        let on_change = on_change.clone();
        density_row.connect_selected_notify(move |r| {
            let density = Density::ALL[r.selected().min(2) as usize];
            if let Some(p) = &prefs {
                p.set_string("density", density.id()).ok();
            }
            on_change();
        });
    }

    page.add(&theme_group);
    page.add(&font_group);
    page.add(&density_group);
    dialog.add(&page);
    dialog.present(Some(parent));
}
