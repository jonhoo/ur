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
2. **The city is alive, visibly.** A screen changes with the time and
   with what it shows, as the clock face does with its lit windows,
   moon, beacon and billboard, and the change reads at a glance: two
   shots an hour or a night apart must look like two different nights,
   not like a pixel moved. Each screen has living elements from each of:
   - *the hour*: the far city's windows lit as its people are up
     (`street::city_lights`: most in the evening, hardly any at 02:00),
     the dawn's glow from 04:00, higher each hour, and fewer stars
     (`street::horizon`, `street::daylight`); a shop open, winding
     down, shut;
   - *the night's weather*: a third of nights rain (`street::rainy`,
     picked from the date, keyed to the evening so a night doesn't
     change at midnight, the same on every screen). Rainy: rain falling,
     wet ground with the lights broken up in it, no stars. Clear: stars
     (one twinkling), the moon, steam rising, light pooling on dry
     ground. Both nights must be alive, neither a bare version of the
     other;
   - *the data*: how much sleep, how many steps, how long until the
     alarm, which app, how stale a reading is;
   - *gentle motion* on the frame the UI already draws once a second:
     a lamp flashing, rain falling, a star twinkling, steam rising.
     Never a frame of its own: no animation may cost a wake-up.
3. **The dim look holds still.** The dim look (a glance, a notification
   that came with the screen off, any screen in the quiet night) is
   idle mode, 8 colours: there the city keeps its shapes and its data
   but drops the weather, the light and the motion (no rain, no glow,
   no twinkle, no lit interiors, no far city's half-toned walls). Every
   screen that can be dim is told so: a `dim` argument, or `Face::dim`,
   `Glance::dim` and `Grid::dim`.
4. **Draw, don't store.** Shapes from code (discs, runs, dithers) rather
   than bitmaps: flash is scarcer than the cycles to draw them once a
   second. A sprite only where a shape won't do, and small.
5. **A scene, not a recolour.** Rain over Retro's layout isn't the
   city. Every screen is a place, drawn in layers: a sky (stars or
   rain, the dawn's glow), the far city on the horizon, the place in
   front (a crossing, a shop, a train, a board), and the ground (wet or
   dry). Subtle screens, used often (the lists, the settings), keep
   their scene to a strip or a sign and leave the rows their room;
   elaborate ones, that take the watch over (an alarm, a call, finding
   the watch), fill the screen round the information. The three
   showcase screens (an alarm ringing, Last night, a notification) set
   the level.

## Resolution

Living Tokyo isn't a retro theme, and needn't be as coarse as the
canvas's 80×80. Its builds can draw rows at the panel's own 240×240
(`Canvas::fine_rect`, `fine_tone`, `fine_text`, and the `fine_` pieces
in [street.rs](street.rs), all in the panel's pixels), over and under
the canvas's drawing in the order drawn. A tenth of a millimetre a
pixel: detail that makes a thing read as what it is (a noren's slits
and lettering, a stool's seat, a lantern's hoops), and tones (amber
checkered over the dark is wood) beyond the sixteen roles.

What it costs: a frame that draws finely is drawn again for every eight
canvas rows it draws finely in, as they're sent (see `canvas.rs`), and
a fine row that changes goes to the panel whole. So:

- Draw finely where detail tells: the scene's subject. Text, signs and
  the sky can stay on the canvas, at 3×, where they always were.
- A fine row that moves costs a full row's SPI each second it does: keep
  what moves in few rows (steam, not a whole rainy sky, for now).
- Shapes drawn a pixel at a time walk only `canvas.fine_rows()`.


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
| `glow` | A dithered halo round a light |
| `stars` | Single pixels, every third brighter |
| `steam` | Two wisps rising and swaying with a tick |
| `skyline` | The far city along a horizon, any height, its windows lit by a share |
| `city_lights` | How many of the city's windows are lit at an hour |
| `rain` | Leaning streaks, falling with a tick |
| `reflection` | A light broken up in the puddles below it |
| `rails` | A railway line seen from the side |
| `daylight`, `DAYLIGHT` | How light the sky is at an hour: 0 until 04:00, up to 4 by 07:00, down again from 17:00 |
| `horizon` | The dawn's (or dusk's) glow above a horizon: 4, 9, 15 or 22 rows, yellow at the bottom, violet above |
| `evening`, `rainy` | Which evening a local second belongs to (noon to noon), and whether it rains that night (a third of nights) |
| `local_now`, `hour_of` | The local time now, for a screen not handed it, and a local second's hour |

At the panel's resolution (coordinates 0..240):

| Piece | What it is |
|---|---|
| `fine_ellipse` | A filled ellipse in a box: a stool's seat, a lantern's body |
| `fine_line` | A line a pixel thick: chopsticks, a board's legs |
| `fine_glow` | A sparse haze round a light, thinning at its edge |
| `fine_lantern` | A red paper lantern (chōchin), 12×18 on a cord: black caps, its hoops, lit from inside (and hazed) or dark |
| `fine_steam` | Wisps rising from a bowl, swaying wider and thinning as they rise, lifted by the seconds |
| `fine_reflection` | A light broken up in a wet street below it |

The clock face's moon (the battery) is `clock::moon`, and its city
`clock::city`. A screen handed the time (`Status::local_second`, an
`Instant`) uses that; one that isn't, `local_now`.

Pieces the plan below needs, to add to `street.rs` when the first
screen does (one agent each, so two don't draw their own):

- **board**: a station board, a `SHADOW` panel with a `TRACK` frame,
  its rows in `HEART_SHINE` like an LED board. Alarms, the phone's
  alarms, Info.
- **noren**: the notification's (`messages::render_shop`), moved here
  for the call's booth or Settings if they hang one: panels in a neon,
  slit, a fold under the rail and a hem, a letter dyed on each.

## The screens

What each `Theme` method draws, and what makes it live: the hour, the
night's weather (rain, or clear) and the data. *Done* is drawn so; the
rest is the plan, to be drawn at the showcase's level (principle 5).
Jon has approved the two that rework a screen's layout: Today as a
skyline, and the alarms (and the phone's) as a departure board; and
the hanging title sign (`street::title`) on every list and settings
screen. By day (Living Tokyo alone shows its night schemes by day too)
it stays the night's city under the full dawn glow, with no stars.

| Method | Weight | The scene | What lives |
|---|---|---|---|
| `clock` | elaborate | *Done.* The skyline at night, the time over it | Windows lit by the steps, the moon by the battery, the beacon by the heart rate, the billboard while notifications are unseen |
| `glance` (time) | subtle, dim | *Done.* The hours over the minutes, big, as before; at the foot, the far city's lit windows, a row of them | Still (dim): the windows lit are the day's steps' share of the goal (only the lit ones show) |
| `glance` (timers) | subtle, dim | *Done.* The time, and an hourglass and the time left a timer | The time left |
| `glance` (route) | subtle, dim | *Done.* The turn's arrow and the distance; the street's name on a street plate | The turn, the distance |
| `glance` (charge) | subtle | *Done.* The moon, big, among stars | The moon waxes with the charge |
| `charged` | elaborate | *Done.* The city under a full moon, lightning, FULL in neon | The windows are the day's steps |
| `today` | medium, dense (approved) | *Done.* The day's steps as the city: a building an hour, as tall as its steps; the heart rate's range as a pink neon tube over it; the steps big and the resting rate above, and the rest left dark (no far city, no glow) | The hour now has a beacon blinking; the hours to come are empty lots; after dark the buildings' windows light, by day they don't; a few stars, or a fine slate rain behind the roofs |
| `last_night` | medium, dense | *Done.* An overnight sleeper train from falling asleep to waking, the times on station signs over its ends, its carriages the stages, in a dark sky (calmed: no far city, glow or headlight beam) | A lit window for each share of the night spent awake; the sky the wearer woke under: the moon setting towards the rails the later the wake, the stars going out towards the morning; that night's fine slate rain instead, on a wet one |
| `alarms` | medium (approved) | A station's departure board under a platform roof: smart wake, the alarms and the timers as departures | The next to ring flagged and counting down ("IN 7H05"); running timers tick; an alarm off is a dark row; rain past the platform's edge, or the night sky |
| `phone_alarms` | subtle (approved) | The same board, ten rows | The next to ring flagged |
| `editor` | subtle | One departure, big, on the board; its buttons as lit panel keys | A running timer ticks |
| `notification` | everyday | *Done, at the panel's resolution.* The app's neon sign hanging top left, who it's from big, and under it a ramen stall: its roof, the app's noren lettered RAMEN over the counter, a red lantern at each end of the eaves, the stock pot and a bowl (chopsticks across it) on the lit counter, four red stools, a menu board on the street | The noren in the app's colour; the stall lit and the bowl steaming in the evening, dimmer and its steam thinner in the small hours, shuttered (noren in, lanterns out, no bowl) from 04:00; rainy, faint rain behind the name and the stall's and lanterns' light broken up in the wet street; clear, a few stars, fewer towards dawn, and the stall's light pooled on the street. A long name pushes the roof down, squeezing the stall (its noren shorter, unlettered, then hiding the bowl). Dim: red outlines (the eaves, posts, counter, stools), the unlit lanterns, and the noren and its letters in the app's colour |
| `banner` | everyday | *Done.* The same sign, over the bottom of the screen | The app's colour |
| `music` | medium | *Done.* The bar's MUSIC sign hanging top left, the artist and track on a dark marquee, and the turntable under it: play or pause on the record's label, previous and next either side | One living thing, the record: playing, the sign lit, the arm on it and a sheen going round a step a second; paused, the sign dark, the arm at rest, the record still. Quieter: the bar's lamp warming the record by the noodle bar's hours; a little rain or a few stars over the marquee. Dim: shapes and text, still |
| `air` | medium | *Done.* A small room: the CO₂ figure on the wall as before, a little window high up beside the title, and a red bowl on the counter at the foot | One living thing, the bowl's steam, taller the stuffier the air (a wisp in fresh air, up to the band's bar past 2000 ppm). Quieter: the window fogs with the humidity and shows the rain or a star and the far city's lit windows; the big figure dims past ten minutes, every figure faint once stale. Dim: no steam, fog or weather |
| `buttons` | elaborate | *Done.* A dark drinks machine on a street corner: its lit shelves (two, or three with two layers), a drink on show over each slot's button, the readout and the tray at its foot. Each slot sits inside its `buttons::at` tap area (asserted as it's drawn) | A pressed slot floods warm; a waiting one's button is lit and a ring round it blinks; a sent press lights its slot green (red if it went nowhere) with the code big on it, says the gesture on the readout, and drops a can into the tray. One thing of the night's: rain falling past the cabinet and the shelves' light broken up on the wet street, or stars and a moth round the light. Dim (`Grid::dim`): the shelves' outlines, unlit; what's pressed or sent in white |
| `settings` | subtle | Each row a hanging wooden menu tag on a rail; a fixed row's tag unlit | The THEME row's tag lit in the scheme's own colours |
| `info` | subtle | A station information board | The uptime ticks; the battery's moon; the phone's sign lit while it's connected |
| `storage_prompt` | subtle | *Done.* A shop's shutter half down, the light on under it; what to do painted on the shutter | Still. Dim: the shutter's outline, the light out |
| `bthome_qr`, `bthome_key` | subtle | The title's sign; the QR code untouched, as it must scan | Still: they're read once |
| `handles` | subtle | The edge bars, as now | |
| `ringing` | elaborate | *Done.* A level crossing: the crossbuck, two red lamps, the boom down; the title on a red sign, the time big, the hints under the boom; the far city behind, the road in front | The lamps take turns each second; the city's windows by the hour, the dawn's glow behind it; rainy, rain, the lamp broken up in the wet road and splashes; clear, stars (one twinkling) and the lamp's light pooled on the road |
| `find` | elaborate | *Done.* FIND MY WATCH on a red sign, HERE big in a puddle, ripples round it, an eave over the street, the far city low behind | One living thing, the water: a ring a second spreads round HERE; clear, a single drop falls from the eave; rainy, the rain falls harder the longer the phone looks (another fall at 20 s and 40 s) and pocks the puddle more each second. Dim: the rings still spread, nothing else moves |
| `call` | elaborate | *Done.* INCOMING CALL on a green sign (Japan's public phones' green), the caller big, a small lit booth on the far city's horizon, its green phone on the back wall; decline and answer as lit red and green keys | One living thing, the handset: it jumps off its hook every other second while it rings; silenced, it's still and the booth's light goes down. Quieter: rain past the booth (its light in the wet ground) or a few stars; the far city's windows by the hour. Dim: the booth's outline and the keys |
| `pairing` | subtle | *Done.* The passkey, big, on a ticket machine's dark display, its fare buttons and ticket slot under it | Still. Dim: the display's frame and the digits |
| `update` | medium | *Done.* The percentage big over a platform, a train pulling in: its nose at the left edge at 0%, at the platform's end at 100% | The train, as far in as the update. Dim: its roof and nose, unlit |
| `off` | subtle | *Done.* Why, on its sign, what wakes it, and the far city along the bottom | Drawn once: the city's windows all but out, an eighth of the hour's still lit. Dim: no city |

Where taps land stays where `mod.rs`'s `layout` has it (the alarms'
bands, the editor's and a call's buttons, the music's thirds, the
settings' rows), unless a restyle moves the drawing and its tap method
(`alarm_at`, `editor_button`, …) together. The button grid keeps
`buttons::at`.

## Checking a screen

- Render it in both schemes and the dim look, at two or three times of
  night (02:00, 05:00, 06:30, say) and data states (the simulator's
  `time`, `until`, and the data commands), on a clear night and a rainy
  one (the evenings of 2026-11-09 and 2026-11-11, say: `street::rainy`
  is deterministic), and look at each at the size it's seen: 240×240.
  The changes should be plain side by side.
- `cargo clippy` with `--no-default-features --features
  theme-living_tokyo` (its own code is held to dead code), and the
  simulator's tests with `themes-all`, in debug, for the drawing's
  assertions.
- Measure the package's growth (`scripts/release.sh`, its
  `ur-living_tokyo` line).
