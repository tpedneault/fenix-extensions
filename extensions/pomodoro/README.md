# Pomodoro

A focus timer for Fenix: a work round, then a break, and a long break
every few rounds. The round counts down on the modeline -- `🍅 18:42`
while you work, `☕ 04:12` on a break -- and Fenix chimes when it's
over and says what comes next.

`SPC o p s` starts a round. A break starts by itself when the work
before it ends (`pomodoro.auto_breaks`); the next work round waits for
you. Rounds are kept against the clock, so one carries on while Fenix is
closed, and one that ran out meanwhile is counted.

| Key | What it does |
| --- | --- |
| `SPC o p s` | Start the round, or go on with a paused one |
| `SPC o p p` | Pause it |
| `SPC o p n` | Skip to the next round (a skipped work round isn't counted) |
| `SPC o p r` | Back to the first round |
| `SPC o p o` | The page: the round, today's rounds, the week's |
| `SPC o p t` | The agenda task work rounds clock into |

## The page

The round under way and how far it's gone, what comes after it, today's
work rounds (when each ended, how long, the task), and the last seven
days. Four weeks are kept, in `pomodoro.json`.

| Key | What it does |
| --- | --- |
| `s` | Start, or pause |
| `n` | Skip |
| `r` | Reset |
| `t` | Pick the agenda task |

## With the agenda

With the Agenda extension, a work round runs the agenda's clock:

- If its clock is already running, the round is for that task, and the
  clock is left as it is.
- If it isn't, the round clocks into the task picked with `SPC o p t`
  (in progress or to do), and the clock stops when the round ends, is
  paused or reset.

Today's rounds on the page name the task each was for.
`pomodoro.agenda` turns it off.

## Settings

| Setting | What it does |
| --- | --- |
| `pomodoro.work` | A work round, in minutes (25) |
| `pomodoro.short_break` | A short break (5) |
| `pomodoro.long_break` | A long break (15) |
| `pomodoro.long_every` | Work rounds to a long break (4) |
| `pomodoro.auto_breaks` | Whether a break starts by itself when work ends |
| `pomodoro.agenda` | Whether work rounds clock into the agenda |
| `pomodoro.sound` | Whether a round that runs out chimes (on) |
| `pomodoro.unfocused` | Whether to warn when you wander off during work (off) |

A setting changed mid-round applies from the next round.

## The "unfocused" warning

With `pomodoro.unfocused` on, going to a page during a work round -- another
extension's, Fenix's settings or Home -- says so on the message line,
once a round: `Round 2 has 18:42 to go -- this page isn't your work`.
Files are your work, whichever project they're in (it isn't told which
file, only that it's one), and so are terminals, the explorer and its own
page. A break, or a round paused, never warns.

## What it may do

- Play its own chime (`sound = true`), `sounds/chime.wav`: two sine
  tones, made for it.
- Be told what has the focus (`events = ["focus"]`): a file, text, or a
  page and its kind -- not which file.
