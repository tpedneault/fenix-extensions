# VNC

Remote desktops in a pane.
List the machines you use in `vnc.hosts`, and `SPC v v` opens one -- a
VM's console, a lab PC -- in the pane you're in. Each stays connected in
the background once opened, shown or not, until you close it.

- **The keyboard**: its pane takes every key, Caps Lock and the
  modifiers too (Ctrl-C, Alt-Tab and Ctrl-Alt-F2 go to the guest), until
  `Ctrl+\` gives them back; a click takes them again.
- **The pointer**: the guest's own, drawn by Fenix where the pointer is;
  its buttons and its wheel.
- **The clipboard**: what the guest copies is on yours; yours goes to the
  guest when its pane takes the keyboard, so its own paste keys paste it.
- **Its size**: a server that can be resized (ExtendedDesktopSize) is
  asked to be the pane's size, and asked again when the pane changes;
  one that can't is stretched to fill it.
- **A dropped connection** says so on the pane and is dialled again,
  backing off (1, 2, 4, 8, 16 seconds), six times; then `SPC v v` on it
  tries again.
- **The bell** is said in the message line.

It speaks RFB 3.3, 3.7 and 3.8, with no authentication or VNC
Authentication (with an empty password), and the ZRLE, CopyRect
and Raw encodings.

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC v v` | `vnc.open` | A picker over `vnc.hosts`: connect to one, or bring it here |
| `SPC v q` | `vnc.close` | Close the session you're in |
| `SPC v s` | `vnc.screenshot` | Save its picture as a PNG |

`vnc.open` may run from outside Fenix too (`fenix/runCommand`). A
`[workspaces]` entry `vnc:NAME` opens host `NAME`.

## Settings

| Setting | What it is |
| --- | --- |
| `vnc.hosts` | `[[vnc.hosts]]` tables of `name`, `host` and `port` (5900 when it's left out) |
| `vnc.screenshots` | Where `SPC v s` saves pictures; empty keeps them in Fenix's data folder |

```toml
[[vnc.hosts]]
name = "build-vm"
host = "10.0.0.5"

[[vnc.hosts]]
name = "test-vm"
host = "10.0.0.6"
port = 5901
```

No passwords: every host is taken to be on a trusted network.

## What it may do

- Open connections to the hosts in `vnc.hosts`, and to any host on port
  5900.
- Read your clipboard, when one of its panes takes the keyboard.
- Write into its screenshots folder.

## Building

```text
cargo xtask pack extensions/vnc dist
```

`cargo test` runs the decoder and the viewer against a fake RFB server
(`tests/fake_server.rs`).
