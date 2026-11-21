//! The TOML shape of a mapping file, and turning it into a [`Mapping`]'s lookup tables.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;

use serde::Deserialize;

use super::{Input, Keepalive, Mapping, MappingError, Output, OutputTarget, Slot};
use crate::action::{Button, Continuous, Deck, Encoder, Layer, PadIndex, PadMode, Switch};
use crate::message::Address;
use crate::value::{RelativeEncoding, U7};

/// Reads a mapping file and builds its lookup tables, checking that no two controls share a
/// message and no light has two messages.
pub(super) fn parse(text: &str) -> Result<Mapping, MappingError> {
    let file: MappingFile = toml::from_str(text)?;
    MappingBuilder::new(&file.name).build(file)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MappingFile {
    name: String,
    ports: Vec<String>,
    jog_ticks_per_revolution: Option<u16>,
    #[serde(default)]
    startup: Vec<Vec<u8>>,
    keepalive: Option<KeepaliveEntry>,
    #[serde(default)]
    buttons: Vec<ButtonEntry>,
    #[serde(default)]
    switches: Vec<SwitchEntry>,
    #[serde(default)]
    continuous: Vec<ContinuousEntry>,
    #[serde(default)]
    encoders: Vec<EncoderEntry>,
    #[serde(default)]
    pad_modes: Vec<PadModeEntry>,
    #[serde(default)]
    pad_banks: Vec<PadBankEntry>,
    #[serde(default)]
    outputs: Vec<OutputEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeepaliveEntry {
    interval_ms: u64,
    message: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ButtonEntry {
    button: Button,
    message: Address,
    #[serde(default)]
    layer: Layer,
    #[serde(default)]
    light: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchEntry {
    switch: Switch,
    message: Address,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuousEntry {
    control: Continuous,
    message: Address,
    lsb: Option<Address>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncoderEntry {
    encoder: Encoder,
    message: Address,
    encoding: RelativeEncoding,
    #[serde(default)]
    layer: Layer,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PadModeEntry {
    mode: PadMode,
    first_pad: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PadBankEntry {
    deck: Deck,
    status: u8,
    pads: u8,
    #[serde(default)]
    layer: Layer,
    #[serde(default)]
    light: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputEntry {
    output: OutputTarget,
    message: Address,
}

struct MappingBuilder {
    name: String,
    inputs: Box<[Option<Slot>]>,
    fourteen_bit_controls: usize,
    outputs: HashMap<Output, Address>,
}

impl MappingBuilder {
    fn new(name: &str) -> MappingBuilder {
        MappingBuilder {
            name: name.to_owned(),
            inputs: vec![None; Address::COUNT].into_boxed_slice(),
            fourteen_bit_controls: 0,
            outputs: HashMap::new(),
        }
    }

    fn build(mut self, file: MappingFile) -> Result<Mapping, MappingError> {
        if file.ports.is_empty() {
            return Err(self.invalid("it names no MIDI ports to match".to_owned()));
        }
        let keepalive = file
            .keepalive
            .map(|entry| {
                if entry.interval_ms == 0 || entry.message.is_empty() {
                    Err(self.invalid("keepalive needs an interval and a message".to_owned()))
                } else {
                    Ok(Keepalive {
                        interval: Duration::from_millis(entry.interval_ms),
                        message: entry.message,
                    })
                }
            })
            .transpose()?;

        for entry in file.buttons {
            self.add_button(entry.button, entry.layer, entry.message, entry.light)?;
        }
        for entry in file.switches {
            self.add_input(entry.message, Input::Switch(entry.switch), Layer::Normal)?;
        }
        for entry in file.continuous {
            self.add_continuous(entry)?;
        }
        for entry in file.encoders {
            let input = Input::Encoder(entry.encoder, entry.encoding);
            self.add_input(entry.message, input, entry.layer)?;
        }
        for bank in &file.pad_banks {
            self.add_pad_bank(bank, &file.pad_modes)?;
        }
        for entry in file.outputs {
            self.add_output(Output::Target(entry.output), entry.message)?;
        }

        Ok(Mapping {
            name: self.name,
            ports: file.ports,
            jog_ticks_per_revolution: file.jog_ticks_per_revolution,
            startup: file.startup,
            keepalive,
            inputs: self.inputs,
            fourteen_bit_controls: self.fourteen_bit_controls,
            outputs: self.outputs,
        })
    }

    fn add_button(
        &mut self,
        button: Button,
        layer: Layer,
        address: Address,
        light: bool,
    ) -> Result<(), MappingError> {
        self.add_input(address, Input::Button(button), layer)?;
        if light {
            self.add_output(Output::Light(button, layer), address)?;
        }
        Ok(())
    }

    fn add_continuous(&mut self, entry: ContinuousEntry) -> Result<(), MappingError> {
        let Some(least_significant) = entry.lsb else {
            return self.add_input(
                entry.message,
                Input::Continuous(entry.control),
                Layer::Normal,
            );
        };
        if !entry.message.same_channel(least_significant) {
            return Err(self.invalid(format!(
                "{} sends its halves on different channels ({} and {least_significant})",
                entry.control, entry.message
            )));
        }
        let pair = self.fourteen_bit_controls;
        self.fourteen_bit_controls += 1;
        self.add_input(
            entry.message,
            Input::MostSignificant { pair },
            Layer::Normal,
        )?;
        let input = Input::LeastSignificant {
            control: entry.control,
            pair,
        };
        self.add_input(least_significant, input, Layer::Normal)
    }

    fn add_pad_bank(
        &mut self,
        bank: &PadBankEntry,
        modes: &[PadModeEntry],
    ) -> Result<(), MappingError> {
        if modes.is_empty() {
            return Err(self
                .invalid("pad banks need pad_modes to say which notes each mode uses".to_owned()));
        }
        let pads = (1..=bank.pads)
            .map(PadIndex::new)
            .collect::<Option<Vec<PadIndex>>>()
            .ok_or_else(|| {
                self.invalid(format!(
                    "a pad bank has {} pads; it can have 1 to {}",
                    bank.pads,
                    PadIndex::MAX_PADS
                ))
            })?;
        for mode in modes {
            for pad in &pads {
                let number = U7::new(mode.first_pad.saturating_add(pad.number() - 1))
                    .ok_or_else(|| self.invalid(format!("{} pads run past note 127", mode.mode)))?;
                let address = Address::from_status(bank.status, number)
                    .map_err(|reason| self.invalid(reason))?;
                let button = Button::Pad {
                    deck: bank.deck,
                    mode: mode.mode,
                    pad: *pad,
                };
                self.add_button(button, bank.layer, address, bank.light)?;
            }
        }
        Ok(())
    }

    fn add_input(
        &mut self,
        address: Address,
        input: Input,
        layer: Layer,
    ) -> Result<(), MappingError> {
        let slot = self
            .inputs
            .get_mut(address.index())
            .ok_or_else(|| MappingError::Invalid {
                mapping: self.name.clone(),
                reason: format!("message {address} is out of range"),
            })?;
        if slot.is_some() {
            return Err(MappingError::DuplicateMessage {
                mapping: self.name.clone(),
                address: address.to_string(),
            });
        }
        *slot = Some(Slot { input, layer });
        Ok(())
    }

    fn add_output(&mut self, output: Output, address: Address) -> Result<(), MappingError> {
        match self.outputs.entry(output) {
            Entry::Occupied(_) => Err(MappingError::DuplicateOutput {
                mapping: self.name.clone(),
                output: output.to_string(),
            }),
            Entry::Vacant(vacant) => {
                vacant.insert(address);
                Ok(())
            }
        }
    }

    fn invalid(&self, reason: String) -> MappingError {
        MappingError::Invalid {
            mapping: self.name.clone(),
            reason,
        }
    }
}
