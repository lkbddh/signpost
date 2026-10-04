pub mod app;
pub mod bus;
pub mod cli;
pub mod colors;
pub mod frost;
pub mod host;
pub mod icons;
pub mod index;
pub mod launch;
pub mod localize;
mod native_focus;
pub mod picker;
pub mod picker_view;
pub mod profiles;
pub mod settings;
pub mod setup;
#[cfg(test)]
mod test_support;
mod widget_bounds;

use std::ffi::OsString;
use std::process::ExitCode;

/// Entry point used by `main`: election first; only a winner needs Wayland.
pub fn run(args: &[OsString]) -> ExitCode {
    match cli::parse(args) {
        Ok(cli::Cli::Help) => {
            println!("{}", cli::HELP);
            ExitCode::SUCCESS
        }
        Ok(cli::Cli::Version) => {
            println!("{}", version_line());
            ExitCode::SUCCESS
        }
        Ok(cli::Cli::HostReport) => {
            print!("{}", host::report(&host::detect()));
            ExitCode::SUCCESS
        }
        Ok(cli::Cli::Run(role)) => {
            init_tracing();
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("signpost: cannot start async runtime: {e}");
                    return ExitCode::from(1);
                }
            };
            let token = std::env::var("XDG_ACTIVATION_TOKEN")
                .ok()
                .filter(|t| !t.is_empty());
            let (role, refused) = match role {
                cli::Role::Service => (bus::Role::Service, 0),
                cli::Role::Starter { uris } => {
                    let (uris, refused) = bus::admit_starter_links(uris);
                    (bus::Role::Starter { uris, token }, refused)
                }
            };
            if refused > 0 {
                eprintln!(
                    "signpost: {refused} links were not opened: at most {} can wait",
                    picker::MAX_WAITING_LINKS
                );
            }
            let outcome = runtime
                .block_on(async { bus::elect(zbus::connection::Builder::session()?, role).await });
            let code = match outcome {
                Ok(bus::Outcome::Forwarded | bus::Outcome::LostQuietly) => ExitCode::SUCCESS,
                Ok(bus::Outcome::Won(daemon)) => {
                    match cli::require_wayland(std::env::var_os("WAYLAND_DISPLAY")) {
                        Ok(wayland_display) => {
                            tracing::info!(%wayland_display, "signpost daemon ready");
                            // Before the app admits work, so links that already arrived wait for it.
                            app::run(daemon, runtime, host::detect())
                        }
                        Err(e) => {
                            tracing::error!("{e}; see README (dbus-update-activation-environment)");
                            eprintln!("signpost: {e}");
                            runtime.block_on(daemon.fail());
                            ExitCode::from(1)
                        }
                    }
                }
                Err(e) => {
                    eprintln!("signpost: {e}");
                    ExitCode::from(1)
                }
            };
            failing_when_refused(code, refused)
        }
        Err(e) => {
            eprintln!("signpost: {e}\n\n{}", cli::HELP);
            ExitCode::from(2)
        }
    }
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,signpost=info")),
        )
        .with_writer(std::io::stderr)
        .try_init();
}

/// A starter that refused links fails, even when the rest were opened.
fn failing_when_refused(code: ExitCode, refused: usize) -> ExitCode {
    if refused == 0 {
        return code;
    }
    ExitCode::FAILURE
}

/// What `signpost --version` prints.
#[must_use]
pub fn version_line() -> String {
    format!(
        "signpost {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("SIGNPOST_COMMIT")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_names_the_commit_it_was_built_from() {
        let line = version_line();
        let commit = line
            .strip_prefix(&format!("signpost {} (", env!("CARGO_PKG_VERSION")))
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or_else(|| panic!("no commit in {line:?}"));
        let hash = commit.strip_suffix("-dirty").unwrap_or(commit);
        assert!(
            hash == "unknown" || (hash.len() == 12 && hash.chars().all(|c| c.is_ascii_hexdigit())),
            "{line:?} names no commit"
        );
    }

    #[test]
    fn a_starter_that_refused_links_fails_whatever_else_happened() {
        assert_eq!(
            failing_when_refused(ExitCode::SUCCESS, 0),
            ExitCode::SUCCESS
        );
        assert_eq!(
            failing_when_refused(ExitCode::SUCCESS, 3),
            ExitCode::FAILURE
        );
        assert_eq!(
            failing_when_refused(ExitCode::FAILURE, 3),
            ExitCode::FAILURE
        );
    }
}
