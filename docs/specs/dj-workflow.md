# The DJ workflow

What a professional DJ does at the decks and what they must see, action by action. Dubplate is built and checked against this list. Each item says what must happen, what the DJ must see on the screen and on the DDJ-FLX4, and what rekordbox's Performance mode and a CDJ-3000 with a DJM do, because that is what the DJ's hands and eyes already expect.

[The feature inventory](feature-inventory.md) lists every rekordbox 7 feature and whether Dubplate has it. This file is the other half: for the features a DJ uses at the decks, it says how each one must behave and look, and it is the list an audit walks through, screen and controller, before a change is called done. Where the inventory already describes a gap, the item links to it rather than repeating it.

Sources: the rekordbox 7.2.18 manual (Performance mode screens, the Mixer panel, Preferences > Controller > Deck and Mixer), the CDJ-3000 and DJM-900NXS2 operating instructions, the DDJ-FLX4 operating instructions and MIDI message list, and how DJs work in the booth.

## How to read it

Each item has an identifier, so a pull request or a bug can name it (**C3**). Its status is one of:

- **Done**: works as described, checked in the app.
- **Gap**: exists, but something the DJ sees or hears is wrong or missing. The fix is named.
- **In progress**: an open pull request delivers it. Checked again when it merges.
- **Missing**: not built. A one-line spec says what to build.

"See" always means both places: the screen, and the controller's lights where the FLX4 has one. A state shown in only one of them is a gap. Colour never carries a state alone: every coloured state also has a word, a shape or a position.

The second half, [Delight](#delight), is about how it feels. A DJ should smile while using Dubplate, not only get the right result.

Statuses are as audited on 22 November 2026, on `main` with a second Dubplate running offline (see [development](../development.md)), driven by FLX4 messages and keys through its script.

## At every window size

**V1 Nothing breaks when the window is small.** Must: at the smallest window (1100 × 700) every control a DJ needs while playing stays visible and usable, and text shortens to words, not fragments. Status: **Done** for the decks, the header and the lanes: the tempo slider wraps under the buttons, titles end in "…", the header shows status dots with tooltips, and the lanes get lower in a short window. The effect strip keeps its three units on one row down to the smallest window: below 1,500 px wide each unit narrows, with one target button that steps through A, B, AB and M and short Release FX names, and the CH SELECT lever shows as a dot over its target instead of a word.

**V2 Readouts never jitter.** Must: numbers use tabular figures and fixed widths so a changing value never moves its neighbours. Status: **Done** for the deck readouts and the browser, which use the monospaced face.

## Before the first track

**S1 First launch.** Must: open in under two seconds to a calm window that says what to do first. See: one call to action ("Select a track and press ⇧ ← to load it on deck A"), no error text, no empty grey boxes. Status: **Done**, except the empty overview, a blank grey box until a track loads (**Gap**: give it the same hint as the lane, or draw nothing).

**S2 Sound card.** Must: play on the controller's sound card when it is plugged in, master on outputs 1 and 2, headphones on 3 and 4, with a buffer size the DJ can choose. See: the card's name and latency. rekordbox: Preferences > Audio. Status: **Done**, with the buffer size and the dropouts since launch in Settings > Audio.

**S3 Controller connects.** Must: recognised within a second of plugging in, no restart, every light showing the decks' state at once. See: "Pioneer DDJ-FLX4 connected" and the controller's name in the header. Status: **Done.**

**S4 Controller lost.** Must: keep playing. See: "… disconnected" until it is back. Status: **Done.**

## Loading a track

**L1 Load to a deck.** Must: ready within half a second, cued at the first memory or hot cue when it has one, else at its first sound on the beat (rekordbox's Load Setting and Auto Cue, Preferences > Controller > Deck). See: title, artist, BPM, key, time left, the scrolling waveform with its grid, the overview with its cues, the cue marker. FLX4: CUE lit (at the cue point), PLAY blinking (paused), the load signal. Status: **Done** (Settings > Decks switches either rule off).

**L2 A track Dubplate has to analyse.** Must: the waveform, BPM, key and grid appear as soon as they are ready, and the deck says "Analysing" until then. Status: **Done** on the deck ("Analysing…" where the BPM goes). The header's "N to analyse" counts only the tracks with nothing to show until Dubplate's analysis arrives: rekordbox's analysed tracks, missing files and tracks a rescan is confirming are left out, so it counts down to nothing.

**L3 Load onto a playing deck.** Must: refused while Lock playing deck is on, with a notice; the playing deck untouched. rekordbox: Preferences > Controller > Deck > Eject/Load Lock. Status: **Done.**

**L4 Instant doubles.** Must: LOAD twice copies the other deck at the same place, playing, in phase. Status: **Done.**

**L5 Played tracks.** See: tracks played this session marked in the library, as rekordbox colours them. Status: **Done**: a track played 60 seconds or more of track time gets a dimmed title and a tick, the same rule as the History, and Hide played beside Key fits puts `played:no` in the search.

## The cue point and CUE

**C1 Set the cue point.** Must: paused away from the cue point, CUE sets a new one there, on the nearest beat with quantize on. See: the orange cue marker moves at once on both waveforms and flashes. FLX4: CUE lit. Status: **Done.**

**C2 Back cue.** Must: playing, CUE returns to the cue point and pauses within one audio buffer. FLX4: PLAY blinking, CUE lit. Status: **Done.**

**C3 Cue preview.** Must: holding CUE at the cue point plays; releasing returns and pauses; PLAY while holding keeps playing. Status: **Done.**

**C4 What the lights mean.** CUE lit at the cue point, blinking when paused elsewhere, off while playing; PLAY lit while playing, blinking while paused, on screen and on the FLX4. Status: **Done.**

**C5 Back to the start.** SHIFT + CUE. Status: **Done.**

## Play and pause

**P1 Play.** Must: starts within one buffer from exactly where the deck is. See: PLAY lit, waveform scrolling, time left counting down. Status: **Done.**

**P2 Pause.** Must: stops at once with a short fade, no click. Status: **Done.**

**P3 Time left and the end of the track.** See: time left large enough to read at arm's length; in the last 30 seconds it turns amber and the end of the overview is tinted. Status: **Done.**

**P4 The track ends.** Must: the deck stops at the end and says so. Status: **Not checked** in this audit.

## Tempo

**T1 Tempo fader.** Must: continuous, no audible steps; on Pioneer hardware toward the DJ is faster. See: tempo in percent with two decimals (+1.25%), the BPM it gives with two decimals. Status: **Done.**

**T2 Tempo range.** SHIFT + BEAT SYNC steps ±6, ±10, ±16, WIDE. See: the range beside the tempo. Status: **Done.**

**T3 Fine tempo by key.** Must: 0.02% steps, a CDJ's finest at ±6. Status: **Done**: each press moves the range's finest step, 0.02% at ±6, 0.04% at ±10 and ±16, 0.5% at WIDE.

**T4 BEAT SYNC.** Must: takes the master's BPM and beat phase at once and follows it. See: SYNC lit (armed while it waits for a master); the beat lines line up. FLX4: BEAT SYNC lit. Status: **Done.**

**T5 Tempo master.** Holding BEAT SYNC makes the deck the master. See: Master lit on that deck alone. Status: **Done.**

**T6 Tempo fader under sync and Smart Fader.** The fader takes over when moved through the playing tempo; the screen shows where it waits. Status: **Done.**

**T7 Key lock.** Status: **Done.**

**T8 Key shift and key sync.** Status: key shift **Done** (Key Shift and Keyboard pad modes); key sync **Done**: the deck's Key sync button or Option+K (Shift+Option+K for deck B) moves the key to the other deck's Camelot number, the same key or its relative, by the fewest semitones, and lights while it holds; a second press puts the track's own key back.

**T9 Tap tempo.** Status: **Done.**

## Beatmatching by ear

**B1 Two waveforms on one time axis.** Status: **Done.**

**B2 Phase.** See: a phase meter showing how far the following deck's beats are from the master's, and a bar.beat counter. Status: **Done** (beat counter on each lane, phase meter on the following deck, green when in phase, "+2 beats" when the bars do not line up).

**B3 Nudge.** Status: **Done.**

**B4 Scratch and hold.** Status: **Done.** Vinyl or CDJ jog mode per deck is **Done** (Vinyl switch, Option+V).

**B5 Search.** SHIFT + jog. Status: **Done.**

**B6 Smooth scrolling.** Must: the waveforms move every display frame, no jitter. Measured with `DUBPLATE_FRAME_STATS=1`, two decks playing, a loop on deck A: frames every 12.2 ms on average (worst 38 ms once in four seconds), render 0.45 ms, waveform paint 2.2 ms (worst 4.5 ms). Status: **Gap** for 120 Hz screens: 82 frames a second where the display offers 120. The paint fits a 8.3 ms frame, so the pacing, not the drawing, is the limit; to measure again with the display awake.

**B7 Zoom.** Status: **Done** (seven levels, Cmd+= and Cmd+-, Shift + BROWSE, pinch).

**B8 Drag the waveform.** Status: **Done** (drag the lanes to scrub a stopped deck or nudge a playing one); to check on the decks.

**B9 Pitch-bend feedback.** See: while nudging, the BPM shows the bent tempo. Status: **Missing.**

## Waveforms

**W1 Scrolling waveform colours.** rekordbox's 3-band colours. Status: **Done.**

**W2 Overview.** 3 bands, played part, playhead, hot cues, memory cues, loops, phrases, minute ticks, the last 30 seconds. Status: **Done.**

**W3 Beat grid on the waveforms.** A line per beat, the downbeat stronger, bar numbers. Status: lines **Done**; bar numbers **Done** with the beat counter.

**W4 Marks on the scrolling waveform.** Cue point, hot cues in colour, memory cues, the loop and its length, the slip shadow. Status: **Done.**

**W5 Folder tracks.** A waveform for every track. Status: **Done.**

## Hot cues

**H1 Set a hot cue.** An empty pad sets one at the playhead, quantized, without interrupting playback. See: the letter on both waveforms, a flash, the pad lit, the time on the pad. Status: **Done.**

**H2 Jump to a hot cue.** Status: **Done.**

**H3 Delete a hot cue.** SHIFT + pad. Status: **Done.**

**H4 Colours and names.** rekordbox's colours and comments on screen and pads. Status: **Done** on screen (own colour, else rekordbox's COLORFUL default for the pad); the FLX4's pads only light on or off.

**H5 Hot loops.** A hot loop's pad starts its loop; an empty pad pressed during a loop keeps it as a hot loop. Status: **Done.**

**H6 Hot cues survive.** Must: a hot cue set in Dubplate is there next time the track loads, after a relaunch, and in the rekordbox XML. Status: **Done** (kept per track in Dubplate's database, in place of rekordbox's cues, and exported).

## Memory cues

**M1 Show memory cues.** Red triangles on the overview and the lanes. Status: **Done.**

**M2 Store and delete a memory cue.** SHIFT + CUE/LOOP CALL ▶ keeps the cue point (or the loop that is on), up to 10; SHIFT + ◀ deletes the one at the cue point. Status: **Done.**

**M3 Call memory cues.** CUE/LOOP CALL ◀ ▶ when no loop plays; the deck pauses on the called cue as its cue point. Status: **Done.**

## Loops

**O1 Loop in and out.** See: a band growing from IN to the playhead, then the tinted loop with its length; IN, OUT and EXIT lit. Status: **Done.**

**O2 Exit and reloop.** Status: **Done.**

**O3 Halve and double.** Status: **Done.**

**O4 Beat loop.** Status: **Done.**

**O5 Loop roll.** Status: **Done.**

**O6 Loop adjust.** Status: **Missing** ([inventory 18](feature-inventory.md#top-40-gaps)).

**O7 Saved and active loops.** Status: shown on the overview; calling them **Missing** ([inventory 19](feature-inventory.md#top-40-gaps)).

## Beat jump and slip

**J1 Beat jump.** Status: **Done.**

**SL1 Slip.** Status: **Done.**

## Pad modes

**PD1 to PD3** Hot Cue, Pad FX 1 and 2, Beat Jump, Sampler, Keyboard, Beat Loop, Key Shift. Status: **Done.**

**PD4 Presses show on screen.** Must: a pad or button pressed on the FLX4 lights its twin on screen while held, a quick tap flashes, and a pad shows what it did (the jump, the loop length, the effect held). Status: **Done.**

## Effects

**FX1 to FX6 Beat FX.** Status: the FLX4 lever and knob **Done**; effect tuning and the full Beat FX list **In progress** (effects work).

**CF1 Filter knob.** Status: **Done.**

**CF2 Smart CFX.** Status: **Done**.

## The mixer

**X1 Trim**, **X2 EQ**: Status: **Done.**

**X3 Channel fader curve**, **X4 Crossfader curve**, **X5 Crossfader assign (A, THRU, B)**, **X10 Crossfader off**: Status: **Done** (Settings > Mixer has the curves and the crossfader switch, as rekordbox's Preferences > Controller > Mixer; the assign switches sit under the channel faders, as in its mixer panel).

**X6 Meters.** Channel meters before the fader and a master meter. Status: **Done.** A clip light and peak hold: **Missing.**

**X7 Master level and limiter.** Status: **Done.**

**X8 Headphone cue.** Status: **Done.**

**X9 Smart Fader.** Status: **Done**; it rests with a deck on THRU or the crossfader off, and says so.

## Sampler and recording

**SP1, SP2 Sampler.** Status: **Done.**

**R1 Record.** Status: **Done.**

## The browser

**BR1 Browse** with the rotary selector. Status: **Done.**

**BR2 Search.** Status: **Done.**

**BR3 Columns and sorting.** Status: **Done** for the columns there; more columns **Missing** ([inventory 21](feature-inventory.md#top-40-gaps)).

**BR4 Compatible tracks.** Key fits: **Done**; the Match column: **Done.**

**BR5 Preview in the headphones.** Status: **Done** (P previews the selected track on the headphone bus, from its first cue).

**BR6 History.** See: under History in the browser's sidebar, this session's plays newest first (start time, deck, title, artist) and earlier sessions by day; a track counts after 60 seconds of track time on one load, once per load, as in rekordbox's History. Export TXT and CSV tracklists with times into the mix. Status: **In progress** (play history pull request). rekordbox's own histories: **Missing.**

## The app

**A1 Menu bar.** A Mac app's menus (Dubplate, File, Edit, View, Window, Help) with Settings ⌘, and every command's shortcut, dimmed when they do not apply. Status: **Done.**

**A2 Errors and notices.** Calm, one line, saying what to do. Status: **Done**: the dropout count lives in Settings > Audio, and one notice appears only when the audio glitches three times in a minute.

## Preparing

**PR1 Beat grid.** Status: **Done** in the track check and on the decks.

**PR2 Cues.** Status: **Done**: hot cues, memory cues and loops made on the decks are kept per track and written to the rekordbox XML.

**PR3 The set.** Status: **Done** ([set preparation](set-prep.md)).

## Delight

Correctness is the floor. A DJ should also enjoy the app: it answers at once, moves smoothly, stays calm, and gives small, tasteful signs that things went right. Nothing here may distract during a mix, flash for attention, or cost frame time.

### Feel

- **Instant response.** A key, a click or a controller message changes the sound within one audio buffer (about 5 ms) and the screen on the next frame. Measured: controller message to command under 1 ms.
- **Smooth motion.** Waveforms and meters move every display frame, 120 a second on a ProMotion screen, with the playhead interpolated between audio buffers. Frame budget: render and paint together under 4 ms, so the frame is never late. See B6 for today's numbers.
- **A beatmatch you can see lock.** When the following deck comes within 1/64 of a beat of the master, the phase meter turns green and stays still. When sync engages, the beat lines of both lanes meet without a jump.
- **Landing feedback.** A cue lands with a short flash where it landed (done); a loop engages with its band filling in, not popping; an effect switched on lights its unit and its level bar at once.

### Calm

- **First launch and empty states** say what to do next in one line, never an error or an empty grey box.
- **No jitter, no stale values.** Every number updates on the same frame as the sound; readouts use fixed-width figures.
- **Nothing ugly at any window size** (V1).
- **Diagnostics stay out of the DJ's way**: counts and timings live in Settings, and a notice appears only when the DJ can do something about it.

### Small pleasures

Proposed, to agree on before building, in the Graphite look (quiet greys, deck colours as accents, no glow, no bounce):

1. **Beat pulse in the deck header.** While a deck plays, its letter badge brightens for 80 ms on each beat and a little more on the downbeat. Two synced decks pulse together, a second confirmation of the beatmatch. Off when stopped.
2. **The master meter breathes.** Segments fall back slowly (a 300 ms release) with a thin peak hold that drops after a second, so the meter moves with the music rather than flickering.
3. **Set ready.** When the last track of a set becomes Ready, the set's "18 of 20 ready" becomes a quiet green "Set ready", once, with the energy curve drawn in from left to right over half a second.
4. **A living recording timer.** While recording, the REC dot fades slowly in and out once a second, the time counts in fixed-width figures, and the file's size sits beside it.
5. **Phase lock.** The moment the phase meter turns green, the centre line of the lanes brightens for a beat. Once, not every beat.
