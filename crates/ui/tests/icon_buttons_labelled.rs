//! Icon-only buttons must carry an accessible label.
//!
//! GTK4 falls back to the *icon name* for an icon button's accessible name, so
//! an unlabelled one is announced as "view-refresh-symbolic". A tooltip is not
//! a substitute: it becomes the accessible *description*, which a screen
//! reader does not read in place of a name.
//!
//! Source-level rather than widget-level, because GTK cannot be initialised
//! headlessly in this environment — broadwayd starts but no client can reach
//! its socket.

use std::path::Path;

#[test]
fn icon_buttons_go_through_the_labelling_helper() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    for entry in std::fs::read_dir(&src).expect("src/") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        // The helpers themselves are where the raw constructor belongs.
        if name == "commands.rs" {
            continue;
        }

        let text = std::fs::read_to_string(&path).expect("read");
        for (i, line) in text.lines().enumerate() {
            if !line.contains("from_icon_name") && !line.contains("set_icon_name") {
                continue;
            }
            // An explicit accessible Property::Label nearby is equally valid —
            // some buttons choose their icon at runtime and cannot use the
            // helper.
            let window: String = text
                .lines()
                .skip(i.saturating_sub(6))
                .take(14)
                .collect::<Vec<_>>()
                .join("\n");
            if window.contains("accessible::Property::Label")
                || window.contains("commands::icon_button")
                || window.contains("commands::icon_toggle")
            {
                continue;
            }
            offenders.push(format!("{name}:{}", i + 1));
        }
    }

    assert!(
        offenders.is_empty(),
        "these icon buttons have no accessible label, so a screen reader \
         announces their icon name instead: {offenders:?}. Use \
         commands::icon_button, or set accessible::Property::Label."
    );
}
