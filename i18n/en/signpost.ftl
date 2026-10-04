app-name = Signpost
copy-link = Copy link
open = Open
open-keep = Open and keep picker
no-apps = No app can open this link
no-apps-body = Install a browser, or copy the link and open it yourself.
private-window = Open in private window
pin-tooltip = Keep chooser open after opening a link
pin-on = Keep chooser open after opening a link: on
pin-off = Keep chooser open after opening a link: off
link-copied = Link copied
not-secure = Not secure
failure-title = Couldn't open this link in { $tile }
failure-title-unnamed = Couldn't open this link
failure-hint = Try again or choose another app.
next-link = Next
links-waiting =
    { $count ->
        [one] { $count } link waiting
       *[other] { $count } links waiting
    }
skip-to-next = Close this link and show the next
early-failure = { $program } exited early (code { $code }).
handler-none = None
handler-mixed = Mixed
setup-overridden-title = Another settings file overrides this change
setup-overridden-body = You can put back the defaults you had before Signpost.
setup-task-failed = The setup task failed
setup-write-failed = Couldn't save { $path }: { $reason }
setup-read-failed = Couldn't read { $path }: { $reason }
setup-nothing-to-restore = There's no previous browser to restore
setup-path-changed = Signpost was set up in { $recorded }. Restore the previous browser before using Signpost in { $current }.
setup-unresolvable = Couldn't follow { $path } ({ $reason }). Fix or remove that link, then try again.
setup-link-changed = { $path } now leads to { $current }, not { $recorded } where Signpost was set up. Point it back to restore the previous browser.
setup-list-unreachable = Signpost's sandbox can't reach your mimeapps.list. Let it with: flatpak override --user --filesystem=<its folder> com.lkbddh.signpost
record-unreadable-title = Saved defaults couldn't be read
record-unreadable-body = Signpost can't change your web browser or restore the previous one until this is fixed. Repair the file named in the details, or remove it to forget the previous browser.
toast-links-waiting = Too many links are waiting
file = File
open-test-link = Open a test link
keyboard-shortcuts = Keyboard shortcuts
about-signpost = About Signpost
default-handler = Web browser
use-signpost = Use Signpost
restore-defaults = Restore defaults
toast-app-default = { $name } now opens your web links
toast-scheme-app = { $scheme } links now open in { $name }
toast-scheme-none = { $scheme } links have no default
toast-cleared = Web-link defaults cleared
apps-in-picker = Apps in the picker
apps-empty = No apps available for web links. Install a browser to get started.
profile-count =
    { $count ->
        [one] { $count } profile
       *[other] { $count } profiles
    }
undo = Undo
show-details = Show details
hide-details = Hide details
shortcut-select = Select an app or profile
shortcut-numbered = Open a numbered tile
shortcut-open = Open the selection
shortcut-keep = Open and keep the picker
shortcut-copy = Copy the link
shortcut-close = Close the picker
key-arrows = Arrow keys
key-digits = 1–9
key-enter = Enter
key-ctrl = Ctrl
key-c = C
key-esc = Esc
shortcuts-footnote = Right-click, press Menu, or touch and hold a tile for more actions. Ctrl+click or middle-click also keeps the picker open.
about-version = Version
about-source = Source code
about-issues = Report an issue
about-issues-link = Open an issue
about-license = License
drawer-caption = { $id } · { $kind }
app-native = Native
app-flatpak = Flatpak
drawer-profiles = Profiles
drawer-menu = In the picker menu
menu-private = Private window
menu-available = Available
menu-desktop-action = Desktop action
reset-color = Reset
color-of = Color of { $profile }
color-save-failed = Couldn't save the color: { $reason }
color-store-missing = the settings folder cannot be used
