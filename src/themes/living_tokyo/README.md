# Living Tokyo

[Try it live](https://jonhoo.github.io/ur/sim/?theme=living_tokyo): the simulator, in your browser.

A city at night on the clock face, in [Tokyo Night]'s colours: the time
and its seconds across the top, the moon as the battery, windows lit by
the day's steps, a beacon blinking and a sign showing the heart rate
(OFF, unlit, on the charger, where it isn't measured), and a billboard
for notifications nobody has seen. It's drawn once a
second and stays still in the dim look. No weather.
(`theme-living_tokyo`.)

Its other screens are being restyled as the same city at street level
(see [DESIGN.md](DESIGN.md)), and live as the clock face does, on the
hour, the night's weather and what they show. So far: the glance on the
charger (the clock face's moon, big), the full charge's (the city under
a full moon, struck by lightning, FULL in neon), Last night (an
overnight sleeper train), a notification and its banner (the app's
noren over a noodle bar's door), and an alarm ringing (a level crossing
at night). The rest are still copies of [Retro](../retro/)'s, in Tokyo
Night's colours.

## Colour schemes

Two, both for the night: LIVING TOKYO and LIVING STORM, Tokyo Night's
Night and Storm. The dim look (a glance, a notification on a dark
screen, the quiet night) is white and red on black in both.

| LIVING TOKYO | LIVING STORM |
|---|---|
| <img src="shots/clock-living-tokyo.png" width="120" alt="The clock face in LIVING TOKYO: blue-white on near-black indigo, the windows yellow, cyan and magenta"> | <img src="shots/clock-living-storm.png" width="120" alt="The clock face in LIVING STORM: the same, on a lighter, bluer slate"> |

It has no schemes for the day: it's a city at night. Tokyo Night's Day
is one of Retro's schemes instead, TOKYO DAY, beside Retro's TOKYO NIGHT
and TOKYO STORM: Retro's screens in the same colours.

## By day

Its package, `ur-living_tokyo-<version>-dfu.zip`, has Living Tokyo
alone, so it shows the city by day too. FORCE, on the settings' LOOK,
can't change there, as there's no day scheme to force: a tap on it says
so with FAQ D02.

For Tokyo Night's Day by day, build it beside Retro: THEME then steps
through Retro's day schemes, TOKYO DAY among them, from sunrise, and
through both themes' night schemes from sunset.

```sh
cargo build --release --no-default-features --features theme-retro,theme-living_tokyo
```

(Then package it as [AUTHORING-THEMES.md](../../../AUTHORING-THEMES.md#on-a-watch)
says.)

## The screens

| Screen | At night (LIVING TOKYO) |
|---|---|
| Last night | <img src="shots/night-last-night.png" width="120" alt="Last night: LAST NIGHT on a blue neon sign, 7:20 big, ASLEEP, and a sleeper train from 23:20 to 06:55, the times on station signs over its ends, its carriages the deep, REM and light sleep in cyan, violet and grey, with their times under it; behind it the far city against the dawn's glow, the moon setting behind it"> |
| A notification's banner | <img src="shots/night-banner.png" width="120" alt="The clock face with a banner over its bottom: a dark sign outlined in the app's cyan neon, SIGNAL in cyan and Nora Lindqvist in white"> |
| A notification | <img src="shots/night-notification.png" width="120" alt="A notification in the history at 21:30: SIGNAL on a cyan neon sign top left, the time top right, Nora Lindqvist big in the middle; under it a noodle bar, its cyan noren over a warmly lit doorway with diners at the counter, a red lantern either side, steam from its vent, and the light spilling onto the pavement"> |
| An alarm ringing | <img src="shots/night-ringing.png" width="120" alt="An alarm ringing at 06:10: ALARM on a red neon sign, the time big, and a level crossing on the right, its crossbuck yellow, one red lamp lit and hazing, the yellow and black boom down across the screen over TAP: SNOOZE and BUTTON: STOP; behind, the far city against a dawn glowing yellow and violet, a few windows lit"> |
| LOOK, in Settings | <img src="shots/night-look-settings.png" width="120" alt="LOOK's settings, at night, in LIVING TOKYO"> |

They change with the hour, the night's weather and what they show. A
third of nights rain, picked from the date, the same on every screen
that night: the rain falls and the streets are wet, the lights broken
up in their puddles; on a clear night the stars are out, a star
twinkles and the noodle bar's vent steams. The far city's windows are
lit as its people are up, most in the evening and hardly any at 02:00,
and from 04:00 the sky behind it glows, higher each hour. The noodle
bar is busy in the evening, down to one regular in the small hours,
and shut from 04:00. The sleeper train has a window lit for every share
of the night spent lying awake, and the later the wearer woke, the
lower the moon. A notification's noren and sign are in its app's own
colour, the same on the banner, so the colour says which app before
the letters do. The dim look keeps still: no rain, no glow, no light
in the bar.

## On the charger

Woken there, the screen shows the glance on the charger in Tokyo
Night's colours; a press or a touch goes on to the clock face. Once the
charge is done, the full charge's screen takes over for a few seconds,
with a buzz.

| Woken on the charger (LIVING TOKYO) | The clock face there | Full |
|---|---|---|
| <img src="shots/charging.png" width="120" alt="Woken on the charger: the time at the top in pale lavender; the moon, big, waxing in green among faint stars, with 66% under it; then FULL IN, and 1:12, big, on the night blue"> | <img src="shots/clock-charging.png" width="120" alt="The clock face on the charger: the moon waxing, in green; the beacon dark; and the heart rate's sign reading OFF, unlit"> | <img src="shots/charged.png" width="120" alt="Fully charged: the city at night under a big full moon, lightning in yellow and white striking a distant roof, and FULL in magenta neon on a dark sign in front of the buildings"> |

## The dim look

| A glance | With timers running | A notification, with the screen off | A glance on the charger |
|---|---|---|---|
| <img src="shots/dim-glance.png" width="120" alt="A glance: the hours above the minutes, big, white on black"> | <img src="shots/dim-glance-timers.png" width="120" alt="A glance with two timers running: the time in one row, white on black, and under it a red hourglass with the time left on each, 2:46, then 45 MIN"> | <img src="shots/dim-notification.png" width="120" alt="A notification that arrived with the screen off: the app on its sign, outlined, top left, who it's from, big, in white, and under it the noodle bar's eave and its noren, outlined; no rain, no light, nothing moving"> | <img src="shots/dim-charging.png" width="120" alt="On the charger: the time small in red; the moon, big, waxing to the charge, among red stars, with 66% under it; then FULL IN in red, and 1:12, big, in white"> |

These are `shots.txt`'s, drawn in a build with every theme (where
FORCE is a choice), which the simulator's tests check (see
[AUTHORING-THEMES.md](../../../AUTHORING-THEMES.md#the-readme-and-its-screenshots)).
Every other screen, and the clock face in the states it shows
(notifications unseen, a low battery, charging, a second's blink), is
`screens.txt`'s, which they only run, to check that each draws; to look
at them all, run it with `--out` (see its first lines).

## Credits

- Inspired by: Matteo Bigoi's own Living Tokyo face for InfiniTime,
  [`WatchFaceLivingTokyo.cpp`] in his fork of it, which this redraws for
  the 80×80 canvas; and its colours by folke's [Tokyo Night] for Neovim.
  Its other screens started as copies of Retro's.
- Contributed by: Matteo Bigoi ([crisidev](https://github.com/crisidev)).
- Maintained by: Matteo Bigoi ([crisidev](https://github.com/crisidev)).

[`WatchFaceLivingTokyo.cpp`]: https://github.com/crisidev/InfiniTime/blob/87341ef4d0b4276e83a3ce8c1d1540fe21adb9d2/src/displayapp/screens/WatchFaceLivingTokyo.cpp
[Tokyo Night]: https://github.com/folke/tokyonight.nvim
