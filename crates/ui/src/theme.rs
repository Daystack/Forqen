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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Theme {
    /// Cold metal with one hot accent. The default.
    #[default]
    Forge,
    /// Dark-first, colour used only to identify branches.
    Lane,
    /// Warm paper, near-square corners, rules instead of shadows.
    Ledger,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Forge, Theme::Lane, Theme::Ledger];

    /// The value stored in GSettings.
    pub fn id(self) -> &'static str {
        match self {
            Theme::Forge => "forge",
            Theme::Lane => "lane",
            Theme::Ledger => "ledger",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "lane" => Theme::Lane,
            "ledger" => Theme::Ledger,
            // An unknown id means a settings file from a newer version, or a
            // typo. Falling back to the default beats refusing to start.
            _ => Theme::Forge,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Forge => "Forge",
            Theme::Lane => "Lane",
            Theme::Ledger => "Ledger",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Theme::Forge => "Cold metal, one hot accent. Warmth marks state.",
            Theme::Lane => "Dark. Colour identifies a branch, nothing else.",
            Theme::Ledger => "Warm paper and rules. History as a written record.",
        }
    }

    /// Whether this theme is drawn dark-first.
    ///
    /// Reported so the preferences dialog can say so — picking Lane while the
    /// desktop is in light mode is legitimate, but the user should know the
    /// theme will not follow the system.
    pub fn prefers_dark(self) -> bool {
        matches!(self, Theme::Lane)
    }

    /// The accent, for callers that need it outside CSS — the app icon tint
    /// and the diff-view selection among them.
    pub fn accent(self) -> &'static str {
        match self {
            Theme::Forge => "#c1662f",
            Theme::Lane => "#0e7c7b",
            Theme::Ledger => "#8c3a2b",
        }
    }

    /// Corner radius in px. Radius reads as tone: square is instrumental,
    /// round is friendly, and the three directions sit at different points.
    pub fn radius(self) -> u32 {
        match self {
            Theme::Forge => 6,
            Theme::Lane => 4,
            Theme::Ledger => 2,
        }
    }

    fn palette(self) -> Palette {
        match self {
            Theme::Forge => Palette {
                bg: "#f4f3f1",
                bg_alt: "#efedea",
                bar: "#eae8e5",
                fg: "#1c1f21",
                fg_dim: "#5a6165",
                rule: "#d8d4cf",
                accent: "#c1662f",
                accent_fg: "#ffffff",
                accent_soft: "#f7e4d6",
                add: "#dcebdd",
                del: "#f6dcd6",
                add_fg: "#1d4023",
                del_fg: "#5c1f19",
            },
            Theme::Lane => Palette {
                bg: "#0f1417",
                bg_alt: "#131a1d",
                bar: "#151c20",
                fg: "#dfe7ea",
                fg_dim: "#93a3aa",
                rule: "#232d32",
                accent: "#0e7c7b",
                accent_fg: "#eafcfb",
                accent_soft: "#16302f",
                add: "#14301f",
                del: "#331a1c",
                add_fg: "#a8e6b8",
                del_fg: "#f2b8b5",
            },
            Theme::Ledger => Palette {
                bg: "#faf8f4",
                bg_alt: "#f6f2ea",
                bar: "#f3efe7",
                fg: "#1a1815",
                fg_dim: "#5d564b",
                rule: "#ddd6c9",
                accent: "#8c3a2b",
                accent_fg: "#faf8f4",
                accent_soft: "#e6d9c9",
                add: "#e0e9d8",
                del: "#efdcd6",
                add_fg: "#2c4020",
                del_fg: "#5c211a",
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
    fg: &'static str,
    fg_dim: &'static str,
    rule: &'static str,
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
pub fn stylesheet(theme: Theme, font: Font, density: Density) -> String {
    let p = theme.palette();
    let r = theme.radius();
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
@define-color card_bg_color {bg_alt};
@define-color popover_bg_color {bg_alt};
@define-color dialog_bg_color {bg};
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

/* Lists carry the density: padding on the row, not margins on its children,
   so a change here moves every view at once. */
listview > row, list > row, row.activatable {{
    padding-top: {pad}px;
    padding-bottom: {pad}px;
    padding-left: {gut}px;
    padding-right: {gut}px;
    border-radius: {r}px;
}}

button {{ border-radius: {r}px; }}
entry, .card, popover > contents {{ border-radius: {r}px; }}

.navigation-sidebar {{ background-color: {bg_alt}; }}

/* Diff colours are semantic, not accent — they must stay legible whichever
   palette is loaded, so each theme supplies its own foreground too. */
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
        fg = p.fg,
        fg_dim = p.fg_dim,
        rule = p.rule,
        accent = p.accent,
        accent_fg = p.accent_fg,
        accent_soft = p.accent_soft,
        add = p.add,
        del = p.del,
        add_fg = p.add_fg,
        del_fg = p.del_fg,
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
        for t in Theme::ALL {
            let css = stylesheet(t, Font::Plex, Density::Default);
            let accent = t.accent();
            let added = css
                .lines()
                .find(|l| l.contains(".diff-added"))
                .unwrap_or_default();
            assert!(
                !added.contains(accent),
                "{t} tints additions with its accent"
            );
        }
    }

    #[test]
    fn density_changes_row_padding_and_nothing_else_structural() {
        let tight = stylesheet(Theme::Forge, Font::Plex, Density::Tight);
        let roomy = stylesheet(Theme::Forge, Font::Plex, Density::Roomy);
        assert!(tight.contains("padding-top: 3px"));
        assert!(roomy.contains("padding-top: 9px"));
        // The palette must not move when only density changes.
        assert!(tight.contains("#f4f3f1") && roomy.contains("#f4f3f1"));
    }

    #[test]
    fn each_theme_produces_a_distinct_stylesheet() {
        let sheets: Vec<String> = Theme::ALL
            .iter()
            .map(|t| stylesheet(*t, Font::Plex, Density::Default))
            .collect();
        assert_ne!(sheets[0], sheets[1]);
        assert_ne!(sheets[1], sheets[2]);
        assert_ne!(sheets[0], sheets[2]);
    }

    #[test]
    fn only_lane_is_dark_first() {
        assert!(Theme::Lane.prefers_dark());
        assert!(!Theme::Forge.prefers_dark());
        assert!(!Theme::Ledger.prefers_dark());
    }
}
