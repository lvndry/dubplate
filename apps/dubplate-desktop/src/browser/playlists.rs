//! The DJ's editable playlist tree, beside the read-only library sources.
use super::*;
use dubplate_prep::{PlaylistKind, PlaylistTrack, PlaylistTree};

#[derive(Clone, Copy)]
enum Form {
    Create(Option<PlaylistId>, PlaylistKind),
    Rename(PlaylistId),
    Delete(PlaylistId),
}

pub(super) struct PlaylistPanel {
    pub(super) tree: Arc<PlaylistTree>,
    pub(super) selected: Option<PlaylistId>,
    pub(super) input: Entity<InputState>,
    form: Option<Form>,
    destination: Option<PlaylistId>,
}

impl PlaylistPanel {
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, Browser>) -> Self {
        Self {
            tree: Arc::default(),
            selected: None,
            form: None,
            destination: None,
            input: cx.new(|cx| InputState::new(window, cx).placeholder("Name")),
        }
    }
}

impl Browser {
    /// Installs the latest saved tree before the table is filtered.
    pub fn set_playlists(&mut self, tree: PlaylistTree, cx: &mut Context<'_, Self>) {
        if *self.playlists.tree == tree {
            return;
        }
        self.playlists.selected = self.playlists.selected.filter(|id| tree.get(*id).is_some());
        self.playlists.tree = Arc::new(tree);
        self.playlists.destination = self
            .playlists
            .destination
            .filter(|id| self.playlists.tree.get(*id).is_some());
        self.refilter(cx);
    }

    /// The tree used for exports, including entries outside current library sources.
    pub fn playlists(&self) -> &PlaylistTree {
        &self.playlists.tree
    }

    fn playlist_tracks(&self, tracks: &[TrackIndex]) -> Vec<PlaylistTrack> {
        let LibraryStatus::Ready(collection) = &self.library else {
            return Vec::new();
        };
        tracks
            .iter()
            .map(|index| {
                let entry = collection.track(*index);
                PlaylistTrack {
                    identity: entry.identity(),
                    track_path: entry.track.file_path.clone(),
                }
            })
            .collect()
    }

    /// Copies a rekordbox list, preserving its name and order.
    pub fn copy_playlist(&self, index: CrateIndex, cx: &mut Context<'_, Self>) {
        let LibraryStatus::Ready(collection) = &self.library else {
            return;
        };
        let Some((_, entry)) = collection.indexed_crates().nth(index.position()) else {
            return;
        };
        if entry.kind == CrateKind::Playlist {
            cx.emit(BrowserEvent::EditPlaylist(PlaylistEdit::Create {
                parent: None,
                name: entry.name.to_string(),
                kind: PlaylistKind::Playlist,
                tracks: self.playlist_tracks(&entry.tracks),
            }));
        }
    }

    /// Opens the keyboard's destination picker.
    pub fn pick_playlist(&mut self, cx: &mut Context<'_, Self>) {
        if self.selected_tracks().is_empty() {
            return;
        }
        let first = self.playlists.tree.playlists().next().map(|entry| entry.id);
        self.playlists.form = None;
        self.playlists.destination = first;
        if first.is_none() {
            cx.emit(BrowserEvent::PlaylistNotice(
                "Create a playlist in the sidebar first.",
            ));
        }
        cx.notify();
    }

    /// Handles selection and confirmation while picking a playlist.
    pub fn playlist_picker_command(
        &mut self,
        command: crate::commands::Command,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        use crate::commands::{Command, SetEdit};

        let Some(current) = self.playlists.destination else {
            return false;
        };
        match command {
            Command::MoveSelection(direction) => {
                let ids = self
                    .playlists
                    .tree
                    .playlists()
                    .map(|entry| entry.id)
                    .collect::<Vec<_>>();
                let Some(position) = ids.iter().position(|id| *id == current) else {
                    return true;
                };
                if let Some(next) = offset(
                    position,
                    match direction {
                        Direction::Up => -1,
                        Direction::Down => 1,
                    },
                    ids.len(),
                ) {
                    self.playlists.destination = Some(ids[next]);
                }
                cx.notify();
            }
            Command::EditSet(SetEdit::Add) => self.add_selection_to_playlist(current, cx),
            Command::Dismiss => {
                self.playlists.destination = None;
                cx.notify();
            }
            _ => return false,
        }
        true
    }

    pub(super) fn playlist_picker(&self, cx: &mut Context<'_, Self>) -> Option<impl IntoElement> {
        let id = self.playlists.destination?;
        let entry = self.playlists.tree.get(id)?;
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .px(theme::GUTTER)
                .py(px(4.0))
                .bg(theme::active().raised)
                .child(
                    div()
                        .text_color(theme::active().muted)
                        .child("Add to playlist · ↑/↓"),
                )
                .child(
                    button(
                        "add-to-picked-playlist",
                        playlist_label(&self.playlists.tree, entry),
                        Emphasis::Primary,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.add_selection_to_playlist(id, cx),
                    )),
                )
                .child(
                    button("cancel-playlist-picker", "Cancel", Emphasis::Secondary).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.playlists.destination = None;
                            cx.notify();
                        }),
                    ),
                ),
        )
    }

    /// Adds the current selection in its displayed order.
    pub fn add_selection_to_playlist(&mut self, id: PlaylistId, cx: &mut Context<'_, Self>) {
        self.add_playlist_tracks(id, &self.selected_tracks(), cx);
        self.playlists.destination = None;
        cx.notify();
    }

    fn add_playlist_tracks(
        &self,
        id: PlaylistId,
        indices: &[TrackIndex],
        cx: &mut Context<'_, Self>,
    ) {
        let tracks = self.playlist_tracks(indices);
        if !tracks.is_empty() {
            cx.emit(BrowserEvent::EditPlaylist(PlaylistEdit::AddTracks {
                playlist: id,
                tracks,
            }));
        }
    }

    pub(super) fn reorder_playlist(
        &self,
        id: PlaylistId,
        drag: TrackIndex,
        target: TrackIndex,
        cx: &mut Context<'_, Self>,
    ) {
        let LibraryStatus::Ready(collection) = &self.library else {
            return;
        };
        let Some(entry) = self.playlists.tree.get(id) else {
            return;
        };
        if drag == target {
            return;
        }
        let Some(position) = entry
            .tracks
            .iter()
            .position(|track| playlist_track_index(collection, track) == Some(target))
        else {
            return;
        };
        let Some(moved) = entry
            .tracks
            .iter()
            .find(|track| playlist_track_index(collection, track) == Some(drag))
        else {
            return;
        };
        cx.emit(BrowserEvent::EditPlaylist(PlaylistEdit::MoveTrack {
            playlist: id,
            track: moved.identity.clone(),
            position,
        }));
    }

    /// Removes selected rows when browsing a writable playlist.
    pub fn remove_playlist_selection(&self, cx: &mut Context<'_, Self>) -> bool {
        let Some(playlist) = self
            .writable_playlist()
            .and_then(|id| self.playlists.tree.get(id))
        else {
            return false;
        };
        if self.pane == BrowserPane::Tracks
            && let LibraryStatus::Ready(collection) = &self.library
        {
            let tracks = playlist
                .tracks
                .iter()
                .filter(|track| {
                    playlist_track_index(collection, track)
                        .is_some_and(|index| self.selection.contains(index))
                })
                .map(|track| track.identity.clone())
                .collect::<Vec<_>>();
            if !tracks.is_empty() {
                cx.emit(BrowserEvent::EditPlaylist(PlaylistEdit::RemoveTracks {
                    playlist: playlist.id,
                    tracks,
                }));
            }
        }
        true
    }

    pub(super) fn writable_playlist(&self) -> Option<PlaylistId> {
        self.playlists.selected.filter(|id| {
            self.playlists
                .tree
                .get(*id)
                .is_some_and(|entry| entry.kind == PlaylistKind::Playlist)
        })
    }

    pub(super) fn playlist_rows(&self, id: PlaylistId, collection: &Collection) -> Vec<TrackIndex> {
        let mut seen = HashSet::new();
        self.playlists
            .tree
            .tracks(id)
            .into_iter()
            .filter_map(|track| playlist_track_index(collection, track))
            .filter(|index| {
                seen.insert(*index) && self.query.matches(&collection.track(*index).search_text)
            })
            .collect()
    }

    fn open_form(&mut self, form: Form, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.playlists.form = Some(form);
        self.playlists.destination = None;
        let value = match form {
            Form::Rename(id) => self.playlists.tree.get(id).map(|e| e.name.clone()),
            Form::Create(..) => Some(String::new()),
            _ => None,
        };
        if let Some(value) = value {
            self.playlists.input.update(cx, |input, cx| {
                input.set_value(value, window, cx);
                input.focus(window, cx)
            });
        }
        cx.notify();
    }

    pub(super) fn save_playlist_name(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let name = self.playlists.input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let edit = match self.playlists.form {
            Some(Form::Create(parent, kind)) => PlaylistEdit::Create {
                parent,
                name,
                kind,
                tracks: Vec::new(),
            },
            Some(Form::Rename(playlist)) => PlaylistEdit::Rename { playlist, name },
            _ => return,
        };
        self.playlists.form = None;
        window.focus(&self.table_focus, cx);
        cx.emit(BrowserEvent::EditPlaylist(edit));
        cx.notify();
    }

    pub(super) fn clear_playlist(&mut self) {
        self.playlists.selected = None;
        self.playlists.form = None;
        self.playlists.destination = None;
    }

    pub(super) fn choose_playlist(&mut self, id: PlaylistId, cx: &mut Context<'_, Self>) {
        self.playlists.selected = Some(id);
        self.history_page = None;
        self.pane = BrowserPane::Crates;
        self.playlists.form = None;
        self.playlists.destination = None;
        self.refilter(cx);
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    pub(super) fn playlist_sidebar_controls(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let selected = self
            .playlists
            .selected
            .and_then(|id| self.playlists.tree.get(id));
        let parent = selected.and_then(|entry| {
            if entry.kind == PlaylistKind::Folder {
                Some(entry.id)
            } else {
                entry.parent
            }
        });
        let mut panel = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .mt(px(8.))
            .child("Dubplate playlists");
        panel = panel.child(
            div().flex().gap(px(3.)).children(
                [
                    ("Playlist", PlaylistKind::Playlist),
                    ("Folder", PlaylistKind::Folder),
                ]
                .map(|(label, kind)| {
                    button(
                        ("new-playlist-kind", kind as usize),
                        format!("+ {label}"),
                        Emphasis::Secondary,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, win, cx| {
                            this.open_form(Form::Create(parent, kind), win, cx)
                        },
                    ))
                }),
            ),
        );
        if let Some(entry) = selected {
            let id = entry.id;
            panel = panel.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(3.))
                    .children(
                        [("Rename", Form::Rename(id)), ("Delete…", Form::Delete(id))].map(
                            |(label, form)| {
                                button(label, label, Emphasis::Secondary).on_click(cx.listener(
                                    move |this, _: &ClickEvent, win, cx| {
                                        this.open_form(form, win, cx)
                                    },
                                ))
                            },
                        ),
                    )
                    .children([("XML…", false), ("USB…", true)].map(|(label, usb)| {
                        button(label, label, Emphasis::Secondary).on_click(cx.listener(
                            move |_, _: &ClickEvent, _, cx| {
                                cx.emit(BrowserEvent::ExportPlaylist(id, usb))
                            },
                        ))
                    })),
            );
        }
        if let Some(form) = self.playlists.form {
            panel = match form {
                Form::Create(..) | Form::Rename(_) => {
                    panel.child(Input::new(&self.playlists.input)).child(
                        button("save-playlist", "Save name", Emphasis::Primary).on_click(
                            cx.listener(|this, _: &ClickEvent, win, cx| {
                                this.save_playlist_name(win, cx)
                            }),
                        ),
                    )
                }
                Form::Delete(id) => panel
                    .child("Delete this playlist or folder and its contents?")
                    .child(
                        button("delete-playlist", "Confirm deletion", Emphasis::Primary).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.playlists.form = None;
                                cx.emit(BrowserEvent::EditPlaylist(PlaylistEdit::Delete(id)));
                                cx.notify()
                            }),
                        ),
                    ),
            }
            .child(
                button("cancel-playlist", "Cancel", Emphasis::Secondary).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.playlists.form = None;
                        cx.notify()
                    },
                )),
            );
        }
        panel
    }

    pub(super) fn playlist_sidebar_rows<'a>(
        &'a self,
        cx: &'a mut Context<'_, Self>,
    ) -> impl Iterator<Item = gpui::Stateful<gpui::Div>> + 'a {
        self.playlists.tree.entries().iter().map(move |entry| {
            let id = entry.id;
            let is_list = entry.kind == PlaylistKind::Playlist;
            let chosen = self.playlists.selected == Some(id);
            let label = if is_list {
                entry.tracks.len().to_string()
            } else {
                "Folder".into()
            };
            sidebar_row(
                SharedString::from(format!("dubplate-list-{id}")),
                entry.name.clone().into(),
                label.into(),
                entry.depth,
                chosen,
                chosen && self.pane == BrowserPane::Crates,
                RowExtras::default(),
            )
            .mt(px(1.0))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if this.playlists.destination.is_some() && is_list {
                    this.add_selection_to_playlist(id, cx)
                } else {
                    this.choose_playlist(id, cx)
                }
            }))
            .when(is_list, |row| {
                row.drag_over::<DraggedTrack>(|style, _, _, _| style.bg(theme::active().raised))
                    .on_drop(cx.listener(move |this, drag: &DraggedTrack, _, cx| {
                        let tracks = if this.selection.contains(drag.track) {
                            this.selected_tracks()
                        } else {
                            vec![drag.track]
                        };
                        this.add_playlist_tracks(id, &tracks, cx);
                    }))
            })
        })
    }
}

fn playlist_track_index(collection: &Collection, track: &PlaylistTrack) -> Option<TrackIndex> {
    collection.find(&track.identity).or_else(|| {
        track
            .track_path
            .as_deref()
            .and_then(|path| collection.find_path(path))
    })
}

pub(super) fn playlist_label(tree: &PlaylistTree, playlist: &dubplate_prep::Playlist) -> String {
    let mut names = vec![playlist.name.as_str()];
    let mut parent = playlist.parent.and_then(|id| tree.get(id));
    while let Some(folder) = parent {
        names.push(folder.name.as_str());
        parent = folder.parent.and_then(|id| tree.get(id));
    }
    names.reverse();
    names.join(" / ")
}
