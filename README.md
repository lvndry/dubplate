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

## Layout

- `crates/dubplate-rekordbox`: reads the rekordbox library and, in `src/analysis`, the per-track analysis files. `schema/rekordbox-7.2.19.sql` holds the tables of rekordbox 7.2.19, with no data, and the tests build an encrypted library from it.
- `apps/dubplate-cli`: the `dubplate` command.

## Development

```
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```
