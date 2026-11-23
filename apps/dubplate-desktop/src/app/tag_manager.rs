//! The My Tag manager, over the window: make, rename, reorder and delete the DJ's categories
//! and tags. Every change goes to the set worker as a [`TrackTagEdit`] and the panel redraws
//! from the metadata cache once it lands.

use dubplate_prep::{TagCategory, TagCategoryId, TrackTagEdit};
use gpui::component::input::Input;
use gpui::prelude::*;
use gpui::{ClickEvent, Context, FontWeight, MouseButton, SharedString, Window, div, px};

use super::DubplateApp;
use crate::commands::Direction;
use crate::tag_editing::{
    Deleting, Editing, commit, delete_edit, initial_text, move_category, move_tag, tag_count,
};
use crate::theme;
use crate::widgets::{Emphasis, button};

/// What the manager is doing: the name being typed, and the delete waiting to be confirmed.
#[derive(Debug, Clone, Default)]
pub(super) struct TagManager {
    editing: Option<Editing>,
    deleting: Option<Deleting>,
    problem: Option<&'static str>,
}

fn element_number(id: i64) -> usize {
    usize::try_from(id).unwrap_or_default()
}

impl DubplateApp {
    /// Opens the manager.
    pub(super) fn open_tag_manager(&mut self, cx: &mut Context<'_, Self>) {
        self.tag_manager = Some(TagManager::default());
        self.library_open = false;
        self.help_open = false;
        cx.notify();
    }

    /// Closes the manager, or only the name field when it is open.
    pub(super) fn dismiss_tag_manager(&mut self, cx: &mut Context<'_, Self>) {
        match &mut self.tag_manager {
            Some(manager) if manager.editing.is_some() || manager.deleting.is_some() => {
                manager.editing = None;
                manager.deleting = None;
                manager.problem = None;
            }
            _ => self.tag_manager = None,
        }
        cx.notify();
    }

    fn start_editing(&mut self, editing: Editing, window: &mut Window, cx: &mut Context<'_, Self>) {
        let text = initial_text(editing, self.track_metadata.categories());
        if let Some(manager) = &mut self.tag_manager {
            manager.editing = Some(editing);
            manager.deleting = None;
            manager.problem = None;
        }
        self.tag_name_input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// Saves the name in the field.
    pub(super) fn commit_tag_name(&mut self, cx: &mut Context<'_, Self>) {
        let Some(editing) = self
            .tag_manager
            .as_ref()
            .and_then(|manager| manager.editing)
        else {
            return;
        };
        let name = self.tag_name_input.read(cx).value().to_string();
        match commit(editing, &name, self.track_metadata.categories()) {
            Ok(edit) => {
                if let Some(manager) = &mut self.tag_manager {
                    manager.editing = None;
                    manager.problem = None;
                }
                self.store_tag_edit(edit, cx);
            }
            Err(problem) => {
                if let Some(manager) = &mut self.tag_manager {
                    manager.problem = Some(problem);
                }
                cx.notify();
            }
        }
    }

    fn store_tag_edit(&mut self, edit: TrackTagEdit, cx: &mut Context<'_, Self>) {
        self.queue_set_edit(
            move |editor| editor.edit_track_tags(edit).map(|_| ()),
            |_, (), _| {},
            cx,
        );
        cx.notify();
    }

    fn ask_to_delete(&mut self, deleting: Deleting, cx: &mut Context<'_, Self>) {
        if let Some(manager) = &mut self.tag_manager {
            manager.deleting = Some(deleting);
            manager.editing = None;
            manager.problem = None;
        }
        cx.notify();
    }

    fn confirm_delete(&mut self, cx: &mut Context<'_, Self>) {
        let Some(deleting) = self
            .tag_manager
            .as_mut()
            .and_then(|manager| manager.deleting.take())
        else {
            return;
        };
        self.store_tag_edit(delete_edit(deleting), cx);
    }

    fn cancel_delete(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(manager) = &mut self.tag_manager {
            manager.deleting = None;
        }
        cx.notify();
    }
}

pub(super) fn render(
    app: &DubplateApp,
    manager: &TagManager,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    let categories = app.track_metadata.categories();
    let editing_label = manager
        .editing
        .map(|editing| editing_label(editing, categories));
    div()
        .id("tag-manager")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::active().scrim)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.tag_manager = None;
            cx.notify();
        }))
        .child(
            div()
                .id("tag-manager-card")
                .role(gpui::Role::Dialog)
                .aria_label("My Tags")
                .key_context("Modal")
                .track_focus(&app.modal_focus)
                .tab_group()
                .tab_stop(false)
                .flex()
                .flex_col()
                .gap(px(12.0))
                .w(px(520.0))
                .max_h(px(620.0))
                .overflow_y_scroll()
                .p(px(20.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(theme::active().line)
                .bg(theme::active().panel)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::MEDIUM)
                                .child("My Tags"),
                        )
                        .child(
                            button("close-tag-manager", "Close", Emphasis::Secondary).on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.tag_manager = None;
                                    cx.notify();
                                }),
                            ),
                        ),
                )
                .when_some(editing_label, |card, label| {
                    card.child(name_field(app, label, manager.problem, cx))
                })
                .child(div().flex().child(
                    button("new-category", "New category", Emphasis::Primary).on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.start_editing(Editing::NewCategory, window, cx);
                        }),
                    ),
                ))
                .when(categories.is_empty(), |card| {
                    card.child(div().text_color(theme::active().muted).child(
                        "No tags yet. Make a category, such as Situation, then add tags to it.",
                    ))
                })
                .children(
                    categories
                        .iter()
                        .map(|category| category_block(category, categories, manager, cx)),
                ),
        )
}

fn editing_label(editing: Editing, categories: &[TagCategory]) -> SharedString {
    let category_name = |id: TagCategoryId| {
        categories
            .iter()
            .find(|category| category.id == id)
            .map(|category| category.name.clone())
            .unwrap_or_default()
    };
    match editing {
        Editing::NewCategory => "New category".into(),
        Editing::RenameCategory(id) => format!("Rename category {}", category_name(id)).into(),
        Editing::NewTag(id) => format!("New tag in {}", category_name(id)).into(),
        Editing::RenameTag(id) => {
            let name = initial_text(Editing::RenameTag(id), categories);
            format!("Rename tag {name}").into()
        }
    }
}

fn name_field(
    app: &DubplateApp,
    label: SharedString,
    problem: Option<&'static str>,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(10.0))
        .rounded(theme::CONTROL_RADIUS)
        .border_1()
        .border_color(theme::active().line)
        .child(
            div()
                .text_size(theme::SMALL_TEXT_SIZE)
                .text_color(theme::active().muted)
                .child(label),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px(px(8.0))
                        .rounded(theme::CONTROL_RADIUS)
                        .border_1()
                        .border_color(theme::active().line)
                        .bg(theme::active().background)
                        .child(Input::new(&app.tag_name_input).appearance(false)),
                )
                .child(
                    button("save-tag-name", "Save", Emphasis::Primary).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.commit_tag_name(cx)),
                    ),
                )
                .child(
                    button("cancel-tag-name", "Cancel", Emphasis::Secondary).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss_tag_manager(cx)),
                    ),
                ),
        )
        .when_some(problem, |field, problem| {
            field.child(
                div()
                    .text_size(theme::SMALL_TEXT_SIZE)
                    .text_color(theme::active().attention)
                    .child(problem),
            )
        })
}

fn category_block(
    category: &TagCategory,
    categories: &[TagCategory],
    manager: &TagManager,
    cx: &mut Context<'_, DubplateApp>,
) -> impl IntoElement + use<> {
    let id = category.id;
    let number = element_number(id.0);
    let name = category.name.clone();
    let deleting = manager.deleting == Some(Deleting::Category(id));
    let up = move_category(categories, id, Direction::Up).is_some();
    let down = move_category(categories, id, Direction::Down).is_some();
    let tags = category.tags.len();
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .pt(px(8.0))
        .border_t_1()
        .border_color(theme::active().row_line)
        .child(
            row_line(
                SharedString::from(name.clone()),
                true,
                RowButtons {
                    key: ("category", number),
                    name: name.clone(),
                    can_move_up: up,
                    can_move_down: down,
                    deleting,
                    delete_warning: format!(
                        "Delete {name} and its {tags} {}?",
                        if tags == 1 { "tag" } else { "tags" }
                    ),
                },
                cx,
                move |this, direction, cx| {
                    let edit = move_category(this.track_metadata.categories(), id, direction);
                    if let Some(edit) = edit {
                        this.store_tag_edit(edit, cx);
                    }
                },
                move |this, window, cx| this.start_editing(Editing::RenameCategory(id), window, cx),
                move |this, cx| this.ask_to_delete(Deleting::Category(id), cx),
            )
            .child(
                button(("add-tag", number), "Add tag", Emphasis::Secondary).on_click(cx.listener(
                    move |this, _: &ClickEvent, window, cx| {
                        this.start_editing(Editing::NewTag(id), window, cx);
                    },
                )),
            ),
        )
        .children(category.tags.iter().map(|tag| {
            let tag_id = tag.id;
            let tag_number = element_number(tag_id.0);
            let tag_name = tag.name.clone();
            div().pl(px(20.0)).child(row_line(
                SharedString::from(tag_name.clone()),
                false,
                RowButtons {
                    key: ("tag", tag_number),
                    name: tag_name.clone(),
                    can_move_up: move_tag(categories, tag_id, Direction::Up).is_some(),
                    can_move_down: move_tag(categories, tag_id, Direction::Down).is_some(),
                    deleting: manager.deleting == Some(Deleting::Tag(tag_id)),
                    delete_warning: format!("Delete {tag_name} from every track?"),
                },
                cx,
                move |this, direction, cx| {
                    let edit = move_tag(this.track_metadata.categories(), tag_id, direction);
                    if let Some(edit) = edit {
                        this.store_tag_edit(edit, cx);
                    }
                },
                move |this, window, cx| this.start_editing(Editing::RenameTag(tag_id), window, cx),
                move |this, cx| this.ask_to_delete(Deleting::Tag(tag_id), cx),
            ))
        }))
        .when(tag_count(categories, id) == 0, |block| {
            block.child(
                div()
                    .pl(px(20.0))
                    .text_size(theme::SMALL_TEXT_SIZE)
                    .text_color(theme::active().muted)
                    .child("No tags in this category yet."),
            )
        })
}

/// What a category's or tag's buttons need to know.
struct RowButtons {
    key: (&'static str, usize),
    name: String,
    can_move_up: bool,
    can_move_down: bool,
    deleting: bool,
    delete_warning: String,
}

fn row_line(
    label: SharedString,
    heading: bool,
    buttons: RowButtons,
    cx: &mut Context<'_, DubplateApp>,
    on_move: impl Fn(&mut DubplateApp, Direction, &mut Context<'_, DubplateApp>) + Clone + 'static,
    on_rename: impl Fn(&mut DubplateApp, &mut Window, &mut Context<'_, DubplateApp>) + 'static,
    on_delete: impl Fn(&mut DubplateApp, &mut Context<'_, DubplateApp>) + 'static,
) -> gpui::Div {
    let (kind, number) = buttons.key;
    let id = |action: &'static str| -> gpui::ElementId {
        (SharedString::from(format!("{kind}-{action}")), number).into()
    };
    let mover = |direction: Direction, arrow: &'static str, word: &'static str, enabled: bool| {
        let on_move = on_move.clone();
        button(
            id(if direction == Direction::Up {
                "up"
            } else {
                "down"
            }),
            arrow,
            Emphasis::Secondary,
        )
        .aria_label(format!("Move {} {word}", buttons.name))
        .when(!enabled, |step| step.opacity(0.35))
        .when(enabled, |step| {
            step.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                on_move(this, direction, cx);
            }))
        })
    };
    let up = mover(Direction::Up, "\u{2191}", "up", buttons.can_move_up);
    let down = mover(Direction::Down, "\u{2193}", "down", buttons.can_move_down);
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .flex_1()
                .min_w(px(140.0))
                .truncate()
                .when(heading, |name| name.font_weight(FontWeight::MEDIUM))
                .child(label),
        )
        .child(up)
        .child(down)
        .child(
            button(id("rename"), "Rename", Emphasis::Secondary).on_click(
                cx.listener(move |this, _: &ClickEvent, window, cx| on_rename(this, window, cx)),
            ),
        )
        .map(|line| {
            if buttons.deleting {
                line.child(
                    div()
                        .text_size(theme::SMALL_TEXT_SIZE)
                        .text_color(theme::active().attention)
                        .child(buttons.delete_warning),
                )
                .child(
                    button(id("confirm-delete"), "Delete", Emphasis::Primary).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.confirm_delete(cx)),
                    ),
                )
                .child(
                    button(id("keep"), "Keep", Emphasis::Secondary).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.cancel_delete(cx)),
                    ),
                )
            } else {
                line.child(
                    button(id("delete"), "Delete", Emphasis::Secondary).on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| on_delete(this, cx)),
                    ),
                )
            }
        })
}
