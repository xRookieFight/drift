//! Light or dark, as the desktop wants it.
//!
//! winit only reports a theme on some platforms (not KDE or GNOME on Wayland), so we ask the
//! XDG desktop portal, which every modern Linux desktop answers.

use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    Light,
    Dark,
}

/// Reads org.freedesktop.appearance color-scheme from the portal: 1 is dark, 2 is light,
/// 0 is "no preference". Returns None when there is no portal to ask.
pub fn portal_preference() -> Option<Preference> {
    // yeah, shelling out to gdbus is cheap and saves pulling a whole D-Bus stack in for one
    // read. --timeout keeps a missing portal from stalling startup
    let output = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--timeout",
            "1",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.ReadOne",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse(&String::from_utf8_lossy(&output.stdout))
}

/// Parses gdbus output like `(<uint32 1>,)`. Older portals wrap it twice: `(<<uint32 1>>,)`.
fn parse(text: &str) -> Option<Preference> {
    let value = text.split("uint32").nth(1)?;
    let digit = value.trim_start().chars().next()?;
    match digit {
        '1' => Some(Preference::Dark),
        '2' => Some(Preference::Light),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_portal_answers() {
        assert_eq!(parse("(<uint32 1>,)\n"), Some(Preference::Dark));
        assert_eq!(parse("(<<uint32 2>>,)"), Some(Preference::Light));
        assert_eq!(parse("(<uint32 0>,)"), None);
        assert_eq!(parse("Error: no such interface"), None);
    }
}
