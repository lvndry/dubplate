# Feature inventory and gap analysis

Every feature of rekordbox 7 (all plans, read against the 7.2.18 manual and release notes up to 7.2.20), what the CDJ-3000 does with rekordbox-prepared media, every control of the DDJ-FLX4, and the features of Traktor, Serato, VirtualDJ, djay and Engine DJ that DJs ask rekordbox for, each marked with Dubplate's status on 22 November 2026.

## How to read it

Each table has the same columns:

- **Feature** and **what it does**, with the values and behaviour that matter.
- **Where in rekordbox**: the screen, menu or Preferences page, or the app that has it when rekordbox does not.
- **Tier**, from rekordbox's plan page:
  - **Free**: on every plan, including Free.
  - **Free (FLX4)**: Performance mode features the DDJ-FLX4 unlocks on the Free plan.
  - **Core (FLX4 unlocks)**: Core features the FLX4 also unlocks on Free (recording, mix sharing, MIDI Learn, Merge FX, Sample Scratch).
  - **Core**, **Creative**, **Professional**, **Cloud Option**: the cheapest plan that has it.
  - **n/a**: not a rekordbox feature.
  - Text in brackets qualifies the tier: "registered" means Free after registering a Pioneer product, "service subscription" means the streaming service's own plan, and a device name means that device unlocks it.
- **Dubplate**, judged on `main` at the commit this file was written on:
  - **Done**: works as a DJ would expect.
  - **Partial**: exists, but a DJ would notice what is missing, which the cell names.
  - **In progress**: an open pull request delivers it.
  - **Missing**: not there.
- **Priority** for an afro house and amapiano DJ who prepares on a DDJ-FLX4 and plays clubs on CDJs:
  - **P0**: blocks a club night.
  - **P1**: professionals expect it.
  - **P2**: nice to have.
  - **P3**: later.

Some controls appear twice, once in their area and once in the FLX4 table, so the counts below count rows, not unique features. Details marked "unconfirmed" in the research (palette sizes, the full Beat FX list, fader curve names) are written as rekordbox's hardware manuals describe them.

## Counts

| Status | Rows | P0 | P1 | P2 | P3 |
| --- | --- | --- | --- | --- | --- |
| Done | 193 | 99 | 62 | 24 | 8 |
| Partial | 128 | 11 | 46 | 49 | 22 |
| In progress | 2 | 2 | 0 | 0 | 0 |
| Missing | 302 | 7 | 57 | 99 | 139 |
| n/a | 3 | 0 | 0 | 0 | 3 |
| Total | 628 | 119 | 165 | 172 | 172 |

The 17 features in the last section, which rekordbox does not have, are not counted.

## Top 40 gaps

Ranked by what stops the owner from playing a club night with Dubplate alone, then by what a professional notices first.

1. **USB export for CDJs.** The Device Library (export.pdb) and the ANLZ files are written for sets, playlists and music folders, from the Set panel, the library and the command line, with progress. Still to do: a first test on CDJs, OneLibrary (exportLibrary.db) for the players that read only it, a Devices pane with eject, adding to a stick rekordbox wrote, and artwork.
2. **Save the cues made in Dubplate (done for XML).** Hot cues A to H, memory cues and loops set live or in prep are kept per track in Dubplate's store, survive a relaunch, and go into the XML export; the USB export waits for item 1.
3. **Memory cues on the decks (done).** Draw them as red triangles on the lanes and the overview; CUE/LOOP CALL ◁ ▷ calls the previous or next one when no loop plays; SHIFT + ▷ saves one, SHIFT + ◁ deletes it; up to 10 per track.
4. **Grid editing on the decks for every track.** Set the downbeat at the playhead, shift 1 ms, type a BPM, x2 and /2, widen and narrow, edit from here, metronome; saved and exported like track check edits.
5. **Waveform and overview (done).** Cue point mark, loops and the slip shadow on the lanes, a 3-band overview with hot cues, memory cues, mix cues, loops and phrases, and the end-of-track warning.
6. **FLX4 Beat FX fixes (in progress).** CH SELECT 1&2 puts the effect on both decks, LEVEL/DEPTH follows the knob from its first report, FX SELECT shows its choice.
7. **Auto Gain (done).** Read the gain rekordbox stores for its tracks (djmdMixerParam), measure loudness for folder tracks, and set the trim on load, with a switch in Settings.
8. **Auto cue on load.** Every track loads cued at its first beat (or first audio above -36 to -78 dB, or the nearest memory or hot cue), never at 0:00 in silence.
9. **History and tracklists (in progress).** Record each play of a minute or more with start time, duration and deck in the play history table that already exists, show a history per session, and export it as TXT, CSV or M3U with timestamps, also beside each recording.
10. **rekordbox's hot cue colours, names and loops (done).** Use the stored colour (16-colour palette) and comment on the pads, lanes and overview, and start the loop when a hot loop is called.
11. **Browser preview.** P or a click on a preview waveform plays a track in the headphones from that point without loading a deck, with its own volume.
12. **Waveform zoom.** SHIFT + BROWSE turn and on-screen buttons zoom the lanes from about 4 to 32 seconds across, with a reset.
13. **Phase meter and beat counter.** A bar per deck showing its beat offset from the master, the bar.beat position, and bars to the next memory cue or mix-out.
14. **Release FX and the echo out.** SHIFT + BEAT FX ON plays Echo, Back Spin or Vinyl Brake while cutting the Beat FX; Smart Fader echoes the outgoing deck at the edge; a fader echo option for the afro house echo-out.
15. **Beat FX parity.** Add Delay, Low Cut Echo, Spiral, Pitch, Helix, Roll and Slip Roll, a cyclic Filter, Mobius, Multi Tap Delay and Stretch, each with its own parameter as well as LEVEL/DEPTH.
16. **Sound Color FX on the CFX knob.** Choose Filter, Dub Echo, Space, Sweep, Noise, Crush or Pitch for the CFX knobs, for both channels or each one.
17. **Quantize and vinyl mode switches.** Q and VINYL per deck on screen and on the keyboard; quantize stays on by default.
18. **Loop adjust and full loop range.** With a loop on, IN or OUT then the jog moves that point; beat loops from 1/64 to 512 beats.
19. **Saved and active loops.** Call and save loops as memory or hot cues; an active loop engages when reached; SHIFT + 4 BEAT/EXIT toggles it.
20. **Stems on the decks.** Wire the separation queue into the app and add ACTIVE PART mute and solo for vocals, melody, bass and drums per deck, with stem waveforms and FLX4 pads.
21. **Library columns and metadata.** A column chooser and the album, label, remixer, year, date added, rating, colour, comment, My Tag, bit rate and play count columns, reading rating, colour and My Tag from master.db.
22. **Track filter by rating, colour and My Tag.** AND/OR across My Tags, next to the BPM and key filters, plus hide played.
23. **Playlists in Dubplate.** Create, rename and delete playlists and folders, add, reorder and remove tracks, export M3U8 or TXT, and include them in the XML and USB exports.
24. **Key sync and key on the deck.** KEY SYNC, semitone up and down with the key difference, the deck key coloured by its fit to the master, and a Classic or Camelot setting.
25. **Played tracks (done).** Colour a row once its track has played for 60 seconds, and offer a hide played filter.
26. **BPM range for analysis.** A range setting (58-115, 78-155, 88-175 and so on) with a default that keeps 108 to 124 BPM where it is, plus x2 and /2 per track.
27. **Crossfader curve and assign.** Smooth, constant and sharp cut curves; A, THRU or B per channel.
28. **Audio settings.** Buffer size with the latency shown, the dropout count on screen, output mapping and a booth output.
29. **Edit track info.** Title, artist, genre, label, comment, rating and colour edited in Dubplate's store, rekordbox untouched, carried into the exports.
30. **My Settings and device settings on the stick.** Write player settings (auto cue level, quantize and beat jump values, hot cue colour on, load lock) and the category, sort, column and waveform colour choices with the USB export.
31. **Suggestions in Perform.** Next follows the playing deck while mixing, and pairs can be saved as Matching.
32. **Phrases for every track.** Phrase analysis for folder tracks and a phrase strip on the lanes as well as the overview.
33. **Smart Fader options.** Crossfader, channel faders or both, and the echo on the outgoing deck at the edge of the throw.
34. **The next set track on deck B.** Deck B shows the next set track with the seam plan and a bar countdown to the mix-out, as the set preparation spec describes.
35. **Deck preferences.** Auto cue, needle lock, sync type, Smart Cue, Gate Cue, auto beat loop length and memory cue call lock in Settings.
36. **Read back from CDJ sticks.** After a gig, import histories and the cues, grids, ratings and comments edited on the players.
37. **Track marks in recordings.** A cue sheet beside each WAV with track starts, and an optional split per track.
38. **Fader start and reverse.** SHIFT + channel fader or crossfader starts and stops a deck; reverse and slip reverse.
39. **Editable Pad FX.** Choose the effect and beat of each of the 32 pad effects, and a Release FX pad.
40. **Sampler banks and samples.** Four banks of 16 slots, sample packs, the OSC sampler (sirens, horns) and loop capture into a slot.

## 1. Library: collection, browsing and screens

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Collection | Every track rekordbox manages; removing a track never deletes the file. | Media Browser > Collection | Free | Done (rekordbox collection plus the DJ's music folders) | P0 |
| Read the rekordbox library | Uses the collection, playlists, cues, grids and waveforms rekordbox already made. | n/a (native) | Free | Done (read-only copy of master.db and ANLZ files) | P0 |
| Write to the rekordbox library | Edits made in the app land in rekordbox's own library. | n/a (native) | Free | Partial: only through a set exported as rekordbox XML and imported by hand | P1 |
| Import tracks and folders | [File] > [Import Track/Folder] or drag from Finder; tags read, analysis starts. | File menu, drag and drop | Free | Done (music folders, rescanned from Settings) | P0 |
| Drag a track or file onto a deck | Drop a track from the browser, or a file from Finder, onto a deck. | Deck | Free | Done | P1 |
| Readable formats | MP3, AAC/M4A, ALAC, WAV and AIFF (16/24-bit up to 192 kHz), FLAC up to 192 kHz. | n/a | Free | Done (MP3, M4A AAC/ALAC, FLAC, WAV, AIFF, Ogg) | P0 |
| Readable tags | ID3 v1 to v2.4, M4A atoms, WAV RIFF INFO, FLAC Vorbis comments. | On import | Free | Partial: title, artist, genre, comment, BPM and key; album, label, year, artwork not shown | P1 |
| Explorer | Browse disk folders inside the app; tracks already in the collection carry an icon. | Media Browser > Explorer | Free | Partial: music folders appear as crates; no free browsing of the disk | P2 |
| Sub browser and split screen | A second tree and list side by side for drag and drop between lists. | [View] > Sub Browser / Split Screen | Free | Partial: Prepare shows library and set side by side; no second library list | P2 |
| Playlist palette | Up to 4 playlists side by side next to the list, plus Tag List and Request Catalog. | Browser palette | Free | Missing | P2 |
| Shortcuts pane | Up to 8 pinned folders or playlists; drop a track on one to add it. | Top of Media Browser | Free | Missing | P3 |
| Browse layout | Minimised decks and an enlarged browser; Space toggles in rekordbox. | Layout selector | Core | Partial: Cmd+, hides nothing; Prepare is a library-first screen | P2 |
| Info window | Side pane with the selected track's tags and artwork, with a reload-tags button. | [View] > Information Window | Free | Missing | P2 |
| Font size and line spacing | Track list font size, bold, line spacing, keep the selection centred. | Preferences > View > Display Type > Browse | Free | Missing | P2 |
| Dark and light skin | Dark or Light (outdoor) skin. | Preferences > View > Color > Skin | Free | Partial: dark only | P3 |
| Played track colouring | Rows change colour after a track has played 1 to 60 s; reset on quit or with RESET. | Preferences > View > Display Type | Free | Partial: a dimmed title and a tick after 60 s of track time, for the session; the time is not adjustable and there is no RESET | P1 |
| Missing file mark | [!] in the status column for a file that cannot be found. | Status column | Free | Done (missing tracks marked, never loaded) | P0 |
| Outdated analysis mark | [?] when analysis is from an old version; Add New Analysis Data fixes it. | Status column | Free | Partial: a blank BPM/Key/Energy cell means still analysing | P3 |
| Library protection | Locks track info and lists against accidental edits. | Preferences > Advanced > Browse | Free | Done in effect: rekordbox data is read-only | P3 |
| Edit in place | Double-click a cell or a playlist name to edit it. | Preferences > Advanced > Browse > Edit Library | Free | Missing | P2 |
| Remove from collection | Delete key removes a track from the collection, file kept. | Context menu | Free | Missing | P2 |
| Show in Finder | Reveals the file. | [Track] > Show in Finder | Free | Missing | P2 |
| Library backup and restore | Zips the database and analysis (optionally the music) and restores it. | [File] > [Library] | Free | Partial: Dubplate's set store takes its own snapshots; no user backup of its settings and relinks | P2 |
| Database location | Moves the library database, for example onto the drive with the music. | Preferences > Advanced > Database | Free | Missing | P3 |
| Multiple libraries | Separate collections (club, wedding); asked for on Pioneer forums, not in rekordbox. | Not in rekordbox | n/a | Missing | P3 |
| NAS or network library | Music and library on a network share; not supported by rekordbox. | Not in rekordbox | n/a | Partial: any mounted folder can be added; no special handling | P3 |
| Mobile import location | Folder for files transferred from rekordbox for iOS/Android. | Preferences > Cloud | Free | Missing | P3 |

## 2. Library: columns, sorting, search and filters

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Column chooser | Right-click the header to show columns; drag headers to reorder. | Track list header | Free | Missing: fixed columns | P1 |
| Title, Artist, Genre, BPM, Key, Time columns | The core columns. | Track list | Free | Done | P0 |
| Album, Album artist, Original artist, Remixer, Composer, Label columns | Release metadata columns. | Track list | Free | Missing | P1 |
| Comments column | Free text, often holds Mixed In Key or My Tag text. | Track list | Free | Missing as a column (comments are searched) | P1 |
| Rating column | 0 to 5 stars. | Track list | Free | Missing | P1 |
| Colour column | One of 8 track colours. | Track list | Free | Missing | P1 |
| My Tag column | The track's My Tags. | Track list | Free | Missing | P1 |
| Date Added column | Import date, the usual "new music" sort. | Track list | Free | Missing | P1 |
| Release year and date columns | Year and release date. | Track list | Free | Missing | P2 |
| Bit rate, sample rate, bit depth, file type, size columns | File quality at a glance. | Track list | Free | Missing | P2 |
| DJ play count column | Times played. | Track list | Free | Missing | P2 |
| Artwork column | Cover thumbnail; hover previews, each click skips 30 s. | Track list | Free | Missing | P2 |
| Preview waveform column | A small waveform per row; clicking plays from that point in the headphones. | Track list | Free | Missing | P1 |
| Cue markers on preview | Hot cue and memory cue marks on the preview waveform. | Preferences > View > Layout | Free | Missing | P2 |
| Energy column | Not in rekordbox. | Not in rekordbox | n/a | Done (1 to 10) | P1 |
| Source column | Not in rekordbox (rekordbox or folder). | Not in rekordbox | n/a | Done | P3 |
| Sort by column | Click to sort, click again to reverse. | Header | Free | Done (third click restores the crate order; remembered per screen) | P0 |
| Track number order | The playlist's own order, renumber to the current sort. | [Playlist] > Renumber Track Order | Free | Partial: crate order restorable; no renumber | P3 |
| Search | Incremental search over chosen columns. | Search box | Free | Done (title, artist, genre, comment, folder names) | P0 |
| Search field choice | Pick which columns the search looks in. | Search drop-down | Free | Missing | P3 |
| Alphabet search | Type letters in the list to jump to titles starting with them. | Track list | Free | Missing | P2 |
| Collection filter | Tree categories Date Added, Genre, Artist, Album under the collection. | Tree under Collection | Free (registered) | Missing | P2 |
| Category browser | Genre, Artist and Album panes above the list. | Funnel icon | Free | Missing | P2 |
| Track filter: BPM | Keeps tracks within a BPM range of a value or the master deck. | Track filter | Free | Done (a few BPM either side of the set track or playing deck, ends adjustable) | P1 |
| Track filter: key | Keeps tracks whose key relates to a value or the master deck. | Track filter | Free | Done ("Key fits": same, one step, relative) | P1 |
| Track filter: rating, colour, My Tag | Filter by stars, colour and My Tags (AND/OR inside My Tag). | Track filter | Free | Missing | P1 |
| Track filter from the master deck | [MASTER DECK] takes BPM and key from the playing master. | Track filter | Free | Done (Perform uses the playing deck) | P1 |
| Traffic Light | Colours compatible keys in the Key column against the selected deck. | Preferences > View > Display Type | Free | Partial: key and BPM cells tinted in Prepare against the set; not against a deck in Perform | P1 |
| Playlist search | Find a playlist or folder by name. | Above the tree | Free (registered) | Missing | P2 |
| Track counts in the tree | Number of tracks shown on each playlist. | Preferences > View > Layout | Free | Missing | P3 |
| Folder shows all its tracks | Selecting a folder lists the tracks of every playlist inside. | Playlists tree | Free | Partial: rekordbox folders appear; whether selecting one lists all child tracks is not confirmed | P2 |
| Column view of playlists | Columns of nested playlists, multi-select to combine. | Playlists tree | Free (registered) | Missing | P3 |
| Show which playlists a track is in | Lists every playlist containing the track (Serato 4 "Show Track in Crates"). | Not in rekordbox | n/a | Missing | P2 |

## 3. Library: playlists, intelligent playlists, histories and tag lists

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Show rekordbox playlists and folders | The playlist tree as in rekordbox. | Tree view | Free | Done | P0 |
| Create, rename and delete playlists | Make crates and folders inside the app. | Tree view [+] | Free | Partial: sets can be created and renamed; no general playlists or folders | P1 |
| Add tracks to a playlist | Drag tracks onto a playlist. | Tree view | Free | Partial: tracks go into the set only | P1 |
| Reorder tracks in a playlist | Drag rows when sorted by track number. | Track list | Free | Partial: set tracks move with Alt+Up/Down | P1 |
| Playlist artwork | JPEG or PNG on a playlist. | Context menu | Free | Missing | P3 |
| Sort playlists in a folder | Sort Items. | [Playlist] menu | Free | Missing | P3 |
| Import M3U, M3U8, PLS | Turns a playlist file into a playlist. | [File] > Import Playlist | Free | Missing | P2 |
| Export playlist as TXT or M3U8 | For other apps and tracklists. | Context menu | Free | Missing | P1 |
| Intelligent playlists | Rule-based lists that update themselves. | Context menu > Create New Intelligent Playlist | Free | Partial: rekordbox's smart playlists are listed but their rules do not run | P2 |
| Intelligent playlist operators | =, ≠, >, <, contains, does not contain, starts with, ends with, in range, in the last / not in the last days or months. | Condition editor | Free | Missing | P2 |
| Intelligent playlist fields | Artist, album, genre, BPM, key, rating, colour, My Tag, date added, play count, time and more. | Condition editor | Free | Missing | P2 |
| Match all or any | One AND or one OR across all rules; no nesting. | Condition editor | Free | Missing | P2 |
| Nested AND/OR and "in playlist" rules | Grouped logic such as (Afro House or Amapiano) and BPM 112 to 124; asked for since 2015. | Not in rekordbox | n/a | Missing | P2 |
| Collaborative playlist | Share a playlist with up to 5 people, edits sync. | Playlists > Collaborative Playlist | Professional | Missing | P3 |
| Histories | Each session's played tracks (1 minute or more) as HISTORY yyyy-mm-dd in year and month folders. | Media Browser > Histories | Free | Partial: History in the browser lists this session's plays (a minute of track time, once per load) and earlier sessions by day; rekordbox's own histories (djmdHistory) are not read | P1 |
| History to playlist | Copy a history into a playlist. | Context menu | Free | Missing | P2 |
| History with times | Start time and duration for each play, for tracklists (Traktor and Serato have it; rekordbox does not). | Not in rekordbox | n/a | Done (start time, track time played and deck per play) | P1 |
| Tracklist export | Text or CSV tracklist of a session with timestamps for Mixcloud and YouTube; one of the most requested gaps. | Not in rekordbox | n/a | Done (TXT and CSV from any page of the history) | P1 |
| Histories from CDJs | Imports the play history from a USB used on CDJs, named by date; option to delete it from the stick. | Preferences > DJ System > Device > History | Free | Missing | P2 |
| Tag List | A 100-track scratch list, also browsable from linked CDJs; auto-remove when loaded. | Palette [TAG] | Free | Partial: B keeps a track as a set backup | P2 |
| Request Catalog | Up to 200 tracks the audience can request through the CoBeat web app. | Palette [REQ] | Cloud | Missing | P3 |
| Preparation list "Add as Next" | Queue a track after the one playing (Traktor). | Not in rekordbox | n/a | Done in spirit: the set and Cmd+Enter insert after the selection | P2 |
| Set planning | Not in rekordbox. | Not in rekordbox | n/a | Done (see the last section) | P0 |

## 4. Library: track metadata, My Tag, ratings and colours

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Edit track info | Edit title, artist, album, genre, label, comments, year and other fields. | Info window | Free | Missing | P1 |
| Reload tags | Re-reads the file's tags. | Context menu | Free | Partial: a folder rescan rereads tags | P3 |
| Write tags to files | rekordbox writes only the key to ID3 (option); other edits stay in its database. | Preferences > Analysis > Key Analysis | Free | Missing | P3 |
| Artwork | Shown in the deck, the list and the CDJ jog; add JPEG or PNG by drag. | Info window > Artwork | Free | Missing | P2 |
| Rating | 0 to 5 stars, editable in the list and on the CDJ-3000X. | Rating column | Free | Missing (read from rekordbox, not shown) | P1 |
| Track colour | One of 8 colours (Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple) with a comment per colour shown on CDJs. | Colour column, Preferences > Advanced > Browse > Color | Free | Missing | P1 |
| My Tag | Categories of user tags (for example Genre, Components, Situation), several tags per track, editable in a window. | My Tag window | Free | Missing (not read from rekordbox) | P1 |
| My Tag into comments | Appends My Tag names to the comment. | Preferences > Advanced > Browse | Free | Missing | P3 |
| Comments | Free-text comment per track. | Info window | Free | Partial: searched, never shown or edited | P1 |
| Multi-value genre | Split "Afro House; Amapiano" into separate genres; asked for, not in rekordbox. | Not in rekordbox | n/a | Missing | P3 |
| Energy rating | Not in rekordbox. | Not in rekordbox | n/a | Done (1 to 10, cached) | P1 |
| Vocal tags | Hand-tagged vocals at the start and end of a track for clash warnings. | Not in rekordbox | n/a | Partial: the prep crate warns on vocals but the app never sets the tags | P2 |
| Online metadata | Fetch genre and label from Beatport; asked for, contested. | Not in rekordbox | n/a | Missing | P3 |
| Duplicate finder | Lists tracks with the same title and artist; delete or merge into one. | [File] > Display All Duplicated Files | Free | Missing (the set warns when a track appears twice) | P2 |
| Merge duplicates keeping cues | Merge the MP3 promo and the WAV purchase, keeping cues and grids. | [File] > Merge Duplicate Files | Free | Missing | P2 |

## 5. Library: related tracks and suggestions

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Related Tracks | Tracks related to the loaded or selected track by named rules (BPM, key, genre, rating, My Tag, Matching). | Media Browser > Related Tracks | Free | Partial: Next suggests five tracks after a set track in Prepare; no rules and nothing in Perform | P1 |
| Related Tracks from a deck | Follows the master deck or a chosen deck while playing. | Related Tracks [MASTER], [1] to [4] | Free | Missing in Perform (Key fits and BPM filters do follow the playing deck) | P1 |
| Matching | Saves a pair of tracks as a good match; the button lights when the pair is loaded again; CDJ MATCHING category. | 2 PLAYER waveform [Matching] | Free | Missing | P2 |
| Track Suggestion | Collection Radar, Streaming Radar, Era, Mood, Association recommenders. | Media Browser > Track Suggestion | Free | Partial: Next uses key, tempo and energy, with reasons | P2 |
| Suggestions with reasons | Not in rekordbox ("One step on the wheel · +1.6% · energy +2"). | Not in rekordbox | n/a | Done | P1 |
| Live compatibility filter | VirtualDJ filter folders recompute BPM and key difference against the playing deck. | Not in rekordbox | n/a | Done (BPM range and Key fits follow the playing deck) | P1 |

## 6. Analysis

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Auto analysis on import | Analyses every imported track in the background. | Preferences > Analysis > Track Analysis | Free | Done (set tracks and loaded tracks first, cached) | P0 |
| Analyse selected tracks again | Re-analyse chosen tracks, skipping locked ones. | Context menu > Analyze Track | Free | Partial: a folder rescan only; no per-track re-analysis | P2 |
| Analysis items | Choose BPM/grid, key, phrase and vocal analysis. | Preferences > Analysis | Free | Partial: BPM, grid, key and energy always; no phrase or vocal analysis | P1 |
| Normal or dynamic tempo mode | Normal (one tempo, uses BPM range), Dynamic (follows tempo changes), Auto. | Preferences > Analysis > Track Analysis Mode | Free | Partial: rekordbox's dynamic grids play correctly; Dubplate's own analysis assumes one tempo | P2 |
| BPM range | Analysis range, for example 58-115, 68-135, 78-155, 88-175, 98-195, so 112 BPM amapiano is not read as 56 or 224. | Preferences > Analysis > BPM Range | Free | Missing | P1 |
| High precision beat grid | Slower, more accurate grid analysis (7.0.7). | Preferences > Analysis | Free | Missing | P2 |
| Analysis speed setting | Performance (fast) or Power saving. | Preferences > Analysis > Analysis Process | Free | Done in effect (one low-priority worker) | P3 |
| Cloud analysis | Downloads BPM, grid, waveform and vocal data from AlphaTheta's servers. | Preferences > Analysis | Core | Missing | P3 |
| Key analysis | Detects the key. | Track analysis | Free | Done (rekordbox's key first, Dubplate's for folder tracks) | P0 |
| Write key to the file | Stores the detected key in the ID3 tag. | Preferences > Analysis > Key Analysis | Free | Missing | P3 |
| Phrase analysis | Intro, Up, Down, Chorus, Bridge, Verse, Outro with Fill-in marks and a High/Mid/Low mood. | Track analysis | Free | Partial: rekordbox's phrases are read and shown; folder tracks get none | P1 |
| Vocal detection | AI vocal positions drawn on the overview and scrolling waveform. | Track analysis, Preferences > View > Layout > Vocal | Free | Missing | P2 |
| Auto gain analysis | Loudness per track for Auto Gain. | Track analysis | Free | Done (BS.1770 integrated loudness in the folder-track analysis, cached) | P1 |
| Energy analysis | Not in rekordbox. | Not in rekordbox | n/a | Done | P1 |
| Analysis lock | Blocks re-analysis and grid edits; lock icon. | Context menu > Analysis Lock | Free | Partial: Dubplate never overwrites rekordbox's analysis; confirmed grids are kept | P3 |
| Radar analysis | Uploads data for Track Suggestion's radar. | Preferences > Analysis | Free | Missing | P3 |

## 7. Key display

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Classic key names | Am, F#m, Db. | Preferences > View > Key display format | Free | Missing: Camelot only | P2 |
| Alphanumeric (Camelot) | 1A to 12B. | Preferences > View > Key display format | Free | Done | P0 |
| Open Key | 1d to 12m (Traktor). | Not in rekordbox | n/a | Missing | P3 |
| Key sorted on the wheel | Sorting keys by wheel position. | Key column | Free | Done (1A, 1B, 2A on to 12B) | P2 |
| Key colouring | Keys coloured by wheel position or by fit to the master deck. | Traffic Light; CDJ key turns green when related | Free | Partial: Prepare tints fit; decks do not | P1 |

## 8. Beat grid and phrase editing

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Grid display | White beat lines, red line on each bar's first beat. | Waveform | Free | Done (downbeat drawn so it reads) | P0 |
| Set downbeat (1.1 bars) | Makes the nearest beat bar 1 beat 1. | GRID panel | Free | Partial: move the downbeat a beat at a time in the track check, set tracks only | P0 |
| Shift grid | Moves the whole grid 1 ms (3 ms in Performance) per click. | GRID panel | Free | Partial: 5 ms steps, in the track check only | P0 |
| Type a BPM | Respaces the grid to a typed BPM. | GRID panel BPM field | Free | Missing | P1 |
| Tap the grid | Respaces the grid from taps. | GRID panel TAP | Free | Partial: tap tempo on a deck without a grid only | P2 |
| Widen or narrow spacing | Changes beat spacing 1 ms per click. | GRID panel | Free | Missing | P1 |
| Double or halve BPM | x2 and /2 for the grid. | GRID panel | Free | Missing | P1 |
| Edit from here | Re-grids from the playhead, leaving earlier beats (partial grid). | GRID panel | Free | Missing | P2 |
| Grid undo and redo | Undo grid edits. | GRID panel | Free | Partial: set edits undo with Cmd+Z; grid nudges unconfirmed | P2 |
| Metronome | Click track on the grid, 3 volumes. | GRID panel | Free | Missing | P1 |
| Grid edit on the deck | Edit the grid of any loaded track in Performance mode. | Deck GRID/PHRASE EDIT panel | Free | Missing (track check only; a deck grid editor exists on an unmerged branch) | P0 |
| Grid confirm | Not in rekordbox. | Not in rekordbox | n/a | Done (G) | P1 |
| Grid edits follow the track | Edits apply on decks and in the export. | n/a | Free | Done (onto the decks and into the XML export) | P0 |
| Grid edit on CDJ | CDJ grid adjust (snap, shift, half beat, reset; 3000X adds x2, /2, 1 ms); comes back with Update Collection. | CDJ | Free | Missing | P3 |
| Flexible grid markers | Several tempo zones per track (Traktor 4, djay Fluid Beatgrid). | Not in rekordbox (dynamic analysis only) | n/a | Partial: rekordbox's tempo-changing grids play right; no manual markers | P3 |
| Phrase display | Phrase strip under the waveform, coloured by type. | Preferences > View > Layout > Phrases | Free | Partial: track check shows phrase names, the deck overview shows the phrase strip; folder tracks have no phrases | P1 |
| Phrase edit | CUT to split, drag to move or merge, change type, CLEAR. | GRID/PHRASE EDIT | Free | Missing | P2 |
| Fill-in marks | Up to 4 beats marked at the end of Intro, Up and Chorus. | Waveform | Free | Missing | P3 |

## 9. Cue preparation and automatic cues

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Set and edit cues in prep | Hot cues A to H (16 in Performance), memory cues and loops set while preparing. | 1 PLAYER CUE/LOOP panel | Free | Partial: mix-in and mix-out on set tracks; hot cues set live on the decks and saved per track | P0 |
| Save hot cues and memory cues | Cues are stored with the track and exported to USB. | Automatic | Free | Partial: cues made in Dubplate are kept per track in Dubplate's database, in place of rekordbox's, and go into the XML export; no USB export yet | P0 |
| Memory cue colours and comments | Colour per memory cue, comment shown on CDJs. | MEMORY panel | Free | Missing | P1 |
| Hot cue colours and comments | Colour per hot cue (16 colours), comment shown on CDJ-3000. | HOT CUE panel | Free | Partial: rekordbox's colours and names shown on the pads, lane flags and overview, kept and exported; not editable yet | P1 |
| Hot cue colour scheme | CDJ (green cues, amber loops), COLD1, COLD2, COLORFUL. | Preferences > View > Color | Free | Missing | P3 |
| Intelligent Cue Creation | Sets hot and memory cues during analysis from phrases and the DJ's own cue habits learned from a 30-track playlist; Manual mode repeats every N bars. | Preferences > Analysis > CUE Analysis | Core | Partial: proposes mix-in and mix-out from phrases or the grid (Y accepts) | P1 |
| Cue at the first downbeat | Sets the cue on 1.1 bars at analysis. | CUE Analysis > Set to 1.1 Bars | Core | Missing | P1 |
| Convert memory cues to hot cues | Fills empty hot cues in order. | Context menu | Free | Missing | P3 |
| Sort hot cues by time | Reorders A to P by time (7.2.16). | Context menu | Free | Missing | P3 |
| Hot cue bank lists | 8 banks of cues from any tracks, for CDJs. | Media Browser > Hot Cue Bank Lists | Free | Missing | P3 |
| Hot Cue Auto Load flag | Per-track flag so CDJs load its hot cues. | Info window | Free | Missing | P2 |
| Playlist-specific cues | Different cues per playlist; asked for, not in rekordbox. | Not in rekordbox | n/a | Partial: mix cues live per set entry, not per track | P3 |
| Export a loop as WAV | Saves the playing loop as a 48 kHz 16-bit WAV in the collection. | 1 PLAYER menu | Free | Missing | P3 |
| Seam plan per transition | Not in rekordbox: overlap, tempo, EQ intent, note, Planned or Free. | Not in rekordbox | n/a | Partial: key move, tempo step, warnings and Planned/Free; overlap bars, EQ intent and note not editable | P2 |

## 10. Files, relocation, import and export

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Missing file manager | Lists unplayable tracks with Relocate, Auto Relocate and Delete. | [File] > Display All Missing Files | Free | Done (count above the list, Relink review) | P0 |
| Relocate | Point a missing track at a new file. | Context menu | Free | Done (Relink, point at a folder) | P0 |
| Auto relocate | Searches chosen folders for moved files. | Preferences > Advanced > Database | Free | Done (same name and size relinked at once; length matches reviewed; ties never guessed) | P0 |
| rekordbox XML import | Browse an XML library and import tracks and playlists. | Media Browser > rekordbox xml | Free | Missing | P2 |
| rekordbox XML export | Whole collection with grids and cues to XML. | [File] > Export Collection in xml format | Free | Partial: a set (and its backups) only, not the collection | P1 |
| iTunes / Music app library | Browse the Music app library and its playlists. | Media Browser > iTunes | Free | Missing | P2 |
| Import from Serato, Traktor, VirtualDJ | Bring cues, loops and grids from other apps; not in rekordbox, shipped by djay 5.4 and Engine DJ 5.0. | Not in rekordbox | n/a | Missing | P2 |
| rekordbox v5 library conversion | Converts old libraries. | [File] > [Library] | Free | n/a: reads rekordbox 6 and 7 libraries | P3 |
| Inflyte promos | Promo locker inside the browser. | Media Browser > Inflyte | Free | Missing | P3 |
| Video files | mp4, mov and others in the collection. | Collection (video on) | Creative | Missing | P3 |

## 11. Cloud, mobile and streaming

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Cloud Library Sync | Syncs library and files across computers and phones through Dropbox or Google Drive; Pro 8 devices, Creative 4, Core 3. | Preferences > Cloud > Library Sync | Cloud Option (Creative and Professional include it) | Missing | P3 |
| Trial playlist | Free and Core sync up to 20 tracks. | Tree view | Free | Missing | P3 |
| Collection and playlist auto upload | Uploads new tracks or chosen playlists automatically. | Preferences > Cloud | Cloud Option | Missing | P3 |
| Cloud Export and CloudDirectPlay | Upload playlists so CDJ-3000/3000X play them from the cloud after a USB authentication. | Media Browser > Cloud Export; Preferences > Cloud | Cloud Option (capped at 1000 tracks on Free and Core) | Missing | P3 |
| Device library backup | Backs up a USB library to Dropbox or Google Drive to restore a lost stick. | Preferences > Cloud > Device Library | Professional | Missing | P2 |
| Dropbox storage | 5 TB with Professional, 1 TB with the Cloud Option. | Plan | Professional | Missing | P3 |
| Mobile Library Sync | Wi-Fi transfer of tracks, playlists and My Settings to rekordbox for iOS and Android. | Devices > Mobile Library Sync | Free | Missing | P3 |
| iCloud Drive library | Music folder kept in iCloud Drive. | Not in rekordbox | n/a | Partial: any local folder, including a downloaded iCloud folder, can be added | P3 |
| Beatport Streaming | Search, genres, playlists, offline locker, FLAC option. | Media Browser > Beatport | Free (service subscription) | Missing | P3 |
| SoundCloud | Search, likes, playlists; AAC 160 or 256 kbps; offline with SoundCloud DJ. | Media Browser > SoundCloud | Free (service subscription) | Missing | P3 |
| TIDAL | Search, moods, collection, lossless, offline with DJ Extension. | Media Browser > TIDAL | Free (service subscription) | Missing | P3 |
| Spotify | Library and Made to DJ tabs, Automix, no offline (7.2.3, on CDJs since 7.2.18). | Media Browser > Spotify | Free (service subscription) | Missing | P3 |
| Apple Music | Library, charts, genres; up to 3 tracks on 3 decks. | Media Browser > Apple Music | Free (service subscription) | Missing | P3 |
| Beatsource | Merged into Beatport and removed in 7.2.16. | Removed | n/a | n/a | P3 |

## 12. USB export and CDJ media

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Devices pane | Shows USB sticks and SD cards with their library, settings and eject. | Media Browser > Devices | Free | Missing | P0 |
| Export tracks and playlists to USB | Drag to the stick; tracks, analysis, cues and grids copied. | Devices | Free | Partial: sets, playlists and music folders export from the Set panel, the library and `dubplate usb export`; no Devices pane or drag | P0 |
| Device Library (export.pdb) | The library CDJ-3000, NXS2 and older players read. | USB | Free | Done (written and read; not yet tried on a player) | P0 |
| OneLibrary (Device Library Plus, exportLibrary.db) | The library CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO and XDJ-AZ read; written alongside the old one. | USB | Free | Missing | P0 |
| ANLZ analysis files on USB | Waveforms (Blue, RGB, 3-band), grid, cues and phrases per track (.DAT, .EXT, .2EX). | USB | Free | Done (rekordbox's waveforms and phrases copied, or waveforms drawn from the audio) | P0 |
| Keep the folder tree on export | Exports nested playlists into the same folders; asked for, not in rekordbox. | Not in rekordbox | n/a | Missing | P2 |
| Delete tracks with a playlist | Deleting a playlist from the stick removes its unused tracks. | Preferences > DJ System > Device | Free | Missing | P2 |
| Device settings: General | Device name, background colour, waveform colour, overview type. | Device settings | Free | Missing | P2 |
| Device settings: Category, Sort, Column | Which browse categories, sort keys and info column the player shows. | Device settings, Preferences > DJ System | Free | Missing | P2 |
| Colour comments on the stick | Text shown for each of the 8 colours. | Device settings | Free | Missing | P3 |
| My Settings on the stick | Player settings loaded when the stick goes in: load lock, quantize beat value, beat jump value, hot cue auto load, hot cue colour, auto cue level, vinyl speed, play mode, brightness, jog indicator, on air display, time mode, jog mode, tempo range, master tempo, sync, phase meter. | Preferences > DJ System > My Settings | Free | Missing | P1 |
| Update Collection from a stick | Pulls cues, loops, hot cues, grids, colours, ratings and comments edited on players back into the library. | Devices > Update Collection | Free | Missing | P2 |
| Import playlists made on players | Imports playlists and Tag Lists saved on CDJs. | Devices > Import Playlist | Free | Missing | P3 |
| Sync Manager | Keeps chosen playlists mirrored on a stick. | SYNC MANAGER | Free | Missing | P2 |
| File systems | FAT16, FAT32, exFAT, HFS+ (not NTFS). | n/a | Free | Missing | P1 |
| Convert to OneLibrary | Rebuilds OneLibrary from the old Device Library. | Devices > OneLibrary | Free | Missing | P2 |
| PRO DJ LINK export | Loads tracks from the laptop straight to linked CDJs over LAN, Wi-Fi or USB. | LINK button | Core (or hardware unlock) | Missing | P3 |
| Tag List from CDJs | Linked players browse and add to the rekordbox Tag List. | Palette [TAG] | Core | Missing | P3 |
| LINK HISTORY | Plays over LINK create a dated history. | Histories | Core | Missing | P3 |
| LINK MONITOR | Hear rekordbox previews in the DJM headphones. | Preferences > DJ System | Core | Missing | P3 |
| Export a set to rekordbox | Not in rekordbox: a set becomes a rekordbox playlist with grids and mix cues. | n/a | n/a | Done (XML; the DJ imports it in rekordbox, then exports to USB) | P0 |

## 13. CDJ-3000 behaviour that rekordbox prepares

These are features of the players. Dubplate prepares for them once it writes USB libraries; until then each row is Missing.

| Feature | What it does | Where | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Track filter on CDJs | BPM (±0 to 6% range, RESET = 130 BPM), related key, rating, colour and My Tag (AND/OR) from the master player; saved per stick. | CDJ TRACK FILTER | Free | Missing | P1 |
| My Tag and rating on CDJs | Browse and filter by My Tag; rate on the CDJ-3000X. | CDJ browse, INFO | Free | Missing | P1 |
| Hot cues A to H with colours and names | 8 hot cues; HOT CUE COLOR on shows rekordbox's colour per cue, loops orange; comments shown. | CDJ pads | Free | Done on screen (own colour, else the COLORFUL default for the pad, hot loops orange); the FLX4's pads only light on or off | P0 |
| Hot Cue Auto Load | Off, rekordbox SETTING (flagged tracks) or On. | CDJ UTILITY | Free | Missing | P2 |
| Memory cues and active loops | CUE/LOOP CALL steps through memory cues; active loops engage when reached. | CDJ | Free | Partial: memory cues and loops are called, saved and deleted; a called loop is not armed yet | P0 |
| Waveform colour on CDJ | Blue, RGB or 3-band, overview and zoomed; phrase strip on 3000X. | CDJ SHORTCUT > Device settings | Free | Missing | P1 |
| Beat countdown | Bars and beats to the next memory cue. | CDJ waveform screen | Free | Missing | P1 |
| Key display on CDJ | Classic or alphanumeric as set in rekordbox; green when related to the master's key. | CDJ, Preferences > DJ System > General | Free | Missing | P2 |
| Key sync and key shift | Shifts to the nearest related key (same, dominant, subdominant, relatives); semitone shift. | CDJ KEY SYNC, KEY SHIFT | Free | Done | P2 |
| Beat sync and instant doubles | Needs rekordbox grids; hold BEAT SYNC for instant doubles. | CDJ | Free | Missing | P1 |
| Quantize and beat jump values | Quantize 1/8 to 1; beat jump 1/2 to 64 (default 16). | CDJ UTILITY | Free | Missing | P2 |
| Jog screen | Artwork, slip, sync, vinyl, master, cue marker; custom image on 3000X. | CDJ jog | Free | Missing | P3 |
| Histories on CDJ | A HISTORY per stick insertion, played titles green; PLAYED/UNPLAYED marks on 3000X. | CDJ browse | Free | Missing | P2 |
| Matching category | Browse saved matches. | CDJ browse | Free | Missing | P3 |
| Phrase on CDJ overview | 3000X shows phrases under the overview. | CDJ settings | Free | Missing | P2 |
| Smart Cue and Gate Cue on CDJ | Hot cue becomes the cue; hold a hot cue while paused to play. | CDJ-3000X UTILITY | Free | Missing | P3 |
| Supported files on CDJ | MP3, AAC, WAV, AIFF, ALAC, FLAC; 44.1 and 48 kHz (lossless also 88.2/96); 16 and 24-bit. | n/a | Free | Missing (needs export) | P1 |

## 14. Performance: screens and layouts

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| 2 decks | Two decks with a mixer. | Layout selector | Free (FLX4) | Done | P0 |
| 4 decks | Four decks, horizontal or vertical waveforms, with a 4-channel mixer. | 4Deck Horizontal / 4Deck Vertical | Core | Missing | P2 |
| Horizontal waveforms | Zoomed waveforms stacked across the full width, for mixing. | 2Deck Horizontal | Core (layout choice) | Done | P0 |
| Vertical waveforms | Waveforms running top to bottom, for scratching. | 2Deck Vertical | Core | Missing | P3 |
| Waveform order with 4 decks | Order of the stacked waveforms. | Preferences > View > Layout | Free | Missing | P3 |
| Show or hide pads and platters | Per-deck pads and on-screen platters. | [View] menu | Free | Partial: pads shown; no platter | P3 |
| Panel toggles | Show or hide Effect, Sampler, Mixer, Recording, Video, Lighting, Groove Circuit and Mix Point Link panels. | Global section | Free | Partial: the effects strip, mixer and pads are always shown | P3 |
| Mixer hides with a controller | The on-screen mixer hides when a controller is connected. | Mixer panel | Free | Missing: always shown | P3 |
| Full screen | Full-screen window. | [View] > Full screen | Free | Partial: the macOS window can go full screen | P3 |
| Master level meter in the header | Master output meter at the top. | Global section | Free | Done | P0 |
| Audio dropout meter | Shows when audio skipped. | Global section | Free | Missing on screen (the engine counts dropouts) | P1 |
| Clock and battery | Time of day and laptop battery. | Global section | Free | Missing | P2 |
| Recording time in the header | Elapsed recording time. | Recording panel | Core (FLX4 unlocks) | Done (red timer) | P1 |
| Controller shown in the header | Connected device. | Global section | Free | Done | P1 |
| Hide title, artist and artwork on decks | Hides track info, for streams (7.2.16). | Preferences > View | Free | Missing | P3 |
| Keyboard help | Default shortcut reference PDF. | Help | Free | Done (? opens grouped help in the app) | P2 |
| Prepare and Perform screens | Not in rekordbox: a set-building screen and a mixing screen. | Not in rekordbox | n/a | Done (Cmd+1, Cmd+2) | P1 |
| Reopen as left | Reopens on the same screen and levels, never louder. | n/a | n/a | Done | P1 |
| Multi-display | Library on one screen and decks on another; asked for, not in rekordbox. | Not in rekordbox | n/a | Missing | P3 |

## 15. Performance: deck display, waveforms and overview

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Deck header | Title, artist, original BPM, key, elapsed and remaining time. | Track information panel | Free (FLX4) | Done | P0 |
| Artwork on the deck | Cover art in the deck header (and on the CDJ jog screen). | Track information panel | Free (FLX4) | Missing | P2 |
| Current BPM and tempo % | Playing BPM to two decimals and the pitch %. | Deck | Free (FLX4) | Done (real tempo shown after sync or Smart Fader) | P0 |
| Key and key difference | Current key and semitones from the original; double-click resets. | Deck | Free (FLX4) | Partial: key shown; no semitone difference or reset on the key | P1 |
| Scrolling (enlarged) waveform | Zoomed waveform around the playhead with the grid and cue marks. | Enlarged waveform | Free (FLX4) | Done (both decks on one real-time axis) | P0 |
| Waveform colour | BLUE, RGB or 3Band. | Preferences > View > Color > Waveform color | Free | Partial: 3-band only | P2 |
| 3-band colours | Low, mid and high drawn in separate colours. | Waveform | Free | Done (drawn from rekordbox's 3-band data or Dubplate's own) | P0 |
| Waveform for unanalysed tracks | Waveform drawn for any track. | Analysis | Free | Done (drawn from the audio, shared with the track check) | P0 |
| Waveform zoom | Zoom in and out, RST; FLX4 SHIFT + BROWSE turn. | Enlarged waveform, FLX4 | Free (FLX4) | Missing: fixed at about 9 seconds across | P1 |
| Hot cue marks on the scrolling waveform | Letters and flags at each hot cue. | Enlarged waveform | Free (FLX4) | Done (flags above the lane) | P0 |
| Memory cue marks on the scrolling waveform | Red triangles at memory cues. | Enlarged waveform | Free (FLX4) | Done (memory loops in the saved loop colour) | P0 |
| Cue point mark | Orange marker at the current cue. | Enlarged waveform | Free (FLX4) | Done | P0 |
| Loop shown on the waveform | The active loop as a tinted band; a saved loop as a span. | Enlarged waveform | Free (FLX4) | Done | P0 |
| Slip shadow playhead | The background position while slipping. | Enlarged waveform | Free (FLX4) | Done | P1 |
| Beat count display | Current bar, or bars or beats to the next memory cue. | Preferences > View > Display Type | Free | Missing | P1 |
| Phrases on the scrolling waveform | Phrase strip under the zoomed waveform. | Preferences > View > Layout > Phrases | Free | Missing | P2 |
| Vocal sections on waveforms | Detected vocals marked on both waveforms. | Preferences > View > Layout > Vocal | Free | Missing | P2 |
| Click the waveform to play or cue | Left click play/pause, right click CUE. | Preferences > View > Display Type | Free | Missing | P3 |
| Drag the waveform to scrub | Drag to move through the track. | Enlarged waveform | Free | Missing | P2 |
| Overview (full waveform) | The whole track with playhead and played part. | Track information panel | Free (FLX4) | Done: 3-band overview with cues, loops, phrases and the end warning | P0 |
| Overview cue and loop marks | Hot cues, memory cues, loops and phrases on the overview. | Overview | Free (FLX4) | Done | P0 |
| Overview phrases | Phrase strip on the full waveform. | Preferences > View > Layout > Phrases | Free | Done | P1 |
| End of track warning | The overview and time flash in the last seconds. | Overview, time | Free | Done (last 30 s tinted, time turns amber) | P0 |
| Minute ticks and hover time | Not in rekordbox in this form. | n/a | n/a | Done | P2 |
| Needle search | Click the overview to jump. | Overview | Free (FLX4) | Done | P1 |
| Needle lock | Blocks needle search while playing. | Preferences > Controller > Deck | Free | Partial: keys on the overview no longer move a playing deck; clicks still jump | P1 |
| Phase meter | Bars showing each deck's beat offset from the master. | Above the decks | Free (FLX4) | Missing | P1 |
| Parallel waveforms | Two waveforms on one time axis to see drift (Traktor). | Not in rekordbox | n/a | Done (both lanes on one real-time axis) | P1 |
| Platter display | On-screen jog with cue marker; hot cue countdown mode. | JOG panel | Free | Missing | P3 |
| Waveform drawing rate | Smoother waveforms for more CPU. | Preferences > View | Free | Done in effect (cached views, per-frame quads only) | P3 |
| STEM waveform | Waveform shows only the stems that play. | Preferences > Extensions > STEMS | Free | Missing | P2 |
| Fill-in marker | Marks the last up to 4 beats of Intro, Up and Chorus. | Enlarged waveform | Free | Missing | P3 |

## 16. Performance: transport and cues

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| PLAY/PAUSE | Starts and stops the deck. | Deck, FLX4 PLAY/PAUSE | Free (FLX4) | Done | P0 |
| CUE, CDJ style | Paused: sets the cue (snapped with quantize). Playing: back cue, returns and pauses. | Deck CUE, FLX4 CUE | Free (FLX4) | Done | P0 |
| Cue point sampler | Held at the cue, plays while held and returns on release. | Deck CUE | Free (FLX4) | Done | P0 |
| Real-time cue | IN while playing sets a cue on the fly. | Deck IN | Free (FLX4) | Missing: IN sets only the loop in point | P2 |
| Jump to track start | SHIFT + CUE. | FLX4 SHIFT + CUE | Free (FLX4) | Done | P1 |
| Auto Cue | On load, the cue goes to the first audio above a level (-36 to -78 dB) or to the nearest memory or hot cue. | Preferences > Controller > Deck > Auto Cue | Free | Partial: set tracks load at their mix-in; other tracks load at 0:00, often in silence | P0 |
| Load at the first cue | Playback starts at the cue nearest the start. | Preferences > Controller > Deck > Load | Free | Missing | P1 |
| Load lock | Refuses to load onto a playing deck. | Preferences > Controller > Deck > Eject/Load Lock | Free | Done (header toggle) | P0 |
| Instant doubles | LOAD twice, or drag a deck title onto the other deck, copies track, position, tempo, loop and slip state. | FLX4 LOAD x2, deck drag | Free (FLX4) | Done (LOAD twice; no drag between decks) | P1 |
| Undo load | Puts back the previous track at its position. | Keyboard Cmd+Z | Free | Missing | P2 |
| Eject a deck | Unloads the deck (eject on the artwork). | Deck | Free | Missing (the engine can eject; no control) | P3 |
| Continuous playing | Plays the next track of the list when one ends. | Preferences > Controller > Deck | Free | Missing | P3 |
| Elapsed and remaining time | Both shown; remaining with a minus. | Deck | Free (FLX4) | Done | P0 |
| Reverse | Plays backwards; slip reverse plays 8 beats back and returns in time. | Controller, keyboard S | Free (FLX4) | Missing | P2 |
| Hot cues A to H | Set on an empty pad, call on a full one, quantized; a stopped deck plays from it. | Pads | Free (FLX4) | Done: set, call and delete live, saved per track and exported to XML | P0 |
| Hot cues I to P | 16 hot cues per track in Performance mode (FLX4 reaches A to H). | Pads, scroll | Free | Missing | P3 |
| Hot cue loops | A pad pressed during a loop stores the loop; calling it starts the loop. | Pads | Free (FLX4) | Done | P1 |
| Hot cue colours on screen | Each hot cue's own colour from the 16-colour palette. | Pads, waveform | Free | Partial: one fixed pastel per pad letter | P1 |
| Hot cue names | Comment on each hot cue, shown on the pad. | Pads | Free | Missing | P1 |
| Delete a hot cue | SHIFT + pad, or the x on the pad. | FLX4 SHIFT + pad | Free (FLX4) | Done (live only) | P0 |
| Smart Cue | Pressing a hot cue makes it the new cue point; the CUE button takes its colour (7.2.14). | Preferences > Controller > Deck > HOT CUE | Free | Missing | P2 |
| Gate Cue | While paused, holding a hot cue plays from it and releasing returns. | Preferences > Controller > Deck > HOT CUE | Free | Missing | P2 |
| Hot cue call mode | Immediate jump keeping the beat offset, or wait for the next beat. | Preferences > Controller > Others > Mode | Free | Partial: immediate with the beat offset only | P3 |
| Memory cues | Up to 10 cues and 10 loops per track with colour and comment. | MEMORY CUE pad mode, deck | Free | Done: read from rekordbox, drawn on the overview and the lanes, called, saved (up to 10) and deleted, kept per track | P0 |
| Call memory cues | CUE/LOOP CALL ◁ ▷ steps to the previous or next memory cue or loop. | FLX4 CUE/LOOP CALL | Free (FLX4) | Done (H and J on the keyboard; with a loop on they halve and double it) | P0 |
| Save a memory cue | SHIFT + CUE/LOOP CALL ▷, or MEMORY. | FLX4 | Free (FLX4) | Done (N; the loop that is on is saved as a memory loop) | P1 |
| Delete a memory cue | SHIFT + CUE/LOOP CALL ◁. | FLX4 | Free (FLX4) | Done (⌥N) | P2 |
| Memory cue call lock | Blocks memory cue calls while playing. | Preferences > Controller > Deck | Free | Missing | P3 |
| Cue and loop list | Every memory cue, hot cue and loop with time, colour and comment, click to jump. | MEMORY CUE pad mode | Free | Missing | P2 |
| Mix cues on the deck | Not in rekordbox: the set's mix-in and mix-out. | Not in rekordbox | n/a | Done (draws them on the overview) | P1 |
| Bar countdown to the mix-out | Not in rekordbox (planned in Dubplate's spec). | Not in rekordbox | n/a | Missing | P1 |
| Active Censor | Sections where an effect (reverse roll, trans, echo, vinyl brake) replaces the audio automatically. | ACT. CENSR pad mode | Free | Missing | P3 |

## 17. Performance: loops, beat jump, slip and quantize

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Auto beat loop | 1/64 to 512 beats; loops of 1/32 to 1/2 turn sync off. | Deck AU loop, BEAT LOOP pads | Free (FLX4) | Partial: 1/16 to 32 beats | P1 |
| 4 BEAT/EXIT | A 4-beat loop, exit while looping; the length is a preference. | FLX4 4 BEAT/EXIT | Free (FLX4) | Done (fixed 4 beats) | P0 |
| Loop IN and OUT | Manual loop points, quantized. | FLX4 IN, OUT | Free (FLX4) | Done | P0 |
| Loop in and out adjust | With a loop on, IN or OUT then the jog moves that point. | FLX4 IN/OUT + jog | Free (FLX4) | Partial: OUT alone moves the loop end to the playhead; no jog adjust of either point | P1 |
| Halve and double | CUE/LOOP CALL while looping. | FLX4 | Free (FLX4) | Done | P0 |
| Reloop and exit | Leave the loop or go back into it. | FLX4 4 BEAT/EXIT, deck | Free (FLX4) | Done | P0 |
| Saved loops | Memory loops and hot loops, 10 per track, orange icon. | MEMORY CUE, HOT CUE | Free | Partial: drawn on the overview, called and saved as memory and hot loops; no limit on hot loops beyond the 8 pads | P1 |
| Active loop | A saved loop marked active engages when the playhead reaches it; SHIFT + 4 BEAT/EXIT toggles it. | FLX4 SHIFT + 4 BEAT/EXIT | Free (FLX4) | Missing | P2 |
| Loop roll | Rolls of 1/32 to 4 beats while held, slipping back in time. | PAD FX, BEAT FX ROLL | Free (FLX4) | Done (1/16 to 8 beats) | P1 |
| Slicer | 8 slices of a 2 to 16 bar range on the pads, rolls of 1/64 to 1/8 of the range, slicer or slicer loop mode. | SLICER pad mode | Free | Missing | P2 |
| Beat jump pads | 1, 2, 4 and 8 beats each way; SHIFT + pads 7 and 8 change the sizes. | FLX4 BEAT JUMP | Free (FLX4) | Done | P0 |
| Beat jump values | FINE (5 ms), 1/8 to 128 beats; a playing loop moves with it. | BEAT JUMP pad mode | Free | Partial: scaled from 1/8 to 64 beats; no FINE 5 ms | P2 |
| Beat jump on screen | Buttons each side of an amount on the deck. | Deck | Free | Missing (keys and pads only) | P3 |
| Slip | Loops, scratches, hot cues, pause and reverse play while the track runs underneath; red ring and shadow line. | Deck SLIP | Free (FLX4) | Done | P1 |
| Slip pause and slip reverse | Pause or reverse and come back in time. | Deck, controller | Free (FLX4) | Missing | P2 |
| Quantize switch | Q per deck, or all decks at once. | Deck Q, Preferences > Controller > Others | Free (FLX4) | Partial: always on; no switch | P1 |
| Quantize types | SNAP (cue and loop start to the beat, with a beat length) and QUANTIZE for hot cue, loop, sampler, reverse and sequencer. | Preferences > Controller > Others > Type | Free | Missing | P2 |
| Emergency loop | A 4-beat loop holds at the end of a track (CDJ). | CDJ | Free | Missing | P2 |

## 18. Performance: tempo, sync and key

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Tempo fader | Changes tempo within the range. | Deck, FLX4 TEMPO | Free (FLX4) | Done | P0 |
| Tempo range | ±6% (0.02% steps), ±10% and ±16% (0.04%), WIDE (0.5%, -100% stops). | Deck, FLX4 SHIFT + BEAT SYNC | Free (FLX4) | Done (same steps) | P1 |
| Tempo pick-up | A fader moved after sync takes over only when it passes the playing tempo. | Deck | Free (FLX4) | Done (amber outline shows where the fader waits) | P1 |
| Tempo reset | Double-click the tempo % or TEMPO RESET. | Deck | Free | Missing | P2 |
| Tempo nudge keys | Small and medium tempo steps from the keyboard. | Keyboard | Free | Done (- and =) | P2 |
| Pitch bend | Hold to bend; jog edge; keyboard. | Deck, jog | Free (FLX4) | Done | P0 |
| Master Tempo (key lock) | Keeps the pitch while the tempo changes. | Deck MT | Free (FLX4) | Done | P0 |
| BEAT SYNC | Locks tempo and phase to the master. | Deck, FLX4 | Free (FLX4) | Done | P0 |
| BPM sync or beat sync | Sync type preference: tempo only, or tempo and phase. | Preferences > Controller > Deck | Free | Partial: beat sync, dropping to tempo sync when nudged; no preference | P2 |
| Double and half sync | Allows sync at x2 or x1/2 of the master; SYNC RATE button cycles x1, x2, x1/2. | Preferences > Controller > Deck, deck | Free | Partial: automatic half/double lock; no SYNC RATE button | P2 |
| Tempo master | MASTER button or hold BEAT SYNC; moves on when the master track ends. | Deck, FLX4 | Free (FLX4) | Done | P0 |
| Key sync | Shifts the deck's key to match the other deck; stays until KEY RESET. | Deck KEY SYNC | Free | Done | P1 |
| Semitone up and down | One semitone per click on the deck. | Deck | Free | Partial: Key Shift pads; no deck buttons | P2 |
| Key shift range | -12 to +12 semitones. | KEY SHIFT pad mode | Free (FLX4) | Done | P2 |
| Tap tempo | SHIFT + channel CUE taps the BPM. | FLX4 | Free (FLX4) | Done | P2 |
| Ableton Link | Shares tempo and phase on the network; start/stop sync. | Global LINK | Free | Missing | P3 |
| MIDI clock out | Sends tempo to drum machines (Traktor). | Not in rekordbox | n/a | Missing | P3 |

## 19. Performance: jog wheels

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Scratch | Vinyl mode on: touching the top holds the record, turning scratches. | FLX4 jog top | Free (FLX4) | Done (33⅓ RPM feel, 720 ticks a turn) | P1 |
| Vinyl mode on/off | Top of the jog scratches or bends; on the FLX4 only from a keyboard shortcut. | Keyboard shortcut | Free | Missing: always vinyl | P1 |
| Pitch bend from the jog | Jog edge, or the top with vinyl off. | FLX4 jog | Free (FLX4) | Done | P0 |
| Search | SHIFT + jog moves fast. | FLX4 | Free (FLX4) | Done (15 s a turn) | P1 |
| Frame search when paused | The jog moves the playhead by hand. | Jog | Free (FLX4) | Done | P1 |
| Vinyl speed adjust | How fast the deck stops on touch or pause, and starts on release or play. | Preferences > Controller > Deck | Free | Missing | P3 |
| Back spin length | Short, normal or long (FLX4). | Preferences > Controller > Deck | Free (FLX4) | Missing | P3 |
| Cue and spin return | After CUE and a spin, returns to the cue and pauses. | Preferences > Controller > Deck > JOG | Free | Missing | P3 |
| On-screen platter speed | Platter rotation speed. | Preferences > Controller > Deck > JOG | Free | Missing | P3 |

## 20. Performance: pads, sampler, sequencer and capture

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| HOT CUE pad mode | Hot cues on the pads. | FLX4 HOT CUE | Free (FLX4) | Done | P0 |
| KEYBOARD pad mode | Plays the chosen hot cue at +4 to +7 and ±0 to +3 semitones; SHIFT + pads 7 and 8 move the range. | FLX4 SHIFT + HOT CUE | Free (FLX4) | Done | P2 |
| PAD FX1 and PAD FX2 | 32 pad effects (16 per mode with SHIFT), held to apply; several at once. | FLX4 PAD FX | Free (FLX4) | Partial: all 32 pads, every Beat FX and loop rolls, editable; one pad plays at a time per deck | P1 |
| Pad FX editing | Choose the effect, beat and parameter of each of the 32 pads. | PAD FX edit mode | Free | Done: effect or loop roll, parameter, depth, hold or latch, ring out or dry; Classic, Afro house and Amapiano presets | P2 |
| Release FX on a pad | A pad that cancels held effects and plays a release effect. | PAD FX | Free | Partial: a pad can play Echo Out, Vinyl Brake or Backspin, which takes over from the pad held | P2 |
| Temporary beat change | Hold a Pad FX pad and press BEAT ◁ ▷. | Controller | Free | Missing | P3 |
| BEAT JUMP pad mode | See loops and beat jump. | FLX4 BEAT JUMP | Free (FLX4) | Done | P0 |
| BEAT LOOP pad mode | 1/4, 1/2, 1, 2, 4, 8, 16, 32 beats on pads 1 to 8. | FLX4 SHIFT + BEAT JUMP | Free (FLX4) | Done | P0 |
| SAMPLER pad mode | Left pads slots 1 to 8, right pads 9 to 16; SHIFT stops or loads the selected track. | FLX4 SAMPLER | Free (FLX4) | Done | P1 |
| KEY SHIFT pad mode | Shifts the deck's key by the pad's semitones. | FLX4 SHIFT + SAMPLER | Free (FLX4) | Done | P2 |
| Pad editor and USER pad modes | Custom pad maps from templates, 2 assign pages. | PAD EDITOR (not for the FLX4) | Core | Missing | P3 |
| Sample scratch | Loads a sampler slot's sound onto the deck to scratch it. | SP.SCRATCH pad mode | Core (FLX4 unlocks) | Missing | P3 |
| STEMS pad mode | Stem mute, solo and stem FX on pads. | Pad editor | Free | Missing | P2 |
| SEQ. CALL pad mode | Up to 8 saved sequences on the pads. | Pad mode | Creative (saving) | Missing | P3 |
| Sampler slots and banks | 16 slots per bank, 4 banks (64 slots), 20 voices. | Sampler deck | Free (FLX4) | Partial: 16 slots, one bank | P2 |
| Load and unload a slot | Drag a track into a slot; eject on hover; slots kept after quitting. | Sampler deck | Free (FLX4) | Done (selected track via SHIFT + pad; kept after quitting) | P1 |
| One-shot, loop and gate | Slot play modes, with a gate that plays only while held. | Slot EDIT | Free (FLX4) | Done | P1 |
| Slot sync and BPM | Loop slots follow the master BPM; sampler BPM editable; x2 and /2. | Sampler deck, slot EDIT | Free (FLX4) | Partial: synced slots follow the master (half or double when closer); no sampler BPM or x2 controls | P2 |
| Slot gain and master tempo | Per-slot gain and key lock. | Slot EDIT | Free (FLX4) | Partial: per-slot gain; no slot key lock | P3 |
| Slot fine start | Fine-tune a one-shot's start. | Slot EDIT | Free | Missing | P3 |
| Sampler volume and CUE | Sampler level and headphone cue. | Sampler deck | Free (FLX4) | Done | P1 |
| Sampler as sync master | The sampler can lead tempo. | Sampler deck | Free | Missing | P3 |
| Sample packs | Download or import sample packs; first 16 tracks of a playlist into a bank. | [File] > Additional contents | Free | Missing | P2 |
| OSC sampler | Oscillator sounds (sirens, horns) on the sampler. | Sampler deck | Free | Missing | P2 |
| Loop capture | Captures the playing loop or slicer range into a sampler slot. | Enlarged waveform CAPTURE | Free | Missing | P2 |
| Sequencer | Records 1, 2 or 4-bar patterns of one-shot slots with mute, erase and metronome; loads a sequence onto a deck. | Sequencer panel | Free (saving: Creative) | Missing | P3 |
| Remix decks | Traktor: 4 slots, sets of 64 cells, loop or one-shot, capture. | Not in rekordbox | n/a | Missing | P3 |
| Pattern player | Traktor: a drum sequencer in an FX unit with ducking. | Not in rekordbox | n/a | Missing | P3 |
| Loop recorder | Traktor: records the master, cue or a deck into a loop to overdub. | Not in rekordbox | n/a | Missing | P3 |
| Scratch bank | Serato: a pad swaps a scratch sample onto the platter. | Not in rekordbox (Sample Scratch is close) | n/a | Missing | P3 |

## 21. Performance: Beat FX

rekordbox does not print its full Beat FX list; these names come from the plan page ("DJM effects", "RMX effects") and the DJM and DDJ manuals whose effects rekordbox mirrors.

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Two Beat FX units | FX1 and FX2, each on any deck, the sampler or the master. | Effect panel | Free (FLX4) | Done (three units on deck A, deck B or the master) | P0 |
| Effect on both decks (1&2) | FLX4 CH SELECT 1&2 runs the effect on channels 1 and 2. | FLX4 CH SELECT | Free (FLX4) | In progress | P0 |
| Effect on the sampler | Assign a unit to the sampler. | Effect panel [S] | Free | Missing | P3 |
| Single mode | One effect with its own parameter knobs and snapshots. | Effect panel | Free | Partial: one depth knob per effect | P1 |
| Multi mode | Up to 3 effects chained per unit, each with on, beat and level. | Effect panel | Free | Missing | P2 |
| Snapshots | Saves a parameter set per effect. | Effect panel | Free | Missing | P3 |
| Beat values | 1/16 to 16 for echo-type effects, up to 64 for filters and modulation. | Effect panel BEAT ◁ ▷ | Free (FLX4) | Done (1/16 to 32, with 1/3, 3/8, 2/3, 3/4, 1.5) | P0 |
| AUTO or TAP BPM | Effects follow the deck BPM or a tapped BPM (FLX4 SHIFT + BEAT). | Effect panel | Free (FLX4) | Partial: follows the deck; no tap | P3 |
| FX frequency bands | DJM X-PAD and LOW/MID/HI band buttons. | DJM hardware | n/a | Missing | P3 |
| DELAY | One repeat, 1/16 to 16 beats. | Beat FX | Free | Missing | P1 |
| ECHO | Decaying repeats, 1/16 to 16 beats. | Beat FX | Free | Done | P0 |
| PING PONG | Echoes alternating left and right. | Beat FX | Core (FLX4 unlocks) | Done | P1 |
| SPIRAL | Reverb whose pitch moves as the time changes. | Beat FX | Free | Missing | P2 |
| REVERB | Room size 1 to 100%. | Beat FX | Free | Done | P0 |
| TRANS | Rhythmic cut, 1/16 to 16 beats. | Beat FX | Free | Done (Gate) | P1 |
| FILTER | Cyclic filter sweep, up to 64 beats. | Beat FX | Free | Partial: low-pass, high-pass, band-pass and sweep filters, not a cyclic LFO sweep | P2 |
| FLANGER | Cyclic flanger. | Beat FX | Free | Done | P1 |
| PHASER | Cyclic phaser. | Beat FX | Free | Done | P1 |
| PITCH | Pitch from -50 to +100%. | Beat FX | Free | Missing | P2 |
| ROLL | Records on press and repeats, 1/16 to 16. | Beat FX | Free | Partial: rolls on pads, not as a Beat FX | P2 |
| SLIP ROLL | Roll that re-records when the beat changes. | Beat FX | Free | Missing | P3 |
| VINYL BRAKE | Slows to a stop over 1/16 to 16 beats. | Beat FX | Core (FLX4 unlocks) | Done (Brake) | P1 |
| BACKSPIN | Spins the record backwards. | Release FX, Pad FX | Free | Done | P1 |
| HELIX | Records and repeats; full depth freezes the sound. | Beat FX | Core (FLX4 unlocks) | Missing | P2 |
| STRETCH | Records and time-stretches. | Beat FX | Core (FLX4 unlocks) | Missing | P3 |
| MOBIUS (SAW and TRI) | Endless rising or falling oscillator. | Beat FX | Free | Missing | P3 |
| ENIGMA JET | Endless rising or falling flanger. | Beat FX | Free | Missing | P3 |
| MULTI TAP DELAY | Up to 7 repeats in 1/8-beat steps. | Beat FX | Free | Missing | P3 |
| LOW CUT ECHO | Echo with the lows cut, so the bass does not pile up. | Beat FX | Free | Missing | P1 |
| SHIMMER | Reverb with pitch-shifted tails (DJM-V10). | DJM effects | Core | Missing | P3 |
| TRIPLET FILTER and TRIPLET ROLL | Triplet-timed filter and roll (DJM-A9). | DJM effects | Core | Missing | P3 |
| Bit crusher | Crush and sample-rate reduction (Sound Color FX CRUSH). | Sound Color FX | Free | Done (Crusher) | P2 |
| RMX effects | BPF ECHO, NOISE, SPIRAL UP and DOWN, REVERB UP and DOWN, HPF ECHO, LPF ECHO, CRUSH ECHO build and release effects. | Effect lists, PAD FX | Creative | Missing | P2 |
| Effects ring out | Echo and reverb tails continue after the effect turns off or the fader closes. | Beat FX | Free | Done | P0 |
| FX on a deck after the fader | Tails keep ringing when the channel fader closes. | Beat FX routing | Free | Done | P0 |

## 22. Performance: Sound Color FX, Release FX, Merge FX, Smart CFX and Smart Fader

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| CFX FILTER | Low-pass to the left, high-pass to the right, resonance as the parameter. | FLX4 CFX | Free (FLX4) | Done | P0 |
| CFX selection | Pick the Sound Color FX the CFX knobs play (USER mode, 9 types), or a different one per channel (Multi). | Effect panel CFX | Free | Done: Filter and 9 colours, for both channels or each | P1 |
| DUB ECHO | Echo on mids to the left, highs to the right. | Sound Color FX | Free | Done: as a Sound Color FX on either channel, or a Smart CFX preset | P1 |
| SPACE | Reverb on mids and lows or mids and highs. | Sound Color FX | Free | Done: as a Sound Color FX on either channel, or a Smart CFX preset | P1 |
| SWEEP | Notch one way, band-pass the other. | Sound Color FX | Free | Done: a tightening gate to the left, band-pass to the right | P2 |
| NOISE | Filtered white noise for builds. | Sound Color FX | Free | Done | P2 |
| CRUSH | Crush, or crush then high-pass. | Sound Color FX | Free | Done: as a Sound Color FX on either channel, or a Smart CFX preset | P2 |
| PITCH | Pitch down or up. | Sound Color FX | Free | Done | P3 |
| Release FX | Applies ECHO, BACK SPIN or VINYL BRAKE while cutting the running Beat FX; SHIFT + BEAT FX ON on the FLX4. | Effect panel, FLX4 | Free (FLX4) | Missing | P1 |
| Release FX on CFX | Release FX also turns off Sound Color FX. | Preferences > Controller > Effect | Free | Missing | P3 |
| Merge FX | One knob builds with effects and samples, then releases into a drop sample (4 presets). | MFX panel | Core (FLX4 unlocks) | Missing | P3 |
| Smart CFX | One CFX knob drives a preset of several effects (some change loops and key). | FLX4 SMART CFX | Free (FLX4) | Done (six presets: Space, Dub Echo, Crush, Chop, Jet, Phase) | P1 |
| Smart CFX preset window | SHIFT + SMART CFX opens a preset picker; window position in Preferences. | FLX4, Preferences | Free (FLX4) | Done (picker and SHIFT stepping) | P2 |
| Smart Fader | The crossfader blends tempo, swaps the low EQ between 1/3 and 2/3 of its travel, and echoes the outgoing deck at the far edge; sync forced on. | FLX4 SMART FADER | Free (FLX4) | Done: tempo blend, bass swap, and the echo at the edge with Echo out on the faders on | P1 |
| Smart Fader source | Crossfader and channel faders, crossfader only, or channel faders only; recommended curve option. | Preferences > Controller > Mixer | Free (FLX4) | Done: Both, Crossfader or Channel faders in Settings > Mixer; no recommended curve option | P2 |
| Fader echo | An echo tail when a fader closes (Engine DJ), the afro house echo-out move. | Not in rekordbox | n/a | Missing | P1 |
| Stem echo out | Echo only the vocal or the drums (Serato, VirtualDJ, djay). | STEM FX | Free | Missing | P2 |
| Crossfader FX | djay: moving the crossfader morphs with echo, filter, riser or stem transitions. | Not in rekordbox | n/a | Missing | P3 |

## 23. Performance: mixer

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Trim | Channel gain before the EQ. | Mixer, FLX4 TRIM | Free (FLX4) | Done | P0 |
| Auto Gain | Levels every loaded track from its analysed loudness; adjustable in the grid panel. | Preferences > Controller > Mixer | Free | Partial: rekordbox's stored gain or Dubplate's loudness on load, on top of TRIM, with a switch in Settings; not adjustable per track yet | P0 |
| 3-band EQ | HI, MID, LOW. | Mixer, FLX4 EQ | Free (FLX4) | Done (isolator: centre flat, left kill, right +6 dB; splits at 250 Hz and 2.5 kHz) | P0 |
| EQ or isolator | The knobs act as EQ (DJM style, -26 dB) or isolator (full kill). | Preferences > Controller > Mixer > EQ | Free | Partial: isolator only | P2 |
| EQ type | DJM-900NXS or DJM-900NXS2 curve. | Preferences > Controller > Mixer > EQ | Free | Missing | P3 |
| EQ kill by click | Click a band name to kill it; click a knob to reset it. | Mixer panel | Free | Missing | P3 |
| CFX filter knob | Per-channel filter or Sound Color FX. | Mixer, FLX4 CFX | Free (FLX4) | Done | P0 |
| Filter resonance | Resonance of the CFX filter. | Sound Color FX parameter | Free | Missing (the engine can set it; no control) | P3 |
| Channel faders | Volume per channel. | Mixer, FLX4 | Free (FLX4) | Done | P0 |
| Channel fader curve | Curve options (DJM has steep top, linear, steep bottom). | Preferences > Controller > Mixer | Free | Missing | P2 |
| Crossfader | Blends the two sides. | Mixer, FLX4 | Free (FLX4) | Done (constant power) | P0 |
| Crossfader curve | Curve options (DJM has sharp cut, constant, smooth). | Preferences > Controller > Mixer | Free | Missing | P1 |
| Crossfader assign | Each channel to A, THRU or B. | Mixer panel | Free | Missing: channel 1 is always A, channel 2 always B | P1 |
| Crossfader reverse | Swaps the crossfader's sides (FLX4 utility). | Preferences > Controller > Mixer | Free (FLX4) | Missing | P3 |
| Crossfader cut lag | 0.3 to 5.5 mm in 0.1 mm steps, default 1.0 mm (FLX4 utility). | Preferences > Controller > Mixer | Free (FLX4) | Missing | P3 |
| Channel level meters | Pre-fader meters per channel. | Mixer | Free (FLX4) | Done | P0 |
| FLX4 meter source | FLX4 meters show channel level or master level. | Preferences > Controller > Mixer | Free (FLX4) | Missing | P3 |
| Master level | Master volume. | Global section, FLX4 MASTER LEVEL | Free (FLX4) | Done (a louder knob waits; turning down always works) | P0 |
| Output level and limiter | Internal mixer output level -21 to -3 dB in 3 dB steps; limiter on clip. | Preferences > Controller > Mixer | Free | Done (look-ahead limiter at -0.3 dBFS on master and headphones) | P0 |
| Limiter indicator | Shows when the master is pushed into the limiter. | Master meter | Free | Partial: the engine reports limiter gain; no LIMIT light on screen confirmed | P2 |
| Booth output | Separate booth output in internal mixer mode. | Preferences > Audio > Output | Free | Missing | P2 |
| Internal or external mixer mode | Mix in software, or send each deck to its own output for a hardware mixer. | Preferences > Audio > Mixer Mode | Free | Missing | P2 |
| Microphone panel | Mic on/off, meter, EQ, mic FX, talkover, feedback reducer. | Microphone panel | Free | Missing (the FLX4 mixes its mic in hardware) | P3 |
| Talkover | Ducks the music -6, -12, -18 or -24 dB, all bands or mids only. | Preferences > Controller > Mixer | Free | Missing | P3 |
| Fader start | SHIFT + channel fader or crossfader starts and stops the deck. | FLX4 SHIFT + faders | Free (FLX4) | Missing | P2 |
| Master isolator and send/return | DJM-V10 master isolator, send effects, external returns. | DJM hardware | n/a | Missing | P3 |

## 24. Performance: headphones, audio and recording

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Channel CUE | Sends a channel to the headphones before its fader. | FLX4 CUE | Free (FLX4) | Done | P0 |
| Master CUE | Sends the master to the headphones. | FLX4 HEADPHONE CUE (MASTER) | Free (FLX4) | Done | P0 |
| Headphones MIX and LEVEL | Cue and master balance, volume. | FLX4 | Free (FLX4) | Done (-3 dB each in the middle) | P0 |
| Split cue | Cue on one ear, master on the other; asked for, not documented in rekordbox. | Not in rekordbox | n/a | Missing | P3 |
| Smart headphone cue | Cue follows the last loaded deck (Engine DJ). | Not in rekordbox | n/a | Missing | P3 |
| Browser preview | Plays a track from the list in the headphones without a deck; preview volume setting. | Preview column, Preferences > Audio | Free | Missing | P1 |
| Seam audition | Not in rekordbox: hears a planned transition in the headphones only. | Not in rekordbox | n/a | Done | P1 |
| Sandbox | VirtualDJ: rehearse the next transition in the headphones while the master plays on. | Not in rekordbox | n/a | Partial: the seam audition plays a fixed 20 s; no free rehearsal | P2 |
| Sound card choice | Output device; FLX4: master on outputs 1/2, headphones on 3/4. | Preferences > Audio | Free | Done | P0 |
| Carry decks to a new sound card | Decks keep their tracks when the card changes. | n/a | Free | Done, including a card that stopped | P1 |
| Buffer size | Samples per transfer, with the latency shown. | Preferences > Audio | Free | Missing on screen (the engine supports a fixed buffer) | P1 |
| Sample rate | Sampling rate sent to the device (44.1 or 48 kHz on the FLX4). | Preferences > Audio | Free | Partial: follows the device | P3 |
| Output channel mapping | Master, headphones and booth outputs chosen by hand. | Preferences > Audio > Input/Output | Free | Partial: automatic | P2 |
| Metronome tone and volume | For grid checking. | Preferences > Audio | Free | Missing | P2 |
| Mono master output | FLX4 master mono or stereo. | Preferences > Controller > DDJ-FLX4 | Free (FLX4) | Missing | P3 |
| Recording | Records the master as WAV, with REC, elapsed and remaining time. | Recording panel | Core (FLX4 unlocks) | Done (24-bit WAV after the limiter, Music/Dubplate Recordings) | P1 |
| Recording source and level | Choose the source (master or an input) and set the record level with a meter. | Recording panel | Core (FLX4 unlocks) | Partial: master only, no level control | P2 |
| Auto start and stop | Waits for audio; stops after 20 s of silence. | Preferences > Controller > Recordings | Core (FLX4 unlocks) | Missing | P3 |
| Auto split on silence | Splits files on 1 to 10 s of silence. | Preferences > Recordings | Core (FLX4 unlocks) | Missing | P3 |
| Track marks in a recording | Split or mark a mix per track; asked for, not in rekordbox. | Not in rekordbox | n/a | Missing | P1 |
| Recording format choice | WAV or AIFF, 16 or 24-bit; asked for (Serato offers it). | Not in rekordbox | n/a | Partial: 24-bit WAV only | P3 |
| Recordings library | Recordings listed in the browser. | Media Browser > Recordings | Free | Missing | P2 |
| Share Mix | Uploads to Mixcloud (M4A) or YouTube (MP4) at 16-bit 44.1 kHz 192 kbps. | Recordings > Share Mix | Core (FLX4 unlocks) | Missing | P3 |
| Record vinyl through an input | Records a USB turntable or mixer input in Export mode. | Recording panel | Free | Missing | P3 |
| Broadcast | VirtualDJ: stream to Icecast or Shoutcast. | Not in rekordbox | n/a | Missing | P3 |

## 25. Performance: stems

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Stem separation | Separates in real time into 3 stems (vocal, inst, drums) or 4 (adds bass, 7.2.8). | Preferences > Extensions > STEMS | Free | Partial: 4-stem separation ahead of time runs in a library crate; the app does not use it yet | P1 |
| ACTIVE PART buttons | Per-deck buttons that mute or solo each stem (MUTE or SOLO setting). | Deck | Free | Missing (deck controls on an unmerged branch) | P1 |
| STEM ISO knobs | Per-stem volume knobs in place of the EQ. | Mixer | Free | Missing | P2 |
| STEM FX | Send effects to chosen stems only. | Effect panel | Free | Missing | P2 |
| Stem quality or speed | Prioritise sound quality or speed (7.0.4). | Preferences > Extensions > STEMS | Free | Done in effect (Demucs ahead of time, quality first) | P2 |
| Stems saved and reused | Stems kept between sessions; rekordbox cannot save or export them (a top request). | Not in rekordbox | n/a | Partial: stems kept in Dubplate's folder, not playable yet | P1 |
| Stem export to files | Write stems as WAV files (VirtualDJ request). | Not in rekordbox | n/a | Missing | P3 |
| 5 stems with kick and hats | VirtualDJ splits kick and hi-hat; lets amapiano DJs keep the log drum while swapping hats. | Not in rekordbox | n/a | Missing | P3 |
| Per-stem crossfader | djay: blend or swap single stems between decks. | Not in rekordbox | n/a | Missing | P3 |
| Groove Circuit | Swap a deck's drums for 8 drum loops (2 banks of 4), single or multi mode, drum capture of 4 to 32 beats, drum release FX. | GROOVE CIRCUIT panel | Core (FLX4 unlock unconfirmed) | Missing | P3 |
| STEMS layout and instant doubles options | Stem controls layout, stem instant doubles target, memory and threads. | Preferences > Extensions > STEMS | Free | Missing | P3 |

## 26. Automix, Mix Point Link and edits

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Automix | Plays a playlist alternately on decks 1 and 2 with automatic transitions; repeat, random, save as playlist. | [View] > Automix | Free | Missing | P3 |
| Smart automix | VirtualDJ and djay pick intro and outro mix points and use FX transitions. | Not in rekordbox | n/a | Missing | P3 |
| MIX POINT LINK | Links a MIX OUT point on the playing deck to a MIX IN point on the next; the next deck starts by itself, optionally silent until then. | MIX POINT LINK panel | Core (FLX4 unlock unconfirmed) | Partial: the set stores mix-in and mix-out and loads track 2 at its mix-in; no automatic start | P2 |
| Load the set onto the decks | Not in rekordbox: track 1 on A, track 2 on B at their mix cues. | Not in rekordbox | n/a | Done (Cmd+L) | P1 |
| Next set track on deck B | Deck B shows the next set track, the seam plan and a countdown. | Not in rekordbox | n/a | Missing | P1 |
| EDIT mode | Non-destructive track edits (cut, copy, insert, mute, 100 palette ranges, 1 or 4-beat quantize), rendered to a file. | EDIT mode | Creative (render) | Missing | P3 |
| Flip | Serato: records cue jumps and censors as an edit of the track, 6 per track. | Not in rekordbox | n/a | Missing | P3 |

## 27. Video, lyrics, lighting and DVS

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Video decks | Video with crossfader, auto transition, 33 transition FX, 24 touch FX, text, image and camera overlays, Syphon out, karaoke (CDG). | Video panel | Creative | Missing | P3 |
| Lyric | Synced lyrics; discontinued on 30 Nov 2024. | Removed | n/a | n/a | P3 |
| Lighting mode | DMX fixtures (512 addresses), scenes linked to phrases and mood, ambient mode, DMX direct control, SoundSwitch link. | LIGHTING mode | Core (FLX10 unlocks) | Missing | P3 |
| DVS | Control vinyl in absolute, relative or internal mode, calibration, Phase wireless. | Preferences > DVS | Core (DJM-A9/V10/S11 unlock; the FLX4 does not) | Missing | P3 |

## 28. MIDI, keyboard and controllers

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| DDJ-FLX4 support | Plug and play, lights included. | n/a | Free (FLX4) | Done (see the control-by-control table) | P0 |
| Other Pioneer controllers | DDJ-400, FLX6, FLX10, REV, 1000, SX, XDJ and CDJ HID control. | Controller support | Core or hardware unlock | Missing | P2 |
| Other brands | rekordbox supports only some; VirtualDJ and djay map hundreds. | n/a | n/a | Missing | P3 |
| MIDI Learn | Map any MIDI control: Button, Pad, Knob/Slider (7 or 14-bit), Rotary, Indicator, Value; import and export. | MIDI window | Core (FLX4 unlocks) | Missing (mappings are built in code) | P2 |
| Mapping conditions and modifiers | Traktor: up to 8 modifiers and 2 conditions per mapping. | Not in rekordbox | n/a | Missing | P3 |
| MIDI monitor | Not in rekordbox: shows each message and what it does. | Not in rekordbox | n/a | Done (CLI) | P3 |
| Keyboard control | Two shortcut presets (Performance 1 and 2), fully editable, export and import. | Preferences > Keyboard | Free | Partial: a fixed built-in set; no editor | P2 |
| Keyboard: decks | Play, cue, sync, pitch bend, memory cue call, hot cues, loops, beat jump, key lock, slip reverse per deck. | Preferences > Keyboard | Free | Partial: play, cue, pads, tempo, key lock, slip, 4-beat loop, tap, pad mode for decks A and B; no sync, memory cue, beat jump or bend keys | P2 |
| Keyboard: browser | Load to deck 1 or 2, undo load, locate the loaded track, info window, Tag List, search. | Preferences > Keyboard | Free | Partial: load, move, search; no undo load, locate or info | P2 |
| Controller lights | Buttons and pads lit by state. | n/a | Free (FLX4) | Done | P1 |
| Controller connect and reconnect | Works when plugged in before or after launch. | n/a | Free (FLX4) | Done | P0 |
| Single instance | Not in rekordbox in this form: a second launch never plays the music twice. | n/a | n/a | Done | P0 |

## 29. Preferences

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Preferences window | View, Audio, Analysis, Controller (Deck, Mixer, Effect, Sampler, Recordings, Mix Point Link, Others, device tab), DJ System, Extensions, Advanced, Keyboard, Cloud, Plan. | Preferences | Free | Partial: Settings lists music folders, rekordbox import and the sound card only | P1 |
| View: language and tooltips | UI language, tooltips. | Preferences > View | Free | Missing (English only) | P3 |
| View: layout toggles | Media browser sources, phrases, vocal, pad mode display, browser panel options. | Preferences > View > Layout | Free | Missing | P3 |
| Controller > Deck | Auto cue, load, eject/load lock, needle lock, sync type, hot cue options, jog options, auto beat loop length, vinyl speed. | Preferences > Controller > Deck | Free | Partial: load lock only | P1 |
| Controller > Effect | Release FX units, Smart CFX window, Merge FX. | Preferences > Controller > Effect | Free | Missing | P3 |
| Controller > Sampler | Slot count, overwrite lock, capture location. | Preferences > Controller > Sampler | Free | Missing | P3 |
| Advanced > Database | iTunes XML, rekordbox xml, auto relocate folders, database location. | Preferences > Advanced | Free | Partial: music folders and relink folder | P3 |
| Settings kept between versions | An older Dubplate keeps the settings a newer one wrote. | n/a | n/a | Done | P2 |

## 30. DDJ-FLX4, control by control

Every control in Pioneer's "DDJ-FLX4 Hardware Diagram for rekordbox", with and without SHIFT. All of them are unlocked by the FLX4 on rekordbox's Free plan.

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| PLAY/PAUSE | Play/pause | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| CUE | Set or call cue, back cue; with SHIFT: Jump to track start | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| Jog top | Scratch (vinyl mode on), pitch bend (vinyl off); with SHIFT: Search | FLX4 hardware diagram | Free (FLX4) | Done (vinyl mode is always on) | P0 |
| Jog side | Pitch bend; with SHIFT: Search | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| IN | Loop in; with a loop on, press then jog adjusts the loop in point | FLX4 hardware diagram | Free (FLX4) | Partial: no loop in adjust | P1 |
| OUT | Loop out; with a loop on, adjusts the loop out point | FLX4 hardware diagram | Free (FLX4) | Partial: OUT alone moves the loop end to the playhead; no jog adjust | P1 |
| 4 BEAT/EXIT | 4-beat loop on; loop exit when looping; with SHIFT: Active Loop on/off | FLX4 hardware diagram | Free (FLX4) | Partial: no Active Loop | P2 |
| CUE/LOOP CALL ◁ | Call previous cue or loop; halve while looping; with SHIFT: Delete cue or loop | FLX4 hardware diagram | Free (FLX4) | Partial: halve only | P0 |
| CUE/LOOP CALL ▷ | Call next cue or loop; double while looping; with SHIFT: Save cue or loop point | FLX4 hardware diagram | Free (FLX4) | Partial: double only | P0 |
| BEAT SYNC | Sync on/off; hold for tempo master; with SHIFT: Tempo range | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| TEMPO | Tempo | FLX4 hardware diagram | Free (FLX4) | Done (with pick-up) | P0 |
| FX CH SELECT lever | Beat FX channel 1, 2 or 1&2 | FLX4 hardware diagram | Free (FLX4) | In progress (1&2 means both decks; on main 1&2 means the master) | P0 |
| FX SELECT | Next Beat FX; with SHIFT: Previous Beat FX | FLX4 hardware diagram | Free (FLX4) | Done; picker popover in progress | P1 |
| BEAT ◁ | Shorter beat; with SHIFT: BPM auto mode | FLX4 hardware diagram | Free (FLX4) | Partial: SHIFT moves the FLX4 to another effect unit instead of AUTO/TAP BPM | P2 |
| BEAT ▷ | Longer beat; with SHIFT: BPM tap mode | FLX4 hardware diagram | Free (FLX4) | Partial: SHIFT moves the FLX4 to another effect unit instead of AUTO/TAP BPM | P2 |
| LEVEL/DEPTH | Beat FX parameter | FLX4 hardware diagram | Free (FLX4) | Partial: waits for pick-up; absolute from the first report in progress | P0 |
| BEAT FX ON/OFF | Beat FX on/off; with SHIFT: Release FX on | FLX4 hardware diagram | Free (FLX4) | Partial: no Release FX | P1 |
| MASTER LEVEL | Master volume | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| HEADPHONE CUE (MASTER) | Master in headphones | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| TRIM | Trim | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| EQ HI, MID, LOW | EQ | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| CFX | Sound Color FX or Smart CFX | FLX4 hardware diagram | Free (FLX4) | Done: filter, Sound Color FX per channel, or Smart CFX | P1 |
| HEADPHONE CUE (channel) | Channel cue; with SHIFT: Tap BPM | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| Channel fader | Volume or Smart Fader; with SHIFT: Channel fader start | FLX4 hardware diagram | Free (FLX4) | Partial: no fader start | P2 |
| Crossfader | Crossfader or Smart Fader; with SHIFT: Crossfader start | FLX4 hardware diagram | Free (FLX4) | Partial: no crossfader start | P2 |
| MIC LEVEL, MIC ATT. | Mic level and attenuator (hardware) | FLX4 hardware diagram | Free (FLX4) | Done in hardware | P3 |
| SMART CFX | On/off; with SHIFT: Preset selection | FLX4 hardware diagram | Free (FLX4) | Done | P1 |
| SMART FADER | On/off | FLX4 hardware diagram | Free (FLX4) | Done | P1 |
| HEADPHONES MIX, LEVEL | Cue and master balance, level | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| BROWSE turn | Scroll list or tree; with SHIFT: Zoom the enlarged waveforms | FLX4 hardware diagram | Free (FLX4) | Partial: no waveform zoom | P1 |
| BROWSE press | Move between list and tree; with SHIFT: Open a folder | FLX4 hardware diagram | Free (FLX4) | Partial: no folder open | P2 |
| LOAD (deck 1, deck 2) | Load; twice for instant doubles; with SHIFT: Deck 1: move into related tracks; deck 2: browse view | FLX4 hardware diagram | Free (FLX4) | Partial: SHIFT functions missing | P2 |
| Pad mode HOT CUE | Hot cue mode, SHIFT + pad deletes; with SHIFT: KEYBOARD mode | FLX4 hardware diagram | Free (FLX4) | Done (cues set live are not saved) | P0 |
| Pad mode PAD FX1 | Pad FX 1 (effects 1 to 8, SHIFT 9 to 16); with SHIFT: PAD FX2 (17 to 32) | FLX4 hardware diagram | Free (FLX4) | Partial: 32 fixed pad effects, not editable | P1 |
| Pad mode BEAT JUMP | Beat jump 1, 2, 4, 8 each way, SHIFT pads 7 and 8 change the size; with SHIFT: BEAT LOOP (1/4 to 32 beats) | FLX4 hardware diagram | Free (FLX4) | Done | P0 |
| Pad mode SAMPLER | Slots 1 to 8 left deck, 9 to 16 right deck; SHIFT stops or loads; with SHIFT: KEY SHIFT | FLX4 hardware diagram | Free (FLX4) | Done | P1 |
| KEYBOARD pads | Plays from the hot cue at +4 to +7 and ±0 to +3 semitones; SHIFT pads 7 and 8 shift the range | FLX4 hardware diagram | Free (FLX4) | Done | P2 |
| KEY SHIFT pads | Same layout, changes the key of the whole deck | FLX4 hardware diagram | Free (FLX4) | Done | P2 |
| Lights | Play, cue, sync, loop, pad mode and pad lights follow each deck's state. | FLX4 | Free (FLX4) | Done | P1 |
| Vinyl mode switch | Not on the FLX4: rekordbox sets it with a keyboard shortcut | Keyboard shortcut | Free (FLX4) | Missing | P1 |

## 31. Best of Traktor, Serato, VirtualDJ, djay and Engine DJ

Features rekordbox lacks, or does worse, that DJs cite when comparing apps. The "Where" column names the app that has it.

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| Stems prepared ahead and kept | VirtualDJ "Prepare saved stems" (about 4x the file size), Traktor stem files, Engine DJ pre-rendered stems; rekordbox separates live only. | VirtualDJ, Traktor, Engine DJ | n/a | Partial: separation runs and keeps files; no playback yet | P1 |
| Stem deck with per-stem filter and FX send | Traktor Stem Deck: 4 rows of volume, filter and FX send. | Traktor | n/a | Missing | P2 |
| Stem FX pads | Serato: pads 5 to 8 are vocal echo, instrumental echo, instrumental braker, drums echo. | Serato | n/a | Missing | P2 |
| StemSwap sampler | VirtualDJ: capture a stem from any track into a sampler slot and play it over another. | VirtualDJ | n/a | Missing | P3 |
| Flux mode | Traktor: a second playhead runs while you loop or juggle cues; same as rekordbox slip. | Traktor | n/a | Done (slip) | P2 |
| Freeze mode | Traktor: the next bars split into 8 slices on the pads. | Traktor | n/a | Missing | P3 |
| Effect chains | Traktor Group mode: 3 effects in a chain per unit, 4 units. | Traktor | n/a | Missing | P2 |
| Mixer FX menu | Traktor: pick 4 of 8 one-knob channel effects. | Traktor | n/a | Partial: Smart CFX presets | P2 |
| Ozone-style master limiter | Traktor: a maximiser with a gain-reduction meter. | Traktor | n/a | Done (look-ahead limiter; no meter on screen) | P2 |
| Timestamped history | Traktor and Serato: start time, duration and deck per played track. | Traktor, Serato | n/a | Done | P1 |
| History export as CSV, M3U or TXT | Serato history export for tracklists. | Serato | n/a | Partial: CSV and TXT; no M3U | P1 |
| Practice mode | Serato: two decks with the mouse and keyboard, no hardware. | Serato (rekordbox Performance mode needs a plan or hardware) | n/a | Done (keyboard and mouse decks without a controller) | P2 |
| Saved loops, 8 per track | Serato Loops tab, written to the file. | Serato | n/a | Missing | P1 |
| Cue names and colours written to file tags | Serato: cues survive moving apps. | Serato | n/a | Missing | P3 |
| Smart Crates | Serato: rule-based crates on added date, album, artist, BPM, comment, genre, key, label, plays, year. | Serato | n/a | Missing | P2 |
| Prepare window | Serato: a staging list; played tracks drop off. | Serato | n/a | Partial: the set and the backup list | P2 |
| Emoji or custom ratings | Serato 4: rating column with custom emoji (energy, mixability). | Serato | n/a | Partial: energy 1 to 10 | P3 |
| Crate footer | Serato 4: track count, total time and size. | Serato | n/a | Partial: set running time and count | P3 |
| Sandbox | VirtualDJ: rehearse the next transition privately while the master plays on. | VirtualDJ | n/a | Partial: seam audition | P2 |
| Points of interest | VirtualDJ POI: cue, saved loop with auto-trigger, action, automix start and end, remix point, load point, marker. | VirtualDJ | n/a | Partial: mix-in and mix-out cues per set track | P2 |
| Load point | VirtualDJ: a per-track position where it parks on load, skipping a long intro. | VirtualDJ | n/a | Partial: set tracks load at the mix-in | P1 |
| Filter folders | VirtualDJ query language with BPM and key difference against the deck, play count, last played. | VirtualDJ | n/a | Partial: live BPM and key filters | P2 |
| Automix with smart transitions | VirtualDJ smart mix points; djay Automix AI with crossfader FX. | VirtualDJ, djay | n/a | Missing | P3 |
| Neural Mix crossfaders | djay: per-stem crossfaders, stem echo out, stem fade. | djay | n/a | Missing | P3 |
| Fluid beatgrid | djay and VirtualDJ: automatic variable tempo grids. | djay, VirtualDJ | n/a | Partial: rekordbox's tempo-changing grids play right | P3 |
| Import from other DJ apps | djay External Apps and Engine DJ Import Assistant read rekordbox, Serato and Traktor cues, loops and grids. | djay, Engine DJ | n/a | Partial: reads rekordbox; not Serato or Traktor | P2 |
| OneLibrary export from another app | djay 5.5 writes AlphaTheta's OneLibrary USB format for CDJ-3000X. | djay | n/a | Missing | P0 |
| CDJ export from another app | VirtualDJ writes Pioneer USBs with playlists and stems. | VirtualDJ | n/a | Missing | P0 |
| Match filter | Engine DJ: filters a playlist by BPM range, key and hide-played relative to the playing track. | Engine DJ | n/a | Done (BPM, Key fits and Hide played) | P1 |
| Hide played tracks | Engine DJ Match option; rekordbox only colours them. | Engine DJ | n/a | Done | P1 |
| Fader echo | Engine DJ: echo tail when a fader closes, one-shot option. | Engine DJ | n/a | Missing | P1 |
| Phase meter | Engine DJ, rekordbox: ahead or behind bar under the track. | Engine DJ, rekordbox | n/a | Missing | P1 |
| Track ratings on the player | Engine DJ 5.0 and CDJ-3000X rate tracks on the hardware. | Engine DJ | n/a | Missing | P3 |
| Cloud-shared analysis | Engine DJ: streamed tracks get BPM, key and grid from other users' analysis. | Engine DJ | n/a | Missing | P3 |
| Standalone play | Engine DJ and CDJs play without a laptop. | Engine DJ, CDJ | n/a | Missing (needs USB export) | P0 |
| Analyse on import with hardware connected | Serato 4 analyses while the controller is plugged in. | Serato | n/a | Done | P2 |

## 32. What DJs ask rekordbox for

Requests from the Pioneer DJ forums, Digital DJ Tips and other apps' communities, ranked by evidence. Reddit and the Pioneer forum pages could not be fetched directly, so the forum threads are cited from search results.

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| No subscription paywall | Every rekordbox paid feature free; the strongest complaint in press and reviews. | Plans | n/a | Done (free, GPL) | P0 |
| Stem quality and saving | Stems as good as Serato and VirtualDJ, kept between sessions and exportable. | STEMS | Free | Partial | P1 |
| Tracklist export with timestamps | History with times, for Mixcloud and YouTube; split or marked recordings. | Histories | Free | Partial: TXT and CSV tracklists with times into the mix; nothing beside recordings yet | P1 |
| Import from Serato and Traktor with cues | Move a collection with cues, loops and grids. | Not in rekordbox | n/a | Missing | P2 |
| Open interchange format | One format for grids, cues, loops, keys and playlists; AlphaTheta's answer is OneLibrary. | OneLibrary | Free | Partial: reads rekordbox's database and ANLZ, writes rekordbox XML | P1 |
| Nested AND/OR smart playlists | Grouped rules and an "in playlist" condition. | Intelligent playlists | Free | Missing | P2 |
| Automatic cue points | N colour-coded cues per section type, like Mixed In Key; partly shipped as Intelligent Cue Creation. | CUE Analysis | Core | Partial: proposed mix cues | P1 |
| Flexible beatgrids and BPM range | Manual grid markers and a BPM range so amapiano at 108 to 115 is not doubled. | Analysis | Free | Missing | P1 |
| Duplicate merge with cues | Merge copies keeping cues and grids. | Duplicated File Manager | Free | Missing | P2 |
| Multiple libraries and NAS | Separate libraries, network shares. | Not in rekordbox | n/a | Missing | P3 |
| Keep folder structure on export | Nested playlists land in the same folders on the stick. | Not in rekordbox | n/a | Missing | P2 |
| Camelot or Open Key on CDJs | Key notation choice on players. | DJ System > General | Free | Missing | P2 |
| Read every tag on import | Fill all fields from the file. | Import | Free | Partial | P2 |
| Multi-genre split | Separate genres in one tag. | Not in rekordbox | n/a | Missing | P3 |
| 24-bit or AIFF recording | Recording format choice. | Recording | Core | Partial: 24-bit WAV | P3 |
| Non-destructive edits | Cut a breakdown without a new file, like Serato Flip. | EDIT mode renders files | Creative | Missing | P3 |
| Disable the deck limiter | Turn off each deck's limiter. | Not in rekordbox | n/a | Done (no per-deck limiter; only master and headphones) | P3 |
| Mono split cue | Cue left, master right, for laptop-only setups. | Not in rekordbox | n/a | Missing | P3 |
| Multi-display | Library and decks on two screens. | Not in rekordbox | n/a | Missing | P3 |

## 33. Afro house and amapiano needs

| Feature | What it does | Where in rekordbox | Tier | Dubplate | Priority |
| --- | --- | --- | --- | --- | --- |
| BPM range for 108 to 124 | Amapiano sits at 108 to 115 BPM (Pioneer DJ's own blog), afro house at 118 to 124; analysis must not halve or double them. | Analysis BPM range | Free | Missing (no range setting; grid confirm and nudge fix errors) | P1 |
| Grid fixes on swung amapiano | Off-by-a-beat downbeats and drifting grids are common. | Grid edit | Free | Partial: track check nudge and downbeat; not on the decks | P0 |
| Long blends of 16 to 32 bars | Phrase markers every 16 and 32 bars, a bar countdown, long-blend tempo help. | Phrases, beat countdown | Free | Partial: phrases shown in the track check; Smart Fader glides tempo; no countdown | P1 |
| Bass swap | Swap the log drum and bass between decks during the blend. | EQ, Smart Fader | Free (FLX4) | Done (isolator kill, Smart Fader swap) | P0 |
| Echo, fade and return | Drench the outgoing track in echo, drop the fader, bring it back on the one. | Beat FX, Release FX | Free (FLX4) | Partial: echo works; no fader echo or release echo | P1 |
| Stem split that keeps the log drum | A bass stem that holds the log drum's body; drums that hold its attack. | STEMS 4-stem | Free | Partial: Demucs 4 stems tuned for this; not playable yet | P2 |
| Vocal clash warnings | Chants and call-and-response vocals meeting in a blend. | Vocal detection | Free | Partial: the warning exists but vocal tags are never set | P2 |
| Key mixing | Same key, one step on the wheel, relative. | Key display, Traffic Light | Free | Done (Key fits, seam key moves, suggestions) | P0 |
| Tempo honesty | Show original BPM and pitch % so tracks are not pushed too fast. | Deck | Free (FLX4) | Done | P1 |
| Lossless files | WAV, AIFF and FLAC from Beatport, Traxsource and Bandcamp; FLAC on CDJs. | Formats | Free | Done for playback; export missing | P1 |
| Long intros | Load at the first downbeat or a load point, skipping silence. | Auto Cue | Free | Partial: set tracks only | P1 |

## 34. What Dubplate has that rekordbox does not

These are listed so the inventory is complete; none of them is a gap.

| Feature | What it does | Dubplate |
| --- | --- | --- |
| Set preparation | An ordered set beside the library in Prepare, with backups, running time against a target and undo. | Done |
| Seam checks | Each transition shows the key move, the tempo step and warnings: key clash, tempo gap over 6%, vocals meeting, energy drop, same track twice. Planned or Free per seam. | Done |
| Ready badge | Each set track says Ready or what is missing: grid unconfirmed, no mix-in or mix-out cue, file missing, key unknown. | Done |
| Proposed mix cues | Mix-in and mix-out proposed from rekordbox's phrases or the grid; Y accepts. | Done |
| Next suggestions with reasons | Five next tracks with "One step on the wheel · +1.6% · energy +2", steered by the set's energy target; Option+1 to 5 inserts. | Done |
| Energy rating 1 to 10 | Every track rated in the background and cached; the set's energy curve drawn against a target. | Done |
| Seam audition | Hears 20 seconds of a planned transition in the headphones only, from the outgoing mix-out to the incoming mix-in at the right tempo. | Done |
| Grid confirm and nudge | Confirm a grid, nudge it 5 ms, move the downbeat a beat; the fix follows the track onto the decks and into the XML export. | Done |
| Folder tracks analysed | Tracks outside rekordbox get BPM, beat grid with downbeats, key and energy from one decode. | Done |
| Read rekordbox without changing it | Reads a copy of the SQLCipher master.db and the ANLZ files while rekordbox stays open. | Done |
| Set export to rekordbox XML | The set and backups as playlists, with grids, mix cues as memory cues and the existing cues. | Done |
| Relink without touching rekordbox | Finds moved files by name, size and length; relinks live in Dubplate's settings. | Done |
| Smart CFX on any filter knob | Six presets driving the filter and a second effect from one knob. | Done |
| Look-ahead limiter on master and headphones | No sample passes -0.3 dBFS. | Done |
| Stems ahead of time | Demucs separation of set tracks into four stems while the DJ prepares, kept in Dubplate's folder. | Partial: separation runs; no deck controls yet |
| Restore on relaunch | Reopens on the same screen and levels; never louder than it closed. | Done |
| Command line | Inspect the rekordbox library, analyse folders, export sets, monitor MIDI. | Done |

## Sources

Read on 22 November 2026. Reddit and the Pioneer DJ forum pages refused automated access, so forum threads are cited from search results of their exact addresses, which shows each request exists but not how many votes it has. manual.rekordbox.com did not respond; the PDF manual was used instead.

### rekordbox and AlphaTheta

- [rekordbox 7.2.18 Instruction Manual (PDF)](https://cdn.rekordbox.com/files/20260807093645/rekordbox7.2.18_manual_EN.pdf)
- [rekordbox plans](https://rekordbox.com/en/plan/) and [Hardware Unlock](https://rekordbox.com/en/support/hardware-unlock/)
- [rekordbox feature overview](https://rekordbox.com/en/feature/overview/)
- Release notes, [page 1](https://rekordbox.com/en/support/releasenote/) and [page 2](https://rekordbox.com/en/support/releasenote/page/2), with [7.2.14](https://rekordbox.com/en/2026/04/rekordbox-v7214-release-information/), [7.1.4](https://rekordbox.com/en/2025/07/rekordbox-v714-release-information/) and [6.6.6](https://rekordbox.com/en/2022/11/rekordbox-v666-release-information) release information
- FAQ: [v7](https://rekordbox.com/en/support/faq/v7/), [OneLibrary](https://rekordbox.com/en/support/faq/onelibrary-7/), [Device Library Plus](https://rekordbox.com/en/support/faq/devicelibraryplus-6/), [library](https://rekordbox.com/en/support/faq/library-7/), [operation hints](https://rekordbox.com/en/support/faq/operation-hints-7/), [EDIT mode](https://rekordbox.com/en/support/faq/edit-mode-7/), [Lyric](https://rekordbox.com/en/support/faq/lyric-6/), [CloudDirectPlay](https://rekordbox.com/en/support/faq/clouddirect-7/), [cloud](https://rekordbox.com/en/support/faq/cloud-7/), [streaming](https://rekordbox.com/en/support/faq/streaming-7/), [library sync](https://rekordbox.com/en/support/faq/library-sync-7/), [device library backup](https://rekordbox.com/en/support/faq/device-library-backup-7/), [collaborative playlist](https://rekordbox.com/en/support/faq/collaborative-playlist-7/), [plans](https://rekordbox.com/en/support/faq/plans-7/), [troubleshooting](https://rekordbox.com/en/support/faq/trouble-shooting-7/), [STEMS](https://rekordbox.com/en/support/faq/stems-7/)
- [USB export support page](https://rekordbox.com/en/support/usb-export/), [USB export guide (PDF)](https://cdn.rekordbox.com/files/20251021171528/USB_export_guide_en_251007.pdf), [OneLibrary-compatible USB export guide (PDF)](https://cdn.rekordbox.com/files/20260317132052/OneLibrary-Compatible-USB-Device-Export-.pdf)
- Operation guides: [Phrase Edit](https://cdn.rekordbox.com/files/20241203210634/rekordbox7.0.5_Phrase_Edit_operation_guide_EN.pdf), [Cloud Library Sync](https://cdn.rekordbox.com/files/20251202174725/rekordbox7.2.8_cloud_library_sync_operation_guide_EN.pdf), [CloudDirectPlay](https://cdn.rekordbox.com/files/20250908133738/rekordbox7.2.2_CloudDirectPlay_EN.pdf), [streaming services](https://cdn.rekordbox.com/files/20261021112320/rekordbox7.2.19_streaming_service_usage_guide_EN.pdf), [PRO DJ LINK setup](https://cdn.rekordbox.com/files/20251117092919/PRODJLINK_SetupGuide_ver2_en.pdf), [introduction](https://cdn.rekordbox.com/files/20260409151246/rekordbox7.2.14_introduction_EN.pdf), [default keyboard shortcuts](https://cdn.rekordbox.com/files/20241203185020/rekordbox7.0.5_default_keyboard_shortcut_reference_EN.pdf), [video](https://cdn.rekordbox.com/files/20241203185046/rekordbox7.0.5_video_operation_guide_EN.pdf), [MIDI Learn](https://cdn.rekordbox.com/files/20241203210623/rekordbox7.0.5_midi_learn_operation_guide_EN.pdf), [DVS](https://cdn.rekordbox.com/files/20260709200752/rekordbox7.2.16_dvs_setup_guide_EN.pdf), [lighting](https://cdn.rekordbox.com/files/20260825084250/rekordbox-7219_lighting_operation_guide_EN.pdf), [pad editor](https://cdn.rekordbox.com/files/20251202174704/rekordbox7.2.8_pad_editor_operation_guide_EN.pdf), [edit](https://cdn.rekordbox.com/files/20200918151433/rekordbox6.1.1_edit_operation_guide_EN.pdf)
- [Discontinuation of the Lyric feature](https://rekordbox.com/en/2024/08/discontinuation-of-rekordbox-lyric-feature/)
- [DDJ-FLX4 manual](https://docs.pioneerdj.com/Manuals/DDJ_FLX4_DRI1804A_manual/) and [DDJ-FLX4 hardware diagram for rekordbox (PDF)](https://www.pioneerdj.com/-/media/pioneerdj/software-info/controller/ddj-flx4/ddj-flx4_hardwarediagram_rekordbox_e1.pdf)
- [DJM-A9 manual](https://docs.pioneerdj.com/Manuals/DJM_A9_DRI1785A_manual/) ([PDF copy](https://cdn01.4wall.com/cms/rentals/files/f6835eb526b3d2.pdf)) and [DJM-V10-LF manual (PDF)](https://www.adorama.com/col/productManuals/PIDJMV10LF.pdf)
- [CDJ-3000 manual (PDF copy)](https://cdn01.4wall.com/cms/rentals/files/f6835e4eabc543.pdf) and [CDJ-3000X manual (PDF copy)](https://www.allfordj.ru/upload/iblock/2f5/rj1ljugu3d217lmafrfb3rw9m1il3rgx/rukovodstvo_polzovatelya_alphatheta_cdj_3000x.pdf)
- [DJ TechTools forum, echo freeze in rekordbox](https://forum.djtechtools.com/t/echo-freeze-effect-in-rekordbox-dj/74088)

### Traktor

- [Traktor Pro 4 manual (PDF)](https://native-instruments.com/fileadmin/ni_media/downloads/manuals/traktor/Traktor-Pro-4-Manual-English-170724.pdf), [Kontrol S4 MK3 manual](https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/traktor/TRAKTOR_KONTROL_S4_MK3_Manual_English_1218.pdf), [Kontrol D2 manual](https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/traktor/traktor_kontrol_d2_manual_english.pdf)
- [What's new in Traktor 4.5](https://support.native-instruments.com/support/solutions/articles/69000879414-what-s-new-in-traktor-pro-play-4-5), [update status thread](https://community.native-instruments.com/discussion/43/official-update-status-traktor-pro-4-current-version-4-4-1), [compare page](https://www.native-instruments.com/jp/products/traktor/dj-software/traktor-pro-4/compare)
- [Digital DJ Tips, Traktor Pro 4 review](https://www.digitaldjtips.com/reviews/traktor-pro-4-review/)
- Community requests: [timestamped history](https://community.native-instruments.com/discussion/51279/timestamped-history-mix-recorder-performance-logs), [library waveforms like rekordbox](https://community.native-instruments.com/discussion/47020/waveform-view-in-library-like-on-rekordbox), [waveform previews with hot cues](https://community.native-instruments.com/discussion/585/feature-request-add-waveform-previews-with-hotcues-in-browser), [Smartlist AND/OR](https://community.native-instruments.com/discussion/comment/59453), [Smartlist conditions](https://community.native-instruments.com/discussion/comment/58678), [active loop cue](https://community.native-instruments.com/discussion/12439/active-loop-cue-point-that-automatically-activates)

### Serato

- [Serato DJ Pro](https://serato.com/dj/pro), [Stems](https://serato.com/dj/pro/stems), [Flip](https://serato.com/dj/pro/expansions/flip), [4.0.0 release notes](https://serato.com/dj/pro/downloads/4.0.0), [3.1.0 release notes](https://serato.com/dj/pro/downloads/3.1.0/releasenotes), [3.3.0 release notes](https://serato.com/dj/pro/downloads/3.3.0/releasenotes), [expansions](https://serato.com/dj/pricing/expansions), [upgrade page](https://serato.com/upgrade)
- Support: [Practice mode](https://support.serato.com/hc/en-us/articles/360001274635-Practice-mode), [Smart Crates](https://support.serato.com/hc/en-us/articles/360001447715-Library-Smart-Crates), [Scratch Banks](https://support.serato.com/hc/en-us/articles/360002234676-Using-Scratch-Banks-with-Serato-DJ-Pro), [Learn: Smart Crates](https://serato.com/latest/blog/17881/learn-smart-crates)
- [Magnetic Mag, Serato DJ 4.0 library](https://magneticmag.com/2025/11/serato-dj-4-0-arrives-with-a-cleaner-faster-library-experience/), [Digital DJ Tips, Serato DJ Pro launch](https://www.digitaldjtips.com/serato-dj-pro-serato-dj-lite-launched/), [Digital DJ Tips, Scratch Bank on Rane](https://www.digitaldjtips.com/scratch-bank-rane/), [We Are Crossfader, hot cues guide](https://wearecrossfader.co.uk/?p=227220), [Pioneer DJ, DDJ-XP2 pad modes](https://www.pioneerdj.com/en/product/features/controller/ddj-xp2-16-pad-modes)

### VirtualDJ

- Manual: [Sandbox](https://virtualdj.com/manuals/virtualdj/interface/topsection/appcontrols/sandbox.html), [Stems 2.0](https://virtualdj.com/manuals/virtualdj/interface/stems2.html), [POI editor](https://virtualdj.com/manuals/virtualdj/editors/poieditor.html), [Automix](https://virtualdj.com/manuals/virtualdj/interface/browser/sideview/automix.html), [filter folder syntax](https://virtualdj.com/manuals/virtualdj/appendix/filtersyntax.html), [SideView](https://virtualdj.com/manuals/virtualdj/interface/browser/sideview.html), [VDJscript verbs](https://virtualdj.com/manuals/virtualdj/appendix/vdjscriptverbs.html), [manual index](https://www.virtualdj.com/manuals/virtualdj/)
- [Digital DJ Tips, VirtualDJ 2026](https://www.digitaldjtips.com/virtualdj-2026/), [fluid beatgridding](https://www.digitaldjtips.com/virtualdj-2026-brings-fluid-beatgridding-ai-video-visuals/), [Gearnews, VirtualDJ 2025](https://www.gearnews.com/atomix-virtualdj-2025), [DJ Mag, VirtualDJ CDJ export](https://djmag.com/news/new-virtualdj-adds-cdj-export-including-playlists-and-stems), [stems export request](https://www.virtualdj.com/forums/249423/Wishes_and_new_features/Stems_custom_export.html)

### djay

- [djay Pro for Mac](https://www.algoriddim.com/djay-pro-mac), [release notes](https://www.algoriddim.com/djay-pro-mac/releasenotes), [Crossfader FX manual](https://help.algoriddim.com/user-manual/djay-pro-mac/dj-tools/effects/crossfader-fx)
- Community: [djay 6 dream features](https://community.algoriddim.com/t/djay-6-0-dream-features/41025?page=3), [Related Tracks request](https://community.algoriddim.com/t/full-blown-match-feature-like-related-tracks-in-rekordbox/23883), [auto-split recording](https://community.algoriddim.com/t/auto-split-file-size-tracks-while-recording/21996), [Philips Hue](https://community.algoriddim.com/t/philips-hue-zigbee-lights-integration/16184), [library waveform preview](https://community.algoriddim.com/t/waveform-preview-in-library/27328), [Automix Neural Mix](https://community.algoriddim.com/t/automix-neural-mix-transition-type-keeps-going-back-to-automatic/26199)
- [Digital DJ Tips, Neural Mix 4 stems](https://www.digitaldjtips.com/better-neural-mix-djay-pro-ai/), [We Are Crossfader, djay 5.3](https://wearecrossfader.co.uk/blog/djay-5-3-update/), [MusicTech, AudioShake stems](https://www.musictech.com/news/gear/algoriddim-free-update-djay-pro-improved-stem-separation-audioshake/)

### Engine DJ

- [Engine DJ 5.0 release notes](https://support.numark.com/en/support/solutions/articles/69000878658-engine-dj-5-0-release-notes), [4.3](https://support.denondj.com/en/support/solutions/articles/69000866933-engine-dj-4-3-release-notes), [4.1](https://support.denondj.com/en/support/solutions/articles/69000861060-engine-dj-4-1-release-notes), [stems in 4.2](https://enginedj.com/news/articles/engine-dj-stems-now-available-in-version-4-2), [supported streaming services](https://support.denondj.com/en/support/solutions/articles/69000867689-engine-dj-hardware-supported-streaming-services), [Denon DJ players](https://www.denondj.com/products/dj-players)
- [Digital DJ Tips, Engine DJ 3.3](https://www.digitaldjtips.com/engine-dj-3-3-brings-user-profiles-track-recommendation-new-fx-more/), [DJ Mag, Dropbox on Denon DJ](https://djmag.com/news/you-can-now-play-dropbox-denon-dj-hardware), [Bonedo, Engine OS 1.6](https://www.bonedo.de/artikel/denon-dj-bringt-prime-software-update-und-engine-os-16-mit-beatsource-dropbox-flexiblen-beatgrids-und-grid-import)
- Community: [no NAS support](https://community.enginedj.com/t/enginedj-3-0-still-no-nas-support/47569), [playlists to Mixcloud tracklists](https://community.enginedj.com/t/i-made-a-website-that-converts-engine-dj-playlists-to-mixcloud-tracklists/48978)

### Press and community wishlists

- Digital DJ Tips: [DJ software in 2026](https://www.digitaldjtips.com/?p=2795710), [2024 wishlist](https://www.digitaldjtips.com/dj-gear-software-wishlist-2024/), [rekordbox 7.2.8](https://www.digitaldjtips.com/rekordbox-v7-2-8-adds-4-stem-separation-new-cloud-setup-wizard/), [rekordbox 7 launch](https://www.digitaldjtips.com/rekordbox-7-launched/), [Smart Crates with play count](https://digitaldjtips.com/?p=2754662)
- [CDM, rekordbox 7](https://cdm.link/rekordbox-7/), [MusicRadar, rekordbox 7](https://musicradar.com/news/rekordbox-7-dj-software)
- Pioneer DJ community requests: [advanced smart playlist logic](https://community.pioneerdj.com/hc/en-us/community/posts/48791114720793/comments/48867835835289), [nested playlists](https://community.pioneerdj.com/hc/en-us/community/posts/22976429773465-Nested-Playlists), [smart playlist "in playlist"](https://forums.pioneerdj.com/hc/fr/community/posts/360048239711/comments/360007928932), [track marks in recordings](https://community.pioneerdj.com/hc/en-us/community/posts/22978590315417-Recording-sets-mixes-in-Rekordbox-and-adding-track-marks), [XDJ-RX3 history export](https://community.pioneerdj.com/hc/en-us/community/posts/22977276260761-XDJ-RX3-Export-actual-played-history-file-to-USB-Stick-when-Recording-Session-is-stopped), [recording format](https://community.pioneerdj.com/hc/en-us/community/posts/22977276165145-Rekordbox-Recordings-File-Format-and-Bit-Depth), [playlist-specific cues](https://community.pioneerdj.com/hc/en-us/community/posts/46494936012441-Feature-Request-Playlist-Specific-Memory-Hot-Cues), [duplicates](https://forums.pioneerdj.com/hc/de/community/posts/13910407688857/comments/14208009007001), [multiple libraries](https://forums.pioneerdj.com/hc/fr/community/posts/206749103/comments/209517046), [NAS](https://forums.pioneerdj.com/hc/es/community/posts/360046944131/comments/360006011691), [multi-select playlists](https://community.pioneerdj.com/hc/en-us/community/posts/22979049766937/comments/22990672451097), [keep folders on export](https://forums.pioneerdj.com/hc/fr/community/posts/360048845931/comments/360007230791), [stems on Pioneer equipment](https://community.pioneerdj.com/hc/en-us/community/posts/22979177581977/comments/22982888702745), [sample remix sets](https://community.pioneerdj.com/hc/en-us/community/posts/22980085813657-Sample-Remix-Sets-like-Ableton-and-Traktor), [Music app library](https://forums.pioneerdj.com/hc/fr/community/posts/360055666452/comments/900000736686), [key traffic light](https://community.pioneerdj.com/hc/en-us/community/posts/22977756639385/comments/22989272218137), [Camelot option](https://community.pioneerdj.com/hc/en-us/community/posts/22978797020441/comments/22986949490585), [tempo range](https://community.pioneerdj.com/hc/en-us/community/posts/22979603162009-Tempo-range), [70 BPM doubled](https://forums.pioneerdj.com/hc/en-us/community/posts/203062159/comments/203375209), [metadata sync](https://community.pioneerdj.com/hc/en-us/community/posts/22978722460057/comments/22986391243673), [automatic cue points](https://community.pioneerdj.com/hc/en-us/community/posts/22977465872281-Automatic-Cue-Point-placement-during-track-anaylisis), [improvements list](https://forums.pioneerdj.com/hc/fr/community/posts/360049253151-Some-improvements-to-the-rekordbox), [more suggestions](https://community.pioneerdj.com/hc/en-us/community/posts/29441298397465/comments/29441282594841), [advanced analysis](https://forums.pioneerdj.com/hc/de/community/posts/23398307563161-Rekordbox-Advanced-analysis-features), [disable deck limiter](https://community.pioneerdj.com/hc/en-us/community/posts/22976432628377-Rekordbox-an-Option-to-disable-the-limiter-compressor-of-each-deck), [multiple displays](https://community.pioneerdj.com/hc/en-us/community/posts/22977456149913-rekordbox-multiple-display-support-Export-Performance-Mode), [history from USB](https://community.pioneerdj.com/hc/en-us/community/posts/36986278156313-Create-a-history-file-when-using-a-USB-drive-in-Rekordbox-performance-mode), [import barrier](https://forums.pioneerdj.com/hc/de/community/posts/900003195163/comments/900001604906), [pull all tags](https://forums.pioneerdj.com/hc/de/community/posts/204560196/comments/205980206), [mono split](https://community.pioneerdj.com/hc/en-us/community/posts/22977774892441-Rekordbox-Mono-Split)

### Afro house, amapiano and other references

- [Pioneer DJ blog, mixing techniques behind every major genre](https://blog.pioneerdj.com/?p=6295)
- [Mixgraph, afro house mixing guide](https://www.mixgraph.io/mixing-guide/afro-house)
- [Native Instruments, afro house production 101](https://www.native-instruments.com/blogs/blog/afro-house-production-101) and [amapiano music](https://blog.native-instruments.com/amapiano-music/)
- [Splice, what is amapiano](https://splice.com/blog/what-is-amapiano-music/)
- [Pulse, DJ Maphorisa on amapiano tempo](https://www.pulse.ug/entertainment/music/dj-maphorisa-tells-nigerian-dj-how-to-play-amapiano-songs/9345h5y)
- [Beats To Rap On, making afro house](https://beatstorapon.com/blog/how-to-make-release-afro-house-music/)
- [StemSplit, rekordbox stems cannot be exported](https://stemsplit.io/blog/rekordbox-stem-separation)
- [Mixxx, export to rekordbox for CDJs](https://bugs.launchpad.net/mixxx/+bug/1796177) and [rekordbox export](https://bugs.launchpad.net/mixxx/+bug/1915639)
- [Deejayplaza, rekordbox review](https://www.deejayplaza.com/en/articles/rekordbox-review)
- [djay community, batch half or double BPM](https://community.algoriddim.com/t/batch-edit-half-or-double-time-bpm/43393) and [auto-generated hot cues](https://community.algoriddim.com/t/auto-generated-hot-cue-points-for-individual-songs-playlists/26749)
- [Engine community, BPM ranges](https://community.enginedj.com/t/possible-bpm-ranges/44791)

### Dubplate

- The code on `main` (apps/dubplate-desktop, apps/dubplate-cli and the crates), docs/guide.md, docs/engine.md, docs/cli.md, docs/specs/set-prep.md, and the titles and descriptions of the open and merged pull requests.
