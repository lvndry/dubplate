//! Small controls shared by the views, drawn the Graphite way.

use crate::controls::Control;
use dubplate_engine::{DeckId, EqBand};
use gpui::prelude::*;
use gpui::{
    AnyView, App, Context, Div, ElementId, MouseButton, SharedString, Stateful, Window, div, px,
};

use crate::theme;

/// How much a button stands out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// The one thing to do next.
    Primary,
    /// Everything else.
    Secondary,
}

/// A text button. The caller adds what a click does.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    emphasis: Emphasis,
) -> Stateful<Div> {
    let label = label.into();
    let base = div()
        .id(id)
        .accessible_button(label.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(28.0))
        .px(px(10.0))
        .rounded(theme::CONTROL_RADIUS)
        .text_size(theme::SMALL_TEXT_SIZE)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label);
    match emphasis {
        Emphasis::Primary => base
            .bg(theme::TEXT)
            .text_color(theme::BACKGROUND)
            .font_weight(gpui::FontWeight::MEDIUM)
            .focus_visible(|style| {
                style.shadow(vec![
                    gpui::BoxShadow::new(px(0.0), px(0.0), theme::BACKGROUND.into())
                        .spread_radius(px(2.0))
                        .inset(),
                ])
            })
            .hover(|button| button.opacity(0.9)),
        Emphasis::Secondary => base
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .text_color(theme::TEXT)
            .hover(|button| button.bg(theme::RAISED)),
    }
}

/// Shared keyboard, focus and accessibility behavior for the application's drawn controls.
/// GPUI's `on_click` dispatches pointer, Enter/Space and assistive activation through one path.
pub trait InteractiveControl: Sized {
    /// Makes an action a named button and a stop in the natural tab order.
    fn accessible_button(self, label: impl Into<SharedString>) -> Self;
    /// A button whose on/off state is announced alongside its name.
    fn accessible_toggle(self, label: impl Into<SharedString>, on: bool) -> Self;
    /// A continuous control with a normalized value. Callers supply adjustment handlers.
    fn accessible_slider(self, label: impl Into<SharedString>, value: f32) -> Self;
}

impl InteractiveControl for Stateful<Div> {
    fn accessible_button(self, label: impl Into<SharedString>) -> Self {
        self.role(gpui::Role::Button)
            .aria_label(label)
            .key_context("Control")
            .tab_index(0)
            .focus_visible(|style| {
                style.shadow(vec![
                    gpui::BoxShadow::new(px(0.0), px(0.0), theme::TEXT.into())
                        .spread_radius(px(2.0))
                        .inset(),
                ])
            })
    }

    fn accessible_toggle(self, label: impl Into<SharedString>, on: bool) -> Self {
        self.accessible_button(label).aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
    }

    fn accessible_slider(self, label: impl Into<SharedString>, value: f32) -> Self {
        self.accessible_button(label)
            .role(gpui::Role::Slider)
            .aria_min_numeric_value(0.0)
            .aria_max_numeric_value(100.0)
            .aria_numeric_value(f64::from(value) * 100.0)
            .aria_numeric_value_step(1.0)
            .aria_value(format!("{:.1}% of range", value * 100.0))
    }
}

/// Full spoken name of a mixer control, including its deck when there is one.
pub fn control_label(control: Control) -> &'static str {
    match control {
        Control::ChannelFader(DeckId::A) => "Deck A channel level",
        Control::ChannelFader(DeckId::B) => "Deck B channel level",
        Control::Tempo(DeckId::A) => "Deck A tempo",
        Control::Tempo(DeckId::B) => "Deck B tempo",
        Control::Trim(DeckId::A) => "Deck A trim",
        Control::Trim(DeckId::B) => "Deck B trim",
        Control::Eq(DeckId::A, EqBand::Low) => "Deck A low EQ",
        Control::Eq(DeckId::B, EqBand::Low) => "Deck B low EQ",
        Control::Eq(DeckId::A, EqBand::Mid) => "Deck A mid EQ",
        Control::Eq(DeckId::B, EqBand::Mid) => "Deck B mid EQ",
        Control::Eq(DeckId::A, EqBand::High) => "Deck A high EQ",
        Control::Eq(DeckId::B, EqBand::High) => "Deck B high EQ",
        Control::Filter(DeckId::A) => "Deck A filter",
        Control::Filter(DeckId::B) => "Deck B filter",
        Control::Crossfader => "Crossfader",
        Control::MasterLevel => "Master level",
        Control::HeadphoneMix => "Headphone cue/master mix",
        Control::HeadphoneLevel => "Headphone level",
        Control::SamplerVolume => "Sampler volume",
    }
}

/// Keyboard adjustment shared by faders, knobs and effect depth. Modified shortcuts are left
/// to the window; Shift provides fine adjustment. Home/End reach the bounds and Backspace resets.
pub fn adjusted_value(event: &gpui::KeyDownEvent, value: f32, rest: f32) -> Option<f32> {
    let stroke = &event.keystroke;
    if stroke.modifiers.platform || stroke.modifiers.control || stroke.modifiers.alt {
        return None;
    }
    let step = if stroke.modifiers.shift { 0.001 } else { 0.01 };
    match stroke.key.as_str() {
        "left" | "down" => Some((value - step).max(0.0)),
        "right" | "up" => Some((value + step).min(1.0)),
        "home" => Some(0.0),
        "end" => Some(1.0),
        "backspace" => Some(rest),
        _ => None,
    }
}

/// The keyboard equivalent of a Shift-click, for alternate pad modes and cue deletion.
pub fn shift_activation(event: &gpui::KeyDownEvent) -> bool {
    let stroke = &event.keystroke;
    !event.is_held
        && stroke.modifiers.shift
        && !stroke.modifiers.platform
        && !stroke.modifiers.control
        && !stroke.modifiers.alt
        && matches!(stroke.key.as_str(), "enter" | "space")
}

/// A continuous control's requested adjustment, normalized to its 0–1 range.
#[derive(Clone, Copy)]
pub enum NumericChange {
    /// Move relative to the value when the action arrives.
    Step(f32),
    /// Set an absolute position in the range.
    To(f32),
}

impl NumericChange {
    /// Applies the request to the current position, keeping it within the control's bounds.
    pub fn apply(self, current: f32) -> f32 {
        match self {
            Self::Step(delta) => current + delta,
            Self::To(value) => value,
        }
        .clamp(0.0, 1.0)
    }
}

/// Adds assistive increment, decrement and absolute-value actions to a numeric control.
/// State is read when an action arrives so repeated requests never reuse a stale frame value.
pub fn numeric_actions<T: 'static>(
    mut element: Stateful<Div>,
    cx: &mut Context<'_, T>,
    adjust: impl Fn(&mut T, NumericChange, &mut Context<'_, T>) + Copy + 'static,
) -> Stateful<Div> {
    for (action, delta) in [
        (gpui::AccessibleAction::Increment, 0.01),
        (gpui::AccessibleAction::Decrement, -0.01),
    ] {
        let entity = cx.entity().downgrade();
        element = element.on_a11y_action(action, move |_, _, cx| {
            let _ = entity.update(cx, |this, cx| adjust(this, NumericChange::Step(delta), cx));
        });
    }
    let entity = cx.entity().downgrade();
    element.on_a11y_action(gpui::AccessibleAction::SetValue, move |data, _, cx| {
        let value = match data {
            Some(gpui::accesskit::ActionData::NumericValue(value)) => Some(*value),
            Some(gpui::accesskit::ActionData::Value(value)) => value.parse::<f64>().ok(),
            _ => None,
        };
        if let Some(value) = value.filter(|value| value.is_finite()) {
            let fraction = (value.clamp(0.0, 100.0) / 100.0) as f32;
            let _ = entity.update(cx, |this, cx| adjust(this, NumericChange::To(fraction), cx));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(stroke: &'static str) -> gpui::KeyDownEvent {
        gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse(stroke).expect("valid test key"),
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[test]
    fn focused_numeric_controls_adjust_finely_and_stop_at_their_bounds() {
        assert!(
            (adjusted_value(&key("shift-up"), 0.5, 0.5).expect("adjustment") - 0.501).abs()
                < 0.000001
        );
        assert_eq!(adjusted_value(&key("right"), 0.999, 0.5), Some(1.0));
        assert_eq!(adjusted_value(&key("left"), 0.001, 0.5), Some(0.0));
        assert_eq!(adjusted_value(&key("home"), 0.75, 0.5), Some(0.0));
        assert_eq!(adjusted_value(&key("end"), 0.25, 0.5), Some(1.0));
        assert_eq!(adjusted_value(&key("backspace"), 0.25, 0.8), Some(0.8));
    }

    #[test]
    fn numeric_controls_leave_modified_commands_and_tab_navigation_to_the_window() {
        for stroke in [
            "cmd-left",
            "ctrl-right",
            "alt-up",
            "tab",
            "shift-tab",
            "space",
            "escape",
        ] {
            assert_eq!(adjusted_value(&key(stroke), 0.5, 0.5), None, "{stroke}");
        }
    }
}

/// A short explanation that shows under the pointer.
pub struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .max_w(px(360.0))
            .px(px(8.0))
            .py(px(5.0))
            .rounded(theme::SMALL_RADIUS)
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::RAISED)
            .text_color(theme::TEXT)
            .text_size(theme::SMALL_TEXT_SIZE)
            .child(self.0.clone())
    }
}

/// What `.tooltip()` takes to show `text` under the pointer.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
    let text = text.into();
    move |_, cx| cx.new(|_| Tooltip(text.clone())).into()
}
