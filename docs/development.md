# Development

## Run it

The rekordbox library's key is not stored in this repository. Put it in `~/Library/Application Support/dubplate/rekordbox.key` on macOS (`~/.config/dubplate/rekordbox.key` on Linux) or the `DUBPLATE_REKORDBOX_KEY` environment variable.

On first launch the desktop app asks for your music folders; importing the rekordbox collection is a separate choice. For the rekordbox collection it reads the same key as the command line tool (see [the command line](cli.md)) and a copy of rekordbox's `master.db`, so rekordbox can stay open.

```
cargo app
```

`cargo app` and `cargo cli …` build with the `fast` profile: optimised like a release, so audio runs at full speed, but incremental and without link-time optimisation, so a rebuild after a small change takes seconds. Releases keep `--release`.

On macOS, `cargo run` starts it from a `Dubplate.app` next to the binary (see `.cargo/config.toml`), so the menu bar and the Dock show Dubplate and the One groove icon. To build a release `Dubplate.app` to keep:

```
./scripts/bundle-macos.sh && open target/release/Dubplate.app
```

Pick a crate on the left, type in the search field to find a track by title or artist, then drag it onto a deck, double-click it, click A or B at the end of its row, or press ⇧← or ⇧→ to load it on deck A or B. Audio files dragged from Finder (MP3, M4A, AAC, FLAC, WAV, AIFF, Ogg) load too, without beat grid or cues. The decks show rekordbox's 3-band waveforms, beat grid and hot cues, and draw a 3-band waveform from the audio for a track rekordbox has none for. The first build takes a few minutes.

| Keys | Deck A | Deck B |
| --- | --- | --- |
| Play or pause | Space | ⇧ Space |
| Cue | C | ⇧ C |
| Hot cues A to H | 1 to 8 | ⇧ 1 to ⇧ 8 |
| Previous / next memory cue | H / J | ⇧ H / ⇧ J |
| Keep / delete a memory cue | N / ⌥ N | ⇧ N / ⇧ ⌥ N |
| Tempo down / up 0.1% | - / = | ⇧ - / ⇧ = |
| Load the selected track | ⇧ ← | ⇧ → |

← and → move the crossfader, ↑ and ↓ select tracks, / or ⌘F searches, Enter goes from the search field back to the tracks, Esc closes the help and ? shows every key. Drag a fader or the crossfader with the mouse, double-click a slider to put it back to its rest position, and click a deck's overview waveform to jump there. With Lock playing deck on (the button in the header, on by default), a deck that is playing keeps its track until it is paused, as on CDJs.

Set `DUBPLATE_FRAME_STATS=1` to print the frame rate and how long each frame took to draw, once a second.

## Checks

```
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- --deny warnings
cargo test --workspace --locked
```

The manual callback benchmark reports both thread CPU time and wall time. Run it in an optimized build; wall-time scheduling outliers on a busy host are not DSP cost:

```
cargo test --release -p dubplate-engine --locked --test callback_budget -- --ignored --nocapture
```

Tab and Shift-Tab move through controls; Enter and Space activate buttons. Arrow keys adjust a focused knob or slider, Shift gives finer steps, Home/End reach its bounds and Backspace resets it. Ctrl-Tab switches the Prepare pane. Dialogs trap focus and restore it when closed. Selecting a library track reveals its full title, artist and any analysis failure.

## Layout

- `crates/dubplate-rekordbox`: reads the rekordbox library and, in `src/analysis`, the per-track analysis files. `schema/rekordbox-7.2.19.sql` holds the tables of rekordbox 7.2.19, with no data, and the tests build an encrypted library from it.
- `crates/dubplate-engine`: the audio engine. Decks, mixer and crossfader run on a real-time audio thread that never allocates, locks or waits; tracks decode on their own threads; the control side talks to it through lock-free queues. Effect units run the effects of `dubplate-fx` on a deck or the master, in time with the beat grid; any other processor plugs into the insert slots through the `AudioProcessor` trait.
- `crates/dubplate-fx`: the DJ effects (echo, ping-pong, reverb, filters, the one-knob sweep filter, flanger, phaser, trance gate, bit crusher, brake and backspin) and the mixer channel's isolator EQ. They run on the audio thread without allocating, follow the tempo and beat grid, never click when a knob moves, let echo and reverb trails ring out, and keep those trails under full scale on a full-level track.
- `crates/dubplate-midi`: MIDI controllers. DJ actions that do not depend on the controller, controller mappings as TOML in `mappings/` (the DDJ-FLX4 first), decoding on the MIDI thread without allocating, and the messages that light the controller.
- `crates/dubplate-prep`: analysis, suggestions and the prepared-set domain. `SetStore::replace_set` restores an aggregate transactionally; `SetSession` serializes edits on a worker and publishes immutable committed snapshots. Prepare ranking runs in the background and views consume a cached model. Library rescans revalidate backend analysis fingerprints and reject obsolete answers.
- `apps/dubplate-cli`: the `dubplate` command.
- `apps/dubplate-desktop`: the desktop app, built with GPUI. Its fonts are Inter and Geist Mono, both under the SIL Open Font License (`apps/dubplate-desktop/assets/fonts`).
