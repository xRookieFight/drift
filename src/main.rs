//! Drift, a calm browser on Servo.

mod app;
mod bridge;
mod dialogs;
mod favicon;
mod keys;
mod keyutils;
mod layout;
mod omnibox;
mod protocol;
mod session;
mod tabs;
mod theme;
mod updater;

use std::path::PathBuf;
use std::process::ExitCode;

use winit::event_loop::EventLoop;

const HELP: &str = "\
Drift, a calm web browser built on Servo

Usage: drift [OPTIONS] [URL]

Options:
  --no-restore       Start without restoring the last session
  --profile <DIR>    Keep cookies, storage and the session in DIR
  --theme <THEME>    light, dark or auto (default: follow the desktop)
  --no-update-check  Do not look for new releases on GitHub
  -h, --help         Show this help
  -V, --version      Show the version
";

fn main() -> ExitCode {
    let config = match parse_args(std::env::args().skip(1)) {
        Ok(Some(config)) => config,
        Ok(None) => return ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("drift: {err}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("crypto provider installs once");

    let event_loop = match EventLoop::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            eprintln!("drift: could not open a display: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut app = app::App::new(config, event_loop.create_proxy());
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("drift: {err}");
        return ExitCode::FAILURE;
    }
    if let Some(path) = app.relaunch() {
        relaunch(path);
    }
    ExitCode::SUCCESS
}

/// Replaces this process with the freshly installed binary. Only returns if exec failed.
fn relaunch(path: &std::path::Path) {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(path)
        .args(relaunch_args(std::env::args().skip(1)))
        .exec();
    eprintln!("drift: could not restart into {}: {err}", path.display());
}

/// Options to carry over into the restarted process. The URL is dropped, the restored
/// session already has that tab, passing it again would open a duplicate.
fn relaunch_args(args: impl Iterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" | "--theme" => {
                out.push(arg);
                out.extend(args.next());
            }
            "--screenshot" => {
                args.next();
            }
            flag if flag.starts_with('-') => out.push(arg),
            _ => {}
        }
    }
    out
}

/// Returns Ok(None) when the arguments asked for help or version and there is nothing to run.
fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<app::Config>, String> {
    let mut url = None;
    let mut restore = true;
    let mut profile: Option<PathBuf> = None;
    let mut screenshot = None;
    let mut theme = None;
    let mut update_check = true;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("drift {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--no-restore" => restore = false,
            "--no-update-check" => update_check = false,
            "--profile" => {
                let dir = args.next().ok_or("--profile needs a directory")?;
                profile = Some(PathBuf::from(dir));
            }
            "--theme" => {
                theme = match args.next().as_deref() {
                    Some("light") => Some(theme::Preference::Light),
                    Some("dark") => Some(theme::Preference::Dark),
                    Some("auto") => None,
                    _ => return Err("--theme takes light, dark or auto".into()),
                };
            }
            // undocumented in --help on purpose, it exists for docs/assets and smoke tests
            "--screenshot" => {
                let path = args.next().ok_or("--screenshot needs a file")?;
                screenshot = Some(PathBuf::from(path));
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return Err(format!("unknown option {flag}"));
            }
            _ if url.is_some() => return Err("only one URL can be given".into()),
            _ => url = Some(arg),
        }
    }

    let (profile_dir, session_path) = match profile {
        Some(dir) => (Some(dir.join("profile")), Some(dir.join("session.json"))),
        None => (app::default_profile_dir(), app::default_session_path()),
    };
    Ok(Some(app::Config {
        url,
        session_path,
        profile_dir,
        restore,
        // screenshots are for docs and tests, they should never talk to the network on their own
        update_check: update_check && screenshot.is_none(),
        screenshot,
        theme,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<app::Config>, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn url_and_flags() {
        let c = parse(&["--no-restore", "servo.org"]).unwrap().unwrap();
        assert_eq!(c.url.as_deref(), Some("servo.org"));
        assert!(!c.restore);
        assert_eq!(c.theme, None);

        let c = parse(&["--theme", "dark"]).unwrap().unwrap();
        assert_eq!(c.theme, Some(theme::Preference::Dark));
    }

    #[test]
    fn profile_dir_holds_everything() {
        let c = parse(&["--profile", "/tmp/p"]).unwrap().unwrap();
        assert_eq!(c.profile_dir, Some(PathBuf::from("/tmp/p/profile")));
        assert_eq!(c.session_path, Some(PathBuf::from("/tmp/p/session.json")));
    }

    #[test]
    fn relaunch_keeps_options_and_drops_url() {
        let args = [
            "--profile",
            "/p",
            "servo.org",
            "--theme",
            "dark",
            "--no-restore",
        ];
        assert_eq!(
            relaunch_args(args.iter().map(|s| s.to_string())),
            ["--profile", "/p", "--theme", "dark", "--no-restore"]
        );
    }

    #[test]
    fn bad_args() {
        assert!(parse(&["--wat"]).is_err());
        assert!(parse(&["--profile"]).is_err());
        assert!(parse(&["a.com", "b.com"]).is_err());
        assert!(parse(&["--help"]).unwrap().is_none());
        assert!(parse(&["--theme", "purple"]).is_err());
    }
}
