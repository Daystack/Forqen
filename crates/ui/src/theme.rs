//! Themes and typefaces.
//!
//! GTK is styled with CSS, but not the CSS a browser speaks: it has
//! `@define-color` instead of custom properties, no cascade layers, and a
//! limited selector set. Each theme is therefore emitted as a complete
//! stylesheet rather than a set of variables swapped underneath a shared one.
//!
//! The named colours are defined even where this stylesheet does not use them,
//! because libadwaita's own widgets read them — `@theme_bg_color`,
//! `@accent_bg_color` and friends are how a headerbar or a suggested-action
//! button picks up a palette without being restyled individually.

use std::fmt;

/// A complete visual system: palette plus geometry.
///
/// One shipped theme, styled after macOS's Dark Mode — its layered flat
/// surfaces, hairline separators and label tiers, with forqen's own orange
/// accent rather than systemBlue. No light variant: a native Linux app has no
/// portable equivalent to NSVisualEffectView, so the macOS *look* here is
/// flat colour steps standing in for vibrancy, not an attempt at translucency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Theme {
    /// Cold metal with one hot accent. The default, and the only theme.
    #[default]
    Forge,
}

impl Theme {
    pub const ALL: [Theme; 1] = [Theme::Forge];

    /// The value stored in GSettings.
    pub fn id(self) -> &'static str {
        match self {
            Theme::Forge => "forge",
        }
    }

    pub fn from_id(_id: &str) -> Self {
        // Only one theme ships, so every id — current, stale, or typoed —
        // resolves to it. Keeping the function rather than inlining `Forge`
        // at call sites means a second theme, if one is ever added, has one
        // place to teach the fallback.
        Theme::Forge
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Forge => "Forge",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Theme::Forge => "Cold metal, one hot accent. Warmth marks state.",
        }
    }

    fn palette(self) -> Palette {
        match self {
            // Values are macOS's own published Dark Mode system colours
            // (window, sidebar, separator and label tiers) — not eyeballed.
            // The only departures from stock macOS are the accent, which is
            // forqen's own rather than systemBlue, and the diff add/del
            // pair, which predate this palette and already read as
            // dark-mode-native.
            Theme::Forge => Palette {
                bg: "#1e1e1e",
                bg_alt: "#262626",
                bar: "#1f1f1f",
                raised: "#2b2b2b",
                fg: "#f2f2f5",
                fg_dim: "#98989d",
                rule: "#38383a",
                rule_soft: "#2e2e30",
                accent: "#e07d45",
                accent_fg: "#1a1210",
                accent_soft: "#33221a",
                add: "#16301f",
                del: "#331c1a",
                add_fg: "#a5dcb2",
                del_fg: "#efb3ac",
            },
        }
    }
}

impl fmt::Display for Theme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

struct Palette {
    bg: &'static str,
    bg_alt: &'static str,
    bar: &'static str,
    /// Popovers, dialogs and cards — a step above `bg_alt`, standing in for
    /// the elevation a real compositor blur would otherwise provide.
    raised: &'static str,
    fg: &'static str,
    fg_dim: &'static str,
    rule: &'static str,
    /// A softer divider than `rule`, for dividers within a list rather than
    /// between major regions.
    rule_soft: &'static str,
    accent: &'static str,
    accent_fg: &'static str,
    accent_soft: &'static str,
    add: &'static str,
    del: &'static str,
    add_fg: &'static str,
    del_fg: &'static str,
}

/// The interface typeface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Font {
    /// IBM Plex — bundled, so it is always available. The default.
    #[default]
    Plex,
    /// Atkinson Hyperlegible, if the system has it.
    Atkinson,
    /// Whatever the desktop is already using.
    System,
}

impl Font {
    pub const ALL: [Font; 3] = [Font::Plex, Font::Atkinson, Font::System];

    pub fn id(self) -> &'static str {
        match self {
            Font::Plex => "plex",
            Font::Atkinson => "atkinson",
            Font::System => "system",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "atkinson" => Font::Atkinson,
            "system" => Font::System,
            _ => Font::Plex,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Font::Plex => "IBM Plex",
            Font::Atkinson => "Atkinson Hyperlegible",
            Font::System => "System default",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Font::Plex => {
                "Bundled. Drawn for engineering tools; unambiguous 1lI and a slashed zero."
            }
            Font::Atkinson => "Drawn to keep confusable letters apart at small sizes.",
            Font::System => "Follows the desktop's own interface font.",
        }
    }

    /// The CSS family stack.
    ///
    /// Always ends in a generic, because a missing family fails silently in
    /// GTK exactly as it does in a browser — the text simply renders in
    /// something else and nothing says why.
    pub fn ui_stack(self) -> &'static str {
        match self {
            Font::Plex => "\"IBM Plex Sans\", sans-serif",
            Font::Atkinson => "\"Atkinson Hyperlegible\", \"IBM Plex Sans\", sans-serif",
            Font::System => "sans-serif",
        }
    }

    pub fn mono_stack(self) -> &'static str {
        match self {
            // Plex Mono ships alongside Plex Sans, so it is available whenever
            // the bundled font is.
            Font::Plex | Font::System => "\"IBM Plex Mono\", monospace",
            Font::Atkinson => "\"JetBrains Mono\", \"IBM Plex Mono\", monospace",
        }
    }

    /// Whether the family is actually resolvable on this machine.
    ///
    /// Only meaningful for the faces that are not bundled: offering a font
    /// that silently falls back is worse than not offering it, so the
    /// preferences dialog dims what is missing and says why.
    pub fn is_available(self) -> bool {
        match self {
            // Bundled with the application, installed beside it.
            Font::Plex | Font::System => true,
            Font::Atkinson => family_installed("Atkinson Hyperlegible"),
        }
    }
}

/// Ask fontconfig whether a family resolves to itself.
///
/// `fc-match` always answers with *something* — that is its job — so the
/// answer only counts if the family it names is the one asked for.
fn family_installed(family: &str) -> bool {
    let Ok(out) = std::process::Command::new("fc-match")
        .arg("--format=%{family}")
        .arg(family)
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .to_lowercase()
        .contains(&family.to_lowercase())
}

/// How tightly rows are packed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Density {
    Tight,
    #[default]
    Default,
    Roomy,
}

impl Density {
    pub const ALL: [Density; 3] = [Density::Tight, Density::Default, Density::Roomy];

    pub fn id(self) -> &'static str {
        match self {
            Density::Tight => "tight",
            Density::Default => "default",
            Density::Roomy => "roomy",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "tight" => Density::Tight,
            "roomy" => Density::Roomy,
            _ => Density::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Density::Tight => "Tight",
            Density::Default => "Default",
            Density::Roomy => "Roomy",
        }
    }

    /// Vertical padding on a list row, in px.
    fn row_pad(self) -> u32 {
        match self {
            Density::Tight => 3,
            Density::Default => 6,
            Density::Roomy => 9,
        }
    }

    fn gutter(self) -> u32 {
        match self {
            Density::Tight => 8,
            Density::Default => 12,
            Density::Roomy => 16,
        }
    }
}

/// Build the complete stylesheet for a combination.
///
/// Geometry is not one radius applied everywhere. macOS doesn't do that
/// either: push buttons are full capsules regardless of width, surfaces
/// (cards, popovers, dialogs) use a moderate radius, and only the sidebar's
/// selection highlight is a smaller inset pill — the main content area's row
/// selection stays edge-to-edge, the way a Mail message list or an Xcode
/// editor selects. With one theme shipped, these stop being a per-theme
/// `radius()` method and become fixed constants here.
pub fn stylesheet(theme: Theme, font: Font, density: Density) -> String {
    const RADIUS_CONTROL: &str = "999px"; // buttons: always a full capsule
    const RADIUS_SURFACE: &str = "10px"; // cards, popovers, dialogs
    const RADIUS_ROW: &str = "6px"; // sidebar selection pill, and entries

    let p = theme.palette();
    let pad = density.row_pad();
    let gut = density.gutter();
    let ui = font.ui_stack();
    let mono = font.mono_stack();

    format!(
        r#"
/* forqen — {theme_label} · {font_label} · {density_label} */

/* libadwaita reads these to style widgets this sheet never mentions. */
@define-color window_bg_color {bg};
@define-color window_fg_color {fg};
@define-color view_bg_color {bg};
@define-color view_fg_color {fg};
@define-color headerbar_bg_color {bar};
@define-color headerbar_fg_color {fg};
@define-color sidebar_bg_color {bg_alt};
@define-color sidebar_fg_color {fg};
@define-color card_bg_color {raised};
@define-color popover_bg_color {raised};
@define-color dialog_bg_color {raised};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {accent_fg};
@define-color accent_color {accent};
@define-color borders {rule};

window, .background {{
    background-color: {bg};
    color: {fg};
    font-family: {ui};
}}

headerbar {{
    background-color: {bar};
    border-bottom: 1px solid {rule};
}}

.dim-label {{ color: {fg_dim}; }}

.monospace, .diff-view, textview.monospace {{ font-family: {mono}; }}

/* GtkTextView and GtkEntry paint their own text-node background — it is not
   inherited from a parent's `.card`/`.background` class, so leaving this
   unset is how a plain text view or an empty comment box renders as a solid,
   illegible block regardless of which theme is loaded. `bg`/`fg` is the safe
   default everywhere; a textview living inside `.card` (a comment box, the
   commit message editor) steps up to `raised` to match the surface around
   it instead of looking like a hole punched through it. */
textview, textview text, entry {{
    background-color: {bg};
    color: {fg};
    border-radius: {radius_row};
}}
.card textview, .card textview text {{
    background-color: {raised};
}}

/* A bare ScrolledWindow has no background of its own either — the same gap,
   one layer out. Painting it here means a list that has not yet been given
   `.card` still shows the content plane instead of nothing. */
scrolledwindow, list, .boxed-list {{
    background-color: {bg};
}}

/* Lists carry the density: padding on the row, not margins on its children,
   so a change here moves every view at once. Content-area rows stay
   edge-to-edge on purpose — only the sidebar gets an inset selection, below. */
listview > row, list > row, row.activatable {{
    padding-top: {pad}px;
    padding-bottom: {pad}px;
    padding-left: {gut}px;
    padding-right: {gut}px;
}}
listview > row:not(:last-child), list > row:not(:last-child) {{
    border-bottom: 1px solid {rule_soft};
}}

button {{ border-radius: {radius_control}; }}
.card, popover > contents, dialog {{
    border-radius: {radius_surface};
    background-color: {raised};
}}

.navigation-sidebar {{ background-color: {bg_alt}; }}

/* The macOS source-list look: a rounded highlight inset from the sidebar's
   edges, not a full-bleed row. Higher specificity than the plain `row`
   selectors above, so it wins without needing `!important`. */
.navigation-sidebar row {{
    margin-left: {gut}px;
    margin-right: {gut}px;
    border-radius: {radius_row};
}}

/* Diff colours are semantic, not accent — they must stay legible whichever
   palette is loaded, so each theme supplies its own foreground too. This is
   the one definition; diff_view.rs must not duplicate it. */
.diff-view .diff-added   {{ background-color: {add}; color: {add_fg}; }}
.diff-view .diff-removed {{ background-color: {del}; color: {del_fg}; }}
.diff-view .diff-hunk-header {{ color: {fg_dim}; font-weight: bold; }}

/* Selection uses the accent at full strength; the soft tint marks a row that
   is current without being focused. */
:selected, row:selected {{
    background-color: {accent};
    color: {accent_fg};
}}
row.current {{ background-color: {accent_soft}; }}

.error {{ color: {del_fg}; }}
.success {{ color: {add_fg}; }}
"#,
        theme_label = theme.label(),
        font_label = font.label(),
        density_label = density.label(),
        bg = p.bg,
        bg_alt = p.bg_alt,
        bar = p.bar,
        raised = p.raised,
        fg = p.fg,
        fg_dim = p.fg_dim,
        rule = p.rule,
        rule_soft = p.rule_soft,
        accent = p.accent,
        accent_fg = p.accent_fg,
        accent_soft = p.accent_soft,
        add = p.add,
        del = p.del,
        add_fg = p.add_fg,
        del_fg = p.del_fg,
        radius_control = RADIUS_CONTROL,
        radius_surface = RADIUS_SURFACE,
        radius_row = RADIUS_ROW,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_default_is_forge_and_plex() {
        // The promise made to anyone installing this: it looks like Forge in
        // IBM Plex out of the box, with no configuration.
        assert_eq!(Theme::default(), Theme::Forge);
        assert_eq!(Font::default(), Font::Plex);
        assert_eq!(Density::default(), Density::Default);
    }

    #[test]
    fn ids_round_trip() {
        for t in Theme::ALL {
            assert_eq!(Theme::from_id(t.id()), t);
        }
        for f in Font::ALL {
            assert_eq!(Font::from_id(f.id()), f);
        }
        for d in Density::ALL {
            assert_eq!(Density::from_id(d.id()), d);
        }
    }

    #[test]
    fn an_unknown_id_falls_back_rather_than_failing() {
        // A settings file written by a newer version must not stop the app.
        assert_eq!(Theme::from_id("chartreuse"), Theme::Forge);
        assert_eq!(Font::from_id(""), Font::Plex);
        assert_eq!(Density::from_id("enormous"), Density::Default);
    }

    #[test]
    fn every_font_stack_ends_in_a_generic() {
        // A stack without a generic fails silently: the text renders in
        // something arbitrary and nothing reports it.
        for f in Font::ALL {
            assert!(
                f.ui_stack().ends_with("sans-serif"),
                "{:?} ui stack has no generic: {}",
                f,
                f.ui_stack()
            );
            assert!(
                f.mono_stack().ends_with("monospace"),
                "{:?} mono stack has no generic",
                f
            );
        }
    }

    #[test]
    fn the_bundled_font_is_always_offered() {
        // Plex ships with the application, so it cannot be missing. Anything
        // else is only offered when fontconfig can actually resolve it.
        assert!(Font::Plex.is_available());
        assert!(Font::System.is_available());
    }

    #[test]
    fn every_theme_defines_the_colours_libadwaita_reads() {
        // Widgets this sheet never mentions — headerbars, suggested buttons,
        // popovers — pick up a palette only through these names. A theme
        // missing one renders half in the old palette.
        for t in Theme::ALL {
            let css = stylesheet(t, Font::Plex, Density::Default);
            for name in [
                "window_bg_color",
                "window_fg_color",
                "headerbar_bg_color",
                "sidebar_bg_color",
                "card_bg_color",
                "popover_bg_color",
                "accent_bg_color",
                "accent_fg_color",
                "borders",
            ] {
                assert!(
                    css.contains(&format!("@define-color {name}")),
                    "{t} does not define {name}"
                );
            }
        }
    }

    #[test]
    fn every_theme_sets_a_ground_and_a_foreground() {
        // A stylesheet that colours text without painting the window behind it
        // renders one theme's text on another theme's background.
        for t in Theme::ALL {
            let css = stylesheet(t, Font::Plex, Density::Default);
            assert!(css.contains("background-color:"), "{t} paints no ground");
            assert!(css.contains("color:"), "{t} sets no foreground");
        }
    }

    #[test]
    fn diff_colours_differ_from_the_accent() {
        // Semantic colour and brand colour are separate systems; an added line
        // tinted with the accent stops meaning "added".
        let css = stylesheet(Theme::Forge, Font::Plex, Density::Default);
        const ACCENT: &str = "#e07d45";
        let added = css
            .lines()
            .find(|l| l.contains(".diff-added"))
            .unwrap_or_default();
        assert!(
            !added.contains(ACCENT),
            "diff-added is tinted with the accent"
        );
    }

    #[test]
    fn density_changes_row_padding_and_nothing_else_structural() {
        let tight = stylesheet(Theme::Forge, Font::Plex, Density::Tight);
        let roomy = stylesheet(Theme::Forge, Font::Plex, Density::Roomy);
        assert!(tight.contains("padding-top: 3px"));
        assert!(roomy.contains("padding-top: 9px"));
        // The palette must not move when only density changes.
        assert!(tight.contains("#1e1e1e") && roomy.contains("#1e1e1e"));
    }

    #[test]
    fn textviews_and_entries_get_a_real_background_and_foreground() {
        // The bug this whole pass started from: a plain GtkTextView paints
        // its own text-node background, which nothing here set — so a
        // comment box or the commit message editor rendered as an
        // unreadable solid block regardless of which theme was loaded.
        let css = stylesheet(Theme::Forge, Font::Plex, Density::Default);
        let start = css
            .find("textview, textview text, entry")
            .expect("a textview/entry rule exists");
        let end = css[start..].find('}').unwrap() + start;
        let rule = &css[start..end];
        assert!(
            rule.contains("background-color:"),
            "textview sets no background"
        );
        assert!(rule.contains("color:"), "textview sets no foreground");
    }

    #[test]
    fn sidebar_rows_are_inset_but_content_rows_stay_edge_to_edge() {
        // The macOS source-list trait: only the sidebar's selection is a
        // rounded, inset pill. A content-area list (History, the PR list)
        // selects edge-to-edge, the way a real table view does.
        let css = stylesheet(Theme::Forge, Font::Plex, Density::Default);
        assert!(
            css.contains(".navigation-sidebar row") && css.contains("margin-left:"),
            "the sidebar has no inset selection"
        );
        let start = css
            .find("listview > row, list > row, row.activatable")
            .expect("the generic row rule exists");
        let end = css[start..].find('}').unwrap() + start;
        assert!(
            !css[start..end].contains("margin"),
            "content-area rows should stay edge-to-edge, not inherit a margin"
        );
    }
}
