<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark-dark.svg" alt="" width="72" height="72">
</picture>

# Dubplate

Dubplate is free DJ software for preparing and mixing sets, 100% rekordbox compatible. It reads your rekordbox library (tracks, cues, beat grids, waveforms, playlists) without changing it, and sends your sets back to rekordbox.

## What's in it

- **rekordbox library:** read from a copy of rekordbox's database and analysis files, so rekordbox can stay open. Sets export as a rekordbox XML playlist.
- **Set preparation:** an ordered set with the key, tempo and energy of every transition, next-track suggestions, grid and cue checks, and a headphone preview of each transition.
- **Mixing:** two decks with sync, key lock, loops, hot cues, slip, rolls, pad effects, a 16-slot sampler, three beat FX units, Smart CFX, Smart Fader, an isolator EQ and a limiter.
- **Controller:** the Pioneer DDJ-FLX4, mapped like rekordbox, lights included.
- **Your own folders:** tracks outside rekordbox get their tempo, beat grid, key and energy analysed.

## Run it

```sh
cargo app
```

The first build takes a few minutes; after a change, a few seconds. Then follow [the guide](docs/guide.md): add your music, prepare a set, mix it on the FLX4 and record it.

## Learn more

Open `website/index.html` in a browser for the live mixing overview. The Features page covers set preparation and transition rehearsal.

- [Prepare a set and mix it](docs/guide.md): the DJ's walkthrough.
- [How the engine works](docs/engine.md): every feature of the audio engine, with the measurements behind each choice.
- [Command line](docs/cli.md): inspect a rekordbox library, analyse folders, export sets, watch your controller's MIDI.
- [Development](docs/development.md): build, check and the layout of the code.

## License

Dubplate is free software under the GNU General Public License, version 3 or any later version (`GPL-3.0-or-later`); see [LICENSE](LICENSE). It builds on Euphoria's `euphoria-core`, under the same license.
