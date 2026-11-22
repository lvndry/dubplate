# Command line

`cargo cli` runs the `dubplate` command with the fast profile.

## rekordbox, sets and analysis

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

A set can also go straight to a USB stick, or a folder, that CDJs and XDJs read as a rekordbox export: the audio under `Contents`, each track's beat grid, hot cues, memory cues, loops and waveforms under `PIONEER/USBANLZ`, and the set as a playlist (with a second one for its backups) in `PIONEER/rekordbox/export.pdb`:

```
cargo cli usb export "Friday" --to /Volumes/STICK
cargo cli usb export "Friday" --to ~/Desktop/stick-test
```

A stick that already holds a library rekordbox wrote is refused, so nothing of it changes; a stick Dubplate exported to before is added to, and a playlist of the same name is replaced. Dubplate never replaces or deletes a file it did not write, and never writes inside rekordbox's own folders. Folder tracks take Dubplate's beat grid, analysed on the spot when Dubplate has not analysed them yet.

Dubplate analyses the tracks rekordbox never did (tempo, beat grid with downbeats, key and energy, with Euphoria's detectors). The same analysis runs on audio files or whole folders from the command line, and can be compared with rekordbox's BPM, key and grid for the tracks rekordbox also holds. Audio files and rekordbox's files are only read:

```
cargo cli analyse ~/Music/House
cargo cli analyse ~/Music/House --compare-rekordbox
```

## Check your controller

Dubplate maps the Pioneer DDJ-FLX4. Plug it in, then:

```
cargo cli midi devices
cargo cli midi monitor
cargo cli midi monitor --device "DDJ-FLX4" --lights
```

`monitor` prints every message with the time, the bytes and what Dubplate reads from it, such as `B0 22 41  deck A jog turn +1 (scratch)`, until Ctrl-C. `--lights` lights each button while it is held.
