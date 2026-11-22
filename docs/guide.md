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

## 2. Pick the sound card

In Settings, choose **DDJ-FLX4** as the sound card so the master goes to the FLX4's master output and the headphones to its headphone jack. Turn the master down before the first track, then check the headphones with the channel CUE buttons. Dubplate remembers the master and headphone levels and the screen you were on, and opens no louder than it closed: if the FLX4's MASTER LEVEL knob reports a louder level when it connects, it waits until you turn it down through the level on screen, while turning it down always works at once.

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

- Click a column header to sort by it, again to reverse it, and a third time to go back to the crate's own order. Prepare and Perform each remember their sort.
- The search field matches titles, artists, genres, comments and the folders a track is filed in, so `amapiano warmup` finds the tracks in your amapiano/warmup folder. The Genre column shows the genre tag, or those folders when the track has none.
- **BPM** keeps a tempo range, a few BPM either side of the selected set track (or the playing deck); the arrows move each end.
- **Key fits** keeps the tracks whose key mixes well after the selected set track in Prepare, or the deck playing in Perform: the same key, one step on the wheel, or the relative key, as the Next suggestions choose.

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
- the hot cues and memory cues it already had in rekordbox.

Tracks from your own folders go in too, so rekordbox adds them; relinked files point at their new place. Dubplate writes only this file, never rekordbox's library.

To bring it in, in rekordbox:

1. Preferences > Advanced > Database > rekordbox xml: choose the file. If the rekordbox xml tree is hidden, turn it on in Preferences > View > Layout.
2. In the sidebar, open rekordbox xml and the set's playlist.
3. Select all its tracks and choose **Import To Collection**. rekordbox only updates tracks it already has this way; importing just the playlist would keep their old grids and cues.
4. Right-click the playlist and choose **Import Playlist**.

From then on the set is an ordinary rekordbox playlist, ready to export to a USB stick for CDJs.

## 4. Mix

Cmd+L loads the set's first two tracks onto deck A and deck B at their mix-in cues; Cmd+2 opens Perform. A set track whose file is missing is never loaded: its deck keeps what it had, and a notice names the track with a **Relink** button that looks for the file. The same goes for any track loaded from the library.

On the FLX4, everything works as in rekordbox's FLX4 layout:

- PLAY, CUE (CDJ style), hot cue pads, TEMPO, BEAT SYNC (hold for tempo master, SHIFT for the tempo range), the jogs (touch to scratch, side to nudge, SHIFT to search).
- The mixer: TRIM, HI/MID/LOW, the filter knobs, channel faders, crossfader, headphone CUE, MIX and LEVEL.
- The pad modes: Hot Cue, Pad FX 1 and 2 (hold for rolls and effects), Beat Jump, Beat Loop, Sampler, Keyboard, Key Shift; the loop section IN, OUT, 4 BEAT/EXIT.
- BEAT FX: FX SELECT, BEAT left and right, the CH SELECT lever, LEVEL/DEPTH and ON/OFF.
- SMART CFX turns both filter knobs into the chosen preset (Space, Dub echo, Crush, Chop, Jet, Phase; SHIFT + SMART CFX steps through them). SMART FADER makes the crossfader glide a shared tempo and swap the bass for you; a LOW knob you turn takes over.

When Smart Fader or sync has moved a deck's tempo away from where its TEMPO slider sits, the on-screen slider shows the tempo the deck plays, an amber outline where the fader waits, and "Move to take over": move the fader through the playing tempo and it takes over without a jump, as in rekordbox. The BEAT FX LEVEL/DEPTH knob shows an amber mark on the effect's level bar in the same case.

On screen, each deck also has Sync, Master, Key lock and Slip, and the compact effects strip drives the three effect units. Press ? for every keyboard shortcut.

Drag a deck's waveform on the lanes to move it, as with a jog: a stopped deck scrubs under your hand and lands on the nearest beat when you let go (with quantize on); a playing deck is nudged, never jumped; Shift+drag searches.

**Grid** on a deck (or E, Shift+E for deck B) opens its grid bar to fix any track's beat grid while you play, as rekordbox's grid edit does: the downbeat a beat earlier or later, the grid 5 ms earlier or later, and confirm. The grid keys G, [ ], comma and period then work on that deck. A fix never moves the deck in the track, and it is the same fix the track check shows.

## 5. Record

Cmd+Shift+R (or REC in the header) records what the room hears, after the limiter, as a 24-bit WAV in Music/Dubplate Recordings. The header shows the time in red.

## Files that moved

If you moved music after importing it into rekordbox, a line above the tracks says how many files moved or are missing. **Relink** finds them again by name in your music folders, or in a folder you point at:

- a file with the same name and the same size is relinked at once;
- a file that only matches by length is shown for you to Use or Skip;
- two equally good candidates are never guessed between.

The new places live in Dubplate's settings only; rekordbox is not touched. Your sets follow the relinked files.
