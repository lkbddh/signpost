use std::ffi::OsString;

pub const HELP: &str = "Usage: signpost [--service | <url>…]\n\n\
  (no arguments)  open the settings window, starting the daemon if needed\n\
  --service       start the daemon (used by D-Bus activation)\n\
  <url>…          open http(s) links with the Signpost chooser\n\
  --host-report   print where a Flatpak Signpost finds the host's files\n\
  --help, --version";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// Started by D-Bus activation (`--service`).
    Service,
    /// Started by a person or another program, possibly with URIs.
    Starter { uris: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cli {
    Run(Role),
    Help,
    Version,
    /// Print where the host's files are found, and stop.
    HostReport,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CliError {
    #[error("unknown option `{0}`")]
    UnknownOption(String),
    #[error("`--service` takes no other arguments")]
    ServiceWithArguments,
    #[error("argument is not valid UTF-8")]
    NotUtf8,
    #[error("a native Wayland session is required (WAYLAND_DISPLAY is not set)")]
    NoWayland,
}

/// Parses the command line (without `argv[0]`).
///
/// # Errors
///
/// Returns a [`CliError`] for non-UTF-8 arguments, unknown options, or `--service`
/// combined with other arguments.
pub fn parse(args: &[OsString]) -> Result<Cli, CliError> {
    let args: Vec<&str> = args
        .iter()
        .map(|a| a.to_str().ok_or(CliError::NotUtf8))
        .collect::<Result<_, _>>()?;
    match args.as_slice() {
        ["--help" | "-h"] => return Ok(Cli::Help),
        ["--version"] => return Ok(Cli::Version),
        ["--host-report"] => return Ok(Cli::HostReport),
        ["--service"] => return Ok(Cli::Run(Role::Service)),
        _ => {}
    }
    if args.contains(&"--service") {
        return Err(CliError::ServiceWithArguments);
    }
    if let Some(opt) = args.iter().find(|a| a.starts_with('-')) {
        return Err(CliError::UnknownOption((*opt).to_owned()));
    }
    Ok(Cli::Run(Role::Starter {
        uris: args.into_iter().map(str::to_owned).collect(),
    }))
}

/// Returns the `WAYLAND_DISPLAY` value, rejecting an unset, empty or non-UTF-8 one.
///
/// # Errors
///
/// Returns [`CliError::NoWayland`] when there is no usable Wayland display name.
pub fn require_wayland(display: Option<OsString>) -> Result<String, CliError> {
    match display.and_then(|d| d.into_string().ok()) {
        Some(d) if !d.is_empty() => Ok(d),
        _ => Err(CliError::NoWayland),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_is_a_starter_without_uris() {
        assert_eq!(
            parse(&os(&[])),
            Ok(Cli::Run(Role::Starter { uris: vec![] }))
        );
    }

    #[test]
    fn service_flag_is_service_role() {
        assert_eq!(parse(&os(&["--service"])), Ok(Cli::Run(Role::Service)));
    }

    #[test]
    fn service_with_uris_is_rejected() {
        assert_eq!(
            parse(&os(&["--service", "https://a"])),
            Err(CliError::ServiceWithArguments)
        );
    }

    #[test]
    fn positional_arguments_are_uris_verbatim() {
        let uri = "https://example.org/a b?c=%20&d='\"#frag";
        assert_eq!(
            parse(&os(&[uri, "http://x"])),
            Ok(Cli::Run(Role::Starter {
                uris: vec![uri.into(), "http://x".into()]
            }))
        );
    }

    #[test]
    fn help_and_version() {
        assert_eq!(parse(&os(&["--help"])), Ok(Cli::Help));
        assert_eq!(parse(&os(&["-h"])), Ok(Cli::Help));
        assert_eq!(parse(&os(&["--version"])), Ok(Cli::Version));
    }

    #[test]
    fn host_report() {
        assert_eq!(parse(&os(&["--host-report"])), Ok(Cli::HostReport));
    }

    #[test]
    fn unknown_option_is_rejected() {
        assert_eq!(
            parse(&os(&["--bogus"])),
            Err(CliError::UnknownOption("--bogus".into()))
        );
    }

    #[test]
    fn non_utf8_argument_is_rejected() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![0x66, 0xff]);
        assert_eq!(parse(&[bad]), Err(CliError::NotUtf8));
    }

    #[test]
    fn wayland_display_required() {
        assert_eq!(require_wayland(None), Err(CliError::NoWayland));
        assert_eq!(
            require_wayland(Some(OsString::new())),
            Err(CliError::NoWayland)
        );
        assert_eq!(
            require_wayland(Some("wayland-1".into())),
            Ok("wayland-1".into())
        );
    }
}
