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
    message: String,
    #[serde(default)]
    layer: Layer,
    #[serde(default)]
    light: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchEntry {
    switch: Switch,
    message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinuousEntry {
    control: Continuous,
    message: String,
    #[serde(default)]
    lsb: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncoderEntry {
    encoder: Encoder,
    message: String,
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
    message: String,
}

/// Which file entry an error came from, named in the error so it can be found.
struct EntryRef {
    kind: &'static str,
    index: usize,
    as_written: String,
    deck: Option<Deck>,
}

impl EntryRef {
    /// `button 1: "90 0B"`, `pad bank 0 (deck a): status = 0x97, pads = 8`.
    fn describe(&self) -> String {
        let mut description = format!("{} {}", self.kind, self.index);
        if let Some(deck) = self.deck {
            description.push_str(&format!(" ({deck})"));
        }
        if !self.as_written.is_empty() {
            description.push_str(&format!(": {}", self.as_written));
        }
        description
    }
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

        for (index, entry) in file.buttons.into_iter().enumerate() {
            self.add_button(index, entry)?;
        }
        for (index, entry) in file.switches.into_iter().enumerate() {
            self.add_switch(index, entry)?;
        }
        for (index, entry) in file.continuous.into_iter().enumerate() {
            self.add_continuous(index, entry)?;
        }
        for (index, entry) in file.encoders.into_iter().enumerate() {
            self.add_encoder(index, entry)?;
        }
        for (index, bank) in file.pad_banks.iter().enumerate() {
            self.add_pad_bank(index, bank, &file.pad_modes)?;
        }
        for (index, entry) in file.outputs.into_iter().enumerate() {
            let entry_ref = EntryRef {
                kind: "output",
                index,
                as_written: format!("\"{}\"", entry.message),
                deck: None,
            };
            let address = self.parse_address(&entry_ref, &entry.message)?;
            self.add_output(Output::Target(entry.output), address)?;
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

    fn add_button(&mut self, index: usize, entry: ButtonEntry) -> Result<(), MappingError> {
        let button = entry.button;
        let entry_ref = EntryRef {
            kind: "button",
            index,
            as_written: format!("\"{}\"", entry.message),
            deck: None,
        };
        let address = self.parse_address(&entry_ref, &entry.message)?;
        self.add_input(&entry_ref, address, Input::Button(button), entry.layer)?;
        if entry.light {
            self.add_output(Output::Light(button, entry.layer), address)?;
        }
        Ok(())
    }

    fn add_switch(&mut self, index: usize, entry: SwitchEntry) -> Result<(), MappingError> {
        let switch = entry.switch;
        let entry_ref = EntryRef {
            kind: "switch",
            index,
            as_written: format!("\"{}\"", entry.message),
            deck: None,
        };
        let address = self.parse_address(&entry_ref, &entry.message)?;
        self.add_input(&entry_ref, address, Input::Switch(switch), Layer::Normal)
    }

    fn add_continuous(&mut self, index: usize, entry: ContinuousEntry) -> Result<(), MappingError> {
        let (entry_ref, lsb_ref) = match &entry.lsb {
            Some(least_significant) => (
                EntryRef {
                    kind: "continuous",
                    index,
                    as_written: format!("\"{}\", lsb \"{}\"", entry.message, least_significant),
                    deck: None,
                },
                EntryRef {
                    kind: "continuous",
                    index,
                    as_written: format!("lsb \"{}\"", least_significant),
                    deck: None,
                },
            ),
            None => (
                EntryRef {
                    kind: "continuous",
                    index,
                    as_written: format!("\"{}\"", entry.message),
                    deck: None,
                },
                EntryRef {
                    kind: "continuous",
                    index,
                    as_written: String::new(),
                    deck: None,
                },
            ),
        };
        let address = self.parse_address(&entry_ref, &entry.message)?;
        let Some(least_significant_text) = entry.lsb else {
            return self.add_input(
                &entry_ref,
                address,
                Input::Continuous(entry.control),
                Layer::Normal,
            );
        };
        let least_significant = self.parse_address(&lsb_ref, &least_significant_text)?;
        if address == least_significant {
            return Err(self.invalid(format!(
                "{}: {} is also its own lsb; the two halves need two different control change \
                 numbers",
                entry.control,
                entry_ref.describe()
            )));
        }
        if !address.same_channel(least_significant) {
            return Err(self.invalid(format!(
                "{}: {} sends its halves on different channels ({} and {least_significant})",
                entry.control,
                entry_ref.describe(),
                address
            )));
        }
        // Reserve the pair only once both halves are known to be free, so a rejected control
        // does not shift the pair indexes of the ones after it.
        let pair = self.fourteen_bit_controls;
        self.fourteen_bit_controls += 1;
        let most_significant = Input::MostSignificant { pair };
        self.add_input(&entry_ref, address, most_significant, Layer::Normal)?;
        let least_significant_input = Input::LeastSignificant {
            control: entry.control,
            pair,
        };
        self.add_input(
            &lsb_ref,
            least_significant,
            least_significant_input,
            Layer::Normal,
        )
    }

    fn add_encoder(&mut self, index: usize, entry: EncoderEntry) -> Result<(), MappingError> {
        let encoder = entry.encoder;
        let entry_ref = EntryRef {
            kind: "encoder",
            index,
            as_written: format!("\"{}\"", entry.message),
            deck: None,
        };
        let address = self.parse_address(&entry_ref, &entry.message)?;
        self.add_input(
            &entry_ref,
            address,
            Input::Encoder(encoder, entry.encoding),
            entry.layer,
        )
    }

    fn add_pad_bank(
        &mut self,
        index: usize,
        bank: &PadBankEntry,
        modes: &[PadModeEntry],
    ) -> Result<(), MappingError> {
        let bank_ref = || EntryRef {
            kind: "pad bank",
            index,
            as_written: format!("status = 0x{:02X}, pads = {}", bank.status, bank.pads),
            deck: Some(bank.deck),
        };
        if modes.is_empty() {
            return Err(self.invalid(format!(
                "{}: pad banks need pad_modes to say which notes each mode uses",
                bank_ref().describe()
            )));
        }
        let pads = (1..=bank.pads)
            .map(PadIndex::new)
            .collect::<Option<Vec<PadIndex>>>()
            .ok_or_else(|| {
                self.invalid(format!(
                    "{}: a pad bank has {} pads; it can have 1 to {}",
                    bank_ref().describe(),
                    bank.pads,
                    PadIndex::MAX_PADS
                ))
            })?;
        for mode in modes {
            for pad in &pads {
                let number =
                    U7::new(mode.first_pad.saturating_add(pad.number() - 1)).ok_or_else(|| {
                        self.invalid(format!(
                            "{}: pad {} of mode {} pads run past note 127",
                            bank_ref().describe(),
                            pad.number(),
                            mode.mode
                        ))
                    })?;
                let address = Address::from_status(bank.status, number).map_err(|reason| {
                    self.invalid(format!("{}: {reason}", bank_ref().describe()))
                })?;
                let button = Button::Pad {
                    deck: bank.deck,
                    mode: mode.mode,
                    pad: *pad,
                };
                self.add_input(&bank_ref(), address, Input::Button(button), bank.layer)?;
                if bank.light {
                    self.add_output(Output::Light(button, bank.layer), address)?;
                }
            }
        }
        Ok(())
    }

    fn parse_address(&self, entry: &EntryRef, text: &str) -> Result<Address, MappingError> {
        text.parse()
            .map_err(|reason| self.invalid(format!("{}: {reason}", entry.describe())))
    }

    fn add_input(
        &mut self,
        entry: &EntryRef,
        address: Address,
        input: Input,
        layer: Layer,
    ) -> Result<(), MappingError> {
        let Some(slot) = self.inputs.get_mut(address.index()) else {
            return Err(self.invalid(format!(
                "{}: message {address} is out of range",
                entry.describe()
            )));
        };
        if slot.is_some() {
            return Err(MappingError::DuplicateMessage {
                mapping: self.name.clone(),
                address: format!(
                    "{} is already mapped to message {address}",
                    entry.describe()
                ),
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
