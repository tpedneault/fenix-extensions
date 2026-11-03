# Pets: what's next

The plan for growing the Pets extension, most worthwhile first. Each step
should be a release that stands on its own.

## Done in 1.0.2

Everything below needs only extension API 1.0.

- **Rubber duck debugging**: the duck sits by the caret and listens for
  the whole debug session, and quacks at breakpoints.
- **Greetings**: they look up when you're back after 90 s away, and one
  looks round when you change files. New warnings get a tilt of the head.
- **Livelier**: more walking, shorter sits and idles, social play every
  25–75 s, the zoomies, races, and trotting over to the caret.
- **`pets.liveliness`**: calm, normal or lively.
- **Quiet hours**: from 22:00 to 06:00 they nap after 1 minute idle,
  except the owl.
- **New friends**: an owl (hoots, stays up late), a fox (stalks and
  leaps), a snail (slow, hides in its shell). They come from the modeler,
  like the others.
- **Toys and play**: a ball of yarn for the cat, and in the snow they
  gather round Ember to keep warm.
- **Pomodoro**: still during a work round, play in the break. This uses
  Pomodoro 1.0.1's new `pomodoro.state` service.
- **Fewer redraws**: an unchanged layer isn't sent again.
- **Fenix 1.0.1 fix**: model frames are drawn at the rate the extension
  asks for.

## 1.1: reactions that need extension API 1.1 (proposed)

**The constraint.** Extension API 1.0 is one WIT world. Fenix's bindings
(`Extension::new`) need every export the world declares, and the
`export!` macro provides all of 1.0's. So these break every installed 1.0
extension:

- a new **export** (`git-changed`, ...);
- a new case in the **`event`** variant;
- a changed record (for example, test counts added to `task-ended`).

Breaking them all means the whole store has to be rebuilt.
**New host imports are safe**: a 1.0 component doesn't import them, and
`api = "1.1"` keeps a 1.1 extension off an older Fenix. So API 1.1 adds
only imports, and the pets ask for what's new.

1. **Build started**: no API change. `task-started` is already an export
   (`events = ["tasks"]`). A pet sits and watches a long build, and falls
   asleep if it runs past 2 minutes.
2. **Coding agents**: a policy change in Fenix, no API change.
   `events = ["agents"]` without `permissions.agents` gets
   `agent-turn-ended` with the `text` and `title` emptied. That's enough
   for a reaction and reveals nothing of what was said. A pet watches
   the dock during a turn, cheers at `end_turn`, and Ember singes at
   `error`.
3. **Git**: a new import, `git-state() -> option<git-state>`, with
   branch, head, ahead/behind and conflicted files. Allowed with
   `events = ["git"]`. The pets poll it every 2 s and infer what changed:
   a new head on the same branch is a commit (confetti), ahead dropping
   to 0 is a push (they wave it off), conflicts appearing is a merge
   conflict (they hide until they're resolved).
4. **Test counts**: a new import, `last-task() -> option<task-result>`,
   with name, ok, and passed/failed when Fenix recognises the output
   (cargo, pytest, Jest/Vitest, go test). The pets call it when
   `task-ended` arrives, so the celebration scales: a small cheer for 1
   test, a party for 500.

Decisions this needs:

- **The Fenix version that ships API 1.1.** By semver that's Fenix
  1.1.0, not 1.0.x.
- **Whether git state needs consent.** It's gated by `events` (no
  prompt), like the other editor events, or by a permission.

## Polish, anytime

- An animated screenshot or GIF for the store page: the debugging duck
  and the zoomies.
- Check that the pixel style's 12 fps still reads as "chunky" now that it
  actually happens. 15 fps may be the better number.
- Their home page (`pets.home`): a log of the day ("Quackers helped
  debug for 14 min").

Keep the care mechanics light: no hunger, and nothing that makes you
feel guilty for not coming back.
