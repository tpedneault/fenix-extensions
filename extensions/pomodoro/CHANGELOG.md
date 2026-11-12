# Changelog

## 1.2.0

For Fenix 1.5 (extension API 1.5):

- A round that runs out is a notification, not a line in the message
  line: what comes next, and Enter on it, or a click, starts it. The next
  round's end replaces it, and starting a round puts it away. It still
  chimes (`pomodoro.sound`); a round you skip is still said on the
  message line.
- What it says names the keys of the key scheme in use, and yours when
  you've bound your own: "`C-k o p s` starts it" with standard keys,
  `SPC o p s` with Vim keys, and the agenda's key to add a task.
- The README gives standard keys beside Vim's.

## 1.1.0

- `pomodoro.open` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.1

- `pomodoro.state`, a service other extensions read the timer through: `{"phase": "work"}`, `"break"` or `"off"`. The pets keep still while you work and play in the break.

## 1.0.0

- The first release, for Fenix 1.0: a focus timer: work rounds and breaks counted down on the modeline, today's rounds and the week's -- clocked into the agenda when it's there.
