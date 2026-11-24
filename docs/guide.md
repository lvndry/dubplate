# Prepare a set and mix it on the DDJ-FLX4

A walk through Dubplate from an empty library to a recorded mix. Dubplate reads your rekordbox collection and never writes to it: grids, cues, sets and relinks you make live in Dubplate's own files.

## Start Dubplate

```sh
cd ~/github/dubplate
cargo app
```

The first build takes a few minutes. Plug the FLX4 in before or after starting; the header shows it as soon as it is connected.

## 1. Add your music

On first launch the library is empty and asks you where your music is.

- **Add music folders…** (Cmd+O) picks one or more folders. Dubplate reads them and their subfolders, and never changes a file.
- **Import rekordbox collection** brings in your rekordbox tracks, playlists, cues, beat grids and phrases.

You can use both. Settings (Cmd+,) lists your folders with Rescan and Remove.

Dubplate analyses every track in the background and keeps what it found for the next launch, set tracks and the tracks you load first. Each track gets its energy (1 to 10), and the tracks rekordbox never analysed, such as the ones only in your music folders, also get a BPM, a beat grid with its downbeats and a key in Camelot. Sync, beat FX, loops, quantize, the beat marks, the track check, the seam warnings and the suggestions then work for them as for rekordbox tracks. rekordbox's own BPM, key and grid stay first for its tracks, and tap tempo or your grid nudges win over Dubplate's grid. A blank BPM, Key or Energy cell means the track is still being analysed; the header counts how many are left.

### Dubplate playlists

Use **+ Playlist** or **+ Folder** in the library sidebar. Select a folder before creating an item to place it inside that folder. Select an item to rename or delete it, and use **XML…** or **USB…** to export its tracks in their saved order.

Drag selected tracks onto a playlist, choose **Add to playlist** from a track’s context menu, or press Cmd+Shift+P and choose a destination with Up, Down and Enter. In a playlist, choose its original order from the sort controls to reorder tracks by dragging. Delete removes the selected tracks from that playlist. To make an editable copy of a rekordbox playlist, right-click it and choose **Copy to Dubplate playlist**.

## 2. Pick the sound card

In Settings, choose **DDJ-FLX4** as the sound card so the master goes to the FLX4's master output and the headphones to its headphone jack. Turn the master down before the first track, then check the headphones with the channel CUE buttons. Dubplate remembers the master and headphone levels and the screen you were on, and opens no louder than it closed: if the FLX4's MASTER LEVEL knob reports a louder level when it connects, it waits until you turn it down through the level on screen, while turning it down always works at once.

Settings also picks the theme, which changes at once and is remembered: Graphite (Dubplate's own), Set Atlas (the website's lime and mint waveforms on dark green), the darker Booth, Club, Console, Glass, Groove, Platter, Spectral and Cobalt, Midnight (after rekordbox's blue-grey and its waveform colours) and Ember (after Traktor's charcoal with orange and blue), and the light Chassis, Instrument and Paper for bright rooms. Every theme keeps text, states and the deck colours readable against its surfaces.

Settings > Audio also has the buffer size: 256 frames (about 5 ms) by default, smaller to answer the controller sooner, larger to give a busy Mac more time. If the audio drops out three times within a minute, Dubplate says so once, with that hint; the count since launch is under the buffer size.

Stop both decks, any playing samples and any recording before changing the sound card or buffer size. The sampler keeps its loaded slots, settings and CUE button when the output changes.

## 3. Prepare the set

Cmd+1 opens Prepare: the library on the left, your set on the right.

| Do | Key |
| --- | --- |
| Add the selected track to the set / insert it after the selected set track | Enter / Cmd+Enter |
| Move a set track up or down | Alt+Up / Alt+Down |
| Take a track out / undo | Backspace / Cmd+Z |
| Keep a track as a backup | B |
| Move between the library, the set and Next / back | Ctrl+Tab / Ctrl+Shift+Tab |
| Insert Next suggestion 1 to 5 after the selected set track | Option+1 to Option+5 |

The **Next** panel suggests what to play after the selected set track, with the reason ("One step on the wheel · +1.6% · energy +2"), steered by the set's energy curve against its target. The target climbs from your library's calmer tracks to its most intense, so it fits how your music is rated. Each suggestion shows its Option key. Use Ctrl+Tab to reach Next, choose with the arrows and press Enter to insert one. When the refreshed suggestions arrive, they follow the track you just placed. Tab and Shift+Tab move through individual controls.

Each set track has a **ready badge**: Ready, or what is missing (Grid unconfirmed, No mix-in cue, No mix-out cue, File missing, Key unknown), the most urgent first, such as "File missing, and 3 more"; hover it to see every problem. A small **Y** beside it means Dubplate can propose the missing mix cues from the phrases: press Y to accept them. Each seam between two tracks shows the key move, the tempo step and any warning: key clash, a tempo gap over 6%, vocals meeting, an energy drop, or the same track twice.

### Dig through the library

- **Browse a music folder's subfolders.** Under each music folder in the sidebar, a small triangle opens its subfolders, and they open in turn. Right-click a music folder and choose **Rescan folder** to pick up changes in that folder and its subfolders. Click a folder to see the tracks in it and in every folder below it; the search and filters then apply inside it. Folders with no music anywhere below them are not listed. With the cursor in the sidebar, the up and down arrows move through the folders, the right arrow opens a closed folder or steps into an open one, and the left arrow closes an open folder or steps out to the folder holding it. While the cursor is in the sidebar these two keys never move the crossfader, so browsing folders in the middle of a set cannot jolt the mix; click a track to give the crossfader keys back. A long name ends in \u2026 and hovering it shows the whole path. Dubplate remembers which folders are open and which one you chose.
- Right-click a track, or Control-click it, in the library or the set for what you can do with it: load it on deck A or B, preview it in the headphones, add it to the set or insert it after the selected set track (or remove it from the set), separate its stems, relink it when its file moved, **Show in Finder** (Option+Command+R) and **Copy File Path** (Option+Command+C). Each item shows its key, and items that do not apply are dimmed.

- **Select several tracks** as in rekordbox: Command-click adds or removes a track, Shift-click selects everything from the last clicked track, Command+A selects every track shown, and Shift with the up or down arrow grows the selection. Rating, colour, My Tags, Add to Set, Insert After Selection, Separate Stems and Copy File Path then apply to every selected track (Insert After Selection keeps their order). Loading, previewing and Show in Finder use the track with the cursor, the one shown in the details. Right-click inside the selection to act on it, or on another track to select just that one.
- **Rate, colour and tag tracks.** Select a track (or several) and press Control+1 to Control+5 for one to five stars, or Control+0 to take the rating away. Or right-click it and use **Rating**, **Colour** (None, or pink, red, orange, yellow, green, aqua, blue or purple) and **My Tag**, where your tags are listed under their categories and a tick shows the ones the track has; click a tag to put it on or take it off. The **Rating** and **Colour** columns show the result; click a header to sort, the Rating column best first and the Colour column in rekordbox's colour order, with tracks that have none last. A narrow window hides both columns. Ratings, colours and tags are kept in Dubplate, never written to rekordbox, and a track keeps the rating and comment rekordbox gave it until you change them. The `rated:`, `color:` and `tag:` filters below read them. **Edit My Tags…** at the bottom of the My Tag menu, and in Settings, opens the My Tags window: **New category** and **Add tag** make them (type a name and press Enter), **Rename** changes a name, the arrows put them in the order you want, and **Delete** asks once more before it removes a tag from every track, or a category with all its tags. Names must not repeat inside their list. Until you make some, the My Tag menu is empty.
- Click a column header to sort by it, again to reverse it, and a third time to go back to the crate's own order. Prepare and Perform each remember their sort.
- The search field matches titles, artists, genres, comments and the folders a track is filed in, so `amapiano warmup` finds the tracks in your amapiano/warmup folder. The Genre column shows the genre tag, or those folders when the track has none.
- Type filters in the same field, mixed with words: `bpm:120-124` (or `bpm:120`, `bpm:120+`), `key:8A` (any notation) or `key:fits` (what mixes after the reference track, as Key fits), `genre:amapiano`, `artist:kabza`, `energy:6+` (or `energy:4-6`), `played:no` (not played for a minute or more on a deck this session; the Hide played chip next to Key fits puts it in or takes it out) and `rated:4+` (rekordbox stars), `rated:0` or `rated:none` (no rating), `color:red` (also `colour:`, or `color:none` for no colour), `tag:peak` (one of your My Tags, case-insensitive; every `tag:` must match) and `comment:club` (the track's comment contains the word). While the search field has the keyboard, chips under the filter bar suggest them from the track you are mixing from; click one to put it in.
- **BPM** keeps a tempo range, a few BPM either side of the selected set track (or the playing deck); the arrows move each end.
- **Key fits** keeps the tracks whose key mixes well after the selected set track in Prepare, or the deck playing in Perform: the same key, one step on the wheel, or the relative key, as the Next suggestions choose.
- The **Match** column says how each track mixes after the track you mix from, which its heading names ("vs A: Kukaya"): the deck with MASTER pressed, even stopped, else the deck the room hears, else the deck loaded last, so it works before anything plays: the key move with a coloured dot (green for a smooth move such as one step on the wheel, blue for a lift such as an energy boost, red for a clash), the tempo change (amber beyond 6%) and the energy change. Sort by it and the library becomes a ranked list of what to play next, best first, in the Next suggestions' order. **Matches master** keeps only those suggestions. The master's own track says so, and tracks a deck already played this session are dimmed.

### Check each track

Select a set track to open its track check: the waveform with the beat grid, rekordbox's phrases and the mix cues. A track rekordbox never analysed shows Dubplate's grid over a waveform drawn from its audio (a few seconds the first time; the deck and the track check share it, so each track is drawn once), with no phrases, and its proposed cues come from the grid alone.

| Do | Key |
| --- | --- |
| Confirm the grid | G |
| Nudge the grid 5 ms earlier / later | [ / ] |
| Move the downbeat one beat earlier / later | , / . |
| Mix-in / mix-out on the bar at the playhead | I / O |
| Accept the proposed cues (from the phrases) | Y |
| Mark the seam after this track Planned or Free | F |
| Hear that seam in the headphones (20 s) | S |

A Free seam never warns: it means you will feel it live. Grids on amapiano are often off by a beat or a few milliseconds; the nudge keys fix that. A track keeps one grid fix by its file, whichever set it is in, so the fix follows it onto the decks, into sync and into the export.

## Play the set in rekordbox

**Export to rekordbox…** at the top of the Set panel, or Cmd+E, saves the set as a rekordbox XML file, by default in Music/Dubplate Exports. The file holds the set as a playlist in its order (and a second playlist with the backups), and each track with:

- its beat grid, with your nudges and downbeat moves;
- your mix-in and mix-out as memory cues named "Mix in" and "Mix out";
- its hot cues, memory cues and saved loops: the ones you set in Dubplate, or else the ones it already had in rekordbox.

Tracks from your own folders go in too, so rekordbox adds them; relinked files point at their new place. Dubplate writes only this file, never rekordbox's library.

To bring it in, in rekordbox:

1. Preferences > Advanced > Database > rekordbox xml: choose the file. If the rekordbox xml tree is hidden, turn it on in Preferences > View > Layout.
2. In the sidebar, open rekordbox xml and the set's playlist.
3. Select all its tracks and choose **Import To Collection**. rekordbox only updates tracks it already has this way; importing just the playlist would keep their old grids and cues.
4. Right-click the playlist and choose **Import Playlist**.

Repeat the import for the backups playlist if you want it too. To send a saved Dubplate playlist instead of a set, select it in the library and choose **XML…**, then follow the same import steps. For a player that uses OneLibrary, [export the imported playlists from rekordbox](#export-a-onelibrary-usb).

## Play the set on CDJs

**USB…** at the top of the Set panel, or Cmd+Shift+E, exports the set in the classic Device Library format used by players such as the CDJ-3000, CDJ-2000NXS2 and XDJ-RX3. Choose the stick (or any folder) in the picker. [Check which format your player uses.](https://rekordbox.com/en/support/usb-export/)

Dubplate writes the following files.

- the audio files, under `Contents/<artist>/<album>`; a file already on the stick, byte for byte, is not copied again;
- for each track, the analysis files players read: the beat grid with your fixes, the hot cues A to H with their colours and names, memory cues, saved loops, and the blue, RGB and 3-band waveforms (rekordbox's own when rekordbox analysed the track, else drawn by Dubplate from the audio) and rekordbox's phrases;
- the library players browse (`PIONEER/rekordbox/export.pdb`), with the set as a playlist and its backups as a second one, and the title, artist, album, genre, label, key, BPM, length and rating of every track.

A notice follows the export track by track and says what it did. In the library, a chosen playlist or music folder shows **USB…** beside its name to export it the same way.

Dubplate adds to a classic Device Library it previously exported and replaces a playlist of the same name. Use a separate stick or folder if rekordbox or another app prepared your current stick.

### Export a OneLibrary USB

Use the latest rekordbox 7 to prepare a stick for the CDJ-3000X, CDJ-1500X, XDJ-AZ, XDJ-AN, OPUS-QUAD or OMNIS-DUO. rekordbox exports both OneLibrary and the classic Device Library to the same stick, so it can serve both types of players. [AlphaTheta USB compatibility.](https://rekordbox.com/en/support/usb-export/)

1. [Export and import the set as rekordbox XML](#play-the-set-in-rekordbox). In rekordbox, check the playlist order and load a track to check its cues and grid.
2. Switch rekordbox to **EXPORT** mode, connect your USB stick and drag the imported playlist from **Playlists** to the stick under **Devices**. Drag the backups playlist too if you want it on the stick.
3. When the transfer finishes, open **Devices > your stick > OneLibrary > Playlists** and confirm the playlist order and track count. If you need the stick for both types of player, also check **Device Library > Playlists** on the same stick. Eject the stick from rekordbox before unplugging it.

For a stick with an existing classic Device Library, rekordbox also offers **Devices > your stick > OneLibrary > Convert from Device Library** from the OneLibrary context menu. Back up the stick before converting if it already contains OneLibrary. Conversion replaces that library, including playlists and histories saved only there. Dubplate's direct **USB…** export refuses a stick once it contains OneLibrary. Conversion of a Dubplate-written stick has yet to be verified. [rekordbox OneLibrary FAQ.](https://rekordbox.com/en/support/faq/onelibrary-7/#faq-q700038)

### Import from USB after playing

Open Library settings, choose **Import from USB…**, and select the root of the stick. Select a CDJ history to add it to History under the stick’s name. Importing it again updates the same history. The player records track order but omits the date, deck and play times.

Review the differences between the stick’s cues and the cues currently in your library. Select individual changes, click a track to select all its changes, or choose **Select all cue changes**. **Import selected** saves your choices in Dubplate. The review lists tracks with missing analysis or no unique match. The stick stays read-only.

## 4. Mix

P plays the track selected in the library in the headphones, from its first memory or hot cue, without loading it: the headphone MIX and LEVEL knobs set how loud, the header shows "Preview" with its title (click to stop), and P again stops it. It never reaches the master or a deck, and stops by itself after 30 seconds.

Cmd+L loads the set's first two tracks onto deck A and deck B at their mix-in cues; Cmd+2 opens Perform. A set track whose file is missing is never loaded: its deck keeps what it had, and a notice names the track with a **Relink** button that looks for the file. The same goes for any track loaded from the library.

A track loaded from the library waits at its earliest memory or hot cue, or, when it has none, at its first sound on the beat it starts with, never at 0:00 in silence, as rekordbox's Auto Cue does. Settings > Decks switches either off.

On the keyboard, **Space** plays or pauses the deck you can hear: the one whose channel fader is up past 20% and that the crossfader lets through. With both up it acts on the tempo master, else the louder one, and with neither it is deck A. A small **Space** label beside a deck's letter shows which one it will be. Shift+Space is always deck B, and the FLX4's PLAY buttons always their own deck.

Every press on the FLX4 shows on screen too: a pad, CUE, PLAY, BEAT SYNC, the loop buttons, the channel CUE, BEAT FX ON, SMART CFX and SMART FADER light on screen while held, and a quick tap flashes for a moment. A Beat Jump pad shows how far it jumped over the deck's lane ("−4 beats") with a mark where the deck jumped from, a Beat Loop pad the loop it set, and a Pad FX pad its effect while held.

On the FLX4, everything works as in rekordbox's FLX4 layout:

- PLAY, CUE (CDJ style), hot cue pads (a cue you set or delete is kept with the track, for the next time it loads and for the export; a pad pressed while a loop is on keeps that loop as a hot loop, which starts the loop when pressed again), TEMPO, BEAT SYNC (hold for tempo master, SHIFT for the tempo range), the jogs (touch to scratch, side to nudge, SHIFT to search).
- The mixer: TRIM, HI/MID/LOW, the filter knobs, channel faders, crossfader, headphone CUE, MIX and LEVEL.
- The pad modes: Hot Cue, Pad FX 1 and 2 (hold for rolls and effects), Beat Jump, Beat Loop, Sampler, Keyboard, Key Shift; the loop section IN, OUT, 4 BEAT/EXIT.
- CUE/LOOP CALL ◀ ▶ calls the memory cue before or after the playhead: the deck pauses there with its cue point on it, as on a CDJ. With a loop on, they halve and double it. SHIFT + ▶ keeps the cue point (or the loop that is on) as a memory cue, up to 10 per track; SHIFT + ◀ deletes the memory cue at the cue point. Memory cues show as red triangles along the top of the scrolling waveforms and are kept with the track. On the keyboard: H and J call, N keeps, ⌥N deletes (with ⇧ for deck B).
- BEAT FX: FX SELECT, BEAT left and right, the CH SELECT lever, LEVEL/DEPTH and ON/OFF.
- SMART CFX turns both CFX knobs into the chosen preset (Space, Dub echo, Sweep, Noise, Crush, Pitch, Chop, Jet, Phase; SHIFT + SMART CFX steps through them). With SMART CFX off, each CFX knob plays its channel's Sound Color FX: the filter, or one of the same colours, picked from the CFX name between the mixer's knobs for both channels or one (**A and B**, **A**, **B**) and remembered. Sweep gates the music to the left and narrows it to a band-pass to the right, without the filter; Noise lays white noise over the music through the filter. SMART FADER makes the crossfader glide a shared tempo and swap the bass for you; a LOW knob you turn takes over. In Settings > Mixer, **Smart Fader moves with** chooses what drives it: **Both** (the crossfader, leaned toward the louder channel), the **Crossfader** alone, or the **Channel faders** alone, which also works with the crossfader off. **Echo out on the faders**, in Settings, is the afro house exit: close a channel fader or the crossfader fast and the outgoing deck echoes on the half beat as it goes, the echoes ringing on after the fader is shut; with Smart Fader on, throwing the crossfader to the edge echoes the outgoing deck the same way.

SHIFT never turns pages of pads on the FLX4; it gives each pad a second job, as rekordbox's FLX4 layout does. The FLX4 has 8 hot cues per track, A to H, with no banks or pages. While SHIFT is held the pads on screen show their SHIFT job:

| Mode | Pads | SHIFT + pads |
|---|---|---|
| Hot Cue (SHIFT + HOT CUE: Keyboard) | Set or call hot cues A to H | Delete the hot cue |
| Pad FX 1 (SHIFT + PAD FX1: Pad FX 2) | Pad FX 1 to 8 (Pad FX 2: 17 to 24) | Pad FX 9 to 16 (Pad FX 2: 25 to 32) |
| Beat Jump (SHIFT + BEAT JUMP: Beat Loop) | Jump 1, 2, 4 and 8 beats back and forward | Pads 7 and 8 make the jumps shorter and longer |
| Sampler (SHIFT + SAMPLER: Key Shift) | Play slots 1 to 8 of the shown bank (deck B: 9 to 16) | Stop the slot, or load the selected track into an empty one |
| Slicer, Slicer Loop (on screen) | Hold pads 1 to 8 to jump to their slices | Hold pads 1 to 4 for 1/8, 1/16, 1/32 and 1/64 rolls of the current slice |
| Keyboard, Key Shift | Pitch from ±0 to +7 semitones | Pads 7 and 8 move the range up and down a semitone |
| Beat Loop | Loops of 1/4 to 32 beats | Stems, Dubplate's own: vocals, melody, bass and drums in or out, acapella, instrumental, groove, every stem (rekordbox maps nothing here) |

**Slicer.** With a beat grid on the deck, choose **Slicer** to move through the track in groups of eight slices, or **Slicer loop** to repeat the current group. Set its length with the Beats buttons (4, 8, 16, 32 or 64 beats). The scrolling waveform outlines the group and highlights the playing slice. Release a pad to return to where the deck would be. Press D on deck A or Shift+D on deck B to cycle Slicer, Slicer Loop and Hot Cue. The FLX4 pads follow the on-screen slicer until you choose a pad mode on the controller. On the keyboard, press a slice pad key again to release it.

**Sampler banks.** Pick **Bank 1–4** above the sampler pads. Both decks and the FLX4 pads follow that bank, and each bank keeps its own 16 slots. Right-click a slot and choose a sound under **House percussion & chords**. Tap **Low tumba**, **Muted bongo**, **Low agogo**, **Room clap**, or **Cm7 chord stab** for a one-shot. **Shaker groove** loops for one bar and syncs to the master tempo. **DJ effects** includes Air horn, Siren, Riser and Log drum. To free a pad, right-click it and choose **Empty slot**. This stops the sample and clears its assignment. The audio file stays in your library, and the slot stays empty after restarting Dubplate.

In the Pad FX modes a row above the pads names the layer showing, **Pads 1-8** or **SHIFT 9-16**, lit in the deck's colour. Click the other one to show and play that layer with the mouse; hold SHIFT on the FLX4 and it switches while you hold it.

**Tap Echo.** In each factory bank, tap **Pad FX 1 → pad 5** on the hit you want to repeat. The pad captures one echo interval starting at the press. Lift your finger immediately and the repeats fade over the playing track. At 120 BPM, Classic and Amapiano capture half a beat (250 ms) and Afro house captures three quarters of a beat (375 ms). Tap again to catch another hit. The same gesture works with a click or a pad shortcut. On a saved bank, choose the echo pad in **Edit** and set **Pad** to **Tap**. Choose **Hold** to capture just the time under your finger on the controller.

**Pad FX editor.** The **Edit** button in that row, or **Edit Pad FX** in Settings, opens the 32 Pad FX pads, as rekordbox's PAD EDITOR shows them: FX1, FX1 + SHIFT, FX2, FX2 + SHIFT, eight pads each. Click a pad and set what it plays (a loop roll or any Beat FX), its beat or parameter, its depth, how it triggers (Hold, Latch, or Tap for echo pads), and what happens on release: the echoes and reverb ring out, or the music goes back to dry at once. **Start from** loads one of three sets: **Classic** (rolls from 1/4 to 1/32, echoes, spiral, reverb, gates, filters, vinyl brake and back spin, pitch, noise, echo out), **Afro house** (half and quarter beat rolls, 3/4 echoes and delays, the echo out, filtered echoes, slow sweeps) and **Amapiano** (log drum rolls, dotted echoes, reverb washes, pitch drops, brakes). Click a pad on screen or tap it on your controller to select its assignment for editing. In PAD FX controller modes, SHIFT selects the corresponding SHIFT layer. To rearrange effects, select a pad, click **Swap placement**, then click or tap its destination pad. Both assignments, including all parameters, exchange positions. Click **Cancel swap** to cancel. Changes apply at once and are remembered.

When Smart Fader or sync has moved a deck's tempo away from where its TEMPO slider sits, the on-screen slider shows the tempo the deck plays, an amber outline where the fader waits, and "Move to take over": move the fader through the playing tempo and it takes over without a jump, as in rekordbox. The BEAT FX LEVEL/DEPTH knob shows an amber mark on the effect's level bar in the same case.

Each deck's **overview** shows the whole track in rekordbox's three bands (lows blue, mids gold, highs white), so drops and breakdowns read at a glance, with the hot cues as letters on top in rekordbox's colour for each (its own, or its default for the pad; hover one to read its name), memory cues as red triangles, the set's mix-in (green) and mix-out (lavender), saved loops in yellow, a tick every minute, and a strip along the bottom coloured by phrase (intro, build, drop, breakdown, outro). The last 30 seconds are tinted and the time left turns amber. Hover it to read the time and phrase under the pointer; click to jump there. The CUE button's point shows as an orange triangle on the overview and the lanes, and any cue you set, from CUE or a hot cue pad, flashes where it landed. The scrolling waveforms above show each hot cue as a flag in its colour with its letter and name, memory cues as small triangles along the top, an active loop as a tinted band with its length, a loop waiting for RELOOP as an outline, and the slip shadow playhead while slipping.

Cmd+= and Cmd+- zoom both scrolling waveforms in and out together (Shift + BROWSE on the FLX4, or a pinch on the trackpad), from about 3 to 30 seconds across, and Cmd+0 puts them back; the dots at the lanes' top right show the zoom, and Dubplate remembers it.

On screen, each deck also has Sync, Master, Key lock, Slip and Vinyl (on: the jog's top holds the record to scratch; off: CDJ mode, the jog only nudges; Option+V and Shift+Option+V), the header has Quantize for both decks (Option+Q; on by default, so cues, loops, hot cues and beat jumps land on the grid), and the compact effects strip drives the three effect units. Press ? for every keyboard shortcut.

Under the channel faders, each channel has a crossfader assign switch, as on a DJM and in rekordbox's mixer: **A**, **THRU** or **B** (Z and Shift+Z step deck A's and deck B's). THRU takes the channel off the crossfader, so only its own fader sets its level; it is outlined in amber, the crossfader's ends show which decks are on each side, and with both channels on THRU the crossfader dims. Option+X, or Settings > Mixer > Crossfader, turns the crossfader off altogether: both channels then play as if on THRU, so it moves without changing the sound, and Smart Fader is unavailable. Smart Fader needs one deck on each side and rests otherwise. Settings > Mixer has the crossfader curve (Blend, a smooth constant-power mix; Constant, no dip in the middle; Scratch, a sharp cut) and the channel fader curve (Slow, Standard, Fast), as rekordbox's Preferences > Controller > Mixer. Dubplate remembers them.

Key lock (a CDJ's Master Tempo) is on for every track you load, so moving the tempo or syncing keeps a track's pitch. Turn **Key lock on new tracks** off in Settings to load tracks without it, as rekordbox's Master Tempo default; the deck's Key lock button (or K, Shift+K) still changes it for the track on that deck. Key lock steps aside while you scratch.

Drag a deck's waveform on the lanes to move it, as with a jog: a stopped deck scrubs under your hand and lands on the nearest beat when you let go (with quantize on); a playing deck is nudged, never jumped; Shift+drag searches.

**Grid** on a deck (or E, Shift+E for deck B) opens its grid bar to fix any track's beat grid while you play, as rekordbox's grid edit does: the downbeat a beat earlier or later, the grid 5 ms earlier or later, and confirm. The grid keys G, [ ], comma and period then work on that deck. A fix never moves the deck in the track, and it is the same fix the track check shows.

### Stems

Dubplate separates each track of the set into drums, bass, melody and vocals while you prepare, then the shortlist; the separation runs in the background at low priority and takes a few minutes a track. The **Stems** column of the library and each set row show where a track stands: blank, `…` waiting, a percentage while separating, `ready`. Hover a blank cell and click **Separate** to separate a track ahead of the rest, or click **Separate** on a deck. Stems are kept in Dubplate's own folder, so a track is separated once.

Once a deck's track has its stems, the strip under its transport mutes or brings back each stem, with a small level knob beside it, and **Inst** and **Acap** give the instrumental and the acapella in one touch (touch again for every stem back). Every move fades in 20 ms and key lock keeps working. With every stem up the deck plays the track exactly.

- Keys: Q, W, R and V mute or bring back the drums, bass, melody and vocals of deck A, X gives the instrumental and A the acapella; add ⇧ for deck B.
- FLX4: in Beat Loop mode, hold SHIFT. Pads 1 to 4 mute or bring back the vocals, melody, bass and drums; pad 5 is the acapella, 6 the instrumental, 7 drums and bass only (ride a log drum under the other track), 8 every stem back. rekordbox maps no stems on the FLX4, and SHIFT + the Beat Loop pads do nothing there, so this takes no control from you.

When two tracks with stems sing over each other across a seam, the seam says so in the set and suggests taking the vocals out on the outgoing track.

## 5. Record

Cmd+Shift+R (or REC in the header) records what the room hears, after the limiter, as a 24-bit WAV in Music/Dubplate Recordings. The header shows the time in red.

## Files that moved

If you moved music after importing it into rekordbox, a line above the tracks says how many files moved or are missing. **Relink** finds them again by name in your music folders, or in a folder you point at:

- a file with the same name and the same size is relinked at once;
- a file that only matches by length is shown for you to Use or Skip;
- two equally good candidates are never guessed between.

The new places live in Dubplate's settings only; rekordbox is not touched. Your sets follow the relinked files.

On macOS, Dubplate keeps the computer and display awake while the app is open. Normal idle sleep resumes when you quit.
