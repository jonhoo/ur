# Living Tokyo: the design

The language every Living Tokyo screen is drawn in, and the plan for the
screens still to restyle. The code's shared pieces are in
[street.rs](street.rs).

## The idea

Tokyo late at night, as night-time Japan looks in film: neon and
signage, rain and its reflections, lanterns, noren, vending machines,
train lines and level crossings, shrine gates, steam, small lit
interiors. The clock face is the city seen from afar. Every other screen
is the same city at street level, a place passed on a night walk: a
crossing, a station, a shopfront, a sign.

Nothing here recreates a film's shots, characters or images, and
nothing pretends to be Japanese writing: the font has none, and pixel
squiggles posing as kanji would be wrong to anyone who reads them.

## Principles

1. **The information comes first.** Times, names and numbers are on the
   lit surfaces, the signs and boards, in the brightest ink; the city is
   drawn in the dark roles round them, so it frames rather than
   competes. Rain passes behind text, never through its letters.
2. **The city is alive.** A screen changes with the time and with what
   it shows, as the clock face does with its lit windows, moon, beacon
   and billboard. Each screen has at least one living element:
   - *the time of night*: late, the dead of night, near dawn (see
     `street::daylight`): the sky lightens on the horizon from 04:00
     and loses its stars;
   - *the data*: how much sleep, how many steps, how long until the
     alarm, which app, how stale a reading is;
   - *gentle motion* on the frame the UI already draws once a second:
     a lamp flashing, rain falling, a star twinkling, steam. Never a
     frame of its own: no animation may cost a wake-up.
3. **The dim look holds still.** The dim look (a glance, a notification
   that came with the screen off, the quiet night) is idle mode, 8
   colours: there the city keeps its shapes and its data but drops the
   weather and the motion (no rain, no glow, no twinkle). A screen not
   told it's dim asks `street::dim`.
4. **Draw, don't store.** Shapes from code (discs, runs, dithers) rather
   than bitmaps: flash is scarcer than the cycles to draw them once a
   second. A sprite only where a shape won't do, and small.
5. **Subtle where it's used often, elaborate where it takes over.** The
   lists and settings get a sign and a touch of the street; the screens
   that take the watch over (an alarm, a call, finding the watch) get a
   scene.

## Colour

The 16 roles are the Tokyo Night palettes' (see
[schemes/mod.rs](schemes/mod.rs)); Living Tokyo gives them these jobs.

| Role | Tokyo Night | In the city | In the dim look |
|---|---|---|---|
| `BACKGROUND` | bg | the night sky, the street | black |
| `SHADOW` | bg_dark | near silhouettes: buildings, a sign's box, a lamp's lens unlit; digits' shadows | black |
| `TRACK` | bg_highlight | far buildings, unlit windows, a lamp's backplate, the black of yellow-and-black stripes, rain behind text | grey (half-tone) |
| `TEXT_FAINT` | dark5 | structure: posts, wires, rails; hints; rain to be seen | red |
| `TEXT_DIM` | fg_dark | labels, a secondary time | red |
| `TEXT` | fg | the information: times, names, figures | white |
| `ACCENT` | blue | the watch's own signs: screen titles; the seconds | red |
| `HEART` | red | what rings and must be dealt with: an alarm's sign, a crossing's lamps, lanterns; the heart rate | red |
| `HEART_SHINE` | yellow | warm light: lit windows, a headlight, lightning, crossing yellow, the dawn's glow nearest the horizon | white |
| `DEEP` | cyan | neon; deep sleep | white |
| `REM` | magenta | neon; REM sleep; the dawn's glow higher up | red |
| `GOOD` | green | neon; on, charging | white |
| `BAD` | red | low, stop, decline | red |
| `GREY` | fg_dark | light sleep | grey (half-tone) |

A sign's colour says whose it is:

- **blue** (`ACCENT`): the watch's own screens (TODAY, ALARMS, LOOK);
- **an app's own neon** (one of cyan, magenta, green and yellow, by a
  hash of its name): what comes from the phone, the same on every screen
  it shows on (the notification, its banner; later the call and music);
- **red** (`HEART`): what rings and won't stop until it's dealt with
  (an alarm, a timer, finding the watch).

## Type

- Information in `TEXT`: big figures in the 5×7 digits at 2× to 4×, with
  a `SHADOW` drop shadow (`draw_digit`); names in the small font at 2×
  or the medium face, as before.
- Sign lettering in the small font at 1×, in the sign's tube colour.
- Labels `TEXT_DIM`, hints `TEXT_FAINT`, both 1×.
- No fake Japanese; no lowercase (the font has none).

## The shared pieces

In [street.rs](street.rs):

| Piece | What it is |
|---|---|
| `sign`, `SIGN_H` | A sign's box: dark inside a neon tube a pixel thick |
| `title` | A screen's title as a sign hanging from the top edge, its lettering where titles always were (4, 3), so what's under it keeps its place |
| `hanging` | The same box, round lettering the screen draws itself (cut to fit, say) |
| `disc`, `in_disc` | A filled circle: the moon, a lamp's lens |
| `glow` | A dithered halo round a light, as it hazes in the rain |
| `stars` | Single pixels, every third brighter |
| `rain` | Leaning streaks, falling with a tick |
| `reflection` | A light broken up in the puddles below it |
| `rails` | A railway line seen from the side |
| `daylight`, `daylight_now`, `DAYLIGHT` | How light the sky is at an hour: 0 until 04:00, up to 4 by 07:00, down again from 17:00 |
| `horizon` | The dawn's (or dusk's) glow above a horizon, for a level of daylight |
| `dim` | Whether the screen is in the dim look |

The clock face's moon (the battery) is `clock::moon`, and its city
`clock::city`.

Pieces the plan below needs, to add to `street.rs` when the first
screen does (one agent each, so two don't draw their own):

- **lantern** (chōchin): a red oval, 5×7, with dark caps top and bottom
  and a rib or two, glowing (`glow`) when lit. Settings, the call.
- **board**: a station board, a `SHADOW` panel with a `TRACK` frame,
  its rows in `HEART_SHINE` like an LED board. Alarms, the phone's
  alarms, Info.
- **skyline**: a strip of far buildings with windows, from the clock
  face's `FAR` and `NEAR`, for a screen's foot. Today, switching off.

## The screens

What each `Theme` method draws, and what makes it live. *Done* is drawn
so; the rest is the plan.

| Method | Weight | The scene | What lives |
|---|---|---|---|
| `clock` | elaborate | *Done.* The skyline at night, the time over it | Windows lit by the steps, the moon by the battery, the beacon by the heart rate, the billboard while notifications are unseen |
| `glance` (time) | subtle, dim | The hours over the minutes, big, as now | Still (dim): at most a 1-row skyline at the foot whose windows are the steps |
| `glance` (timers) | subtle, dim | *Done.* The time, and an hourglass and the time left a timer | The time left |
| `glance` (route) | subtle, dim | The turn's arrow; the street's name on a street plate | The turn, the distance |
| `glance` (charge) | subtle | *Done.* The moon, big, among stars | The moon waxes with the charge |
| `charged` | elaborate | *Done.* The city under a full moon, lightning, FULL in neon | The windows are the day's steps |
| `today` | medium | The day's steps as a skyline, a building an hour, as tall as its steps, its windows lit with them; the heart rate's range as a pink neon tube over it | The hour now has its beacon blinking; the hours to come are empty lots; after dark, its windows light |
| `last_night` | medium, dense | *Done.* An overnight sleeper train from falling asleep to waking, its carriages the stages | A lit window for each share of the night spent awake; the moon lower the later the wake; fewer stars after 04:00; a star twinkles |
| `alarms` | medium | A station's departure board: smart wake, the alarms and the timers as departures | The next to ring flagged and counting down ("IN 7H05"); running timers tick; an alarm off is a dark row |
| `phone_alarms` | subtle | The same board, ten rows | The next to ring flagged |
| `editor` | subtle | One departure, big, on the board; its buttons as lit panel keys | A running timer ticks |
| `notification` | everyday | *Done.* The app's neon sign hanging top left, who it's from big, on a wet street with the sign's reflection | The sign in the app's colour, its reflection as wide as its name; rain falls; towards dawn, the sky lightens. Dim: still |
| `banner` | everyday | *Done.* The same sign, over the bottom of the screen | The app's colour |
| `music` | medium | A small lit record bar: the track on an LED marquee, a turntable in the window | The record turns a step a second while it plays and stops when paused; the marquee scrolls |
| `air` | medium | A small lit interior: the CO₂ figure on a board, a bowl's steam rising | The steam taller the stuffier the air; the window fogs with the humidity; the board fades as the reading ages |
| `buttons` | elaborate | A vending machine glowing on a dark street, a lit slot a button | A pressed button's slot flashes, a waiting one blinks, and a sent press drops a can into the tray. Dim (`Grid::dim`): the machine, unlit |
| `settings` | subtle | Each row a hanging wooden menu tag on a rail; a fixed row's tag unlit | The THEME row's tag lit in the scheme's own colours |
| `info` | subtle | A station information board | The uptime ticks; the battery's moon; the phone's sign lit while it's connected |
| `storage_prompt` | subtle | A shop's shutter half down: closing up | Still |
| `bthome_qr`, `bthome_key` | subtle | The title's sign; the QR code untouched, as it must scan | Still: they're read once |
| `handles` | subtle | The edge bars, as now | |
| `ringing` | elaborate | *Done.* A level crossing in the rain: the crossbuck, two red lamps, the boom down; the title on a red sign, the time big, the hints under the boom | The lamps take turns each second, the boom's lamps with them; rain falls; the sky lightens towards dawn and loses its stars |
| `find` | elaborate | HERE, big, in a puddle: the rings as ripples | A drop falls each second and its ripples spread; the longer the phone looks, the harder it rains |
| `call` | elaborate | A lit phone booth in the rain, the caller's name on its sign in their app's style; decline and answer as lit red and green keys | The handset jiggles with each ring; silenced, the booth's light dims and it stills; the rain falls |
| `pairing` | subtle | The passkey on a ticket machine's display | Still |
| `update` | medium | A train pulling into a platform: how far along it is | The train moves on with the percentage, its windows lit, a step a second |
| `off` | subtle | The city's lights going out, with why | The windows go out a few a second over its seconds |

Where taps land stays where `mod.rs`'s `layout` has it (the alarms'
bands, the editor's and a call's buttons, the music's thirds, the
settings' rows), unless a restyle moves the drawing and its tap method
(`alarm_at`, `editor_button`, …) together. The button grid keeps
`buttons::at`.

## Checking a screen

- Render it in both schemes and the dim look, at two or three times of
  night and data states (the simulator's `time`, `until`, and the data
  commands), and look at each at the size it's seen: 240×240.
- `cargo clippy` with `--no-default-features --features
  theme-living_tokyo` (its own code is held to dead code), and the
  simulator's tests with `themes-all`, in debug, for the drawing's
  assertions.
- Measure the package's growth (`scripts/release.sh`, its
  `ur-living_tokyo` line).
