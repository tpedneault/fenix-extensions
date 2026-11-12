# Changelog

## 1.3.0

- For Fenix 1.5 (extension API 1.5): what the pets say names the keys
  of the key scheme in use, and yours when you've bound your own --
  "`C-k P a` adopts one" with standard keys, `SPC P a` with Vim keys, and
  likewise hiding them, rehoming one and, at a new level, their home.
  The README gives standard keys beside Vim's.

## 1.2.0

- `pets.home` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.1.0

For Fenix 1.0.1 (extension API 1.1).

- New friends: an owl that's up when you're up late and turns its head right round to hoot, a fox that creeps up on something in the grass and leaps on it, and a snail in no hurry that hides right inside its shell.
- The cat has a ball of yarn: it bats it, creeps after it, and pounces.
- Rubber duck debugging: while you debug, the rubber duck sits by the line you're on and listens. It quacks at each breakpoint, the others start, and it's pleased when the session ends.
- They notice more: back after a while away, they look up and greet you; another file gets a look round from the one nearest the caret; new warnings get a tilt of the head.
- Livelier: they walk more, linger less, play together more often, get the zoomies when they're happy, race each other, and now and then the one nearest the caret trots over to where you're working. In the snow, they gather round Ember to keep warm.
- How busy they are is up to you: `pets.liveliness`, calm, normal or lively.
- Late at night (22:00 to 06:00) they nap after a minute without typing, not three -- all but the owl.
- With the Pomodoro extension (1.0.1), they keep still and quiet in a work round, and stretch and play when the break comes.
- They draw only when something's changed: an unchanged layer isn't sent again, so Fenix doesn't redraw five times a second for pets that are asleep.
- They move as smoothly as their style asks: Fenix 1.0.0 drew a model's next frame only when something else redrew.

- Commits get confetti from the pet nearest the caret. A push and they all hop to see it off; a pull, or another branch, and they look round. A merge conflict sends them into hiding until it's resolved, and then they cheer.
- A big test run is a party: 100 or more tests passed and none failed, and there's confetti from every one of them.
- A build or a test run starting gets a watcher: the pet nearest the caret sits down to watch, and dozes off if it drags on past two minutes.
- A coding agent finishing its turn gets a cheer; an error, and Ember singes and the others look round. They're told only that a turn ended and how, never what was said.

## 1.0.0

- The first release, for Fenix 1.0: little 3D pets on the modeline: they nap, play fetch, cheer your passing tests -- and Ember burns up when one fails.
