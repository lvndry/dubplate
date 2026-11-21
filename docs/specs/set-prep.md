# Prepare a set

How a DJ goes from their rekordbox collection to an ordered set, ready to mix on the decks. This is the spec the set preparation pull requests build against.

## Goal

The owner prepares a set of about 20 afro house and amapiano tracks from their rekordbox collection, then mixes it at home on a DDJ-FLX4. Dubplate suggests and checks; the DJ decides. Dubplate never builds a set on its own.

Done when:

1. From the rekordbox collection, the DJ builds an ordered set in Dubplate.
2. Every track in the set has the grid and cues needed to mix it, and every transition (seam) shows where and how it happens and whether key and tempo fit.
3. Key, energy and next-track suggestions guide the picks.
4. The set loads onto the decks for mixing and is saved. rekordbox is only read; anything written goes to a copy.

## Decisions

| Question | Decision | Why |
| --- | --- | --- |
| Where the set lives | A Set panel docked to the right of the library in Prepare, not its own mode | The DJ's loop is dig, place, check, plan. A mode switch breaks it. |
| Space key | Space plays deck A in every mode. Shortlist is **B** | Muscle memory and safety on a live system: a key never changes meaning between modes. |
| Audition | **P** plays a track from its mix-in cue, **S** plays 20 seconds of a seam, both on the headphone bus only | Never surprise the room. The seam audition waits for the engine's headphone cue. |
| Vocals | Tagged by hand in the first version (vocals in and out per track) | Automatic detection needs stem separation, minutes per track. The clash warning reads the tags. |
| Undo | Every edit saves at once; undo in memory for the session; snapshots on lock and every 5 minutes | Undo across restarts adds complexity for little gain. |
| Suggestions | Euphoria's `next_tracks` and `evaluate_set`, never `build_set` | The DJ picks every track. |
| Key | rekordbox's key is shown; Euphoria's detected key only flags a disagreement | The DJ trusts the key in their library. |
| Words | Euphoria's labels for key moves, such as "One step on the wheel" | One vocabulary across the owner's two apps. |
| Storage | An app-owned SQLite file in Dubplate's support folder, never inside rekordbox's folders | Sets are Dubplate's data. A path guard refuses writes under rekordbox. |

## The Prepare screen

- **Library table:** in-set dot, title, artist, BPM, key (Camelot), energy, stage (warm-up, build, peak), vocals, ready, time, played. Key and BPM are tinted by fit against the set's last track, always with a text mark, never colour alone.
- **Set panel:** name, track count, running time against the target, BPM range, how many tracks are ready. Below it an energy curve with the target, a BPM line and a key strip, then the ordered list with a thin seam row between tracks, then a backup list and opener and closer marks.
- **Next:** five suggestions after the selected set track, each with its reason, such as "One step on the wheel · +1.6% · energy +2".
- **Seam strip** (opens on a seam): the outgoing and incoming waveforms stacked on the phrase grid with the overlap shaded, and one line for the plan: overlap in bars, who adjusts the tempo and by how much, the EQ intent (bass swap, high-pass the incoming track, long filter blend), a note, and Planned or Free.
- **Ready check per track:** Ready, Grid unconfirmed, No mix-in cue, No mix-out cue, File missing, Key unknown. Amber means needs attention; red only for a missing file.
- **Seam warnings:** key clash, tempo gap needing a long blend, vocals overlapping, energy drop. A seam marked Free never warns: it means the DJ will feel it live.

Shortcuts: B shortlist, Enter add to set, Cmd+Enter insert after the selection, Alt+Up and Alt+Down reorder, Backspace remove, G confirm the grid, Shift+G open in Edit, I and O set mix-in and mix-out at the playhead snapped to the bar, Y accept proposed cues, F Planned or Free, P and S audition, Cmd+L load the set to the decks, Cmd+R list everything not ready.

## In Perform

Cmd+L puts track 1 on deck A and track 2 on deck B, cued at its mix-in. Deck B shows the next track, the seam's plan and a bar countdown near the mix-out. Loading anything else marks the set "off plan" and re-ranks the suggestions from what is playing, without asking. The played order is kept as history; the saved set changes only when the DJ asks.

## Build order

| # | Pull request | Acceptance |
| --- | --- | --- |
| 1 | `dubplate-prep` crate on `euphoria-core`, one SQLite in the build | Builds and tests pass; one `libsqlite3-sys` in `cargo tree`; suggestions leave out clashes, far tempos and the set's own tracks |
| 2 | Set store: schema, migrations, saves, snapshots, backups, path guard | Survives close and reopen and a kill mid-edit; refuses a newer schema; refuses a path under rekordbox |
| 3 | Import and analysis queue (energy, key check, phrases) | One cached row per track; a second launch loads 352 tracks in under 2 s; set tracks analysed first; no audio dropouts while analysing |
| 4 | Ready check and proposed cues on phrase starts | Pure functions with fixture tests; proposals carry their source |
| 5 | Seam plans over Euphoria's transition planner | Every seam shows key move, BPM step and overlap; a 10 BPM gap warns; edits persist |
| 6 | Set panel in Prepare: list, reorder, add from the library, running time | A 20-track set builds in under 15 minutes |
| 7 | Energy curve, BPM line, key strip, set warnings | Duplicates and sudden energy drops flagged |
| 8 | Next suggestions panel | Ranks the collection in under 50 ms; set tracks left out |
| 9 | Track check: grid overlay, nudge, confirm, cue editing | Edits stay in Dubplate, not rekordbox; the badge updates |
| 10 | Load the set to the decks, next-track prefetch | Next track on the deck within 1 s; position survives a restart; rekordbox files byte-identical after a session |
| 11 | Seam audition in headphones | After the engine's headphone cue |
| 12 | Play history, backups, opener and closer | Off-plan plays never change the saved set |

## Risks

- Grids and phrases on amapiano (swing, long intros) will often be wrong: grid confirm and a fast nudge matter as much as the set panel.
- Euphoria's energy model was tuned on other libraries: the DJ's override is kept separately.
- Analysis time for the whole collection is unmeasured: set and shortlist tracks go first.
