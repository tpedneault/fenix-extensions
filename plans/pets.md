# Pets: what's next

The plan for growing the Pets extension, most worthwhile first. Each step
should be a release that stands on its own.

## Done in 1.1.0

- **Rubber duck debugging**: the duck sits by the caret and listens for
  the whole debug session, and quacks at breakpoints (`debug` events).
- **Greetings**: they look up when you're back after 90 seconds away
  (`active`), and one looks round when you change files (`focus`).
- **Livelier**: more walking, shorter sits and idles, social play every
  25–75 s instead of 40–120 s, and the zoomies when happy.
- **Fenix 1.0.1 fix**: model frames are drawn at the rate the extension
  asks for. Before, they were drawn only when something else redrew.

## 1.2: quick wins, with today's API

1. **Stop resending an unchanged layer.** `draw()` sends the whole layer
   every 200 ms tick, so each send makes Fenix redraw, about 5 times a
   second even while every pet is asleep. Keep the last `Vec<Item>` and
   call `layer::set` only when it differs. This saves battery, and it
   matters more now that a layer's own frame rate is drawn properly.
2. **Follow the caret.** `Layout` already carries the caret. Now and
   then the nearest free pet trots after it along the modeline, and the
   others turn to face it (`towards_caret` is there and unused for this).
3. **A liveliness setting.** `pets.liveliness = calm | normal | lively`
   scales the wander odds and the social and visit intervals. Calm is
   roughly how 1.0 behaved.
4. **Quiet hours.** After 22:00 local time (`host::utc_offset`), they
   yawn more often and nap after 1 minute idle instead of 3.
5. **Warnings, not just errors.** `diagnostics` already reports warnings.
   A pet could tilt its head (`Look`) when warnings appear, without
   hiding the way it does for errors.

## 1.3: reactions that need a little from Fenix

These need small additions to the extension API (Fenix 1.1, API 1.1).

1. **Git.** A `git` event kind: committed, pushed, pulled, conflict.
   Commit: confetti from the nearest pet. Push: they wave it off. A merge
   conflict: the cat knocks something off a ledge and they hide until
   it's resolved.
2. **Coding agents.** `agent_turn_ended` exists, but it needs
   `permissions.agents`, which also allows `agents::ask`. That's too much
   to ask of a fun extension. Add a read-only `events = ["agents"]` that
   needs no permission and carries no text, only the agent and the stop
   reason. Then a pet watches the agent dock while a turn runs, cheers
   at `end_turn`, and Ember singes at `error`.
3. **Test counts.** `task-ended(bool)` becomes passed/failed counts, so
   the reaction can scale: a small cheer for 1 test, a party for 500.
4. **A build started.** `task_started` is already a kind. A pet could
   sit and watch a long build, and fall asleep if it runs past 2 minutes.

## 1.4: new friends and toys

- **New kinds, original designs**: an owl (awake late, asleep in the
  morning), a snail (slow, follows long builds), a fox. Each needs a
  `.glb` with the shared clips (`idle look walk run sit_down sit
  stand_up lie_down sleep wake jump happy hide stretch`) plus a signature
  clip or two, like the duck's `quack` and Ferris's `snap`.
- **Toys**: a yarn ball for the cat, a bath for the duck, and a log for
  Ember to warm the others by in December.
- **More play between them**: races between the duck and Ferris, Ember
  warming the others when it snows, and grooming when they're sitting
  together.
- **Pomodoro**: if `fenix.pomodoro` is installed, they keep still during
  a focus round and play during the break (through its service, as the
  agenda does).

## Polish, anytime

- An animated screenshot or GIF for the store page: the debugging duck
  and the zoomies.
- Check that the pixel style's 12 fps still reads as "chunky" now that it
  actually happens. 15 fps may be the better number.
- Their home page (`pets.home`): a log of the day ("Quackers helped
  debug for 14 min").

Keep the care mechanics light: no hunger, and nothing that makes you
feel guilty for not coming back.
