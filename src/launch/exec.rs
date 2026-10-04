//! Desktop Entry `Exec` handling: tokenizing, field-code expansion, profile-argument insertion.
//! Everything stays an argv; nothing here ever builds a command string.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::host::Host;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecError {
    #[error("empty Exec")]
    Empty,
    #[error("unterminated quote in Exec")]
    Unterminated,
    #[error("unknown field code `%{0}`")]
    UnknownFieldCode(char),
    #[error("dangling `%` at end of Exec argument")]
    DanglingPercent,
    #[error("Exec runs a shell that would receive the URL or file")]
    ShellReceivesFieldCode,
}

/// Decode the Desktop Entry string escapes in a raw value: `\s` space, `\n`, `\t`, `\r`, `\\` backslash.
/// Any other `\x`, and a lone trailing backslash, are kept as written.
#[must_use]
pub fn unescape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Split an `Exec` value into an argv. The desktop-entry parser keeps `Exec` raw, so run it through
/// [`unescape_value`] first (the Desktop Entry spec unescapes strings before it unquotes arguments).
/// Double quotes group; inside them `\"`, `` \` ``, `\$` and `\\` are escapes. Single quotes group
/// literally (GLib-compatible leniency). Outside quotes a backslash escapes the next character.
///
/// # Errors
/// [`ExecError::Empty`] when no argument remains, [`ExecError::Unterminated`] on an unclosed quote
/// or a backslash with nothing after it.
pub fn tokenize(exec: &str) -> Result<Vec<String>, ExecError> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            '"' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        None => return Err(ExecError::Unterminated),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '`' | '$' | '\\')) => current.push(e),
                            Some(other) => {
                                current.push('\\');
                                current.push(other);
                            }
                            None => return Err(ExecError::Unterminated),
                        },
                        Some(other) => current.push(other),
                    }
                }
            }
            '\'' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        None => return Err(ExecError::Unterminated),
                        Some('\'') => break,
                        Some(other) => current.push(other),
                    }
                }
            }
            '\\' => {
                in_arg = true;
                match chars.next() {
                    Some(next) => current.push(next),
                    None => return Err(ExecError::Unterminated),
                }
            }
            other => {
                in_arg = true;
                current.push(other);
            }
        }
    }
    if in_arg {
        args.push(current);
    }
    if args.is_empty() {
        Err(ExecError::Empty)
    } else {
        Ok(args)
    }
}

/// Values substituted into field codes. Link mode only ever passes a URL.
pub struct Context<'a> {
    pub uri: &'a str,
    pub icon: Option<&'a str>,
    pub name: &'a str,
    pub desktop_file: Option<&'a Path>,
}

/// True if `token` carries one of `codes` as a field code (`%%u` is a literal, not a code).
fn has_code(token: &str, codes: &[char]) -> bool {
    let mut chars = token.chars();
    while let Some(c) = chars.next() {
        if c == '%' && chars.next().is_some_and(|n| codes.contains(&n)) {
            return true;
        }
    }
    false
}

/// True if any argument carries a `%u`/`%U` field code (`%%u` is a literal, not a code).
#[must_use]
pub fn takes_url(tokens: &[String]) -> bool {
    tokens.iter().any(|t| has_code(t, &['u', 'U']))
}

/// Shells known by name; [`SYSTEM_SHELLS`] adds the host's own. Also wrappers, so they never get
/// profile tiles.
const SHELLS: &[&str] = &[
    "sh", "bash", "rbash", "dash", "ash", "zsh", "ksh", "mksh", "oksh", "loksh", "pdksh", "posh",
    "yash", "fish", "csh", "tcsh", "busybox", "elvish", "nu", "xonsh", "rc", "es",
];

/// Basenames of the shells an `/etc/shells` text lists, read like glibc's `getusershell`: a line's
/// shell path starts at its first `/` (a `#` before it makes the line a comment) and ends at the first
/// whitespace or `#`.
fn listed_shells(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let path = &line[line.find(['/', '#'])?..];
            let end = path
                .find(|c: char| c.is_ascii_whitespace() || c == '#')
                .unwrap_or(path.len());
            Some(program_name(&path[..end]).to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Where this process reads the host's `/etc/shells`: natively `/etc/shells`; in a Flatpak, where the sandbox shows
/// the host's, or `None` when it cannot, as its own is the runtime's.
#[must_use]
pub fn shells_file(host: &Host) -> Option<PathBuf> {
    match host {
        Host::Native => Some(PathBuf::from("/etc/shells")),
        Host::Flatpak(env) => env.read_path(Path::new("/etc/shells")),
    }
}

/// The `/etc/shells` [`SYSTEM_SHELLS`] reads, chosen once at start-up by [`read_shells_from`].
static SHELLS_FILE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Chooses, once and before the index first loads, which `/etc/shells` lists the system's shells: the host's.
pub fn read_shells_from(host: &Host) {
    let _ = SHELLS_FILE.set(shells_file(host));
}

/// The host's `/etc/shells`, read once; missing or unreadable means none.
static SYSTEM_SHELLS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
    // A test process reads none unless a test chose one, as only an isolated child does.
    if cfg!(test) && SHELLS_FILE.get().is_none() {
        return Vec::new();
    }
    let file = SHELLS_FILE
        .get()
        .cloned()
        .unwrap_or_else(|| shells_file(&Host::Native));
    file.and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| listed_shells(&text))
        .unwrap_or_default()
});

/// The program a token may name: its basename, also for a `--command=<program>` value (`--command
/// <program>` is a token of its own).
fn named_program(token: &str) -> &str {
    program_name(token.strip_prefix("--command=").unwrap_or(token))
}

/// `env` given any option: env options are not parsed (clusters such as `-iS`, GNU abbreviations such as
/// `--split-str=`), and some pack a whole command line into one argument, so they count as a shell. When the Exec
/// runs `env` itself, its options come before the command it runs, so only the arguments up to that command count:
/// `env NAME=VALUE… program arg…` is plain whatever the program's own arguments are. Further into a chain (a
/// Flatpak's `--command=env`, whose options follow the app id), any dash argument after it counts.
fn env_with_options(tokens: &[String]) -> bool {
    let Some(at) = tokens.iter().position(|t| named_program(t) == "env") else {
        return false;
    };
    let after = &tokens[at + 1..];
    if at > 0 {
        return after.iter().any(|t| t.starts_with('-'));
    }
    after
        .iter()
        .take_while(|t| {
            t.starts_with('-') || t.split_once('=').is_some_and(|(name, _)| !name.is_empty())
        })
        .any(|t| t.starts_with('-'))
}

/// A shell runs somewhere in the Exec chain: a token names a shell from [`SHELLS`] or `system`, or
/// `env` takes options.
fn runs_a_shell(tokens: &[String], system: &[String]) -> bool {
    env_with_options(tokens)
        || tokens
            .iter()
            .map(|t| named_program(t))
            .any(|program| SHELLS.contains(&program) || system.iter().any(|s| s == program))
}

/// A shell must never receive a URL or a file: expanded into a command string it runs as shell code,
/// and shell flags are too varied to tell a command string from a positional argument (an option's
/// argument, a dropped field code, …). So no URL/file field code anywhere once a shell runs.
fn shell_receives_field_code(tokens: &[String], system: &[String]) -> bool {
    runs_a_shell(tokens, system) && tokens.iter().any(|t| has_code(t, &['u', 'U', 'f', 'F']))
}

/// Expand field codes. `%u`/`%U` become the URI as ONE argument (or substring), unchanged byte for byte.
/// `%f`/`%F` are dropped (link mode never passes files). `%i` → `--icon <icon>` when standalone.
/// `%c` → name, `%k` → desktop file path, `%%` → `%`. Deprecated `%d %D %n %N %v %m` are dropped.
/// Any other code is an error (the entry is then excluded at index time).
///
/// # Errors
/// [`ExecError::UnknownFieldCode`] for a code outside the Desktop Entry set,
/// [`ExecError::DanglingPercent`] for a `%` with nothing after it,
/// [`ExecError::ShellReceivesFieldCode`] when a shell runs and the Exec has a URL/file code.
pub fn expand(tokens: &[String], ctx: &Context) -> Result<Vec<String>, ExecError> {
    expand_written(tokens, tokens, ctx)
}

/// [`expand`] for `tokens`, the Exec its desktop entry `written` with the arguments Signpost inserted (a profile,
/// a private window): the shell check reads only what the entry wrote, as an inserted profile name is data even
/// when it reads like a shell.
///
/// # Errors
/// As [`expand`].
pub fn expand_written(
    written: &[String],
    tokens: &[String],
    ctx: &Context,
) -> Result<Vec<String>, ExecError> {
    if shell_receives_field_code(written, &SYSTEM_SHELLS) {
        return Err(ExecError::ShellReceivesFieldCode);
    }
    let mut out = Vec::with_capacity(tokens.len());
    for token in tokens {
        match token.as_str() {
            "%f" | "%F" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
            "%i" => {
                if let Some(icon) = ctx.icon {
                    out.push("--icon".to_owned());
                    out.push(icon.to_owned());
                }
                continue;
            }
            "%k" => {
                if let Some(path) = ctx.desktop_file {
                    out.push(path.to_string_lossy().into_owned());
                }
                continue;
            }
            _ => {}
        }
        let mut arg = String::with_capacity(token.len());
        let mut chars = token.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                arg.push(c);
                continue;
            }
            match chars.next() {
                Some('u' | 'U') => arg.push_str(ctx.uri),
                Some('c') => arg.push_str(ctx.name),
                Some('k') => {
                    if let Some(path) = ctx.desktop_file {
                        arg.push_str(&path.to_string_lossy());
                    }
                }
                Some('%') => arg.push('%'),
                Some('f' | 'F' | 'i' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
                Some(other) => return Err(ExecError::UnknownFieldCode(other)),
                None => return Err(ExecError::DanglingPercent),
            }
        }
        out.push(arg);
    }
    Ok(out)
}

/// Check every field code is valid without a real URI; an entry that fails is left out of the index.
///
/// # Errors
/// Same as [`expand`].
pub fn validate_codes(tokens: &[String]) -> Result<(), ExecError> {
    expand(
        tokens,
        &Context {
            uri: "",
            icon: None,
            name: "",
            desktop_file: None,
        },
    )
    .map(|_| ())
}

/// Programs that wrap the real browser; profile tiles are refused for them, as for [`SHELLS`].
const WRAPPERS: &[&str] = &[
    "env",
    "flatpak-spawn",
    "snap",
    "firejail",
    "gamemoderun",
    "prime-run",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launcher {
    Native,
    Flatpak { app_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InsertError {
    #[error("Exec already selects a profile")]
    AlreadySelectsProfile,
    #[error("unsupported wrapper `{0}`")]
    UnknownWrapper(String),
    #[error("no application id after `flatpak run`")]
    NoFlatpakAppId,
}

pub(crate) fn program_name(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

fn flatpak_app_index(tokens: &[String]) -> Result<Option<usize>, InsertError> {
    if program_name(&tokens[0]) != "flatpak" {
        return Ok(None);
    }
    if tokens.get(1).map(String::as_str) != Some("run") {
        return Err(InsertError::UnknownWrapper("flatpak".into()));
    }
    tokens
        .iter()
        .enumerate()
        .skip(2)
        .find(|(_, t)| !t.starts_with('-'))
        .map(|(i, _)| Some(i))
        .ok_or(InsertError::NoFlatpakAppId)
}

/// Classify the program an `Exec` runs: a native binary or `flatpak run <appid>`.
///
/// # Errors
/// [`InsertError::UnknownWrapper`] for a wrapper program or a non-`run` flatpak command,
/// [`InsertError::NoFlatpakAppId`] when `flatpak run` names no application.
pub fn launcher(tokens: &[String]) -> Result<Launcher, InsertError> {
    let Some(first) = tokens.first() else {
        return Ok(Launcher::Native);
    };
    if let Some(i) = flatpak_app_index(tokens)? {
        // A ref (`<id>/<arch>/<branch>`) names its app by its first part.
        let app_id = tokens[i].split('/').next().unwrap_or(&tokens[i]);
        return Ok(Launcher::Flatpak {
            app_id: app_id.to_owned(),
        });
    }
    let name = program_name(first);
    if WRAPPERS.contains(&name) || SHELLS.contains(&name) {
        return Err(InsertError::UnknownWrapper(name.to_owned()));
    }
    Ok(Launcher::Native)
}

/// Insert `extra` into the argv: before the first `%u`/`%U`-carrying argument, before any `--`, and for
/// `flatpak run … <appid> … @@u %U @@` after the app id and outside the `@@u … @@` block. `selectors`
/// are the family's profile options; an Exec already using one is refused.
///
/// # Errors
/// [`InsertError::AlreadySelectsProfile`] when an argument is one of `selectors` (bare or `--opt=value`),
/// otherwise the errors of [`launcher`].
pub fn insert_args(
    tokens: &[String],
    extra: &[String],
    selectors: &[&str],
) -> Result<Vec<String>, InsertError> {
    let start = match launcher(tokens)? {
        Launcher::Native => 1,
        Launcher::Flatpak { .. } => flatpak_app_index(tokens)?.map_or(1, |i| i + 1),
    };
    let selects = |t: &String| {
        selectors
            .iter()
            .any(|s| t == s || t.starts_with(&format!("{s}=")))
    };
    if tokens.iter().skip(1).any(selects) {
        return Err(InsertError::AlreadySelectsProfile);
    }
    let pos = tokens
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, t)| {
            t.as_str() == "--"
                || t.as_str() == "@@u"
                || t.as_str() == "@@"
                || takes_url(std::slice::from_ref(*t))
        })
        .map_or(tokens.len(), |(i, _)| i);
    let mut out = Vec::with_capacity(tokens.len() + extra.len());
    out.extend_from_slice(&tokens[..pos]);
    out.extend_from_slice(extra);
    out.extend_from_slice(&tokens[pos..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Vec<String> {
        tokenize(s).unwrap()
    }

    #[test]
    fn unescape_value_decodes_the_five_escapes() {
        assert_eq!(unescape_value(r"a\sb\nc\td\re\\f"), "a b\nc\td\re\\f");
        assert_eq!(unescape_value(r"\\\\"), r"\\");
    }

    #[test]
    fn unescape_value_is_single_pass_left_to_right() {
        assert_eq!(unescape_value(r"\\s"), r"\s");
        assert_eq!(unescape_value(r"\\\s"), r"\ ");
    }

    #[test]
    fn unescape_value_keeps_unknown_and_trailing_backslashes() {
        assert_eq!(unescape_value(r"a\xb\$HOME"), r"a\xb\$HOME");
        assert_eq!(unescape_value(r"\é"), r"\é");
        assert_eq!(unescape_value(r"tail\"), r"tail\");
        assert_eq!(unescape_value("plain é"), "plain é");
        assert_eq!(unescape_value(""), "");
    }

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(t("firefox  %u"), ["firefox", "%u"]);
        assert_eq!(t(" a\tb "), ["a", "b"]);
    }

    #[test]
    fn double_quotes_group_and_unescape() {
        assert_eq!(t(r#""/opt/My App/bin" --x"#), ["/opt/My App/bin", "--x"]);
        assert_eq!(t(r#""a\"b""#), [r#"a"b"#]);
        assert_eq!(t(r#""\$HOME""#), ["$HOME"]);
        assert_eq!(t(r#""\`x\`""#), ["`x`"]);
        assert_eq!(t(r#""back\\slash""#), [r"back\slash"]);
        assert_eq!(t(r#""keep\n""#), [r"keep\n"]);
    }

    #[test]
    fn single_quotes_group_literally() {
        assert_eq!(t("sh -c 'echo %u \"x\"'"), ["sh", "-c", "echo %u \"x\""]);
    }

    #[test]
    fn adjacent_segments_join_one_argument() {
        assert_eq!(t(r#"--name="My Profile"x"#), ["--name=My Profilex"]);
        assert_eq!(t(r#""a"'b'c"#), ["abc"]);
    }

    #[test]
    fn empty_quotes_make_an_empty_argument() {
        assert_eq!(t(r#"app """#), ["app", ""]);
    }

    #[test]
    fn backslash_outside_quotes_escapes_next_char() {
        assert_eq!(t(r"a\ b"), ["a b"]);
    }

    #[test]
    fn escaped_backslash_outside_quotes_is_one_backslash() {
        assert_eq!(t(r"a\\b"), [r"a\b"]);
    }

    #[test]
    fn dangling_backslash_is_unterminated() {
        assert_eq!(tokenize(r"\"), Err(ExecError::Unterminated));
        assert_eq!(tokenize(r"browser arg\"), Err(ExecError::Unterminated));
    }

    #[test]
    fn errors() {
        assert_eq!(tokenize(""), Err(ExecError::Empty));
        assert_eq!(tokenize("   "), Err(ExecError::Empty));
        assert_eq!(tokenize(r#"app "open"#), Err(ExecError::Unterminated));
        assert_eq!(tokenize("app 'open"), Err(ExecError::Unterminated));
    }

    fn ctx(uri: &str) -> Context<'_> {
        Context {
            uri,
            icon: Some("firefox"),
            name: "Firefox Web",
            desktop_file: Some(Path::new("/usr/share/applications/firefox.desktop")),
        }
    }

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    const NASTY: &str = "https://ex.org/a b/%20x?q='1'&r=\"2\"#frag-%u";

    #[test]
    fn url_codes_pass_the_uri_byte_identical_as_one_argument() {
        assert_eq!(
            expand(&v(&["firefox", "%u"]), &ctx(NASTY)).unwrap(),
            v(&["firefox", NASTY])
        );
        assert_eq!(
            expand(&v(&["chrome", "%U"]), &ctx(NASTY)).unwrap(),
            v(&["chrome", NASTY])
        );
    }

    #[test]
    fn embedded_url_code_is_substituted_in_place() {
        assert_eq!(
            expand(&v(&["app", "--url=%u"]), &ctx("https://x/")).unwrap(),
            v(&["app", "--url=https://x/"])
        );
    }

    #[test]
    fn file_codes_are_dropped() {
        assert_eq!(
            expand(&v(&["viewer", "%f", "%F"]), &ctx("https://x/")).unwrap(),
            v(&["viewer"])
        );
    }

    #[test]
    fn icon_name_and_desktop_file_codes() {
        assert_eq!(
            expand(&v(&["app", "%i", "%c", "%k", "%u"]), &ctx("https://x/")).unwrap(),
            v(&[
                "app",
                "--icon",
                "firefox",
                "Firefox Web",
                "/usr/share/applications/firefox.desktop",
                "https://x/"
            ])
        );
        let no_icon = Context {
            icon: None,
            desktop_file: None,
            ..ctx("https://x/")
        };
        assert_eq!(
            expand(&v(&["app", "%i", "%k", "%u"]), &no_icon).unwrap(),
            v(&["app", "https://x/"])
        );
    }

    #[test]
    fn percent_escape_and_deprecated_codes() {
        assert_eq!(
            expand(&v(&["app", "100%%", "%%u", "%d", "%m"]), &ctx("https://x/")).unwrap(),
            v(&["app", "100%", "%u"])
        );
    }

    #[test]
    fn unknown_or_dangling_codes_fail() {
        assert_eq!(
            expand(&v(&["app", "%x"]), &ctx("u")),
            Err(ExecError::UnknownFieldCode('x'))
        );
        assert_eq!(
            expand(&v(&["app", "50%"]), &ctx("u")),
            Err(ExecError::DanglingPercent)
        );
        assert_eq!(
            validate_codes(&v(&["browser", "%u", "%x"])),
            Err(ExecError::UnknownFieldCode('x'))
        );
        assert_eq!(validate_codes(&v(&["browser", "%u"])), Ok(()));
    }

    #[test]
    fn any_url_or_file_code_handed_to_a_shell_is_refused() {
        for unsafe_exec in [
            r#"sh -c "firefox %u""#,
            "bash -lc 'firefox %U'",
            "/bin/dash -ec 'viewer %f'",
            "zsh -o pipefail -c 'x %F'",
            "sh -c %u",
            "sh -c --url=%u",
            "env A=1 sh -c 'firefox %u'",
            "firejail bash --norc -c 'firefox --new-window=%u'",
            "flatpak run --command=sh org.x.App -c 'firefox %u'",
            "flatpak run --command sh org.x.App -c 'firefox %u'",
            // Flag parsing was bypassable: an option argument, a wrapped flatpak, a dropped code.
            r#"bash --rcfile /dev/null -lc "firefox %u""#,
            r#"env A=1 flatpak run --command=sh org.x.App -c "firefox %u""#,
            r#"sh -c %d "firefox %u""#,
            // Positional codes are refused as well: no shell ever receives a URL or a file.
            r#"sh -c 'firefox "$1"' sh %u"#,
            r#"bash -c 'exec "$@"' bash firefox %U"#,
            "sh ./launch.sh %u",
            r#"sh -c 'viewer "$1"' sh %f"#,
            "flatpak run --command=/bin/bash org.x.App %u",
            // Every shell, not only the common ones.
            r#"/usr/bin/rbash -c "printf %%s %u""#,
            r#"rbash -c "printf %%s %u""#,
            "ash -c 'x %u'",
            "/usr/bin/yash -c 'x %u'",
            "oksh -c 'x %u'",
            "loksh -c 'x %u'",
            "pdksh -c 'x %u'",
            "posh -c 'x %u'",
            "busybox wget %u",
            "elvish -c 'x %u'",
            "nu -c 'x %u'",
            "xonsh -c 'x %u'",
            "rc -c 'x %u'",
            "es -c 'x %u'",
            // env's packed command string hides the shell inside one argument.
            r#"env -S 'sh -c "firefox %u"'"#,
            r#"/usr/bin/env --split-string='sh -c "firefox %u"'"#,
            r#"env --split-string 'sh -c "firefox %u"'"#,
            r#"env '-Ssh -c "firefox %u"'"#,
            // Any env option is opaque: clusters, GNU abbreviations, unsets, `--`.
            r#"env -iS 'sh -c "firefox %u"'"#,
            r#"/usr/bin/env '-vSsh -c "firefox %u"'"#,
            r#"env --split-str='sh -c "firefox %u"'"#,
            "env -u X firefox %u",
            "/usr/bin/env -- sh -c 'x %u'",
            r#"flatpak run --command=env org.x.App -S 'sh -c "x %u"'"#,
        ] {
            assert_eq!(
                validate_codes(&t(unsafe_exec)),
                Err(ExecError::ShellReceivesFieldCode),
                "{unsafe_exec}"
            );
            assert_eq!(
                expand(&t(unsafe_exec), &ctx("https://x/$(id)")),
                Err(ExecError::ShellReceivesFieldCode),
                "{unsafe_exec}"
            );
        }
        for safe in [
            "sh -c 'echo 100%%u'",
            "env MOZ_X=1 firefox %u",
            "flatpak run org.mozilla.firefox %u",
            "flatpak run org.mozilla.firefox @@u %u @@",
            "flatpak run --command=firefox org.mozilla.firefox %u",
            "firefox --url=%u",
            "curl -S %u",
        ] {
            assert_eq!(validate_codes(&t(safe)), Ok(()), "{safe}");
        }
    }

    #[test]
    fn shells_listed_in_etc_shells_count_too() {
        let system = listed_shells(
            "# /etc/shells: valid login shells\n/bin/sh\n\n  /usr/bin/tmux  \n/opt/weird/myshell\n",
        );
        assert_eq!(system, ["sh", "tmux", "myshell"]);
        for exec in [
            "/usr/bin/tmux new-session %u",
            "/opt/weird/myshell -c 'firefox %u'",
            "myshell %u",
            "env A=1 /opt/weird/myshell %f",
        ] {
            assert!(shell_receives_field_code(&t(exec), &system), "{exec}");
            assert!(
                !shell_receives_field_code(&t(exec), &[]),
                "{exec} without the list"
            );
        }
        for exec in [
            "env MOZ_X=1 firefox %u",
            "flatpak run org.mozilla.firefox %u",
        ] {
            assert!(!shell_receives_field_code(&t(exec), &system), "{exec}");
        }
        assert_eq!(listed_shells(""), Vec::<String>::new());
    }

    #[test]
    fn etc_shells_is_parsed_like_glibc_getusershell() {
        let system = listed_shells(
            "#comment\n/opt/myshell # custom\n/opt/tabsh\t#x\n/opt/hashsh#x\nbare-name\n/\n\r\n",
        );
        assert_eq!(system, ["myshell", "tabsh", "hashsh"]);
        for exec in ["myshell -c 'x %u'", "/opt/tabsh %u", "hashsh %f"] {
            assert!(shell_receives_field_code(&t(exec), &system), "{exec}");
        }
        assert!(
            !shell_receives_field_code(&v(&["app", "", "%u"]), &system),
            "no empty basename from a bare `/`"
        );
    }

    #[test]
    fn takes_url_detection() {
        assert!(takes_url(&v(&["firefox", "%u"])));
        assert!(takes_url(&v(&["app", "--url=%U"])));
        assert!(!takes_url(&v(&["viewer", "%f"])));
        assert!(!takes_url(&v(&["app", "%%u"])));
        assert!(!takes_url(&v(&["app"])));
    }

    #[test]
    fn launcher_detection() {
        assert_eq!(
            launcher(&v(&["/usr/bin/google-chrome-stable", "%U"])),
            Ok(Launcher::Native)
        );
        assert_eq!(
            launcher(&v(&[
                "/usr/bin/flatpak",
                "run",
                "--branch=stable",
                "--command=firefox",
                "--file-forwarding",
                "org.mozilla.firefox",
                "@@u",
                "%u",
                "@@"
            ])),
            Ok(Launcher::Flatpak {
                app_id: "org.mozilla.firefox".into()
            })
        );
        assert_eq!(
            launcher(&v(&["env", "MOZ_X=1", "firefox", "%u"])),
            Err(InsertError::UnknownWrapper("env".into()))
        );
        assert_eq!(
            launcher(&v(&["flatpak", "info", "x"])),
            Err(InsertError::UnknownWrapper("flatpak".into()))
        );
        assert_eq!(
            launcher(&v(&["flatpak", "run", "--branch=stable"])),
            Err(InsertError::NoFlatpakAppId)
        );
    }

    #[test]
    fn inserts_before_url_code_natively() {
        assert_eq!(
            insert_args(
                &v(&["/usr/bin/google-chrome-stable", "%U"]),
                &v(&["--profile-directory=Profile 1"]),
                &["--profile-directory"]
            ),
            Ok(v(&[
                "/usr/bin/google-chrome-stable",
                "--profile-directory=Profile 1",
                "%U"
            ]))
        );
    }

    #[test]
    fn inserts_after_flatpak_app_id_outside_forwarding_block() {
        let exec = v(&[
            "/usr/bin/flatpak",
            "run",
            "--branch=stable",
            "--command=firefox",
            "--file-forwarding",
            "org.mozilla.firefox",
            "@@u",
            "%u",
            "@@",
        ]);
        assert_eq!(
            insert_args(&exec, &v(&["-P", "Work Stuff"]), &["-P"]),
            Ok(v(&[
                "/usr/bin/flatpak",
                "run",
                "--branch=stable",
                "--command=firefox",
                "--file-forwarding",
                "org.mozilla.firefox",
                "-P",
                "Work Stuff",
                "@@u",
                "%u",
                "@@"
            ]))
        );
    }

    #[test]
    fn inserts_before_double_dash_and_embedded_url() {
        assert_eq!(
            insert_args(&v(&["b", "--", "%u"]), &v(&["-X"]), &[]),
            Ok(v(&["b", "-X", "--", "%u"]))
        );
        assert_eq!(
            insert_args(&v(&["b", "--url=%u"]), &v(&["-X"]), &[]),
            Ok(v(&["b", "-X", "--url=%u"]))
        );
        assert_eq!(
            insert_args(&v(&["b"]), &v(&["-X"]), &[]),
            Ok(v(&["b", "-X"]))
        );
    }

    #[test]
    fn refuses_existing_profile_selector_and_wrappers() {
        assert_eq!(
            insert_args(
                &v(&["chrome", "--profile-directory=Default", "%U"]),
                &v(&["--profile-directory=P"]),
                &["--profile-directory"]
            ),
            Err(InsertError::AlreadySelectsProfile)
        );
        assert_eq!(
            insert_args(&v(&["firefox", "-P", "x", "%u"]), &v(&["-P", "y"]), &["-P"]),
            Err(InsertError::AlreadySelectsProfile)
        );
        assert_eq!(
            insert_args(
                &v(&["env", "A=1", "firefox", "%u"]),
                &v(&["-P", "y"]),
                &["-P"]
            ),
            Err(InsertError::UnknownWrapper("env".into()))
        );
    }

    #[test]
    fn a_flatpak_reads_the_hosts_shells_and_rejects_a_url_for_one_only_the_host_lists() {
        let root = tempfile::tempdir().unwrap();
        let host_shells = root.path().join("os/etc/shells");
        std::fs::create_dir_all(host_shells.parent().unwrap()).unwrap();
        std::fs::write(&host_shells, "/bin/sh\n/usr/local/bin/myshell\n").unwrap();
        let sandbox_shells = root.path().join("rest/etc/shells");
        std::fs::create_dir_all(sandbox_shells.parent().unwrap()).unwrap();
        std::fs::write(&sandbox_shells, "/bin/sh\n").unwrap();
        let env = crate::host::HostEnv::parse(b"HOME=/home/u\0", Path::new("/sandbox/home"))
            .mounted_under(root.path());
        let file = shells_file(&crate::host::Host::Flatpak(env)).expect("the host's shells");
        assert_eq!(file, host_shells, "the host's list, not the sandbox's");
        assert_eq!(
            shells_file(&crate::host::Host::Native),
            Some(std::path::PathBuf::from("/etc/shells"))
        );
        let system = listed_shells(&std::fs::read_to_string(file).unwrap());
        let exec = t("myshell -c 'open %u'");
        assert!(
            shell_receives_field_code(&exec, &system),
            "rejected at index validation and launch"
        );
        assert!(
            !shell_receives_field_code(&exec, &listed_shells("/bin/sh\n")),
            "the sandbox's list misses it"
        );
    }

    /// Set in the child that checks both consumers, to the fixture tree its host is shown under.
    const SHELLS_CHILD: &str = "SIGNPOST_HOST_SHELLS_FIXTURE";

    #[test]
    fn index_validation_and_launch_expansion_both_reject_a_url_for_a_shell_only_the_host_lists() {
        if let Some(root) = std::env::var_os(SHELLS_CHILD) {
            let env = crate::host::HostEnv::parse(b"HOME=/home/u\0", Path::new("/sandbox/home"))
                .mounted_under(Path::new(&root));
            read_shells_from(&crate::host::Host::Flatpak(env.clone()));
            let registry = crate::index::Registry::load_in(
                &env,
                &[std::path::PathBuf::from("/home/u/apps")],
                &["en".into()],
            );
            let entry = registry.get("shelled").expect("the entry is indexed");
            assert_eq!(
                entry.exec_error,
                Some(ExecError::ShellReceivesFieldCode),
                "index validation"
            );
            assert_eq!(
                expand(&t("myshell -c 'xdg-open %u'"), &ctx("https://x/")),
                Err(ExecError::ShellReceivesFieldCode),
                "launch expansion"
            );
            return;
        }
        // A child of its own: the shells are chosen once a process, here the host's of this fixture.
        let root = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("os/etc/shells", "/bin/sh\n/usr/local/bin/myshell\n");
        write("rest/etc/shells", "/bin/sh\n");
        write(
            "rest/home/u/apps/shelled.desktop",
            "[Desktop Entry]\nType=Application\nName=Shelled\nExec=myshell -c \"xdg-open %u\"\n",
        );
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "launch::exec::tests::index_validation_and_launch_expansion_both_reject_a_url_for_a_shell_only_the_host_lists",
            ])
            .env(SHELLS_CHILD, root.path())
            .output()
            .unwrap();
        let report = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{report}");
        assert!(
            report.contains("1 passed"),
            "the check did not run:\n{report}"
        );
    }

    #[test]
    fn a_flatpak_named_by_its_ref_is_the_app_of_that_ref() {
        assert_eq!(
            launcher(&v(&[
                "flatpak",
                "run",
                "org.mozilla.firefox/x86_64/stable",
                "%u"
            ])),
            Ok(Launcher::Flatpak {
                app_id: "org.mozilla.firefox".into()
            })
        );
    }

    #[test]
    fn env_options_count_only_before_the_command_env_runs() {
        assert_eq!(
            validate_codes(&v(&["env", "MOZ_X=1", "firefox", "--new-window", "%u"])),
            Ok(())
        );
        for shell in [
            &["env", "-S", "sh -c x", "%u"][..],
            &["env", "-i", "firefox", "%u"],
            &["env", "--", "firefox", "%u"],
            &["/usr/bin/env", "A=1", "-u", "B", "firefox", "%u"],
        ] {
            assert_eq!(
                validate_codes(&v(shell)),
                Err(ExecError::ShellReceivesFieldCode),
                "{shell:?}"
            );
        }
    }
}
