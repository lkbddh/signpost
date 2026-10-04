# Security policy

## Reporting a vulnerability

Report it privately through [Report a vulnerability](https://github.com/lkbddh/signpost/security/advisories/new) on this
repository. Do not open a public issue.

Include the Signpost version (`signpost --version`, or About in its settings window), how you installed it, and the
steps to reproduce. Fixes ship in a normal release, and the advisory credits you unless you ask otherwise.

## Supported versions

Only the latest release receives fixes.

## Scope

Signpost runs locally and makes no network requests of its own. It is in the path of every web link you open, so these
are all in scope:

- how it parses and passes on links and desktop entries (`Exec` field codes, `TryExec`, terminal apps);
- the apps and browser profiles it launches, and with which arguments;
- its D-Bus interface (`com.lkbddh.signpost`), which any app on your session bus can call;
- what it writes: your `mimeapps.list` (only when you press Use Signpost or Restore defaults), its restore record
  and its own settings;
- the packaging: the `.deb`, the Flatpak and its permissions;
- the dependency tree (`cargo audit`).
