<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/mark-white.svg">
  <img src="assets/logo/mark-dark.svg" alt="" width="72" height="72">
</picture>

# Dubplate

Dubplate is free, open-source DJ software for live two-deck mixing with a DDJ-FLX4 or keyboard. Play tracks from your music folders or bring in your rekordbox collection. Dubplate keeps its edits in its own library.

## On the decks

- **Cue and mix.** Preview a track in headphones while the master keeps playing. Line it up by ear or engage beat sync, swap the bass with the isolator EQ, then ride the channel faders or crossfader.
- **Play the pads.** Trigger hot cues, hold a slip roll, or send the outgoing track into an echo. Three beat FX units sit alongside channel colour effects. [Performance controls](docs/guide.md#4-mix)
- **Remix a track.** Separate vocals, melody, bass and drums ahead of time. Layer a vocal over another deck’s groove, or add a tempo-synced shaker from the sampler. Four banks each hold 16 slots. [Stems and sampler](docs/guide.md#4-mix)
- **Keep the take.** Record the master output after the limiter as a 24-bit WAV. Your headphone cues stay private. [Recording](docs/guide.md#5-record)

## Between sets

- **Build a crate.** Browse nested folders or bring across rekordbox playlists, cues and grids. Dubplate analyses folder tracks for BPM, downbeats, Camelot key and energy. [Library walkthrough](docs/guide.md#1-add-your-music)
- **Rehearse the handoff.** Arrange the running order, inspect tempo and key changes, and audition a 20-second transition in headphones. Track readiness checks flag missing files, grids that need confirmation and missing mix cues. [Set preparation](docs/guide.md#3-prepare-the-set)
- **Take the set to the booth.** Export a rekordbox XML playlist or prepare a USB for supported players. The guide covers library formats, existing sticks and importing history after a gig. [USB export and import](docs/guide.md#play-the-set-on-cdjs)

## Install on an Apple silicon Mac

**Desktop app.** Download the disk image, open it, and drag Dubplate into Applications. [Download Dubplate for Mac](https://github.com/lvndry/dubplate/releases/latest/download/Dubplate.dmg)

Version 0.1.0 carries an Apple Developer ID signature and has not been notarized by Apple. Gatekeeper may block it from opening. Check the release notes for the signing status of the version you download. [Release notes](https://github.com/lvndry/dubplate/releases/latest)

**Homebrew.** Install the desktop app from the Dubplate tap. On older Homebrew versions that lack the `trust` command, skip that line.

```sh
brew tap lvndry/dubplate
brew trust lvndry/dubplate
brew install --cask dubplate
```

For the command-line tool, run `brew install --formula dubplate` after adding the tap. [Dubplate Homebrew tap](https://github.com/lvndry/homebrew-dubplate)

**Shell installer.** This installs both the desktop app and the CLI and verifies the downloads against the release’s SHA-256 checksums.

```sh
curl -fsSL https://github.com/lvndry/dubplate/releases/latest/download/install.sh | sh
```

Run the same command again to upgrade. The CLI installs to `~/.local/bin` by default. To check its version, run `~/.local/bin/dubplate --version`. [Read the installer](https://github.com/lvndry/dubplate/releases/latest/download/install.sh)

## Run from source

```sh
cargo app
```

The first build takes a few minutes; after a change, a few seconds. Then follow [the guide](docs/guide.md): add your music, prepare a set, mix it on the FLX4 and record it.

## Learn more

Visit the [Dubplate website](https://lvndry.github.io/dubplate/) for the live mixing overview. The [Features page](https://lvndry.github.io/dubplate/features.html) covers set preparation and transition rehearsal.

- [Prepare a set and mix it](docs/guide.md): the DJ's walkthrough.
- [How the engine works](docs/engine.md): every feature of the audio engine, with the measurements behind each choice.
- [Command line](docs/cli.md): inspect a rekordbox library, analyse folders, export sets, watch your controller's MIDI.
- [Development](docs/development.md): build, check and the layout of the code.

## License

Dubplate is free software under the GNU General Public License, version 3 or any later version (`GPL-3.0-or-later`); see [LICENSE](LICENSE). It builds on Euphoria's `euphoria-core`, under the same license.
