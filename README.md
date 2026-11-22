<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark-dark.svg" alt="" width="72" height="72">
</picture>

# Dubplate

Pro DJ software that works with rekordbox libraries and Pioneer CDJs.

Dubplate reads the rekordbox library directly, so tracks, cues, beat grids and playlists are the same in both programs. A set prepared in Dubplate goes back to rekordbox as a rekordbox XML file that rekordbox imports, so Dubplate never writes rekordbox's own library. Exporting USB sticks for CDJ and XDJ players directly is next.

## Status

To prepare a set and mix it on a DDJ-FLX4, read [the guide](docs/guide.md). What works today:

- **Library:** your own music folders and, if you choose, the rekordbox collection; energy ratings in the background; Relink for files that moved.
- **Prepare:** an ordered set with next-track suggestions, key, tempo, vocal and energy warnings on each seam, a ready check, grid nudging and mix cues, and a headphone preview of each seam.
- **Perform:** two decks with sync, key lock, key shift, loops, rolls, slip, hot cues, pad effects, a 16-slot sampler, three beat-synced effect units, Smart CFX, Smart Fader, an isolator EQ, headphone cue and a limiter.
- **DDJ-FLX4:** every control in rekordbox's FLX4 layout, with its lights.
- **Recording:** the master as the room hears it, to a 24-bit WAV.
- **To rekordbox:** Export to rekordbox… (Cmd+E) writes the set as a rekordbox XML playlist with corrected grids, the mix-in and mix-out as memory cues, and the tracks' rekordbox cues.

The first piece is a reader for the rekordbox 6 and 7 library. It copies `master.db` to a temporary directory, unlocks the copy and reads tracks, cues, saved loops and the playlist tree. rekordbox's own file is never opened or changed.

It also reads each track's analysis files (`ANLZ0000.DAT`, `.EXT` and `.2EX` under rekordbox's `share` directory): the beat grid, hot cues and memory cues with their colours and names, the phrases, and every waveform, including the 3-band ones. These files are opened read-only, in place.

## Try it

Dubplate needs the key of the rekordbox library. It is not stored in this repository. Put it in one of these:

- the file `~/Library/Application Support/dubplate/rekordbox.key` on macOS, or `~/.config/dubplate/rekordbox.key` on Linux
- the `DUBPLATE_REKORDBOX_KEY` environment variable

Then:

```
cargo cli rekordbox inspect
cargo cli rekordbox inspect --json > library.json
cargo cli rekordbox inspect --library /path/to/master.db
cargo cli rekordbox analysis "Track Title"
cargo cli rekordbox analysis <track id> --json > analysis.json
```

`analysis` takes a track's rekordbox ID or its exact title.

The sets you prepared in the desktop app can be listed and exported to a rekordbox XML file, by default in `~/Music/Dubplate Exports/<set name>.xml`:

```
cargo cli set list
cargo cli set export "Friday" --xml friday.xml
```

## Run the desktop app

On first launch the desktop app asks for your music folders; importing the rekordbox collection is a separate choice. For the rekordbox collection it reads the same key as the command line tool (see "Try it" above) and a copy of rekordbox's `master.db`, so rekordbox can stay open.

```
cargo app
```

`cargo app` and `cargo cli …` build with the `fast` profile: optimised like a release, so audio runs at full speed, but incremental and without link-time optimisation, so a rebuild after a small change takes seconds. Releases keep `--release`.

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
cargo cli midi devices
cargo cli midi monitor
cargo cli midi monitor --device "DDJ-FLX4" --lights
```

`monitor` prints every message with the time, the bytes and what Dubplate reads from it, such as `B0 22 41  deck A jog turn +1 (scratch)`, until Ctrl-C. `--lights` lights each button while it is held.

## Sync and beatmatching

Each deck's tempo fader has a range like a CDJ's TEMPO RANGE button: ±6%, ±10%, ±16% or WIDE (±100%), in rekordbox's steps of 0.02%, 0.04% or 0.5%. A pitch bend held on a deck speeds it up or slows it down for as long as it is held and glides back.

SYNC follows the tempo master, the deck chosen with the MASTER button or, by default, the louder playing deck. Tempo sync matches the master's BPM. Beat sync also puts the deck's beats on the master's: it jumps to the nearest beat at once, then holds the lock with bends too small to hear. Synced decks follow every move of the master's tempo fader, and a track at half or double the master's tempo locks to every other beat. Nudging, scratching or bending a following deck drops it to tempo sync, as on a CDJ-3000, so it can still be ridden by hand. When sync goes off, or a synced deck becomes the master, the tempo holds and the fader takes over again once it is moved through the playing tempo.

Beat grids can change tempo: built from every beat rekordbox stores, with its BPM, a grid follows a live drummer, and sync and effects follow it beat by beat.

## Headphones

Each channel has a CUE button that sends its sound to the headphones before its fader, after its EQ and filter, so a track can be prepared with its fader closed. MASTER CUE adds the master. The MIXING knob blends the cue with the master, at -3 dB each in the middle, and the LEVEL knob sets the volume. On a device with four or more outputs, such as the DDJ-FLX4's sound card, the master plays on outputs 1 and 2 and the headphones on 3 and 4. A stereo device plays only the master.

`AudioOutput::output_devices` lists the outputs and `AudioOutput::start_on` opens one by name. The play example takes `--device "DDJ-FLX4"`, and `--devices` lists them; v cues the selected deck and f and g turn the headphone mix. `--volume` sets the headphones' level too. The channel meters (`Meters::channel_peaks`) show each channel before its fader, like a DJ mixer's.

## Key lock

Key lock, the MASTER TEMPO button of a CDJ, changes a deck's tempo without changing its pitch. It runs the Signalsmith Stretch time-stretcher, chosen after comparing it with Rubber Band's R2 and R3 engines on real music at ±8% and ±16%. It kept the music's spectrum closest to the original, kept the stereo image, held a 1 kHz tone to the cent and cost about 0.2 µs per stereo frame, half of R2 and a tenth of R3. Its default preset overlaps its spectral frames four times, which keeps a key-shifted pitch true to within a few cents; the preset that spreads its work more evenly was up to 7 cents off when shifting the key.

The stretcher needs to hear about 120 ms of music ahead of what it plays. The track is fully decoded, so the deck reads ahead by that much, round loops as the playhead will, and the stretched beats land within a millisecond of the playhead. After a jump, hot cue or beat jump the stretcher starts again at the new place at once. Scratching, and a record held still, play without key lock, as on a CDJ; at the track's own tempo the deck plays the track untouched. Turning key lock on or off crossfades over 20 ms. In the play example, a switches key lock on the selected deck.

## Pads: rolls, slip, pad effects and key shift

The DDJ-FLX4's pad modes map onto the engine:

- **Hot Cue** plays hot cues, in rhythm with quantize.
- **Beat Jump** jumps 1, 2, 4 or 8 beats either way.
- **Beat Loop** sets 1/4 to 32-beat loops.
- **Rolls** repeat the beat just passed while a pad is held. The track keeps playing silently underneath, and on release the deck lands in rhythm where it would have been: slip.
- **SLIP** extends slip to scratches and loops. `SlipState` gives the shadow playhead for the waveform.
- **Pad FX** holds any of the effects on one deck, after its EQ and filter. A brake gives the live track back on release, since the track kept playing; echoes and reverbs ring out.
- **Key Shift** and **Keyboard** move the key by -12 to +12 semitones through the key lock time-stretcher, at any tempo. Without key lock the shift adds to the pitch the tempo gives.

While a deck slips, beat sync leaves it alone, because its shadow stays in phase.

## Smart CFX

`Controller::set_smart_cfx` turns a channel's filter knob into a one-knob macro: the filter and a second effect move together, and the knob's centre still leaves the music untouched. There are six presets: Space (reverb), Dub Echo (a 3/4-beat echo), Crush (bit crusher), Chop (a 1/8-beat gate), Jet (a 4-beat flanger) and Phase (a 2-beat phaser). All of them are the effects of the beat FX, with no second copy of their DSP.

The effect grows with the square of the knob's travel. An A/B on real music against linear and smoothstep curves showed the square curve kept the first moves under 0.3 dB, had the smallest loudness steps along the sweep and the least boost. Reverb, echo, flanger and phaser sit before the filter, so a closing low-pass darkens their tails; crusher and gate sit after it. A preset switched off rings out.

## Smart Fader

`Controller::set_smart_fader` makes long blends clean for any DJ:

- The two decks share one tempo, with their beats locked. The crossfader moves the tempo from deck A's own BPM at the left to deck B's at the right, gliding there, with both decks equally loud at the half-way point in the middle.
- Across the middle third of the crossfader the bass swaps: the outgoing deck's LOW turns down to a kill while the incoming deck's comes up to the centre.
- A LOW knob the DJ turns away from its centre takes over from the swap.
- Switching Smart Fader off leaves the tempo where it is until the tempo faders are moved through it.

## Sampler

Sixteen sampler slots, eight on each side of the DDJ-FLX4's pads, each one-shot or loop, with an optional gate that plays only while the pad is held and a gain of its own. A synced slot with a BPM plays at the tempo master's tempo, half or double when closer, and a synced loop starts in phase with the master's beats, as beat sync would put it. Pressing a playing slot starts it again with a crossfade; loops go round sample-accurately. The sampler has its own bus to the master after the channel faders, with a volume and a CUE button for the headphones.

## Recording

`Controller::start_recording` returns a `RecordingTap`: the master output exactly as it goes to the speakers, after the master level and the limiter, as interleaved stereo frames at the engine's sample rate. The audio thread copies each block into the tap's buffer without allocating or waiting. Frames that do not fit, because the tap was not read in time, are dropped and counted in `RecordingTap::dropped_frames`. Reading it on a thread of your own and writing a WAV file records the set.

## Preview a seam

`Controller::preview` auditions a transition in the headphones before playing it: two tracks, each from its own start at its own tempo, the second coming in at the planned overlap, with key lock if asked. It plays on the headphone bus only, after the cue mix and before the headphone level and limiter, using the decks' own reader and time-stretcher, and never touches the decks or the master output: a test checks the master stays bit for bit the same while a preview plays. It stops by itself when both tracks end, or after 30 seconds. `Controller::preview_state` says whether it plays and how far it is.

## Master limiter

The master and the headphones each end in a look-ahead limiter, so however hard the decks are pushed no sample passes -0.3 dBFS. It sees peaks 1.5 ms ahead and turns down smoothly before they arrive, holds the gain for 20 ms so a bass note is turned down without distortion, and lets go over 120 ms. Music under the ceiling passes untouched, only delayed by the look-ahead. `Meters::master_limiter_gain` shows how far the master was pushed into it, for a LIMIT light.

The settings came from an A/B on real music pushed 9 dB too hot and on a 40 Hz sine 6 dB over the ceiling. Without the hold, a 1.5 ms look-ahead distorted the bass at -35 dB and a 5 ms one at -60 dB. The 20 ms hold brought it below -150 dB without adding latency.

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
