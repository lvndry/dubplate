//! MIDI ports: listing them, and opening a controller for input and LED output.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use midir::{Ignore, MidiIO, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use crate::action::ControlEvent;
use crate::decoder::Decoder;
use crate::mapping::{Feedback, Mapping};
use crate::message::RawMessage;

const CLIENT_NAME: &str = "Dubplate";

/// A MIDI port or connection that failed.
#[derive(Debug, thiserror::Error)]
pub enum MidiError {
    /// The system's MIDI service is not available.
    #[error("MIDI is not available on this system: {0}")]
    Init(#[from] midir::InitError),
    /// A port disappeared while it was being read.
    #[error("could not read a MIDI port's name: {0}")]
    PortInfo(#[from] midir::PortInfoError),
    /// No port has the name asked for.
    #[error("no MIDI {direction} port is called \"{query}\"; available: {}", list(.available))]
    NoSuchPort {
        /// Input or output.
        direction: PortDirection,
        /// The name asked for.
        query: String,
        /// The ports there are.
        available: Vec<String>,
    },
    /// More than one port contains the name asked for.
    #[error("\"{query}\" matches more than one MIDI {direction} port: {}", list(.matches))]
    AmbiguousPort {
        /// Input or output.
        direction: PortDirection,
        /// The name asked for.
        query: String,
        /// The ports it matches.
        matches: Vec<String>,
    },
    /// The port exists but could not be opened.
    #[error("could not open MIDI port \"{port}\": {reason}")]
    Connect {
        /// The port.
        port: String,
        /// Why.
        reason: String,
    },
    /// A message could not be sent to the controller.
    #[error("could not send to the controller: {0}")]
    Send(#[from] midir::SendError),
}

/// Whether a port carries messages from the controller or to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDirection {
    /// Messages from the controller.
    Input,
    /// Messages to the controller.
    Output,
}

impl fmt::Display for PortDirection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            PortDirection::Input => "input",
            PortDirection::Output => "output",
        })
    }
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}

/// The names of the MIDI input ports.
pub fn input_ports() -> Result<Vec<String>, MidiError> {
    port_names(&MidiInput::new(CLIENT_NAME)?)
}

/// The names of the MIDI output ports.
pub fn output_ports() -> Result<Vec<String>, MidiError> {
    port_names(&MidiOutput::new(CLIENT_NAME)?)
}

/// The full name of the input port called `query`: an exact name, ignoring case, or else the one
/// input whose name contains it.
pub fn find_input_port(query: &str) -> Result<String, MidiError> {
    let (_, name) = find_port(&MidiInput::new(CLIENT_NAME)?, query, PortDirection::Input)?;
    Ok(name)
}

fn port_names(io: &impl MidiIO) -> Result<Vec<String>, MidiError> {
    io.ports()
        .iter()
        .map(|port| io.port_name(port).map_err(MidiError::from))
        .collect()
}

/// The port called `query`: an exact name, ignoring case, or else the one port whose name
/// contains it.
fn find_port<IO: MidiIO>(
    io: &IO,
    query: &str,
    direction: PortDirection,
) -> Result<(IO::Port, String), MidiError> {
    let ports = io.ports();
    let names = ports
        .iter()
        .map(|port| io.port_name(port))
        .collect::<Result<Vec<String>, _>>()?;
    let wanted = query.to_lowercase();
    let exact = names.iter().position(|name| name.to_lowercase() == wanted);
    let containing: Vec<usize> = (0..names.len())
        .filter(|&position| names[position].to_lowercase().contains(&wanted))
        .collect();
    let position = match (exact, containing.as_slice()) {
        (Some(position), _) | (None, &[position]) => position,
        (None, []) => {
            return Err(MidiError::NoSuchPort {
                direction,
                query: query.to_owned(),
                available: names,
            });
        }
        (None, _) => {
            return Err(MidiError::AmbiguousPort {
                direction,
                query: query.to_owned(),
                matches: containing
                    .iter()
                    .map(|&position| names[position].clone())
                    .collect(),
            });
        }
    };
    Ok((ports[position].clone(), names[position].clone()))
}

/// One message from the controller, with what it means when a mapping knows it.
#[derive(Debug, Clone, Copy)]
pub struct MidiEvent {
    /// When the message reached Dubplate.
    pub received: Instant,
    /// The MIDI driver's timestamp in microseconds, for timing jog wheel movement.
    pub timestamp_micros: u64,
    /// The bytes as they arrived.
    pub message: RawMessage,
    /// What the DJ did, or `None` when the mapping does not know the message, or it is the first
    /// half of a 14-bit control.
    pub control: Option<ControlEvent>,
}

/// An open MIDI input. Messages arrive in its [`InputEvents`] until it is dropped.
pub struct ControllerInput {
    port_name: String,
    _connection: MidiInputConnection<()>,
}

/// Messages from a [`ControllerInput`], handed over from the MIDI thread through a lock-free
/// queue.
pub struct InputEvents {
    queue: rtrb::Consumer<MidiEvent>,
    dropped: Arc<AtomicU64>,
}

impl ControllerInput {
    /// Opens the input port called `port`, decoding with `mapping` when there is one. The queue
    /// holds `capacity` messages; when the reader falls further behind, new messages are dropped
    /// and counted in [`InputEvents::dropped`].
    pub fn open(
        port: &str,
        mapping: Option<Arc<Mapping>>,
        capacity: usize,
    ) -> Result<(ControllerInput, InputEvents), MidiError> {
        let mut input = MidiInput::new(CLIENT_NAME)?;
        input.ignore(Ignore::None);
        let (midi_port, port_name) = find_port(&input, port, PortDirection::Input)?;
        let (mut producer, consumer) = rtrb::RingBuffer::new(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let dropped_by_callback = Arc::clone(&dropped);
        let mut decoder = mapping.map(Decoder::new);
        let connection = input
            .connect(
                &midi_port,
                CLIENT_NAME,
                move |timestamp_micros, bytes, &mut ()| {
                    let event = MidiEvent {
                        received: Instant::now(),
                        timestamp_micros,
                        message: RawMessage::new(bytes),
                        control: decoder.as_mut().and_then(|decoder| decoder.decode(bytes)),
                    };
                    if producer.push(event).is_err() {
                        dropped_by_callback.fetch_add(1, Ordering::Relaxed);
                    }
                },
                (),
            )
            .map_err(|error| MidiError::Connect {
                port: port_name.clone(),
                reason: error.kind().to_string(),
            })?;
        let input = ControllerInput {
            port_name,
            _connection: connection,
        };
        let events = InputEvents {
            queue: consumer,
            dropped,
        };
        Ok((input, events))
    }

    /// The port's full name.
    pub fn port_name(&self) -> &str {
        &self.port_name
    }
}

impl InputEvents {
    /// The oldest message not read yet.
    pub fn pop(&mut self) -> Option<MidiEvent> {
        self.queue.pop().ok()
    }

    /// How many messages were lost because the queue was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// An open MIDI output that lights the controller.
pub struct ControllerOutput {
    port_name: String,
    connection: MidiOutputConnection,
    mapping: Arc<Mapping>,
    last_keepalive: Option<Instant>,
}

impl ControllerOutput {
    /// Opens the output port called `port` and sends the mapping's startup messages.
    pub fn open(port: &str, mapping: Arc<Mapping>) -> Result<ControllerOutput, MidiError> {
        let output = MidiOutput::new(CLIENT_NAME)?;
        let (midi_port, port_name) = find_port(&output, port, PortDirection::Output)?;
        let connection =
            output
                .connect(&midi_port, CLIENT_NAME)
                .map_err(|error| MidiError::Connect {
                    port: port_name.clone(),
                    reason: error.kind().to_string(),
                })?;
        let mut controller = ControllerOutput {
            port_name,
            connection,
            mapping,
            last_keepalive: None,
        };
        for message in controller.mapping.startup() {
            controller.connection.send(message)?;
        }
        controller.last_keepalive = Some(Instant::now());
        Ok(controller)
    }

    /// The port's full name.
    pub fn port_name(&self) -> &str {
        &self.port_name
    }

    /// Shows `feedback` on the controller. Returns `false` when the controller has no such
    /// light, and sends nothing.
    pub fn send(&mut self, feedback: Feedback) -> Result<bool, MidiError> {
        let Some(message) = self.mapping.encode(feedback) else {
            return Ok(false);
        };
        self.connection.send(&message)?;
        Ok(true)
    }

    /// Sends the controller's keepalive when its interval has passed since the last one. Call it
    /// often, from the loop that reads the controller.
    pub fn keep_alive(&mut self, now: Instant) -> Result<(), MidiError> {
        let Some(keepalive) = self.mapping.keepalive() else {
            return Ok(());
        };
        let due = self
            .last_keepalive
            .is_none_or(|last| now.saturating_duration_since(last) >= keepalive.interval);
        if due {
            self.connection.send(&keepalive.message)?;
            self.last_keepalive = Some(now);
        }
        Ok(())
    }
}
