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

## Play tracks

The audio engine plays tracks on two decks with a crossfader. To hear it:

```
cargo run --release -p dubplate-engine --example play -- <file-a> [file-b] [--buffer 256]
```

It prints the buffer size, the output latency and the dropout counters every second. In a terminal, space plays or pauses the selected deck, tab selects the other deck, c is the cue button, up and down change the tempo, left and right move the crossfader, x throws the crossfader to the other side and q quits.

## Check your controller

Dubplate maps the Pioneer DDJ-FLX4. Plug it in, then:

```
cargo run -p dubplate -- midi devices
cargo run -p dubplate -- midi monitor
cargo run -p dubplate -- midi monitor --device "DDJ-FLX4" --lights
```

`monitor` prints every message with the time, the bytes and what Dubplate reads from it, such as `B0 22 41  deck A jog turn +1 (scratch)`, until Ctrl-C. `--lights` lights each button while it is held.

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
- `crates/dubplate-fx`: the DJ effects (echo, ping-pong, reverb, filters, the one-knob sweep filter, flanger, phaser, trance gate, bit crusher, brake and backspin). They run on the audio thread without allocating, follow the tempo and beat grid, never click when a knob moves, let echo and reverb trails ring out, and keep those trails under full scale on a full-level track.
- `crates/dubplate-midi`: MIDI controllers. DJ actions that do not depend on the controller, controller mappings as TOML in `mappings/` (the DDJ-FLX4 first), decoding on the MIDI thread without allocating, and the messages that light the controller.
- `apps/dubplate-cli`: the `dubplate` command.

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
