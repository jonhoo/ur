# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Versions up to 0.8.17 were development builds, installed only on the
author's own watches. Cargo.toml still said 0.1.0 then, so their
version lived only in the update image's header. Version numbers
missing below were test builds for the dev kit.

## [Unreleased]

### Added

- Take a heart rate from two strongly periodic readings in a row that agree, rather than waiting for a third, as long as the wrist isn't walking (a swinging arm can look like a slow pulse) (5d4d89a)
- Show a notification again when you turn your wrist to look just after missing it: one that arrived with the screen off, and went dark again untouched, shows once more (without the buzz) in place of the time at a look within 20 seconds, once; a notification you touched, read in the history, or got with the screen on doesn't, nor any in the quiet night (c4d9d5f)
- Measure the heart rate continuously through a workout, which the watch detects itself: three minutes of walking or running (80 steps a minute or more, until three minutes under 40), or the wrist working hard for eight minutes of ten (until it has let up for five). A moving wrist only reads in its pauses (a crossing, a rest between sets, the minutes after you stop), and until now the watch waited for a still wrist before each try and missed most of them; in the field logs, walks had four times as many readable pauses as readings. It costs about 2% of the battery on an ordinary day, more on a day of walking. The field log says when a workout starts and ends, and why (`workout: started, Walk (…)`; see CONTRIBUTING's "Field logs") (ed51d66)
- Send a workout's minutes to Gadgetbridge as exercise (its `EXERCISE` activity kind, for each 5-minute block that was mostly workout). Gadgetbridge stores it and counts the steps and heart rate as before; its charts don't yet draw exercise differently from other activity, and Health Connect gets no exercise session from it (810762b, 0c28933)
- Take a heart rate reading as soon as your wrist goes still once none has come for 20 minutes (awake), rather than only at the schedule's own tries, which a busy wrist kept missing; at most one such try every 2 minutes, within the same LED budget. Replayed over a four-day field log that's about a third more readings by day, and a third as much time spent more than half an hour from one, for about 0.5 mAh a day. The field log marks these tries `heart: due (Calm)` (3b8832f)
- Log which screen shows whenever that changes, and what changed it (`ui: Music by SwipeUp`, `ui: Glance by Glance`), and the screen going off with how long it was on (`ui: Off by Tick after 3 s`), so a field log can show which screens and gestures get used; also the music screen's buttons, a call answered or declined, an alarm stopped, snoozed or switched, a timer started, and a theme chosen. Never what a notification or a call says, nor who or which app it's from. CONTRIBUTING's "Field logs" lists the lines (c6e08a5)

### Changed

- Keep the last 14½ hours of activity by the minute in memory rather than a day, now that what the phone hasn't fetched is kept in the watch's storage: it frees 3.2 KB of the watch's scarce memory. Today and Last night show what they did (the hours and the night's first part older than that are kept as totals). A fetch now holds a night back for at most 14 hours rather than 18; after a move between time zones that aren't whole hours apart, Today's bars from before it may be an hour out for the rest of that day. An update from an earlier version keeps everything the phone hadn't fetched, and a watch that was switched off (an empty battery in the night, say) no longer loses its last unfetched minutes if it's charged more than a day later (170858f)
- Get more heart rate readings through a busy day: a reading that's overdue (none for half an hour) now waits up to 2 minutes for a still wrist, as other readings do, rather than starting at once on a moving wrist where few of them read, and is retried every 5 minutes rather than 10; and a try on a moving wrist with nothing usable by 19 s stops there rather than running to 30 s. In the field logs, tries that start on a still wrist read about twice as often; the worst case costs about 1% of the battery a day (1c09b62, b3c34ac, 9880356)
- Count a walk from its third arm swing rather than its fourth: the step counter holds back 6 steps rather than 8 before it believes a walk, so stop-and-go walking about the house loses fewer steps. Continuous walks count as before, and a brief burst of arm movement may now and then add a few steps (5b1a1b6)
- Time the night's heart rate readings to finish just before the end of each 5-minute block the phone receives, so a reading reaches Gadgetbridge at most about a minute after it was taken rather than up to 5 (730485b)
- Hold about 2½ days of a worn watch in the logging build's (`ur-logging`) field log, rather than under one (about 20 hours since 0.11.0): it now packs its busiest lines, the accelerometer's every 2 s and the heart-rate sensor's samples, so decode a fetched log with `defmt-print` and then `scripts/expand_log.py`, which writes them back as before (logs from older builds pass through unchanged). Log times are now in milliseconds rather than microseconds. The `ur` build is unchanged (bf66d61, e357323)
- Hold about 5 days of a worn watch in the logging build's (`ur-logging`) field log, rather than about 3: the log now also uses the part of the watch's storage that InfiniTime kept its files in (745 sectors rather than 512, so fetch it with this version's `scripts/fetch-log.py`, which reads logs of either size; an older copy reads only the first 512), and through a heart rate reading it no longer keeps the accelerometer's samples while the wrist lies still, only a short `accel: still` line saying that it did. A log kept by an earlier version stays as it is across the update (ab3e614, 0dd3aa5, 628930c)
- Have the time in the alarm and timer editor follow a finger dragged up or down the hours or the minutes, rather than move one step per swipe: an hour for every 2 mm or so, a minute for every 0.6 mm; the time is set, to the very minute showing (so an alarm can now be at 07:03, not only on the 5 minutes), and saved, when the finger lifts, and a short flick still moves it one step as before, an alarm's minutes by 5 (45eab8b, 48c947d)
- Switch an alarm (or the smart wake) on when its editor's **DONE** is tapped, so setting its time no longer also needs a tap on its switch; an alarm whose switch was tapped while the editor was open stays as that left it, the button closes the editor without switching anything, and a timer's DONE still doesn't start it (e6f059a)
- Show the Info screen's uptime in its two largest units once the watch has been up a day (`3D 4H`, `2W 3D`, `5MO 1W`, `1Y 2MO`, a month being a twelfth of a year), rather than as hours, minutes and seconds that ran into the thousands of hours; under a day it still reads `HH:MM:SS` (8f7de60)
- Show a notification that has neither a sender nor a title under its app's name, as large as a name, on a screen of its own (arriving with the screen off, and in the history), rather than the app small in the corner of an otherwise empty screen (245f3e0)
- Keep the steps, heart rates and sleep the phone hasn't fetched for a week and more (11 days at most) rather than a day: each 5-minute block now goes into a log in the watch's storage once the phone could fetch it, and a fetch after days without one sends it all, oldest first (on a trip, 33 hours without a fetch lost 9½ hours of a day: 5,906 steps and 28 heart rate readings). A crash, an update or switching off now also keeps the last day's history that the phone had already fetched, so the Today screen's chart and last night's sleep come back whole. The watch also writes much less to its storage while the phone is away: only the night under way is still saved again every 2 minutes, not everything since the last fetch (a640dab, 98e9f28, 50a6ae4)
- Compare the brighter heart rate LED with the usual one within each reading, in the logging build (`ur-logging`), rather than between one round of tries and the next: a try that starts awake on a still wrist now spends 13 s at each brightness (which comes first takes turns), and the field log says what each half made of the wrist by itself (`heart: pair phase …`; see CONTRIBUTING's "Field logs"), so a week of logs can say whether the brighter light ever helps, where taking turns would have needed six. A reading still shows the moment either half, or the two together, has one; the try then carries on to the end of its second half, so the LED is lit about 26 s rather than about 11 when a reading comes quickly, some 1 mAh a day more while the comparison runs. Tries that start on a moving wrist stay at the usual brightness. The `ur` build is unchanged (f1674f0)
- Say in the logging build's (`ur-logging`) field log when a fetch held minutes back because the wrist was lying still (they may yet turn out the start of a sleep), and from which minute, as it did for a sleep: `ble: sent 0 buckets of activity; a stillness held from minute …`. Such a fetch read like one with nothing new. The `ur` build is unchanged (585c69d)

### Deprecated

### Removed

- Remove the logging build's longer heart rate sessions and the brighter-LED phases late in a session: in the field logs they cost several times the LED time per reading of simply trying again later (90d0a78, 0760245)

### Fixed

- Keep last night's sleep, heart rates and steps when the watch resets twice within about a second in the morning: after the first reset the watch moves the night out of its log of what the phone can fetch (it holds the night back again for a while, as it did before you got up) and into its regular save, and a second reset between the two left it in neither. It now saves first (f55374e)
- Keep the activity the phone hasn't fetched across a crash soon after the watch's clock, having been a few minutes to an hour ahead, is put right (the phone's was wrong): the regular saves made while it was ahead were dated in the future and looked like the newest, so a crash while the clock caught up with them restored one of them and lost the minutes since. Each save now discards any that claim to be newer than it (41ab0db)
- Keep the activity across a crash after an update or a switch-off made while the watch's clock was ahead (the phone's was wrong, and has been put right since): the save made then was dated in the future, and until the clock caught up with that date a crash restored none of the regular saves made since (da969bf)
- Keep the activity's regular saves when the save made as the watch switches off (or before an update) fails to write: one of them that looked newer than it, from a clock the phone had since set back, was discarded all the same, leaving nothing to restore (dee63b2)
- Keep the steps of the seconds the phone takes back when it trims the watch's clock: it sets the clock back by a few seconds at about one connection in six, and when that went back over the turn of a minute, the steps (and any heart rate) of the new minute's first seconds were dropped, and the steps of the seconds repeated never reached the phone if a 5-minute block had just ended. A new time zone or summer time never moved any of it: the history is kept in UTC (9619d6e)
- Handle what the phone sends during a long activity fetch (a notification, the time, a call, find my watch) as it arrives, rather than once the fetch is done: the watch sends a fetch a block at a time between everything else, where a fetch of a week (possible since the activity log) kept the phone waiting for 10 seconds to over a minute, and anything past its eighth message in that time was lost (e7f9c88)
- Fetch a long stretch of activity several times faster when the connection had already settled at its idle pace, as the phone's does by itself seconds after connecting: the watch asked for a quicker one only if it had itself asked for the idle one (74b689d)
- Keep the activity saved as the watch switches off for an empty battery (or for storage) when the phone had set the watch's clock back by more than a few minutes in the hour before: the watch then put back an older save from while the clock was ahead, and dropped the new one (83bdcfa)
- Stop Gadgetbridge getting stuck on "fetching activity data" until the phone's Bluetooth is restarted: a fetch of more than a few buckets could run out of Bluetooth packet buffers while the phone was sending something too, and then lost the rest of its messages, the end of the fetch among them, which is all that tells Gadgetbridge the watch is free again. Messages now wait for a buffer, and a fetch whose end still can't be sent drops the connection, which frees Gadgetbridge when it reconnects (99f72cb)
- Stop the screen stuttering when it's used in the quiet night: a sweep between screens showed as two or three torn jumps, and taps answered late, because the dim look keeps the panel in its power-saving idle mode (about 8 frames a second rather than 60). From the first press or touch with the screen already on, the panel now runs at its normal rate until the screen goes off, still dim; a look at the time stays in idle mode. Noticed on evenings when the quiet night began with the wearer still awake in bed (introduced in 0.10.0) (a82243b)
- Stop taking two turns in bed a few minutes apart for waking up: a stretch of movement in the night now counts as awake (or restless) only if at least three of its minutes moved, which over eight nights against a Garmin cut the awake time inside the night from 187 to 113 minutes (the Garmin's: 55), and brought the time asleep to within about 2 minutes of the Garmin's on average, from 11 short (774391a)
- Stop taking lying awake in bed for sleep: falling asleep now takes 20 minutes in which the wrist is not just still but mostly doesn't fidget at all, as a sleeping wrist doesn't and a waking one, however still, does every minute or so; and at least one heart rate reading among them (or, should the sensor read nothing, a stir), so a watch left lying somewhere at an angle no longer falls asleep either. Over the 13 nights in the field logs this removed every false sleep (124 minutes that had gone to Gadgetbridge as light sleep, on three evenings and one afternoon) and every quiet night that started while still awake and ignored taps, and moved 5 of the nights' real starts 2 to 7 minutes later, closer to a Garmin's on average; a restless start to a night may now be noticed some minutes later than before (79c02c7, 0595b01)
- Stop taking lying awake after the alarm (or after any wake in the night) for dozing off again: falling asleep again within 45 minutes of waking still takes only 10 still minutes rather than 20, but now at least 5 of them must pass without the wrist fidgeting at all. On one morning, lying still but awake after stopping the alarm counted as sleep after 10 minutes, which added 14 minutes of REM to the night and started a doze's quiet, with taps ignored and notifications muted for 17 minutes. Over the 14 nights in the field logs, all 18 real returns to sleep stay (one starts 4 minutes later), a real doze after an alarm among them, and every quiet night starts and ends where it did; a restless return to sleep may be noticed a few minutes later than before. Alarms ring as before (8329cf6)
- Stop counting a watch lying on a desk as a nap (or a night): a watch that has lain level and face up (on a desk, a table, or the charger) for 10 minutes, without a heart rate reading, is now off the wrist back to when it was set down, so it never starts a quiet night or nap, and Gadgetbridge gets those minutes as not worn rather than as sleep, and the still minutes just before taking the watch off no longer count as sleep either; every night and nap in the field logs keeps exactly the sleep it had, and a watch lying on its side or face down still passes for asleep (24a6e4b, ce28f19)
- Wake the touch controller as soon as the watch is picked up, put on, or the wrist turns, rather than leaving taps dead until sleep tracking sees the wearer up; keep it on through a nap, where taps now wake the screen and play music as by day; and let it sleep on a desk, where taps do nothing until the watch is picked up; in a night's (or a morning doze's) quiet, taps with the screen off still do nothing, as bedding makes them (39561bc)
- Keep the activity the phone hasn't fetched (steps, heart rates, and the night's sleep so far) across a crash or a watchdog reset, rather than losing everything since the last sync (a hard fault at 06:19 once lost a whole night): every 2 minutes the watch saves it to its flash, and the next boot puts back the latest save, so a crash loses at most the last couple of minutes; this costs about 1.5% of the battery (da92071)
- Stop the touch controller's reset from breaking a heart rate or accelerometer reading that happens to be under way: the three chips share a bus, and the controller is reset each time it's put to sleep or woken (some 170 times a quiet night, at every stir), which three times in four days cut another sensor's reading short, once leaving the bus stuck until it was recovered. The others now wait out the reset (25 ms) (08817cc)
- Stop the battery percentage stalling and then dropping (on a four-day trip it took 31 hours to go from 55% to 42%, then 3 hours from 40% to 32%): the curve from voltage to percent is now fitted to this watch's own discharges in the field logs, so the percentage falls at an even pace through a charge. The same voltage now reads up to 7 points lower below about half (what read 38% reads 31%), and within a point or two of before above it; the lowest third is an estimate, on the cautious side, until a watch has been run to empty with the logging build (10dca38)
- Stop the battery percentage falling twice as fast as the charge through the low twenties: the curve bent at 3.735 V on a guess, and a further night without a charge shows the cell holds its pace down to 3.717 V at least, so the same voltage now reads up to 6 points higher between 3.64 V and 3.756 V (what read 18% reads 24%); below 3.717 V it's still an estimate, as the watch has yet to be run to empty, and after an update the percentage can step by a few points either way, as each boot measures the voltage a little differently (79dbe7f)
- Stop losing night heart rate readings to the watch's once-a-minute review of the night's sleep, which by morning held everything else up for as much as a tenth of a second, just as a reading began (9 readings lost in four nights on 0.11.3-rc.3): the review now takes about a fifth of the work per minute asleep, with the same sleep stages, and night readings start 5 seconds into the minute rather than on it (6723b13, 03e75c3, 184acc7)
- Stop a sleeve or a folded arm playing, pausing, or skipping the phone's media: a double tap's two taps (and a tap-and-hold's tap and hold) must now start on the same spot (within a sixth of the screen's width of each other) and in the middle of the screen, a fifth of it away from every edge, whether the screen is off, on a glance, or on the clock face, where they used to count anywhere. Of the 27 media commands in a week's field logs, that turns away 24, and keeps the three that look meant; a deliberate double tap near the edge, or with its taps apart, no longer does anything (7e19b17)
- Stop a brush at the screen's edge snoozing a ringing alarm or timer: a tap snoozes only if it starts away from the very edge (a tenth of the screen), as a tap that wakes a dark screen must (01b7876)
- End a quiet night with two presses of the button up to 2 seconds apart, rather than half a second, which unhurried presses never made; a single press is no slower, as the first press still wakes the screen at once. In a quiet night, pressing again within 2 seconds to turn the screen off now ends the night instead (20d61bc)
- Keep the logging build's (`ur-logging`) last log lines before it switches off or restarts for an update: the flush that writes them out could return before they were written when another part of the watch was using its storage at that moment, and they were lost. The `ur` build is unchanged (9cfb2dd)

### Security

## [0.11.2] - 2026-11-14

The new idea in it: **timers on the glance**, a look at the wrist
showing how long each running timer has left.

### Added

- Show each running timer's time left on the time glance, soonest first, under the time (one row, as on the clock face, while a timer runs), with an hourglass, as "42 MIN", "2H05", or minutes and seconds that tick under 10 minutes (23bdb6c)

### Changed

### Deprecated

### Removed

### Fixed

### Security

## [0.11.1] - 2026-11-13

The new ideas in it: **charger screens**, the screen lighting up as the
watch is set on its charger with the time to full, glancing there every
5 minutes, and buzzing and saying **FULL** once it's done; **no
heart-rate LED on the charger**; **fewer and truer pairing banners**; a
**slow second tap** that wakes the screen rather than playing or
pausing; and a **shorter doze** after a morning alarm or double press,
the quiet night ending 7 minutes after waking from it.

### Added

- Light the screen within a second of setting the watch on its charger, silently (dimly in a quiet night; a wobbly cradle lights it once), to show the charge and the time to full in each theme's own drawing (an estimate that errs long: up to about 20 minutes over, a couple under at worst); the button, a tap or a glance there shows the same (the next press or touch goes on to the clock face), and until it's full it also shows by itself every 5 minutes for 10 s, dimly and silently, outside a quiet night and not over anything on screen (8bb97c0, 523b654, 57541a1)
- Buzz twice and show FULL on the whole screen for 4 s once the charge is done, so it's plain the watch can come off, in each theme's own drawing (Retro's a big bolt, Living Tokyo's its city under a full moon, struck by lightning): once a charge, not for a watch set on the charger full, after any alarm, call, "find my watch", pairing, update or alarm editor on screen has ended, and not if the watch has come off by then; in a quiet night it waits, dark, and shows without the buzz the first time the wearer wakes the screen after the night; the button or a touch puts it away (101db71, 793f77f)

### Changed

- Wake the screen, rather than play or pause, at a second tap too slow for a double tap: with the screen off or on a glance, outside the quiet night, a tap away from the edge that starts 0.3–1.5 s after the first one lifted now wakes it however light (a retry, the first tap having done nothing visible), where with a phone connected one within about half a second used to play or pause, and a later light one did nothing; so a double tap, or a tap-and-hold for the next track, now needs its second touch to start within 0.3 s of the first tap's lift (in field logs, they started within 0.2 s), even right after a tap that woke the screen, and a lighter firm tap on a glance wakes the screen after 0.3 s rather than 0.10.1's half second (60dba38, 0afa4ad)
- End the quiet night 7 minutes, rather than 22, after waking from a doze that follows an alarm or a double press of the button before noon (even in the small hours): dozing off again before noon still resumes the night (muted and dim, and the alarm doesn't ring again), but waking from it is getting up, as from a nap; after an alarm or double press after noon, the next sleep is still a whole night (374a9d7, 3c7bb4c)
- Never light the heart-rate LED on the charger, charging or full: a measurement under way stops within a second (counting as neither a reading nor a failure, unless it had already read), the clock face says OFF for the heart rate, and the watch counts as off the wrist for sleep tracking and the temperature as soon as it's set there, rather than up to an hour later; taken off, it measures again after two minutes, or after 10 s if the clock face is looked at (f95500e, b3c315a)

### Fixed

- Stop a phone or computer that connects without pairing from nagging the wearer and holding one of the watch's two Bluetooth connections, which could keep the phone out (introduced in 0.9.0): it's disconnected once it can no longer pair (40 s after connecting, or a minute after its passkey showed, which also clears a stalled pairing's passkey from the screen), and the "NOT PAIRED" and "PAIRING LOST" banners each show and buzz at most once every 5 minutes, however often such devices reconnect (Info's PHONE row still says so while one is connected) (794b3a9)
- Stop saying "PAIRING LOST" (or "NOT PAIRED") when a connection is merely dying, as when the phone reconnects at the edge of range as the wearer walks away (introduced in 0.9.0): the watch now says so only once the device has talked to it and the connection has lasted about 15 s, or at once when a pairing fails (afcfeb5)
- Leave out the `:shortcodes:` Gadgetbridge sends by default in place of emoji from a notification's sender or title, a caller's name, and the music screen's artist and track (`Book club :books:: Ada` is now `Book club: Ada`, as with the emoji), leaving ordinary colons (`Re: lunch`, `10:30`) alone; a caller's name and the music's texts also leave out other emoji rather than show `?`, end in `…` when cut short, and a caller whose name shows nothing gets their number (5969889, bd13cea)
- Stop the minutes just before falling asleep reaching Gadgetbridge, and so Health Connect, as awake on the odd night the watch then counts them as the start of the night (or of a doze): a fetch while settling down on the sofa could send them before the watch could tell, and Gadgetbridge keeps the first kind it gets; the watch now holds them back until it knows, which can delay sending a nap of under about 55 minutes by up to about 10 minutes (614731d, 66647c2, e955a76)
- Send Gadgetbridge, and so Health Connect, the activity recorded after the phone puts right a clock that was ahead (and had set the watch's ahead too): the minutes from the corrected time on used to be skipped, up to where the last fetch while ahead had reached or to the fetch after next, whichever came first (adfc271)
- Show the day's colour schemes and daytime brightness by day where the sun sets after local midnight, rather than the night's all day: in Tromsø the few days either side of the midnight sun, Reykjavik around midsummer, and anywhere while the phone's time zone is hours from the last location the watch had (1f7bfb3)

## [0.11.0] - 2026-11-12

The new ideas in it: **themes**, each drawing every screen its own way
and each other than Retro published as a package of its own, the first
being **Living Tokyo**, a city at night by Matteo Bigoi, who also brought
**Tokyo Night's colours** to Retro; **FORCE**, to show the day's or the
night's colour schemes at any hour, with Settings split into **LOOK** and
**BUTTON GRID**; **smart wake that waits for light sleep**; and **the
night in Gadgetbridge and Health Connect soon after you're up**, rather
than hours later.

### Added

- Add themes: how every screen looks is now a theme, chosen when the firmware is built, with Retro, today's look, in `ur` and `ur-logging`; a release also has a package for each other theme, `ur-<theme>-<version>-dfu.zip`, with only that theme in it (one with colour schemes only for day or only for night shows them at all hours; FAQ D02), and each theme's README has a "Try it live" link to it in the web simulator; a build of your own can have any themes, instead of Retro or beside it (`theme-<name>`, or `themes-all` for every one), THEME then stepping through all their schemes for the time of day shown (6ff3133, c2d0b0c, bfa2d57, 94e8378)
- Add Living Tokyo, a night-only theme published as its own package, `ur-living_tokyo-<version>-dfu.zip`: a city at night on the clock face, with the time and seconds across the top, the moon as the battery, windows lit by the day's steps, a beacon blinking, a sign showing the heart rate, and a billboard for unseen notifications (in place of the top edge's handle), in two schemes, LIVING TOKYO and LIVING STORM; its other screens are Retro's in its colours for now, and alone in its package it shows the city by day too (FAQ D02); contributed by Matteo Bigoi (6865af1)
- Add Tokyo Night's colours, from folke's tokyonight.nvim, as three more of Retro's colour schemes, in every package with Retro: TOKYO NIGHT and TOKYO STORM by night, after ARCADE and TERMINAL, and TOKYO DAY by day, after HANDHELD and PARCHMENT, so a watch with nothing saved still opens on ARCADE and HANDHELD; contributed by Matteo Bigoi (6865af1)
- Add a FORCE setting, under THEME on LOOK (the new first half of Settings): NONE follows the sun as before, and DAY or NIGHT shows the day's or the night's colour schemes whatever the time, THEME then stepping through those and saving the choice for that time of day; it changes only the look, while the brightness, alarms, sleep, and everything else still follow the sun. With only one scheme for the time of day shown, THEME is dim and can't change, and nor can FORCE in a package with schemes only for day or only for night (Living Tokyo's, say; never `ur` or `ur-logging`): a tap on either says why, "ONLY ONE THEME" / "SEE FAQ D01", or "NO DAY THEMES" (or "NO NIGHT THEMES") / "SEE FAQ D02" (98b856d)

### Changed

- Ring smart wake early only at a stir after 10 minutes of light sleep (none of them deep or REM, and not all still), or else at its time, rather than at the first stir in its window, which on most mornings came within its first few minutes, from deep or REM sleep as often as light: replaying four nights against a Garmin, early rings from deep sleep went from 11% to almost none, and from REM from 37% to 21%, while about one alarm in four now rings at its time, as a plain alarm would (a7f266e)
- Send the night to Gadgetbridge, and so to Health Connect, as soon as 47 minutes after you wake rather than 2¼ hours after: once you've been awake that long, walked or had the watch off your wrist for at least two minutes of it, and aren't lying still, the watch stops revising the night's sleep and stages and the next fetch gets them, and the morning's steps after that are no longer held back (but for a still spell of 10 minutes or more, which may yet be a nap); a lie-in after that is a sleep of its own, and a morning without walking or taking the watch off, or a short nap, waits the 2¼ hours as before (introduced in 0.10.0) (79c00c8, f41606c)
- Split the Settings screen in two, side by side where it was, to make room for FORCE, so Info is a swipe further out: LOOK, with THEME, FORCE, and LIGHT, and BUTTON GRID, with BUTTONS, GRID, and BTHOME KEY; with three rows at most on each, every row is nearly half as tall again (about 6 mm rather than 4), easier to hit with a fingertip, and no row shifts when another appears or disappears (98b856d)
- Save the colour scheme chosen for day and for night by name rather than by its place in THEME's cycle, which now depends on the themes built in, so an update, or another theme's package, keeps it if the new firmware has it (firmware without it shows its first scheme for that time of day, but keeps yours saved until you pick another); a watch updated from 0.10.0 or 0.10.1 keeps its own (59aa7ad)
- Try longer heart-rate measurements on a moving wrist in the logging build (`ur-logging`), an experiment whose field logs will say whether more time reads more, and how well: on half the background measurements while awake, one that has no reading after its usual 30 s on a moving wrist goes on to 90 s, and one on a moving wrist that reads early keeps recording to 90 s (its first reading counts as before; what it reads after is only logged, unless you look at the watch meanwhile), each only when the hourly LED budget can spare it; and every measurement while awake now logs the accelerometer's samples alongside, to tell a walk's rhythm from the pulse. That fills the log faster, so on a worn watch it holds about 20 hours (fetch it at least twice a day), and costs ~2 mAh a day (up to ~8 if the light draws more than its datasheet says). The `ur` build is unchanged (5b59a9a, ec64aa6)

## [0.10.1] - 2026-11-11

A patch for 0.10.0's **media gestures**: a tap, then a hold, skips to the
next track again, and a firm first tap no longer wakes a glance.

### Fixed

- Skip to the next track again with a tap, then a hold (introduced in 0.10.0), rather than playing or pausing, often twice with two buzzes: the touch controller sometimes reports a finger held still as lifted and pressed again about 10 ms later, which cut the hold into short taps that never reached 0.6 s, two of which made a double tap; a press that quick, near where the finger lifted, now continues the same touch, which also keeps a button grid hold from sending a second hold partway through (8fe4930)
- Keep a glance on screen through a double tap or a tap, then a hold, whose first tap is firm (introduced in 0.10.0): such a first tap used to wake the screen at once, the gesture then acting on the clock face it woke to, so whether a glance stayed depended on how firmly it was tapped; now only a heavy tap (75 ms or more) wakes a glance at once, and a firm but lighter tap on its own wakes it half a second later, once no second touch can follow (e6b36c8)

## [0.10.0] - 2026-11-11

The new ideas in it: a **button grid** (Home Assistant over BTHome, or
Tasker, with a blueprint for each layout) and a **Settings screen**;
**room sensors** (Aranet4 air monitors and BTHome thermometers) with an
**Air screen**, and the **room's temperature** on the clock face, from a
sensor or the wrist's self-calibrating estimate; a **redesigned clock
face** (the day's steps as blocks towards 10,000) and **notifications**;
**new media gestures** (double-tap to play or pause, tap then hold to
skip, replacing 0.9.0's); a **heart rate** that reads more often, at a
desk, through movement, and from a faint pulse, and **sleep stages**
closer to a Garmin's; and **every step and the settled night in
Gadgetbridge and Health Connect** (5-minute samples, the night sent in
the morning).

### Added

- Add a button grid for Home Assistant or Tasker, the first screen below the clock face once turned on in Settings, of retro arcade buttons in one of two layouts (Settings → GRID): six buttons (the default), each tapped, double-tapped, held, or long-pressed (held 1.5 s), with a hold's release sent too (for hold-to-dim), which reach Home Assistant as BTHome button events of their own, `button_1` to `button_6`, a trigger each in the automation editor; or two layers of nine buttons, tapped alone or in pairs for codes 1–99, which reach Home Assistant as a button event for the gesture plus a dimmer turned by the code (FAQ B04, which the watch points to as "*B04", explains why a long press turns it left); sent over Bluetooth as encrypted BTHome, or through Gadgetbridge as an Android intent (see docs/buttons.md); a press that can't go to Home Assistant turns red, with "BTHOME NOT SENT" / "SEE FAQ B05", and with Gadgetbridge's "Allow Intents" off, the watch says "INTENTS OFF" / "SEE FAQ C04" (3f335fe, 4accb11, 92340bc, ff82ac9, e754fa6, 02160d9, 24d1126, 28f90ae, eb68544, c470936, 77bc5d6, 3d06fc2, 6c5db54, 6fa7f9f, 11624d7)
- Add a Settings screen right of the clock face, between Alarms and Info: THEME and LIGHT (moved from Info), BUTTONS (OFF by default, or where the button grid's presses go: BTHOME, VIA APP for Gadgetbridge, or BOTH), GRID (its layout), and, while BUTTONS includes BTHome, BTHOME KEY, the button grid's key for Home Assistant, as a QR code to scan and paste, or as text, either on screen until tapped away (3f335fe, b6cb498, 7c69c61, 02160d9, 5386e38, 3cae84f, 5d1fc55)
- Add a Home Assistant blueprint for each button grid layout, imported with one click (see docs/buttons.md), to set every button's actions in one automation from the editor rather than one automation each: for six buttons, an action per button for a tap, double tap, hold, long press, and release; for two layers, the same for each button on its own (bar the double tap), eight slots for any code and gesture (23 double-tapped, say), and one action for every press, given its `code` and `gesture` (f4a051f, 4d43551)
- Read a nearby Aranet4 air monitor's advertisements (with its Smart Home integrations on), listening briefly every 15 minutes while one is around and hourly otherwise, and show the nearest one's CO₂ (with its band), temperature, humidity, pressure, battery, and age on a new Air screen below Music, which is there while one was heard within the hour (e1711da, 099659e)
- Read nearby BTHome thermometers too (unencrypted, such as a Xiaomi LYWSD03MMC on pvvx's firmware) as room sensors alongside Aranet4s: the nearest of either, if heard stronger than −80 dBm (so a neighbour's thermometer through a wall doesn't become the room's), gives the clock face's temperature, and the Air screen shows its temperature big, with its humidity, pressure, and battery if it sends them; one that sends its temperature every few seconds is heard at each look, one that sends seldom (a Shelly BLU H&T, once a minute) only now and then, and a new sensor is found within about two hours even beside ones the watch already hears (d71ecbb, 79bce0e, 5151105)
- Forget room sensors as soon as the phone's location (when it sends one) is over 200 m from where they were heard, without waiting out the hour since they were last heard: the Air screen and the sensor's temperature on the clock face go at once, and the watch looks for sensors only hourly, until a location back within 200 m has it look again at once; only locations the phone says are good to 75 m count (a coarse one keeps the phone looking, for up to the usual 2 minutes), and the watch asks the phone for one when its scans stop hearing the sensors (at most hourly, and four times a day), and when it first hears them after starting (db26de7, 6615c88, 6112ed8)
- Show the heart rate and steps live in Gadgetbridge's Live activity chart (Charts, "Live activity"): while it's open, the watch measures the heart rate continuously and sends a reading and the steps since the last one every 10 s (or as often as Gadgetbridge asks), until the chart closes or the phone disconnects; off the wrist it doesn't measure until the chart is reopened (a8c4c5f, f9cd658)
- Say "TEXT AS BITMAPS" / "SEE FAQ N02" (once per connection) when Gadgetbridge sends a word as a picture, as its "Text as Bitmaps" setting does for words with characters the watch can't show, and show each such word as a `?` rather than a run of junk; FAQ N02 says how to switch the setting off (46d43f3)
- Add a guide to Gadgetbridge's settings for ur (docs/gadgetbridge.md): which to change, which to leave alone, and which are up to you, each with where it is, its default and why (above all "Auto fetch activity data", off by default, without which a day not fetched by hand is lost after 24 hours) (d1bf562, 76b493f)
- Add FAQ U03, for a phone that has to pair again after every update: turn on "Keep bond" in Nordic's nRF Device Firmware Update app (5b48603)

### Changed

- Play or pause media with a double tap, and skip with a tap then a touch held until the buzz (0.6 s), with the screen off, on a glance, or on the clock face, rather than with a long press and a double tap, so an arm folded over the watch no longer plays or pauses (as it could since 0.9.0); a long press on its own now does nothing, and with the screen off or on a glance, the hold must be in the middle of the screen and a touch near the edge does nothing (1c1460d, 41c9753, 2e774fa, 9abc714)
- Redesign the clock face's lower half: the date with small seconds under the time, the heart rate and the next alarm or timer in one row, and the day's steps as ten blocks rather than a number: 1,000 a block (×1K) towards a fixed goal of 10,000, then 10,000 a block past it, its scale (×10K) in the accent colour to mark the goal reached, full at 100,000, the blocks still to fill faded even in the dim look (the exact count is on Today, a swipe right) (a02d355, 718c01d, 6147206, d4525fe)
- Show the room's temperature on the clock face rather than the watch's own: a nearby room sensor's, to the tenth (`21.4°`); or else, on the wrist, an estimate from the watch's own, to the degree (`21°`), which calibrates itself against any room sensor it meets reading 10–35 °C while worn (roughly ±2–3 °C indoors once calibrated), and holds through a still night while heart rate readings keep coming; or, off the wrist, the watch's own, to the degree (e1711da, 13c30c8, 216a6f9, 373457b)
- Show a faded Bluetooth rune struck through in red on the clock face while the phone isn't connected, rather than a Bluetooth rune while it is; the note for music playing moves to the top left corner in its place (2e1f1e1, a1c8505)
- Redesign notifications: with the screen off, and in the history, set who it's from big and centred only if it fits on one line, or on two of about equal width, rather than whenever it fit on four lines (three in the history), which left stubs such as `ADA / LOVELACE`, and otherwise centred in a new bold medium face over as few balanced lines as it takes, rather than small; show both as one card, the app top left over a rule, and in the history when it arrived (as `SUN 20:13`) top right rather than at the bottom; let the banner take a second line for a long name, ending in an ellipsis, rather than scrolling; and leave a slightly wider margin either side of notification text, as a call's has (86ebb6a, 5f98db4, 116942b, 0eaa4c1)
- Show labels, titles, the seconds, and other secondary marks in red in the dim look (glances, notifications with the screen off, and the quiet night), rather than in the same white as the text; on Last night, deep sleep is white, REM red, and light sleep grey (30fb455, ef1d58c)
- Read the heart rate at a desk more often: up to 3 s of hand or finger movement in the 8 s the watch measures over are now left out, and the rest used, rather than the whole 8 s thrown away, and the first conversion after the sensor lights, which comes out darker, no longer drowns the first seconds' pulse; replaying field logs, desk sessions that ended in a reading went from 4 to 18 of 131 (within 5 BPM of a Garmin 9 times in 10), and night readings come sooner, at the same accuracy. A heart rate under 50 now also needs a still wrist unless you're asleep, so fidgeting at a desk isn't read as a slow pulse (5abc884, 676656c, 0b1c769)
- Read a faint pulse on a still wrist, such as lying down awake with cold hands: there, two 8 s stretches in a row that agree within 3 BPM now make a reading, even if neither is clear enough on its own, so most readings also come ~2 s sooner (a night's LED time falls ~15%). Replaying field logs against a Garmin, readings went from 106 to 118 of 252 sessions, as accurate as before, and an hour lying still that gave none now gives one (4aefe15)
- Keep measuring the heart rate through movement: a background measurement waits at most two minutes for a still moment to start (or not at all once there's been no reading for half an hour), rather than however long it takes (busy desk work held one back 18 minutes), and then runs its full 30 s however the wrist moves, rather than giving up after 12 s; to make room, the hourly LED budget rises from 180 to 300 s, at most ~5.6 mAh a day rather than ~3.3 (bd66bc4, f4bbe32)
- Try harder for a heart rate while measurements keep failing: after one fails on a still wrist, try again within 10 minutes rather than backing off to 20; and once there's been no reading for half an hour, asleep or awake, retry every failed measurement within 10 minutes until the next reading, rather than backing off to 20 minutes or an hour (unless the watch is off the wrist), with the hourly LED budget doubled meanwhile so more and brighter tries fit; at worst (a whole day without readings), that's ~5.6 mAh a day more (54c553d, 12e5c93, 090c5a3)
- Try a brighter heart-rate light, an experiment whose field logs will say whether more light helps, at a desk above all: awake, every other background measurement, together with its retries until one reads, now starts at 30 mA rather than 12.5 (or 20 mA if 30 would overwhelm the sensor), so a day's log compares the two on the same wrist, with first tries and retries alike on each; and one that gives no reading in its usual 30 s, on a calm wrist with only a faint pulse, now goes on for up to 15 s more at a brighter setting (and once more at 40 mA if that fails too). The hourly LED budget now counts a brighter second for the current it draws, and a brighter try is only made when the budget can spare it, so ordinary readings aren't held back (633489f, 6a47d2e, 28900de)
- Tell sleep stages apart better: deep sleep now also needs a heart rate that holds steady from one reading to the next, and REM a smaller rise (4 BPM over the slowest of the surrounding hour rather than 5) along with the odd twitch of the wrist; against a Garmin worn alongside, a night that came out with too much deep and too little REM went from 48% to 61% of minutes in agreement, and an earlier one from 67% to 70% (b526bbd)
- Move Info to the right of the clock face, last after Alarms and the new Settings screen, rather than at the end of the screens below it, and its THEME and LIGHT rows to Settings, leaving Info to the watch's status; storage mode's long press moves with Info (now with a "HOLD: STORAGE MODE" hint at its foot) (5f5acf2, 3f335fe)
- Answer Gadgetbridge's "Find lost device" with a screen of its own, "FIND MY WATCH" over a big HERE with rings spreading round it, and the motor's loudest buzz, rather than a call's buzz alone; it now stops at the button or a tap (whoever touched it found it), as well as at the phone's "Found it!", and after two minutes at most, rather than ringing until the phone said stop. It still rings in the quiet night (in the dim look), and waits its turn behind a ringing call or alarm (f86fd26)

### Fixed

- Get every step into Gadgetbridge, and so into Health Connect, which kept only about 40% of them (a day's 10,346 arrived as 4,206): Gadgetbridge merges samples 2 minutes or less apart, keeping the last one's steps, and the watch sent one a minute; it now sends one per 5 minutes, with all their steps, the last heart rate reading (earlier ones in the same 5 minutes are dropped), and the sleep stage most of them had, so Gadgetbridge lags the watch by up to 5 minutes, and sleep there is to the nearest 5 minutes. Steps also land in their own minutes in Health Connect, rather than a minute early. Updating from 0.9.0 counts no step twice in Gadgetbridge, at the cost of up to 5 minutes' steps once; and each fetch sends only what Gadgetbridge doesn't have yet, rather than the last 2¾ hours again (13b318e, 8f7afee, bba7496, 98f8253)
- Stop sleep stages freezing half-revised in Gadgetbridge and Health Connect: Gadgetbridge keeps the first sleep stage it gets for a sample, so a fetch during the night, or soon after waking, froze stages the watch was still revising there for good. The night now reaches Gadgetbridge in the morning, once its stages are settled: about 2¼ hours after you last slept, the first fetch gets the whole night at once, steps included, and passes it on to Health Connect straight away; until then, fetches stop where you fell asleep (or a nap began, or the last 20-odd stepless minutes of sitting still, in case it's sleep), holding back at most 18 hours (introduced in 0.9.0) (6ce2535)
- Make the Health Connect export from Gadgetbridge ("Sync after device sync") keep running, rather than only once when turned on: the watch now ends its first sync after starting, and then a sync at most every 2 hours, in a way that Gadgetbridge counts as a finished sync, so each export catches up on everything since the last one and Health Connect lags the watch by up to 2 hours (Gadgetbridge shows a short "End : Fetching sports details" each time) (f733856, cf88807)
- Get notifications, music, and alarms from the phone onto the watch within about a second once the connection has been quiet for 30 s, rather than a few seconds a line (a long notification took up to 12 s, a burst of them longer): the watch now asks for Bluetooth's long packets, and while the phone is sending, listens at every turn rather than every eighth (4b3ed08, b4575d7)
- Reconnect Gadgetbridge, once, when it connects but never subscribes to what the watch sends (it then shows no battery and firmware "N/A", and gets nothing from the watch), as it can when it asks before the first pairing has finished; it reconnects at once and subscribes properly, and the new FAQ C05 covers the rare case where that doesn't help (ff3b3b2)
- Show long notifications in scripts beyond Latin-1 (Cyrillic, Greek, CJK, Arabic…), which were lost whole, never shown nor buzzed, once their body passed about 150 characters (Gadgetbridge sends each such character as six bytes, and the message outgrew what the watch could take in); likewise, a very long caller, track, street, or title, such as one that Gadgetbridge's "Text as Bitmaps" turns into pictures, is now cut short rather than losing the whole message (47fa44d)
- Keep notification names (the sender, or without one the title) of up to 40 characters whole, however many accented letters such as æ, ø, and å they have (in 0.9.0, 20 of them filled it), and end one that's cut with an ellipsis rather than passing it off as whole; leave emoji out, rather than showing each as `?` (and show the title for a sender that was only emoji); and draw Ø and Å so they don't read as an 8 (86ebb6a, 00024b0)
- Show `&`, `#`, `@`, `"`, `*`, and the rest of ASCII's symbols (and `×`) in notifications, and curly quotes, dashes, and ellipses as their plain look-alikes, instead of `?` (929a956, 718c01d)
- Show the battery level without overstating it, so it no longer drops by ~10 points minutes after the watch comes off the charger: the percentage now follows the PineTime cell's voltage curve rather than a straight line, which overstated the middle of the range by 10–20 points (so red under 20% now means under 20%), climbs on the charger no faster than the cell fills, to 99% until charging is done, and off the charger never climbs and falls at most a point every 3 minutes; Gadgetbridge and Home Assistant get the same number, Gadgetbridge now every half hour even when it hasn't changed (7f78fe8, ff3b3b2)
- Restart and charge when plugged in during the "battery empty" switch-off, rather than staying off until the battery runs flat again, possibly for months (introduced in 0.9.0) (3baa4ec)
- Put the accelerometer, the heart-rate sensor, and the touch controller to sleep when the watch switches itself off (battery empty, or storage mode), even if one doesn't answer at first: one failed write used to fail every write after it, so the watch could switch off with the accelerometer running (~50 µA, more than everything else left on together), or the heart-rate light lit (12.5 mA or more) if a measurement was under way, draining the battery the switch-off was meant to save (not yet seen on a watch) (d5b117e)
- Stop the watch sometimes resetting as it saved something to its flash (a setting, the alarms, a pairing, the temperature calibration, the button grid's counter, or confirming an update), or, more rarely, never finishing the save, which kept every later save from happening until a restart (caead12)
- Refuse a firmware update that's damaged, cut short, or not firmware for this watch, checking it as it arrives and again before restarting into it, and say "UPDATE REFUSED" / "SEE FAQ U02" while keeping the running firmware, rather than restarting into the bootloader only for it to silently refuse it (or, for an image built for another device, install it); also give up on an update whose sender goes quiet for a minute, rather than keeping the progress screen lit while it stays connected, and take one update at a time (c569ee4)
- Recover the step counter, heart rate, and touch when the sensors' connection gets stuck, as a restart (such as an update's) can leave it, rather than counting no steps, measuring no heart rate, and ignoring touches until the next restart: the sensors now keep trying to start, ever less often down to once a minute, and the connection is freed whenever a reading fails, not only at start-up (21d3cc1)
- Switch the heart-rate light off even when the first try fails, retrying every second, rather than leaving it lit (12.5–40 mA) until the next measurement, up to an hour away, or for as long as the clock face showed (not yet seen on a watch) (7f3bb9c)
- Keep counting steps after a garbled reading of the step counter, which could add billions of steps to the day and then count none until a restart (not yet seen on a watch) (3f913b9)
- Ring the smart alarm when its time passes while another alarm rings, rather than not at all that morning (an alarm at 06:58 left to ring out skipped a smart wake at 07:00): alarms whose times come round while another rings now queue, each ringing as soon as the one before is done, at most an hour late; and an alarm at the smart alarm's own minute no longer makes the watch ring again straight after it's dismissed or snoozed (8c420f3)
- Ring each alarm, the smart alarm included, exactly once on the nights summer time starts and ends: one in the skipped hour (02:30, say) now rings as the clock jumps to 03:00 rather than not at all, and one in the repeated hour rings in the first 02:00 only, rather than in both; beyond an hour, a new time zone still skips what it jumps over, and rings again what comes round again (2c6e399, 8c420f3)
- Keep a one-off alarm from the phone off once it has rung at the same minute as one of the watch's own: it was saved as still on, so after a restart before Gadgetbridge next sent its alarms, it rang again the next morning (introduced in 0.9.0) (fb51f17)
- Keep an alarm ringing through a call: the call ending (answered, declined, or over), or Gadgetbridge's "Find lost device" stopping, left the alarm on screen but silent; and stop a notification arriving during a call from cutting its buzz short (3451cca)
- Keep timers and the snooze counting from when they were set, whatever the phone does to the clock: a new time zone shifted them by the difference (a 10-minute timer took 3 h 10 min after flying three zones west), and the clock set far back made opening a running timer reset the watch; a timer now also runs before the phone has set the time (34f5662, 9c65743)
- Count today's steps from midnight in the new time zone after a flight across midnight, as phones do, so the clock face and Today agree: flying west, the evening after landing counted from 0 as a new day (the clock face said 5 steps while Today said 1,205), then started over again at the new zone's midnight; flying east, the count started over at the change rather than at the new zone's midnight. Steps taken before the phone first set the clock that day are dropped by such a change, and a new time zone on the same date leaves the count alone (3438411)
- Keep the day's activity, sleep, and Gadgetbridge's samples straight when the phone puts its clock back (because it was wrong, say): what was recorded under the clock that was ahead is dropped, rather than the minutes recorded from then on being added on top of it (or, over a day back, filed under days still to come and sent to Gadgetbridge as such), with sleep tracking stuck until the clock caught up; and the record the watch saves on switching off now keeps what was recorded since the clock was put right, rather than none of it (81d36d7, b8ecd08)
- Stop a malformed alarm list from the phone hanging the watch until the watchdog reset it (on every connect, if the phone kept sending it; introduced in 0.9.0), and ignore a time set before 2020 or after 2100 (e24a3bb)
- Turn the screen off after a notification that lit it in the dark, even if a second one arrived just as the quiet night began while the alarm editor was left open: the screen stayed on, dimly, until the next touch or press, all night if need be; the screen going off now closes the alarm editor (keeping what was edited, as it's saved as it changes), so the next wake is to the clock face, where the button turns the screen off again, rather than back into the editor (323af29)
- Leave no empty space before the temperature on the clock face once media is paused or stopped: the temperature now moves to where the media icon was once the icon has gone (introduced in 0.9.0) (fcd0a6d)
- Draw deep, REM, and light sleep on Last night in three distinct colours in every theme; deep and REM were the same red in Arcade and Parchment (introduced in 0.9.0) (a1f642d)
- Make releases rebuild byte for byte whether or not the builder already has the dependencies downloaded; 0.9.0 only reproduces from an empty cargo home (e49cb26)

### Security

- Keep pairings from outside meddling with the phone's bond: a pairing without the passkey no longer leaves a bond (one claiming the phone's address replaced the phone's), a failed one no longer drops the phone's bond until the next reboot, and after each failed pairing the next waits longer, up to a minute, so a passkey can't be guessed at speed or the screen kept lit (1700f15)

## [0.9.0] - 2026-11-08

The first release published on GitHub. The new ideas in it:
**quiet nights** (notifications stay silent and dark while you sleep or
nap, until your alarm), **screens around the clock** (swipe for today and
last night, alarms, notifications, music, and info), **sleep stages**
(light, deep, and REM), **directions on the glance** while the phone
navigates, **media gestures** (long-press to play or pause, double-tap to
skip), **storage mode**, and a **paired phone** is now required (FAQ C01
if the watch says it isn't).

### Added

- Keep the last three notifications above the clock face, one a page, each with when it arrived, and a mark on the clock until you swipe down to them (0b74bd9, 9bd8cc8, de479e1)
- Play, pause, and skip media whenever the phone is connected, from the clock face, a glance, or with the screen off (long-press to play or pause, double-tap for the next track; only on the awake clock face during a quiet night), with a double note beside the Bluetooth icon while it plays (6667727, b9d4477, 36aa727, fff3477)
- Show the next turn on the glance while Google Maps, OsmAnd, or CoMaps navigates on the phone (an arrow, the distance, and the street), and buzz twice and show it as a turn comes within 50 m (49f66b8)
- Show today's steps and heart rate hour by hour, with a resting heart rate, and last night's sleep with its stages, on screens of their own left of the clock face (the Info screen's SLEEP row moves there) (0b74bd9, 8519198)
- Tell light, deep, and REM sleep apart from movement and heart rate, rather than deep sleep from long stillness alone, and sync them to Gadgetbridge (and from there to Health Connect) (d9249b8)
- Keep notifications quiet while the wearer sleeps or naps, without a buzz or light, the screen dim and ignoring touches (alarms, timers, and calls still ring), until their alarm, until they've been up a while, or until they double-press the button (20b73a8, 1cf1618)
- Sync the alarms set in Gadgetbridge to the watch, where they ring like its own (ending a quiet night) and show, read-only, on a screen of their own (c1e45cb)
- Save the activity the phone hasn't fetched and switch off when the battery runs flat, or when put away in storage mode (long-press the Info screen, then tap), keeping it until the phone next syncs; the watch then doesn't know the time, or ring alarms, until the phone reconnects (9d067b6)
- Show the watch's own temperature on the clock face (close to your skin's on the wrist, drifting towards the room's off it) (7874b05)
- Record a backtrace and fault details for crashes, hangs, and stack overflows, reported in the logging build's field log at the next boot (1e5b869, 6b4ded1)
- Publish releases on GitHub, with packages you can verify came unmodified from the tagged commit, and rebuild byte for byte from it (bca9899, 63215e2)
- License the firmware as MIT or Apache-2.0, at your option (9c1ec42)

### Changed

- Arrange the screens around the clock face (the past to the left, alarms to the right, notifications above, music and info below), without wrapping round, with transitions that sweep the way you went and handles on the edges where a swipe leads (0b74bd9, 786cf99, e3760a9)
- Wake the screen only with a firm tap or a double tap away from the edge, rather than any touch, so brushing against something no longer lights it; while a phone is connected, a double tap is the next track instead, leaving the screen off (or a glance up) if its first tap is light, while a firm one wakes the screen at once (ce53224, 56050d9, 36aa727, fff3477, 7c131db)
- Act on a long press, and buzz, as soon as it's been held for 0.6 s, rather than when the finger lifts, so there's no guessing how long to hold (65217d7)
- Change the theme from a new THEME row on the Info screen, rather than with a long press, which now plays or pauses media (0b74bd9, 6667727)
- Show the firmware's real version on the Info screen (it always said 0.1.0), and the commit it was built from (63215e2)
- Measure the heart rate in the background every 2 to 20 minutes rather than every 10, more often after movement, during sleep, and while it's well above the wearer's resting rate, and only once the wrist has kept still for a few seconds, giving up early on one that keeps moving, so desk work no longer spends minutes of LED an hour on failed measurements (4979861, 77be9c2)
- Turn the screen off after 10 s without input rather than 15 s, for a longer battery life; the music screen stays on until a long title has scrolled by once (93360e3)
- Set the screen's brightness by the time of day: full while the sun is up, dim from midnight until the wearer gets up, medium otherwise; switching LIGHT to SOFT on the Info screen turns it down a step (ec80454)
- Use less battery while the screen is on, while it shows the dim look (now pure black and white, without greys), and while the wrist keeps still or the wearer sleeps (ec80454, 9ea33f8)
- Start wake alarms gently, building up their vibration and the screen's brightness over about a minute (c1e45cb, ec80454)
- Stack the glance's hours above its minutes, in bigger digits (999279a, c1d9ba3)
- Shrink the firmware by a sixth, so updates over Bluetooth send 70 KB less (0804e1a, 241ee01, 903438d, afc34f4, d401549, e190c6d)

### Removed

- Drop the weekday from the clock face (7874b05)
- Drop sunrise and sunset from the Info screen (e2d2774)

### Fixed

- Stop the watch resetting when a heart rate reading came in while the screen was being redrawn (introduced in 0.6.1) (1bd5e6b)
- Stop the watch crashing while a phone pairs, which made updates from the nRF DFU app fail on a link that wasn't paired (d7ca4d1)
- Restart at once after a crash, instead of freezing until the watchdog resets the watch, and restart on a stack overflow rather than running on with corrupted memory (6b4ded1)
- Keep today's steps, and the activity the phone hasn't fetched yet, across updates (3f55661)
- Stop sleep tracking seeing false wakes, and the activity history sent to the phone gaining tens of thousands of phantom steps a minute, when the accelerometer's data queue overflowed about every half hour (introduced in 0.8.0) (0609657)
- Keep glances paused when the wearer stirs in their sleep (introduced in 0.8.17) (0609657)
- Use less battery while the phone is connected but idle, by no longer asking it for a busier link than it had settled on after 30 s without traffic (introduced in 0.6.1) (0e8ec77)
- Ask the phone for its location at most once an hour when reconnecting, instead of running its GPS for two minutes at every reconnect while none arrives (6f7891d)
- Stop reporting a falsely slow heart rate when the sensor shifts on the skin after the wrist moves (1651aa5)
- Keep a ringing alarm or timer buzzing when a notification arrives, instead of silencing it (introduced in 0.7.2) (c1e45cb)
- Keep the screen on when an alarm or timer starts ringing during a glance (introduced in 0.7.2) (ec80454)
- Keep the screen on until a notification banner has gone, rather than turning it off under a banner that arrived late (93360e3)
- Keep a glance up when the phone reports a new track or play state, rather than waking the screen fully (introduced in 0.7.2) (b9d4477)
- Show music as playing, rather than paused, once a track changes or is skipped after the watch restarts mid-song, since the phone doesn't say again whether it plays (d5ee29a)
- Ignore touches while pairing or an update fills the screen, which could change the screen beneath unseen (0b74bd9)
- Ignore impossible touch positions, which could start phantom swipes (14fb597)
- Centre the sender of a notification that arrives with the screen off (introduced in 0.8.4) (c1adca7)

### Security

- Require a passkey-paired phone for the Gadgetbridge link, so nearby devices can't read activity history or send notifications; the watch warns when a connected phone isn't paired or has lost its pairing, and one that connected without pairing (as Gadgetbridge's Bangle.js support does by default) must pair once, typing the passkey the watch shows (d7ca4d1, 815509e)

## [0.8.17] - 2026-11-07

### Added

- Pause wrist-turn glances while the wearer is asleep (ed8faeb)

## [0.8.16] - 2026-11-07

### Fixed

- Wrap long notification senders over the whole screen, at a smaller size if need be, rather than scrolling them (66cc97e)

## [0.8.15] - 2026-11-07

### Changed

- Wait for the wrist to be still before measuring the heart rate in the background, so more measurements give a reading (119494e)

## [0.8.14] - 2026-11-07

### Fixed

- Blank the panel while it sleeps, so a glance can't briefly show parts of the clock face (1d407e1)

## [0.8.13] - 2026-11-07

### Added

- Record raw heart-sensor samples in the logging build's field log (92d488a)

## [0.8.12] - 2026-11-07

### Added

- Show charging unmistakably, with a red bolt and a filling battery icon (0f6eeee)

## [0.8.10] - 2026-11-07

### Added

- Log where the watch was stuck after the watchdog resets it (db1804f)

### Changed

- Pause the field log's flash writes during a firmware update (db1804f)

## [0.8.7] - 2026-11-07

### Fixed

- Accept more heart-rate readings from real wrists, by loosening the signal's steadiness and periodicity limits (39958b9)

## [0.8.6] - 2026-11-07

### Fixed

- Measure the heart rate on the wrist: the motion check no longer discards almost every measurement (bd9eba6)
- End a heart-rate measurement at once when the watch isn't worn, rather than raising the LED current for 30 s (bd9eba6)

## [0.8.4] - 2026-11-07

### Added

- Show a notification that arrives with the screen off on the whole screen, dim and white on black like a glance (b661680)
- Use red in the handheld theme for the heart, low battery, and update progress (b661680)

### Fixed

- Keep a glance on screen for its full 5 seconds, not under one (b661680)

## [0.8.2] - 2026-11-07

### Changed

- Recognise the wrist turning to look in ur's own code, since the accelerometer's built-in gesture never fired on a wrist (f84295e)

### Fixed

- Let a third device pair by dropping the bond of one that isn't connected, rather than failing (4af2f02)

## [0.8.0] - 2026-11-07

### Added

- Add a second one-off alarm, side by side with the first (fbe1cd6)
- Add a second timer, and count timers down to the second (2aea7dd)

### Fixed

- Read the accelerometer's movement data correctly, which sleep tracking and the heart-rate motion check depend on (2193842)

## [0.7.2] - 2026-11-07

### Added

- Add smart wake, which rings at the first stir in the 30 minutes before the alarm time, plus a one-off alarm and a timer (4e95b20)
- Glance at the time by turning the wrist while the screen is off (6f9d69c)

### Fixed

- Advertise often enough after a disconnect for Android to reconnect within seconds (introduced in 0.6.1) (8288e3f)

## [0.6.1] - 2026-11-07

### Added

- Log an hourly power summary in the field log (e9dba03)

### Changed

- Run from the DC/DC regulator, roughly halving current draw while the CPU or radio is active (ab5b207)
- Save power by asking for a slower connection when idle, advertising less while the phone is connected, and backing off failed heart-rate measurements (e9dba03)

### Fixed

- Stop lighting the screen every time the phone disconnects (ab5b207)

## [0.5.1] - 2026-11-07

### Added

- Track sleep from wrist movement and sync it to Gadgetbridge (e2c3cc9)

## [0.4.3] - 2026-11-07

### Fixed

- Fix firmware updates failing validation on the logging build, when a log write put the SPI flash to sleep mid-update (introduced in 0.4.0) (bb4d0e1)

## 0.4.1 - 2026-11-07

Built from changes that weren't committed until 0.4.3, so it has no tag.

### Added

- Log the wall-clock time to the field log every 10 minutes (bb4d0e1)

## [0.4.0] - 2026-11-07

The first build for a sealed PineTime, and so the first release. Until
that morning, ur only ran on a dev kit: it was flashed straight onto
the chip over a debug probe, and couldn't run under the PineTime's
bootloader, which is the only way onto a sealed watch. The version
numbers before this one (0.1 to 0.3) were test images for that dev kit,
never packaged for anyone to install. This release gathers everything
built up to that point, below.

### Added

- Show a pixel-art clock face with the time, date, battery, steps, and heart rate, plus an Info screen (afbadc4)
- Switch between dark and light themes at sunset and sunrise, with two of each to choose from (c0de571)
- Pair with a phone by passkey, and keep bonds and settings across restarts (1278c23)
- Sync time, steps, and heart rate with Gadgetbridge, which sees the watch as a Bangle.js (f6788a9)
- Show notifications, calls, and music from the phone (659913c)
- Count steps with the accelerometer (d387718)
- Measure the heart rate with the optical sensor (6141e57)
- Install under the PineTime's stock MCUBoot bootloader, and receive firmware updates over Bluetooth (fdc5a70)
- Keep a field log in flash in the logging build, and fetch it over Bluetooth (57f261f)

[Unreleased]: https://github.com/jonhoo/ur/compare/v0.11.2...HEAD
[0.11.2]: https://github.com/jonhoo/ur/compare/v0.11.1...v0.11.2
[0.11.1]: https://github.com/jonhoo/ur/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/jonhoo/ur/compare/v0.10.1...v0.11.0
[0.10.1]: https://github.com/jonhoo/ur/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/jonhoo/ur/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/jonhoo/ur/compare/v0.8.17...v0.9.0
[0.8.17]: https://github.com/jonhoo/ur/compare/v0.8.16...v0.8.17
[0.8.16]: https://github.com/jonhoo/ur/compare/v0.8.15...v0.8.16
[0.8.15]: https://github.com/jonhoo/ur/compare/v0.8.14...v0.8.15
[0.8.14]: https://github.com/jonhoo/ur/compare/v0.8.13...v0.8.14
[0.8.13]: https://github.com/jonhoo/ur/compare/v0.8.12...v0.8.13
[0.8.12]: https://github.com/jonhoo/ur/compare/v0.8.10...v0.8.12
[0.8.10]: https://github.com/jonhoo/ur/compare/v0.8.7...v0.8.10
[0.8.7]: https://github.com/jonhoo/ur/compare/v0.8.6...v0.8.7
[0.8.6]: https://github.com/jonhoo/ur/compare/v0.8.4...v0.8.6
[0.8.4]: https://github.com/jonhoo/ur/compare/v0.8.2...v0.8.4
[0.8.2]: https://github.com/jonhoo/ur/compare/v0.8.0...v0.8.2
[0.8.0]: https://github.com/jonhoo/ur/compare/v0.7.2...v0.8.0
[0.7.2]: https://github.com/jonhoo/ur/compare/v0.6.1...v0.7.2
[0.6.1]: https://github.com/jonhoo/ur/compare/v0.5.1...v0.6.1
[0.5.1]: https://github.com/jonhoo/ur/compare/v0.4.3...v0.5.1
[0.4.3]: https://github.com/jonhoo/ur/compare/v0.4.0...v0.4.3
[0.4.0]: https://github.com/jonhoo/ur/releases/tag/v0.4.0
