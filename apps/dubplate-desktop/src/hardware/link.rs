//! The controller thread: finds controllers as they are plugged in and out, reads what the DJ
//! does on them, keeps them awake and lights them, so none of it waits on the window.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use dubplate_midi::{
    ControlEvent, ControllerInput, ControllerOutput, Feedback, InputEvents, Mapping, MidiError,
    STARTUP_DELAY, input_ports,
};

use super::lights::{LightCache, Lights};

/// How many messages wait between the MIDI thread and the controller thread.
const QUEUE_CAPACITY: usize = 4096;
/// How often the controller thread looks for messages while a controller is connected.
const POLL_INTERVAL: Duration = Duration::from_millis(1);
/// How often it looks for messages and new controllers while none is connected.
const IDLE_INTERVAL: Duration = Duration::from_millis(25);
/// How often the list of MIDI ports is read, to notice controllers plugged in or out.
const SCAN_INTERVAL: Duration = Duration::from_millis(1000);
/// How long a controller that was asked for its positions has to answer before Dubplate notes it
/// stayed silent.
const REPORT_TIMEOUT: Duration = Duration::from_secs(2);

/// What the controller thread tells the window.
#[derive(Debug, Clone)]
pub enum LinkEvent {
    /// A controller was found and opened.
    Connected {
        /// Its port.
        port: String,
        /// The controller's name from its mapping.
        name: String,
        /// How many ticks its jog wheels report per turn, when its mapping says.
        jog_ticks_per_revolution: Option<u16>,
    },
    /// A controller went away.
    Disconnected {
        /// Its port.
        port: String,
        /// The controller's name from its mapping.
        name: String,
    },
    /// The DJ used a control.
    Control {
        /// What the DJ did.
        control: ControlEvent,
        /// When the message reached Dubplate.
        received: Instant,
    },
    /// Something went wrong that the DJ should know about.
    Problem(String),
}

/// What the window asks of the controller thread.
enum Request {
    Lights(Lights),
    Signal(Feedback),
    Stop,
}

/// The running controller thread. Dropping it darkens the controllers and closes them.
pub struct ControllerLink {
    requests: Sender<Request>,
    thread: Option<JoinHandle<()>>,
}

impl ControllerLink {
    /// Starts the controller thread. Events arrive on the returned receiver.
    pub fn start() -> std::io::Result<(Self, async_channel::Receiver<LinkEvent>)> {
        let (event_sender, event_receiver) = async_channel::unbounded();
        let (request_sender, request_receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("dubplate-controllers".to_owned())
            .spawn(move || ControllerThread::new(event_sender, request_receiver).run())?;
        Ok((
            Self {
                requests: request_sender,
                thread: Some(thread),
            },
            event_receiver,
        ))
    }

    /// Shows `lights` on every controller, sending only what changed.
    pub fn show(&self, lights: Lights) {
        self.request(Request::Lights(lights));
    }

    /// Sends a one-off signal, such as the track loaded animation, to every controller.
    pub fn signal(&self, feedback: Feedback) {
        self.request(Request::Signal(feedback));
    }

    fn request(&self, request: Request) {
        // The thread only stops when the link is dropped, so a closed channel means it ended
        // on its own, and there is nobody left to show the lights.
        self.requests.send(request).ok();
    }
}

impl Drop for ControllerLink {
    fn drop(&mut self) {
        self.request(Request::Stop);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

/// One open controller.
struct Connection {
    port: String,
    mapping: Arc<Mapping>,
    _input: ControllerInput,
    events: InputEvents,
    output: Option<ControllerOutput>,
    lit: LightCache,
    reported_drops: u64,
    report_due: Option<Instant>,
}

impl Connection {
    fn open(port: &str, mapping: Mapping) -> Result<(Self, Option<MidiError>), MidiError> {
        let has_startup_request = !mapping.startup().is_empty();
        let mapping = Arc::new(mapping);
        let (input, events) =
            ControllerInput::open(port, Some(Arc::clone(&mapping)), QUEUE_CAPACITY)?;
        let (output, output_problem) = match ControllerOutput::open(port, Arc::clone(&mapping)) {
            Ok(output) => (Some(output), None),
            Err(error) => (None, Some(error)),
        };
        let connection = Self {
            port: input.port_name().to_owned(),
            mapping,
            _input: input,
            events,
            output,
            lit: LightCache::default(),
            reported_drops: 0,
            report_due: has_startup_request
                .then(|| Instant::now() + STARTUP_DELAY + REPORT_TIMEOUT),
        };
        Ok((connection, output_problem))
    }

    fn name(&self) -> String {
        self.mapping.name().to_owned()
    }

    fn show(&mut self, lights: &Lights) -> Result<(), MidiError> {
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        self.lit.show(&self.mapping, lights, |feedback| {
            output.send(feedback).map(|_| ())
        })
    }

    fn signal(&mut self, feedback: Feedback) -> Result<(), MidiError> {
        match self.output.as_mut() {
            Some(output) => output.send(feedback).map(|_| ()),
            None => Ok(()),
        }
    }

    fn service(&mut self, now: Instant) -> Result<(), MidiError> {
        match self.output.as_mut() {
            Some(output) => output.service(now),
            None => Ok(()),
        }
    }
}

struct ControllerThread {
    events: async_channel::Sender<LinkEvent>,
    requests: Receiver<Request>,
    connections: Vec<Connection>,
    lights: Lights,
    next_scan: Instant,
}

impl ControllerThread {
    fn new(events: async_channel::Sender<LinkEvent>, requests: Receiver<Request>) -> Self {
        Self {
            events,
            requests,
            connections: Vec::new(),
            lights: Lights::default(),
            next_scan: Instant::now(),
        }
    }

    fn run(mut self) {
        loop {
            let now = Instant::now();
            if !self.take_requests() {
                break;
            }
            if now >= self.next_scan {
                self.scan();
                self.next_scan = now + SCAN_INTERVAL;
            }
            self.serve(now);
            std::thread::sleep(if self.connections.is_empty() {
                IDLE_INTERVAL
            } else {
                POLL_INTERVAL
            });
        }
        self.lights = Lights::default();
        for connection in &mut self.connections {
            connection.show(&self.lights).ok();
        }
    }

    /// Takes what the window asked for. Returns `false` when the thread should stop.
    fn take_requests(&mut self) -> bool {
        let mut lights_changed = false;
        loop {
            match self.requests.try_recv() {
                Ok(Request::Lights(lights)) => {
                    lights_changed |= lights != self.lights;
                    self.lights = lights;
                }
                Ok(Request::Signal(feedback)) => {
                    let failures: Vec<(String, MidiError)> = self
                        .connections
                        .iter_mut()
                        .filter_map(|connection| {
                            let error = connection.signal(feedback).err()?;
                            Some((connection.port.clone(), error))
                        })
                        .collect();
                    for (port, error) in failures {
                        self.drop_connection(&port, Some(error));
                    }
                }
                Ok(Request::Stop) | Err(TryRecvError::Disconnected) => return false,
                Err(TryRecvError::Empty) => break,
            }
        }
        if lights_changed {
            self.show_lights();
        }
        true
    }

    fn show_lights(&mut self) {
        let failures: Vec<(String, MidiError)> = self
            .connections
            .iter_mut()
            .filter_map(|connection| {
                let error = connection.show(&self.lights).err()?;
                Some((connection.port.clone(), error))
            })
            .collect();
        for (port, error) in failures {
            self.drop_connection(&port, Some(error));
        }
    }

    /// Opens controllers that appeared and closes those whose port went away.
    fn scan(&mut self) {
        let ports = match input_ports() {
            Ok(ports) => ports,
            Err(error) => {
                self.report(LinkEvent::Problem(format!(
                    "Could not list MIDI controllers: {error}"
                )));
                return;
            }
        };
        let gone: Vec<String> = self
            .connections
            .iter()
            .filter(|connection| !ports.contains(&connection.port))
            .map(|connection| connection.port.clone())
            .collect();
        for port in gone {
            self.drop_connection(&port, None);
        }
        for port in ports {
            if self
                .connections
                .iter()
                .any(|connection| connection.port == port)
            {
                continue;
            }
            match Mapping::built_in_for_port(&port) {
                Ok(Some(mapping)) => self.connect(&port, mapping),
                Ok(None) => {}
                Err(error) => self.report(LinkEvent::Problem(format!(
                    "The built-in mapping for {port} is broken: {error}"
                ))),
            }
        }
    }

    fn connect(&mut self, port: &str, mapping: Mapping) {
        match Connection::open(port, mapping) {
            Ok((mut connection, output_problem)) => {
                if let Some(error) = output_problem {
                    self.report(LinkEvent::Problem(format!(
                        "{} is connected, but its lights could not be opened: {error}",
                        connection.name()
                    )));
                }
                let shown = connection.show(&self.lights);
                self.report(LinkEvent::Connected {
                    port: connection.port.clone(),
                    name: connection.name(),
                    jog_ticks_per_revolution: connection.mapping.jog_ticks_per_revolution(),
                });
                let port = connection.port.clone();
                self.connections.push(connection);
                if let Err(error) = shown {
                    self.drop_connection(&port, Some(error));
                }
            }
            Err(error) => self.report(LinkEvent::Problem(format!(
                "Could not open the controller on {port}: {error}"
            ))),
        }
    }

    fn drop_connection(&mut self, port: &str, error: Option<MidiError>) {
        let Some(position) = self
            .connections
            .iter()
            .position(|connection| connection.port == port)
        else {
            return;
        };
        let connection = self.connections.remove(position);
        if let Some(error) = error {
            self.report(LinkEvent::Problem(format!(
                "Lost {}: {error}",
                connection.name()
            )));
        }
        self.report(LinkEvent::Disconnected {
            port: connection.port.clone(),
            name: connection.name(),
        });
        // Forget the controller now, so a scan in the same breath can open it again.
        self.next_scan = Instant::now() + SCAN_INTERVAL;
    }

    /// Hands every message waiting from each controller to the window and keeps each awake.
    fn serve(&mut self, now: Instant) {
        let mut failures: Vec<(String, MidiError)> = Vec::new();
        for connection in &mut self.connections {
            while let Some(event) = connection.events.pop() {
                connection.report_due = None;
                if let Some(control) = event.control {
                    self.events
                        .try_send(LinkEvent::Control {
                            control,
                            received: event.received,
                        })
                        .ok();
                }
            }
            let dropped = connection.events.dropped();
            if dropped > connection.reported_drops {
                self.events
                    .try_send(LinkEvent::Problem(format!(
                        "{} messages from {} were lost because Dubplate fell behind.",
                        dropped - connection.reported_drops,
                        connection.mapping.name()
                    )))
                    .ok();
                connection.reported_drops = dropped;
            }
            if connection.report_due.is_some_and(|due| now >= due) {
                connection.report_due = None;
                eprintln!(
                    "{} did not report its control positions when asked, so they are unknown until moved.",
                    connection.mapping.name()
                );
            }
            if let Err(error) = connection.service(now) {
                failures.push((connection.port.clone(), error));
            }
        }
        for (port, error) in failures {
            self.drop_connection(&port, Some(error));
        }
    }

    fn report(&self, event: LinkEvent) {
        // The window holds the receiver for as long as the link lives.
        self.events.try_send(event).ok();
    }
}
