<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark-dark.svg" alt="" width="72" height="72">
</picture>

# Dubplate

Pro DJ software that works with rekordbox libraries and Pioneer CDJs.

Dubplate reads and writes the rekordbox library directly, so tracks, cues, beat grids and playlists stay the same in both programs. It will export USB sticks that play on CDJ and XDJ players, and run DJ sets with every feature included.

## Status

The first piece is a reader for the rekordbox 6 and 7 library. It copies `master.db` to a temporary directory, unlocks the copy and reads tracks, cues, saved loops and the playlist tree. rekordbox's own file is never opened or changed.

It also reads each track's analysis files (`ANLZ0000.DAT`, `.EXT` and `.2EX` under rekordbox's `share` directory): the beat grid, hot cues and memory cues with their colours and names, the phrases, and every waveform, including the 3-band ones. These files are opened read-only, in place.

## Try it

Dubplate needs the key of the rekordbox library. It is not stored in this repository. Put it in one of these:

- the file `~/Library/Application Support/dubplate/rekordbox.key` on macOS, or `~/.config/dubplate/rekordbox.key` on Linux
- the `DUBPLATE_REKORDBOX_KEY` environment variable

Then:

```
cargo run -p dubplate -- rekordbox inspect
cargo run -p dubplate -- rekordbox inspect --json > library.json
cargo run -p dubplate -- rekordbox inspect --library /path/to/master.db
cargo run -p dubplate -- rekordbox analysis "Track Title"
cargo run -p dubplate -- rekordbox analysis <track id> --json > analysis.json
```

`analysis` takes a track's rekordbox ID or its exact title.

## Run the desktop app

The desktop app opens on your rekordbox collection with two decks, a mixer and the browser. It reads the same key as the command line tool (see "Try it" above) and a copy of rekordbox's `master.db`, so rekordbox can stay open.

```
cargo run --release -p dubplate-desktop
```

On macOS, `cargo run` starts it from a `Dubplate.app` next to the binary (see `.cargo/config.toml`), so the menu bar and the Dock show Dubplate and the One groove icon. To build a release `Dubplate.app` to keep:

```
./scripts/bundle-macos.sh && open target/release/Dubplate.app
```

Pick a crate on the left, type in the search field to find a track by title or artist, then drag it onto a deck, double-click it, click A or B at the end of its row, or press ⇧← or ⇧→ to load it on deck A or B. Audio files dragged from Finder (MP3, M4A, AAC, FLAC, WAV, AIFF, Ogg) load too, without beat grid or cues. The decks show rekordbox's 3-band waveforms, beat grid and hot cues. The first build takes a few minutes.

| Keys | Deck A | Deck B |
| --- | --- | --- |
| Play or pause | Space | ⇧ Space |
| Cue | C | ⇧ C |
| Hot cues A to H | 1 to 8 | ⇧ 1 to ⇧ 8 |
| Tempo down / up 0.1% | - / = | ⇧ - / ⇧ = |
| Load the selected track | ⇧ ← | ⇧ → |

← and → move the crossfader, ↑ and ↓ select tracks, / or ⌘F searches, Enter goes from the search field back to the tracks, Esc closes the help and ? shows every key. Drag a fader or the crossfader with the mouse, double-click a slider to put it back to its rest position, and click a deck's overview waveform to jump there. With Lock playing deck on (the button in the header, on by default), a deck that is playing keeps its track until it is paused, as on CDJs.

Set `DUBPLATE_FRAME_STATS=1` to print the frame rate and how long each frame took to draw, once a second.

## Play tracks

The audio engine plays tracks on two decks with a crossfader. To hear it:

```
cargo run --release -p dubplate-engine --example play -- <file-a> [file-b] [--buffer 256]
```

It prints the buffer size, the output latency and the dropout counters every second. In a terminal, space plays or pauses the selected deck, tab selects the other deck, c is the cue button, up and down change the tempo, left and right move the crossfader, x throws the crossfader to the other side and q quits. On the selected deck, k kills or brings back the low, h the high, j and l turn the filter knob, , and . nudge the jog back and forward, n and m search back and forward, s switches beat sync, p makes the deck the tempo master and r steps through the tempo ranges. For loops, 4 sets a 4-beat loop, i and o are loop in and loop out, 9 and 0 halve and double the loop, u is reloop/exit, and ; and ' jump 4 beats back and forward.

## Mix with the EQ, the filter and the jogs

Each channel has a three-band isolator EQ like a Pioneer DJM's: the knob's centre leaves the music untouched, fully left kills the band and fully right boosts it by 6 dB. The bands split at 250 Hz and 2.5 kHz. A killed low is more than 65 dB down at 60 Hz, a killed high more than 45 dB down at 8 kHz. The bands come from phase-linear Bessel filters subtracted from a delayed copy of the music, so killing the low leaves a kick's attack as sharp as it was; a Linkwitz-Riley crossover, the usual choice, rounds it off. The cost is a constant 2.7 ms delay on both decks, which `EngineStats::mixer_latency_ms` reports.

After the EQ, the filter knob (CFX on the DDJ-FLX4) closes a low-pass to the left and opens a high-pass to the right, with resonance, and passes the music untouched at its centre. It is the sweep filter of the effects, so the two sound the same.

The jog wheels take the ticks a controller reports, 720 per revolution on the DDJ-FLX4:

- In vinyl mode, a hand on the platter top stops the record in a few milliseconds, and turning it scratches: one revolution moves 1.8 seconds of the track, as a record at 33⅓ RPM, forwards or backwards, and bringing the platter back returns to the same frame. Letting go returns to the deck's tempo, or to a stop on a paused deck.
- Turning the edge of a playing deck bends its tempo with the wheel's speed to line up beats. The bend fades out smoothly when the wheel stops, and every revolution moves the music 0.3 seconds whatever the speed. On a stopped deck the wheel moves the playhead by hand, to find a beat by ear.
- With SHIFT, the wheel searches through the track, 15 seconds per revolution.

## Check your controller

Dubplate maps the Pioneer DDJ-FLX4. Plug it in, then:

```
cargo run -p dubplate -- midi devices
cargo run -p dubplate -- midi monitor
cargo run -p dubplate -- midi monitor --device "DDJ-FLX4" --lights
```

`monitor` prints every message with the time, the bytes and what Dubplate reads from it, such as `B0 22 41  deck A jog turn +1 (scratch)`, until Ctrl-C. `--lights` lights each button while it is held.

## Sync and beatmatching

Each deck's tempo fader has a range like a CDJ's TEMPO RANGE button: ±6%, ±10%, ±16% or WIDE (±100%), in rekordbox's steps of 0.02%, 0.04% or 0.5%. A pitch bend held on a deck speeds it up or slows it down for as long as it is held and glides back.

SYNC follows the tempo master, the deck chosen with the MASTER button or, by default, the louder playing deck. Tempo sync matches the master's BPM. Beat sync also puts the deck's beats on the master's: it jumps to the nearest beat at once, then holds the lock with bends too small to hear. Synced decks follow every move of the master's tempo fader, and a track at half or double the master's tempo locks to every other beat. Nudging, scratching or bending a following deck drops it to tempo sync, as on a CDJ-3000, so it can still be ridden by hand. When sync goes off, or a synced deck becomes the master, the tempo holds and the fader takes over again once it is moved through the playing tempo.

Beat grids can change tempo: built from every beat rekordbox stores, with its BPM, a grid follows a live drummer, and sync and effects follow it beat by beat.

## Headphones

Each channel has a CUE button that sends its sound to the headphones before its fader, after its EQ and filter, so a track can be prepared with its fader closed. MASTER CUE adds the master. The MIXING knob blends the cue with the master, at -3 dB each in the middle, and the LEVEL knob sets the volume. On a device with four or more outputs, such as the DDJ-FLX4's sound card, the master plays on outputs 1 and 2 and the headphones on 3 and 4. A stereo device plays only the master.

`AudioOutput::output_devices` lists the outputs and `AudioOutput::start_on` opens one by name. The play example takes `--device "DDJ-FLX4"`, and `--devices` lists them; v cues the selected deck and f and g turn the headphone mix. `--volume` sets the headphones' level too. The channel meters (`Meters::channel_peaks`) show each channel before its fader, like a DJ mixer's.

## Loops, beat jump and quantize

Beat loops run from a sixteenth of a beat to 32 beats on the beat grid, so a loop on a track whose tempo changes still holds whole beats. LOOP IN and LOOP OUT set a loop by hand, halving and doubling work from the loop's start, and RELOOP/EXIT leaves the loop or goes back to it. Beat jump moves any number of beats either way and takes a loop that is on along with it. Every jump and every turn of a loop crossfades over 5 ms, so none of them clicks.

Quantize is on by default, as in rekordbox. With a beat grid, the cue button, LOOP IN and LOOP OUT set their points on the nearest beat, a beat loop starts on the beat just passed so the music never jumps, and hot cues and reloop land in rhythm, as far past their point as the playhead was past its last beat. A hot cue on a stopped deck jumps there and plays.

## Effects while you mix

The engine has three effect units, like the beat effects section of a DJ mixer or the FX section of a DDJ-FLX4. Each unit holds one effect (echo, ping-pong, reverb, the filters, flanger, phaser, gate, crusher, brake or backspin), sits on deck A, deck B or the master, and has a beat length from 1/16 to 32 beats, a depth knob and an on button. On a deck, a unit comes after the channel fader and the crossfader, so an echo or reverb keeps ringing out when the fader closes. On the master, it comes before the master gain.

Effects follow the tempo of the music they sit on: the BPM of the track's beat grid times the deck's speed, with beats counted from the grid's first beat. On the master they follow the louder deck, or the deck chosen as tempo master. A deck without a beat grid runs its effects at 120 BPM.

Every effect of every unit is built when the engine starts, so choosing another effect or moving a unit never allocates on the audio thread: the old effect fades out, or rings out its trail where it was, while the new one fades in.

To try them, give each track's BPM to the play example:

```
cargo run --release -p dubplate-engine --example play -- <file-a> <file-b> --bpm 124 --bpm 126
```

1, 2 and 3 switch the units on and off. For unit 1, e selects the next effect, t moves it to the next target (deck A, deck B, master), [ and ] shorten and lengthen the beat, and - and = turn the depth down and up.

## Layout

- `crates/dubplate-rekordbox`: reads the rekordbox library and, in `src/analysis`, the per-track analysis files. `schema/rekordbox-7.2.19.sql` holds the tables of rekordbox 7.2.19, with no data, and the tests build an encrypted library from it.
- `crates/dubplate-engine`: the audio engine. Decks, mixer and crossfader run on a real-time audio thread that never allocates, locks or waits; tracks decode on their own threads; the control side talks to it through lock-free queues. Effect units run the effects of `dubplate-fx` on a deck or the master, in time with the beat grid; any other processor plugs into the insert slots through the `AudioProcessor` trait.
- `crates/dubplate-fx`: the DJ effects (echo, ping-pong, reverb, filters, the one-knob sweep filter, flanger, phaser, trance gate, bit crusher, brake and backspin) and the mixer channel's isolator EQ. They run on the audio thread without allocating, follow the tempo and beat grid, never click when a knob moves, let echo and reverb trails ring out, and keep those trails under full scale on a full-level track.
- `crates/dubplate-midi`: MIDI controllers. DJ actions that do not depend on the controller, controller mappings as TOML in `mappings/` (the DDJ-FLX4 first), decoding on the MIDI thread without allocating, and the messages that light the controller.
- `apps/dubplate-cli`: the `dubplate` command.
- `apps/dubplate-desktop`: the desktop app, built with GPUI. Its fonts are Inter and Geist Mono, both under the SIL Open Font License (`apps/dubplate-desktop/assets/fonts`).

## Hear the effects

Render one of your tracks through one effect. It reads every format Dubplate plays (MP3, M4A with AAC or ALAC, FLAC, WAV, AIFF and Ogg) and writes a WAV file at the track's own sample rate:

```
cargo run --release -p dubplate-fx --example render -- "$HOME/Music/Afro House/Artist - Title (Extended Mix).m4a" echo echo.wav --bpm 124 --beat 1/2 --on 32 --off 40
cargo run --release -p dubplate-fx --example render -- "$HOME/Music/Afro House/Artist - Title (Extended Mix).m4a" sweep sweep.wav --depth 0 --depth-to 1
```

`--on` and `--off` are seconds into the track: the effect switches on and off there, and echo, ping-pong and reverb ring out after `--off`. `--bpm` is the track's tempo, `--depth` turns the performance knob and `--beat` sets a beat length such as `1/2` or `4`. Put quotes around a path with spaces. Run it with `--help` to list the effects and every option.

## Development

```
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```
