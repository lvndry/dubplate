<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark-dark.svg" alt="" width="72" height="72">
</picture>

# Dubplate

**Pro DJ software, free, that speaks rekordbox.**

Prepare a set, mix it on your controller, take it to rekordbox and the CDJs. Dubplate reads your rekordbox library as it is (cues, beat grids, waveforms, playlists) and every pro feature is in the box: no plan, no subscription, nothing locked.

## Why it's different

- **Your rekordbox library, untouched.** Dubplate opens a copy of rekordbox's database and its analysis files, so rekordbox can stay open and nothing of yours is ever rewritten. Sets go back to rekordbox as a playlist it imports.
- **Set preparation built in.** Order a set, see the key move, tempo step and energy of every transition, get the next track suggested with the reason why, check each track's grid and mix cues, and hear any transition in your headphones before you play it.
- **A real-time engine with no compromises.** Key lock on Signalsmith Stretch, a DJM-style isolator EQ that keeps a kick's attack, beat sync that holds to the millisecond, sixteen sampler slots, Smart CFX and Smart Fader, and a limiter so nothing ever clips. The audio thread never allocates, locks or waits.
- **Your controller, every control.** The Pioneer DDJ-FLX4 works exactly as in rekordbox's layout, pads, jogs, beat FX and lights included.
- **Your music, not just rekordbox's.** Add your own folders. Tracks rekordbox never analysed get their tempo, beat grid, key and energy from Euphoria's detectors, which agree with rekordbox's BPM on 99% of the tracks both know.

## Run it

```sh
cargo app
```

The first build takes a few minutes; after a change, a few seconds. Then follow [the guide](docs/guide.md): add your music, prepare a set, mix it on the FLX4 and record it.

## Learn more

- [Prepare a set and mix it](docs/guide.md): the DJ's walkthrough.
- [How the engine works](docs/engine.md): every feature of the audio engine, with the measurements behind each choice.
- [Command line](docs/cli.md): inspect a rekordbox library, analyse folders, export sets, watch your controller's MIDI.
- [Development](docs/development.md): build, check and the layout of the code.

## License

Dubplate is free software under the GNU General Public License, version 3 or any later version (`GPL-3.0-or-later`); see [LICENSE](LICENSE). It builds on Euphoria's `euphoria-core`, under the same license.
