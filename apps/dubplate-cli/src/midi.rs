//! `dubplate midi`: see which controllers are connected and watch what they send.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use dubplate_midi::{
    Action, ButtonState, ControlEvent, ControllerInput, ControllerOutput, Feedback, Mapping,
    MidiEvent, find_input_port, input_ports, output_ports,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;

const QUEUE_CAPACITY: usize = 4096;
const IDLE_WAIT: Duration = Duration::from_millis(1);

#[derive(Subcommand)]
pub(crate) enum MidiCommand {
    /// List the MIDI inputs and outputs, and the controllers Dubplate has a mapping for.
    Devices,
    /// Print every message a controller sends, and what it means, until Ctrl-C.
    Monitor {
        /// The input to watch, by name or part of it. Defaults to the first controller Dubplate
        /// has a mapping for, or else the first input.
        #[arg(long)]
        device: Option<String>,
        /// Light each button while it is held, to check the controller's lights.
        #[arg(long)]
        lights: bool,
    },
}

pub(crate) fn run(command: MidiCommand) -> Result<()> {
    match command {
        MidiCommand::Devices => devices(),
        MidiCommand::Monitor { device, lights } => monitor(device.as_deref(), lights),
    }
}

fn devices() -> Result<()> {
    print_ports("Inputs", &input_ports()?)?;
    println!();
    print_ports("Outputs", &output_ports()?)
}

fn print_ports(title: &str, ports: &[String]) -> Result<()> {
    println!("{title}");
    if ports.is_empty() {
        println!("  none");
    }
    for port in ports {
        match Mapping::built_in_for_port(port)? {
            Some(mapping) => println!("  {port}  ({} mapping)", mapping.name()),
            None => println!("  {port}"),
        }
    }
    Ok(())
}

fn monitor(device: Option<&str>, lights: bool) -> Result<()> {
    let port = match device {
        Some(query) => find_input_port(query)?,
        None => default_input()?,
    };
    let mapping = Mapping::built_in_for_port(&port)?.map(Arc::new);
    let (input, mut events) = ControllerInput::open(&port, mapping.clone(), QUEUE_CAPACITY)?;
    match &mapping {
        Some(mapping) => println!(
            "Listening to {} with the {} mapping. Press Ctrl-C to stop.",
            input.port_name(),
            mapping.name()
        ),
        None => println!(
            "Listening to {}. Dubplate has no mapping for it, so only the bytes are shown. \
             Press Ctrl-C to stop.",
            input.port_name()
        ),
    }

    let mut output = match &mapping {
        Some(mapping) => open_output(input.port_name(), mapping),
        None => None,
    };
    if lights && output.is_none() {
        eprintln!("No output to the controller, so --lights cannot light anything.");
    }

    let time_zone = TimeZone::system();
    let mut reported_drops = 0;
    loop {
        if let Some(output) = output.as_mut() {
            output.service(Instant::now())?;
        }
        let mut idle = true;
        while let Some(event) = events.pop() {
            idle = false;
            print_event(&event, mapping.as_deref(), &time_zone)?;
            if lights && let Some(output) = output.as_mut() {
                light_while_held(output, event.control)?;
            }
        }
        let dropped = events.dropped();
        if dropped > reported_drops {
            eprintln!(
                "{} messages were lost because the monitor fell behind.",
                dropped - reported_drops
            );
            reported_drops = dropped;
        }
        if idle {
            std::thread::sleep(IDLE_WAIT);
        }
    }
}

/// The first input Dubplate has a mapping for, or else the first input.
fn default_input() -> Result<String> {
    let ports = input_ports()?;
    for port in &ports {
        if Mapping::built_in_for_port(port)?.is_some() {
            return Ok(port.clone());
        }
    }
    match ports.into_iter().next() {
        Some(port) => Ok(port),
        None => bail!("no MIDI inputs. Connect a controller and check `dubplate midi devices`"),
    }
}

/// The controller's output, which asks it to report every fader and knob and keeps it awake.
/// Without one the monitor still shows what the controller sends, so a failure is only reported.
fn open_output(port: &str, mapping: &Arc<Mapping>) -> Option<ControllerOutput> {
    match ControllerOutput::open(port, Arc::clone(mapping)) {
        Ok(output) => {
            println!(
                "Asked {} to report every fader and knob position.",
                output.port_name()
            );
            Some(output)
        }
        Err(error) => {
            eprintln!("Could not open the controller's output, so it gets no keepalive: {error}");
            None
        }
    }
}

fn print_event(event: &MidiEvent, mapping: Option<&Mapping>, time_zone: &TimeZone) -> Result<()> {
    let arrived = time_zone.to_datetime(
        Timestamp::now()
            .checked_sub(event.received.elapsed())
            .context("the clock is out of range")?,
    );
    let meaning = match (event.control, mapping) {
        (Some(control), _) => control.to_string(),
        (None, Some(mapping)) if mapping.recognizes(event.message.bytes()) => {
            "(first half of a 14-bit value)".to_owned()
        }
        (None, Some(_)) => "(not in the mapping)".to_owned(),
        (None, None) => String::new(),
    };
    println!(
        "{}  {:<8}  {meaning}",
        arrived.strftime("%H:%M:%S%.3f"),
        event.message
    );
    Ok(())
}

fn light_while_held(output: &mut ControllerOutput, control: Option<ControlEvent>) -> Result<()> {
    if let Some(ControlEvent {
        action: Action::Button { button, state },
        layer,
    }) = control
    {
        output.send(Feedback::Light {
            button,
            layer,
            on: state == ButtonState::Pressed,
        })?;
    }
    Ok(())
}
