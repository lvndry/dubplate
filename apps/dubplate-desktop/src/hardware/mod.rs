//! DJ controllers: finding them as they are plugged in and out, turning what the DJ does on
//! them into [`crate::commands::Command`]s, and showing the decks on their lights.

mod dispatch;
mod lights;
mod link;

use std::time::{Duration, Instant};

pub use dispatch::{Dispatcher, Surface, controller_deck};
pub use lights::{DeckLights, Lights, PAD_COUNT, meter_level};
pub use link::LinkEvent;

use dubplate_midi::Feedback;
use link::ControllerLink;

/// How long a blinking light stays on, then off.
const BLINK_HALF_PERIOD: Duration = Duration::from_millis(500);

/// A controller that is connected.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Connected {
    port: String,
    name: String,
}

/// The controllers the window knows about, the dispatcher that reads them, and their lights.
pub struct Hardware {
    link: ControllerLink,
    /// Turns controller events into commands.
    pub dispatcher: Dispatcher,
    connected: Vec<Connected>,
    shown: Option<Lights>,
    started: Instant,
}

impl Hardware {
    /// Starts looking for controllers. Their events arrive on the returned receiver. Call it on
    /// the main thread.
    pub fn start() -> std::io::Result<(Self, async_channel::Receiver<LinkEvent>)> {
        // CoreMIDI keeps its list of ports up to date through the run loop of the thread that
        // first talks to it. Talking to it here, on the main thread whose run loop always runs,
        // lets the controller thread see controllers plugged in later.
        dubplate_midi::input_ports().ok();
        let (link, events) = ControllerLink::start()?;
        let hardware = Self {
            link,
            dispatcher: Dispatcher::default(),
            connected: Vec::new(),
            shown: None,
            started: Instant::now(),
        };
        Ok((hardware, events))
    }

    /// Notes a controller that connected or went away.
    pub fn note(&mut self, event: &LinkEvent) {
        match event {
            LinkEvent::Connected { port, name } => {
                self.connected.push(Connected {
                    port: port.clone(),
                    name: name.clone(),
                });
            }
            LinkEvent::Disconnected { port, .. } => {
                self.connected.retain(|connected| &connected.port != port);
            }
            LinkEvent::Control { .. } | LinkEvent::Problem(_) => {}
        }
    }

    /// The names of the connected controllers.
    pub fn connected_names(&self) -> impl Iterator<Item = &str> {
        self.connected
            .iter()
            .map(|connected| connected.name.as_str())
    }

    /// Whether any controller is connected.
    pub fn is_connected(&self) -> bool {
        !self.connected.is_empty()
    }

    /// Whether a blinking light is on at `now`. Every light blinks together.
    pub fn blink_on(&self, now: Instant) -> bool {
        let half_periods =
            now.saturating_duration_since(self.started).as_millis() / BLINK_HALF_PERIOD.as_millis();
        half_periods.is_multiple_of(2)
    }

    /// Shows `lights` on the controllers when they differ from what they show.
    pub fn show(&mut self, lights: Lights) {
        if self.shown != Some(lights) {
            self.link.show(lights);
            self.shown = Some(lights);
        }
    }

    /// Sends a one-off signal, such as the track loaded animation.
    pub fn signal(&self, feedback: Feedback) {
        self.link.signal(feedback);
    }
}
