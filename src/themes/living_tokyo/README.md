# Living Tokyo

A city at night on the clock face, in [Tokyo Night]'s colours: the time
and its seconds across the top, the moon as the battery, windows lit by
the day's steps, a beacon blinking and a sign showing the heart rate,
and a billboard for notifications nobody has seen. It's drawn once a
second and stays still in the dim look. No weather.
(`theme-living_tokyo`.)

Every other screen is, for now, a copy of [Retro](../retro/)'s, in
Tokyo Night's colours, to be restyled in the city's look.

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

## The clock face

| A second on | A low battery, no phone, no heart rate | Notifications unseen | Charging |
|---|---|---|---|
| <img src="shots/night-clock-blink.png" width="120" alt="A second on: the beacon on the antenna is out"> | <img src="shots/night-clock-low.png" width="120" alt="A low battery: the moon is an empty red outline; the struck-out Bluetooth rune after the date; no heart rate sign"> | <img src="shots/night-clock-unseen.png" width="120" alt="Notifications unseen: MAIL on the billboard, on the right-hand roof"> | <img src="shots/night-clock-charging.png" width="120" alt="Charging: the moon is green"> |

## Every screen

| Screen | At night (LIVING TOKYO) |
|---|---|
| The clock face | <img src="shots/night-clock.png" width="120" alt="The clock face, at night, in LIVING TOKYO"> |
| Today | <img src="shots/night-today.png" width="120" alt="Today, at night, in LIVING TOKYO"> |
| Last night | <img src="shots/night-last-night.png" width="120" alt="Last night, at night, in LIVING TOKYO"> |
| Alarms | <img src="shots/night-alarms.png" width="120" alt="Alarms, at night, in LIVING TOKYO"> |
| The alarm editor | <img src="shots/night-editor.png" width="120" alt="The alarm editor, at night, in LIVING TOKYO"> |
| The phone's alarms | <img src="shots/night-phone-alarms.png" width="120" alt="The phone's alarms, at night, in LIVING TOKYO"> |
| The look's settings | <img src="shots/night-look-settings.png" width="120" alt="The look's settings, at night, in LIVING TOKYO"> |
| The button grid's settings | <img src="shots/night-grid-settings.png" width="120" alt="The button grid's settings, at night, in LIVING TOKYO"> |
| Home Assistant's key, as a QR code | <img src="shots/night-bthome-qr.png" width="120" alt="Home Assistant's key, as a QR code, at night, in LIVING TOKYO"> |
| Home Assistant's key, as text | <img src="shots/night-bthome-key.png" width="120" alt="Home Assistant's key, as text, at night, in LIVING TOKYO"> |
| Info | <img src="shots/night-info.png" width="120" alt="Info, at night, in LIVING TOKYO"> |
| Storage mode's prompt | <img src="shots/night-storage-prompt.png" width="120" alt="Storage mode's prompt, at night, in LIVING TOKYO"> |
| A notification's banner | <img src="shots/night-banner.png" width="120" alt="A notification's banner, at night, in LIVING TOKYO"> |
| A notification, in the history | <img src="shots/night-notification.png" width="120" alt="A notification, in the history, at night, in LIVING TOKYO"> |
| The button grid | <img src="shots/night-grid.png" width="120" alt="The button grid, at night, in LIVING TOKYO"> |
| Music | <img src="shots/night-music.png" width="120" alt="Music, at night, in LIVING TOKYO"> |
| Air | <img src="shots/night-air.png" width="120" alt="Air, at night, in LIVING TOKYO"> |
| A call | <img src="shots/night-call.png" width="120" alt="A call, at night, in LIVING TOKYO"> |
| Find my watch | <img src="shots/night-find.png" width="120" alt="Find my watch, at night, in LIVING TOKYO"> |
| Pairing | <img src="shots/night-pairing.png" width="120" alt="Pairing, at night, in LIVING TOKYO"> |
| An update | <img src="shots/night-update.png" width="120" alt="An update, at night, in LIVING TOKYO"> |
| An alarm ringing | <img src="shots/night-ringing.png" width="120" alt="An alarm ringing, at night, in LIVING TOKYO"> |
| Switching off | <img src="shots/night-off.png" width="120" alt="Switching off into storage mode, at night, in LIVING TOKYO"> |

## The dim look

| A glance | Directions, on a glance | A notification, with the screen off | The quiet night | The button grid, in the quiet night |
|---|---|---|---|---|
| <img src="shots/dim-glance.png" width="120" alt="A glance: the hours above the minutes, big, white on black"> | <img src="shots/dim-route-glance.png" width="120" alt="A glance while the phone navigates: the time, small and red, over an arrow turning left, 150 M, and the street"> | <img src="shots/dim-notification.png" width="120" alt="A notification that arrived with the screen off: the app in red, and who it's from, big, in white"> | <img src="shots/dim-quiet-night.png" width="120" alt="The clock face in the quiet night: the city in white, grey and red on black"> | <img src="shots/dim-grid.png" width="120" alt="The button grid in the quiet night: grey keycaps outlined in red"> |

These are `shots.txt`'s, drawn in a build with every theme (where
FORCE is a choice), which the simulator's tests check (see
[AUTHORING-THEMES.md](../../../AUTHORING-THEMES.md#the-readme-and-its-screenshots)).

## Credits

- Inspired by: Matteo Bigoi's own Living Tokyo face for InfiniTime,
  [`WatchFaceLivingTokyo.cpp`] in his fork of it, which this redraws for
  the 80×80 canvas; and its colours by folke's [Tokyo Night] for Neovim.
  Its other screens are Retro's, copied.
- Contributed by: Matteo Bigoi ([crisidev](https://github.com/crisidev)).
- Maintained by: for the contributor to confirm (Matteo Bigoi, if he
  will look after it).

[`WatchFaceLivingTokyo.cpp`]: https://github.com/crisidev/InfiniTime/blob/87341ef4d0b4276e83a3ce8c1d1540fe21adb9d2/src/displayapp/screens/WatchFaceLivingTokyo.cpp
[Tokyo Night]: https://github.com/folke/tokyonight.nvim
