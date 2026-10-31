# Pets

Little 3D pets that live on Fenix's modeline: Ember (Fenix's phoenix),
Ferris the crab, a cat and a rubber duck. They're low-poly toys, drawn
toon-shaded and outlined in your theme's light -- chunky pixel style by
default, or smooth.

They wander, sit, look about and stretch; hop up onto the edges of your
panes; and nap together when you've stopped typing for a while. Now and
then one comes up to sit in the margin beside the line you're on, and
follows you from line to line.

They notice what you do:

- **Save** and the one nearest the caret hops.
- **Errors** in the file send them into hiding until they're fixed.
- **A passing task** and they all celebrate.
- **A failing task**: Ember bursts into flame and burns down to ash, and
  rises again the next time a task passes. The rubber duck comes over to
  listen. The cat sits it out.
- **Type for a while without stopping** and Ember catches fire, running
  along the modeline trailing flames.
- **Click one** for a heart.
- They play together, too: the cat stalks the duck, and they chase each
  other.

## Looking after them

Saving, passing tests, petting and playing fetch teach them things. Each
level unlocks something to wear: a bow at level 2, a scarf at 3,
sunglasses at 4, a party hat at 5, a crown at 7. Happiness drifts down a
little while nobody plays with them, never below content, and never while
you're away: a day off costs nothing.

In December they wear Santa hats and it snows; for Halloween, pumpkins;
on New Year's Day, and on the day you adopted each of them (a year or more
on), party hats.

| Key | What it does |
| --- | --- |
| `SPC P a` | Adopt a pet |
| `SPC P b` | Throw the ball: it bounces along, and the nearest pet fetches it back to the caret |
| `SPC P p` | Their home: levels, happiness, what they wear; pet, dress, rename or rehome one |
| `SPC P w` | Dress one up |
| `SPC P h` | Hide them, or bring them back |
| `SPC P r` | Rehome one |

| Setting | |
| --- | --- |
| `pets.place` | `modeline`, or the foot of the pane you're in (`panes`) |
| `pets.size` | `small`, `medium` or `large` |
| `pets.style` | `pixel` or `smooth` |
| `pets.most` | How many you may have at once |
| `pets.visits` | Whether one comes up to sit beside your line now and then |
| `pets.seasons` | Snow and seasonal hats |

They fade while they pass the line you're on and never take a key. With
Fenix's animations off they stand still.

## Making them

The models are built in code, from simple shapes jointed like toys:
`src/modeler` (`cargo run --release` there) writes `models/*.glb`. The
effects are pixel art: `python src/fx.py` writes `sprites/fx.png`. To see
the models as Fenix draws them, every clip, use Fenix's model preview
(`FENIX_MODEL_PREVIEW=models cargo test -p fenix-gui preview -- --ignored`
in Fenix).
