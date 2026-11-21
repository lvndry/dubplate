<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark-dark.svg" alt="" width="72" height="72">
</picture>

# Dubplate

Pro DJ software that works with rekordbox libraries and Pioneer CDJs.

Dubplate reads and writes the rekordbox library directly, so tracks, cues, beat grids and playlists stay the same in both programs. It will export USB sticks that play on CDJ and XDJ players, and run DJ sets with every feature included.

## Status

The first piece is a reader for the rekordbox 6 and 7 library. It copies `master.db` to a temporary directory, unlocks the copy and reads tracks, cues, saved loops and the playlist tree. rekordbox's own file is never opened or changed.

## Try it

Dubplate needs the key of the rekordbox library. It is not stored in this repository. Put it in one of these:

- the file `~/Library/Application Support/dubplate/rekordbox.key` on macOS, or `~/.config/dubplate/rekordbox.key` on Linux
- the `DUBPLATE_REKORDBOX_KEY` environment variable

Then:

```
cargo run -p dubplate -- rekordbox inspect
cargo run -p dubplate -- rekordbox inspect --json > library.json
cargo run -p dubplate -- rekordbox inspect --library /path/to/master.db
```

## Layout

- `crates/dubplate-rekordbox`: reads the rekordbox library. `schema/rekordbox-7.2.19.sql` holds the tables of rekordbox 7.2.19, with no data, and the tests build an encrypted library from it.
- `crates/dubplate-fx`: the DJ effects (echo, ping-pong, reverb, filters, the one-knob sweep filter, flanger, phaser, trance gate, bit crusher, brake and backspin). They run on the audio thread without allocating, follow the tempo and beat grid, never click when a knob moves, and let echo and reverb trails ring out.
- `apps/dubplate-cli`: the `dubplate` command.

## Hear the effects

Render any WAV file through one effect:

```
cargo run --release -p dubplate-fx --example render -- input.wav echo output.wav --bpm 124
cargo run --release -p dubplate-fx --example render -- input.wav sweep output.wav --depth 0 --depth-to 1
cargo run --release -p dubplate-fx --example render -- input.wav reverb output.wav --on 8 --off 16
```

`--on` and `--off` switch the effect at those seconds, `--depth` turns the performance knob and `--beat` sets a beat length such as `1/2` or `4`. Run it with no arguments to list the effects.

## Development

```
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```
