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
(`Canvas::fine_rect`, `fine_tone`, `fine_type` (see
Type), and the `fine_` pieces in [street.rs](street.rs), all in the
panel's pixels), over and under
the canvas's drawing in the order drawn. A tenth of a millimetre a
pixel: detail that makes a thing read as what it is (a noren's slits
and lettering, a stool's seat, a lantern's hoops), and tones (amber
checkered over the dark is wood) beyond the sixteen roles.

What it costs: a frame that changes what it draws finely is drawn again
for every eight canvas rows it changes them in, as they're sent (see
`canvas.rs`: 4–7 ms each on the watch), and a fine row that changes
goes to the panel whole. Bands whose fine drawing didn't change aren't
drawn again, so a still screen costs only its own pass. So:

- Draw finely where detail tells: the scene's subject, and the big,
  still text (see Type). Small text and the sky can stay on the canvas,
  at 3×, where they always were.
- A fine row that moves costs a full row's SPI each second it does, and
  its band a pass: keep what moves in few rows, and to the screen's one
  living thing (steam, a record's sheen, a crossing's lamps). Rain is a
  still, placed afresh each minute (`street::fine_rain`, the clock's
  `render_rain`), and what's lit by something that moves stays put (the
  crossing's light on the road lies under both lamps).
- Shapes drawn a pixel at a time walk only `canvas.fine_rows()`, and
  are noted whole (`Canvas::fine_shape`, as `street::fine_glow` is), so
  that the frame's own pass doesn't hash their every pixel.


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

Two kinds of type: the canvas's pixel fonts, and the city's own
lettering at the panel's resolution.

- **The lettering** (`font::fine`, drawn with `Canvas::fine_type`): a
  monoline sans traced with a round pen, like a neon tube's lettering or
  a station sign's rounded face, at any size and weight, in capitals and
  lowercase, with Latin-1's letters. For what's big and mostly still:
  who a message or call is from, a screen's title on its sign, big
  times and figures that change by the minute or slower. Done: every
  screen's title on its sign (`street::title`, `street::lettered`), the
  banner's and the card's app signs, the card's name and a call's
  caller, and the air's figures.
- **The pixel fonts** (the small font, the medium face, the 5×7 digits):
  small labels and hints, and anything that changes every second (the
  clock's seconds, a countdown's last minutes, a marquee). A fine row
  that changes goes to the panel whole, and its band of eight rows is
  drawn again (see Resolution), where the canvas sends a changed figure
  as a narrow span.
- Information in `TEXT`; labels `TEXT_DIM`, hints `TEXT_FAINT`; a
  sign's lettering in its tube's colour.
- Names as the phone writes them, capitals and lowercase; signs in
  capitals (`Style::caps`), as signs are. The pixel fonts have no
  lowercase.
- No fake Japanese.

### Using the lettering

A `font::fine::Style` is a size and a weight: `cap`, the capitals'
height in panel pixels (from the middle of the top stroke to the
middle of the baseline's); `stroke`, the pen's width in sixteenths of a
pixel; and `caps`. The card's, as a guide:

| Use | `cap` | `stroke` | Edge | Letters across the card (216 px) |
|---|---|---|---|---|
| A name, big | 26 | 68 (4¼ px) | `TEXT_FAINT` | about 11 |
| A name, longer | 16 | 44 (2¾ px) | `TEXT_FAINT` | about 17 |
| A sign's title (`street::LETTERING`) | 12 | 28 (1¾ px) | none | about 19 |
| The air's CO₂ | 52 | 112 (7 px) | `TEXT_FAINT` | four digits across 216 |

Draw with `canvas.fine_type(x, y, text, style, color, edge)`, in panel
pixels: `x` is the ink's left, `y` the top of the capitals' ink (an
accent over a capital reaches `style.reach().0` above it, a descender
`style.reach().1` below), and it returns where the next letter would
start. `edge` smooths the strokes' edges in a shade between `color` and
what's under them: `Some(TEXT_FAINT)` for `TEXT` on the night's dark,
`None` for neon (no shade between a neon and the dark) and in the dim
look (whose eight colours have none). Measure with
`font::Face::Fine(style).width(text)` (the ink), `style.height()` (the
capitals' ink) and `style.advance(c)`.

To wrap or cut it, it's a layout face: `font::Face::Fine(style)` in
`font::wrap` and `font::fill`, with widths and rooms in panel pixels,
breaking and balancing as the pixel fonts do; `Canvas::line` draws a
laid-out line (with its hyphen or ellipsis) in it, without an edge. A
line's ink is wholly skipped in a band it isn't in, but laying text out
isn't: lay it out (`wrap` balances by filling it a few times) only in
the passes that draw it, `canvas.fine_rows()` says which, as
`messages::render_card` does; a cheap `fill` gives the line count the
rest of the screen's layout needs.

What it costs: about 1.3 KB of flash for the face (every glyph about 11
bytes); CPU in proportion to the ink, about 0.6 M instructions a frame
(about 10 ms) for the card's two-line name, which is drawn again in
each band it's in. A screen whose big text sits on a sign or board
behind it is cheaper drawn there than over rain, whose every second
changes the rows under it.

## The shared pieces

In [street.rs](street.rs). Most are drawn at the panel's resolution
(the `fine_` ones take its coordinates, 0..240); a few still on the
canvas's grid, for the screens that need them so (see each).

| Piece | What it is |
|---|---|
| `fine_sign`, `fine_sign_in` | A sign's box: dark inside a neon tube a pixel thick, a pixel in from its edge, its corners rounded, a glow either side of it; in the canvas `sign`'s footprint, three times over. `fine_sign_in` with `dim`: the glow solid, a tube two pixels thick and no checkerboard (the route glance's street plate) |
| `title`, `lettered`, `sign_title` | A screen's title as a `fine_sign` hanging from the top edge on wires, its lettering in the fine face (`LETTERING`) where titles always were (`LETTERING_AT`), so what's under it keeps its place; `lettered` in an ink of its own, for a sign that's off; `sign_title` either, in the dim look if asked: its tube solid, two pixels thick, without the glow's checker, which flickers at idle mode's refresh |
| `hanging`, `lettering_width` | The same box (in the dim look if asked), round lettering the screen draws itself (cut to fit, say), its width in canvas pixels |
| `SIGN_H` | A sign's height on the canvas's grid: a line of the small font, and a tube above and below |
| `fine_moon`, `moon` | The moon: a real circle, lit from the right by a share (the terminator a half-ellipse), its seas greyed, its dark side ringed faintly. The battery (`clock::moon`), the charge glance, the full charge's, Last night's. `moon` with `dim`: its dark side outlined in red, the lit share solid, no seas, and no tones (the dim charge glance) |
| `fine_stars` | Single panel pixels at the middles of canvas pixels, every third brighter; given the seconds, one twinkling every third second as a small cross |
| `fine_skyline` | The far city along a horizon, any height: `fine_building`s in slate, a few with a water tank or a mast, windows lit by a share |
| `fine_building`, `Roof`, `Windows`, `lit_window` | A building: its wall, a parapet, what's on its roof, and windows of 2×2 pixels 3 apart across and 4 down, lit by their index (`lit_window`: a share of every 256), the dark ones as a tone of glass if shown |
| `city_lights` | How many of the city's windows are lit at an hour |
| `fine_rain` | Thin streaks, a pixel wide and leaning, falling with a tick |
| `daylight`, `DAYLIGHT` | How light the sky is at an hour: 0 until 04:00, up to 4 by 07:00, down again from 17:00 |
| `fine_horizon` | The dawn's (or dusk's) glow above a horizon: 12, 27, 45 or 66 rows, in six steps of tone, yellow at the bottom, violet above |
| `evening`, `rainy` | Which evening a local second belongs to (noon to noon), and whether it rains that night (a third of nights) |
| `local_now`, `hour_of` | The local time now, for a screen not handed it, and a local second's hour |
| `board` | A station board: a dark panel in a slate frame (on the canvas, so a ticking row on it stays coarse), on thin rods |
| `led_digit`, `led_colon`, `LED_H` | A board's big digits, each dot a 3×3 of LEDs of 2×2 panel pixels; or, for a running timer's, a canvas square |
| `fine_noren`, `NOREN_SLIT` | A shop's noren: panels in a neon, slit, a fold under the rail and a hem, a small crest (a bowl and chopsticks) dyed in a shade of the cloth on the middle one; no lettering, which drew the eye from a notification's sender; in the dim look, outlines |

More at the panel's resolution:

| Piece | What it is |
|---|---|
| `fine_ellipse` | A filled ellipse in a box: a stool's seat, a lantern's body |
| `fine_line` | A line a pixel thick: chopsticks, a board's legs |
| `fine_glow` | A sparse haze round a light, thinning at its edge |
| `fine_lantern` | A red paper lantern (chōchin), 12×18 on a cord: black caps, its hoops, lit from inside (and hazed) or dark |
| `fine_steam` | Wisps rising from a bowl, swaying wider and thinning as they rise, lifted by the seconds |
| `fine_reflection` | A light broken up in a wet street below it |
| `led_text` | The small font as a board's dot matrix: an LED of 2×2 panel pixels a pixel of the font, 3 apart (or the canvas's text, for a row that ticks) |
| `fine_ring` | An ellipse's outline, of any thickness and tone: ripples on water, a tile's round end |
| `fine_bowl` | A ramen bowl on a counter: black, its red band, the noodles over its rim, chopsticks across it; the stall's and the air's room's, `fine_steam` rising from it |

The clock face's moon (the battery) is `clock::moon` (a `fine_moon`),
and its city `clock::city`: a `fine_skyline` behind five near towers of
its own, each a different silhouette (a setback, a mast, a tank) and
glazing, with few, big windows. Its beacon blinks on the canvas, in
rows nothing else is drawn finely in, so that the second's change is a
few coarse pixels rather than a fine band (see Resolution). A
screen handed the time (`Status::local_second`, an `Instant`) uses
that; one that isn't, `local_now`.

What the canvas's pieces still left are for, and when they can go:

- `led_digit`'s and `led_colon`'s coarse squares: a running timer's
  digits, which tick (keep them coarse: see the resolution's costs).

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
| `clock` | elaborate | *Done, at the panel's resolution but for the time.* The skyline at night, the time over it: five dark towers in front, each its own silhouette (a low block, an office tower set back, the tall one with a mast, an old block with its tank, the billboard's) and glazing, few big warm windows; the far city behind; the heart rate on a red neon sign on the left roof, MAIL on a magenta billboard on the right | The towers' windows lit by the steps, the moon by the battery, the beacon by the heart rate, the billboard while notifications are unseen (its tube out for a second about once a minute); the hour in the far city's windows, the dawn's glow behind it from 04:00 and the stars going out; rainy, no stars and the rain past the roofs, placed afresh each minute (a still, as a fine row that moves each second costs its whole row's SPI). Dim: the towers outlined, their lit windows, the signs, the moon and stars; no far city, glow or rain |
| `glance` (time) | subtle, dim | *Done, at the panel's resolution.* The hours over the minutes, big, in the lettering; at the foot, a row of low buildings outlined in red | Still (dim): their windows lit are the day's steps' share of the goal (only the lit ones show) |
| `glance` (timers) | subtle, dim | *Done, at the panel's resolution.* The time in a row in the lettering; under it an hourglass and the time left a timer, on the canvas (it ticks) | The time left |
| `glance` (route) | subtle, dim | *Done, at the panel's resolution.* The turn's arrow (its art, its steps smoothed into slopes) and the distance in the lettering; the street's name lettered on a street plate, two lines at most | The turn, the distance |
| `glance` (charge) | subtle | *Done, at the panel's resolution.* The moon, big, among stars; the charge and the time to full in the lettering. Dim: the moon outlined (`street::moon`) | The moon waxes with the charge |
| `charged` | elaborate | *Done, at the panel's resolution.* The city under a full moon, a jagged bolt with a haze and a white core, FULL lettered in neon on a sign | The windows are the day's steps |
| `today` | medium, dense (approved) | *Done, at the panel's resolution.* The day's steps as a street: a building an hour, floors of two windows, as tall as the square root of its steps (so a quiet hour is a house of a few floors beside a busy one's tower; the lit windows are linear: a building has its floors' sixteenth of them lit), water tanks on a few roofs; the heart rate's range as a pink neon tube over it, its edges lit and the gas between them faint; the steps big in the fine lettering, a footprint before them, and the resting rate above; the rest left dark (no far city, no glow) | The hour now has a slender lattice tower up to a blinking beacon just over the city (the beacon on the canvas, in a row with nothing fine in it, so its blink costs a canvas pixel rather than a band of fine rows); the hours to come are fenced empty lots; after dark the buildings' windows light, by day they're dark glass; a few stars, or a fine slate rain behind the roofs. Dim: the shapes, dark windows, the beacon steady, no tones (the tube's glow, the fence's mesh): they flicker at idle mode's refresh |
| `last_night` | medium, dense | *Done, at the panel's resolution.* An overnight sleeper train from falling asleep to waking, the times in the fine lettering on station signs on posts behind its ends, its carriages the stages (a stage longer than a carriage is several): rounded roofs, a row of windows, a livery stripe, bogies on the rail, gangways between, a sloped nose with a headlight and a red tail light; how long, big, in the fine lettering, the colon blue; in a dark sky (calmed: no far city, glow or headlight beam) | A lit window for each share of the night spent awake; the sky the wearer woke under: the moon setting towards the rails the later the wake (a pixel every ten minutes from 04:00), the stars going out towards the morning; that night's fine slate rain instead, on a wet one. Dim: the carriages as outlines in their stages' colours, the lit windows kept, the signs plain frames and the moon an untoned crescent (no tones, which flicker at idle mode's refresh) |
| `alarms` | medium (approved) | *Done, at the panel's resolution.* A station's departure board under a platform roof: smart wake, the alarms and the timers as departures, in its dot matrix of LEDs (a row that ticks every second in the canvas's lettering); the rail and the platform's studded yellow edge under it | The next to ring flagged and counting down ("IN 7H05"); running timers tick; an alarm off is a dark row; rain past the platform's edge, or the night sky |
| `phone_alarms` | subtle (approved) | The same board, ten rows | The next to ring flagged |
| `editor` | subtle | *Done.* One departure, big, on the board in its LEDs (a running timer's in canvas squares); its buttons as lit keys, rounded, lettered finely | A running timer ticks |
| `notification` | everyday | *Done, at the panel's resolution.* The app's neon sign hanging top left, lettered finely, who it's from big in the fine lettering (as written, capitals and lowercase), and under it a ramen stall: its roof, the app's noren over the counter (a quiet bowl crest dyed on it, no lettering), a red lantern at each end of the eaves, the stock pot and a bowl (chopsticks across it) on the lit counter, four red stools, a menu board on the street | The noren in the app's colour; the stall lit and the bowl steaming in the evening, dimmer and its steam thinner in the small hours, shuttered (noren in, lanterns out, no bowl) from 04:00; rainy, faint rain behind the name and the stall's and lanterns' light broken up in the wet street; clear, a few stars, fewer towards dawn, and the stall's light pooled on the street. A long name pushes the roof down, squeezing the stall (its noren shorter, its crest gone, then hiding the bowl). Dim: red outlines (the eaves, posts, counter, stools), the unlit lanterns, and the noren's outline in the app's colour |
| `banner` | everyday | *Done.* The same sign, over the bottom of the screen | The app's colour |
| `music` | medium | *Done, the turntable at the panel's resolution.* The bar's MUSIC sign hanging top left, the artist and track on a dark marquee (on the canvas: it scrolls ten times a second), and under it a turntable under a pendant lamp: the record on its platter, its grooves catching the light, the tonearm from its pivot; play or pause on the record's label, previous and next either side | One living thing, the record: playing, the sign lit, the arm on its grooves and the lamp's sheen going round a step a second, the label's notch with it; paused, the sign dark, the arm lifted back to its rest, the record still. Quieter: the lamp keeps the noodle bar's hours, its light pooled wide on the plinth in the evening (and hazing round its shade), narrower in the small hours, out from 04:00; a little rain or a few stars over the marquee. Dim: shapes and text, still |
| `air` | medium | *Done, at the panel's resolution.* A small room: the CO₂ figure big on the wall in the fine lettering (it changes only with a reading), the temperature and humidity under it so too, a little window high up beside the title (its frame, sash and sill), and the stall's ramen bowl on the counter at the foot | One living thing, the bowl's steam, taller and in more wisps the stuffier the air (a wisp in fresh air, up to the band's bar past 2000 ppm). Quieter: the window fogs with the humidity (a tone of mist from 40%, thicker at 60% and 75%) and shows the rain or a star and the far city's lit windows; the big figure dims past ten minutes, every figure faint once stale. Dim: no steam, fog or weather |
| `buttons` | elaborate | *Done, at the panel's resolution.* A dark drinks machine on a street corner, a blue strip glowing under its roof: its shelves behind glass in metal frames (two, or three with two layers), glowing in tones from a tube along each top rather than glaring white, a drink on show over each slot's rounded button (a bottle with its cap and label, or a can with its white band, each with a glint and a shade), a red light on each button; the tray behind its flap, the readout and a lit coin slot at its foot. Each slot sits inside its `buttons::at` tap area (asserted as it's drawn) | A pressed slot floods warm; a waiting one's button is lit and a ring round it blinks; a sent press lights its slot green (red if it went nowhere) with the code big on it in the fine lettering, lights its button, says the gesture on the readout, and drops a can into the tray. The hour: from 01:00 to 05:00 the machine saves power, its tubes turned down; a thin dawn over its roof from 06:00. The night's: rain over its roof and the shelves' light broken up on the wet street, or stars (fewer towards dawn), its light pooled on the street, and a moth round it until the sky lightens. Dim (`Grid::dim`): the shelves' outlines, unlit, on the canvas's grid; what's pressed or sent in white |
| `settings` | subtle | *Done, at the panel's resolution.* Each row a wooden menu tag hanging on cords from a rail: lamp-lit wood (the windows' amber, a pixel in four, over the dark), its top edge catching the light, a grain line, eyelets; a fixed row's tag unlit, dark in slate. The lettering the small font's | The THEME row's tag lit in the scheme's own colours. Dim: the tags in red outline |
| `info` | subtle | *Done.* A station information board, dotted seams between its rows at the panel's resolution (none in the uptime's band: it ticks, and stays coarse) | The uptime ticks; the battery's moon; the phone's sign lit while it's connected |
| `storage_prompt` | subtle | *Done, at the panel's resolution.* A shop's shutter half down, ribbed steel slats rolled down from a box under the eave between two posts, a handle in its foot; what to do painted on it; the light on under it, the shopkeeper's feet in it, spilling onto the pavement | Still. Dim: the shutter's outline, the light out |
| `bthome_qr`, `bthome_key` | subtle | *Done.* The title's sign, lettered; the QR code untouched, as it must scan | Still: they're read once |
| `handles` | subtle | The edge bars, as now | |
| `ringing` | elaborate | *Done, at the panel's resolution.* A level crossing: the crossbuck's striped boards, two lamps in backplates under hoods, the boom down in slanting stripes, its gate's box at the post, the rail and its sleepers; the title on a red sign, the time big in the fine lettering, the hints under the boom; the far city behind, the road in front | The lamps take turns each second; the city's windows by the hour, the dawn's glow behind it; rainy, rain, the lamps broken up in the wet road and splashes (a still each minute); clear, stars (one twinkling) and the lamps' light pooled on the road under them |
| `find` | elaborate | *Done, at the panel's resolution.* FIND MY WATCH on a red sign, HERE big in the fine lettering in a puddle, ripples round it fading as they spread, a tiled eave and its gutter over the street, the far city low behind | One living thing, the water: a ring a second spreads round HERE; clear, a single drop gathers under the eave; rainy, the rain falls harder the longer the phone looks (another fall at 20 s and 40 s) and pocks the puddle more each second. Dim: the rings still spread, nothing else moves |
| `call` | elaborate | *Done, the booth and the name at the panel's resolution.* INCOMING CALL on a green sign (Japan's public phones' green), the caller big in the fine lettering if it fits on a line (then it never scrolls; a longer name scrolls on the canvas at 2×), a small lit booth on the far city's horizon (its roof's green band, its glass, the door's posts and rail), its green phone on a shelf on the back wall; decline and answer as lit red and green keys (on the canvas) | One living thing, the handset: it jumps off its hook every other second while it rings, the ring's marks beside it; silenced, it's still and the booth's light goes down. Quieter: rain past the booth (its light broken up in the wet ground) or a few stars (its light pooled on the dry ground); the far city's windows by the hour. Dim: the booth's outline and the keys |
| `pairing` | subtle | *Done, at the panel's resolution.* The passkey, big, in amber LEDs on a ticket machine's dark display, the unlit ones faint; its fare buttons, each with its fare lit, a lit coin slot, and a ticket in the ticket slot under it | Still. Dim: the display's bezel and the digits |
| `update` | medium | *Done, at the panel's resolution.* The percentage big over a platform, a commuter train pulling in under the overhead wire: silver carriages, a green band under lit windows, doors, air conditioners on the roofs, a pantograph up to the wire, the cab's rounded nose with its headlight on; the platform's white edge and yellow studded tiles in front. Its nose at the left edge at 0%, at the platform's end at 100% | The train, as far in as the update. Dim: its roof and nose, unlit |
| `off` | subtle | *Done.* Why, on its sign, what wakes it, and the far city along the bottom | Drawn once: the city's windows all but out, an eighth of the hour's still lit; rain behind it, or a few stars over it. Dim: no city |

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
