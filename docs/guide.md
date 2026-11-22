# Prepare a set and mix it on the DDJ-FLX4

A walk through Dubplate from an empty library to a recorded mix. Dubplate reads your rekordbox collection and never writes to it: grids, cues, sets and relinks you make live in Dubplate's own files.

## Start Dubplate

```sh
cd ~/github/dubplate
cargo run --release -p dubplate-desktop
```

The first build takes a few minutes. Plug the FLX4 in before or after starting; the header shows it as soon as it is connected.

## 1. Add your music

On first launch the library is empty and asks you where your music is.

- **Add music folders…** (Cmd+O) picks one or more folders. Dubplate reads them and their subfolders, and never changes a file.
- **Import rekordbox collection** brings in your rekordbox tracks, playlists, cues, beat grids and phrases.

You can use both. Settings (Cmd+,) lists your folders with Rescan and Remove. Each track's energy (1 to 10) is analysed in the background and cached, set tracks first.

## 2. Pick the sound card

In Settings, choose **DDJ-FLX4** as the sound card so the master goes to the FLX4's master output and the headphones to its headphone jack. Turn the master down before the first track, then check the headphones with the channel CUE buttons.

## 3. Prepare the set

Cmd+1 opens Prepare: the library on the left, your set on the right.

| Do | Key |
| --- | --- |
| Add the selected track to the set / insert it after the selected set track | Enter / Cmd+Enter |
| Move a set track up or down | Alt+Up / Alt+Down |
| Take a track out / undo | Backspace / Cmd+Z |
| Keep a track as a backup | B |
| Move between library and set | Tab |

The **Next** panel suggests what to play after the selected set track, with the reason ("One step on the wheel · +1.6% · energy +2"), steered by the set's energy curve against its target. The target climbs from your library's calmer tracks to its most intense, so it fits how your music is rated.

Each set track has a **ready badge**: Ready, or what is missing (Grid unconfirmed, No mix-in cue, No mix-out cue, File missing, Key unknown). Each seam between two tracks shows the key move, the tempo step and any warning: key clash, a tempo gap over 6%, vocals meeting, an energy drop, or the same track twice.

### Check each track

Select a set track to open its track check: the waveform with the beat grid, rekordbox's phrases and the mix cues.

| Do | Key |
| --- | --- |
| Confirm the grid | G |
| Nudge the grid 5 ms earlier / later | [ / ] |
| Move the downbeat one beat earlier / later | , / . |
| Mix-in / mix-out on the bar at the playhead | I / O |
| Accept the proposed cues (from the phrases) | Y |
| Mark the seam after this track Planned or Free | F |
| Hear that seam in the headphones (20 s) | S |

A Free seam never warns: it means you will feel it live. Grids on amapiano are often off by a beat or a few milliseconds; the nudge keys fix that, and the fix follows the track onto the decks.

## 4. Mix

Cmd+L loads the set's first two tracks onto deck A and deck B at their mix-in cues; Cmd+2 opens Perform.

On the FLX4, everything works as in rekordbox's FLX4 layout:

- PLAY, CUE (CDJ style), hot cue pads, TEMPO, BEAT SYNC (hold for tempo master, SHIFT for the tempo range), the jogs (touch to scratch, side to nudge, SHIFT to search).
- The mixer: TRIM, HI/MID/LOW, the filter knobs, channel faders, crossfader, headphone CUE, MIX and LEVEL.
- The pad modes: Hot Cue, Pad FX 1 and 2 (hold for rolls and effects), Beat Jump, Beat Loop, Sampler, Keyboard, Key Shift; the loop section IN, OUT, 4 BEAT/EXIT.
- BEAT FX: FX SELECT, BEAT left and right, the CH SELECT lever, LEVEL/DEPTH and ON/OFF.
- SMART CFX turns both filter knobs into the chosen preset (Space, Dub echo, Crush, Chop, Jet, Phase; SHIFT + SMART CFX steps through them). SMART FADER makes the crossfader glide a shared tempo and swap the bass for you; a LOW knob you turn takes over.

On screen, each deck also has Sync, Master, Key lock and Slip, and the compact effects strip drives the three effect units. Press ? for every keyboard shortcut.

## 5. Record

Cmd+Shift+R (or REC in the header) records what the room hears, after the limiter, as a 24-bit WAV in Music/Dubplate Recordings. The header shows the time in red.

## Files that moved

If you moved music after importing it into rekordbox, a line above the tracks says how many files moved or are missing. **Relink** finds them again by name in your music folders, or in a folder you point at:

- a file with the same name and the same size is relinked at once;
- a file that only matches by length is shown for you to Use or Skip;
- two equally good candidates are never guessed between.

The new places live in Dubplate's settings only; rekordbox is not touched. Your sets follow the relinked files.
