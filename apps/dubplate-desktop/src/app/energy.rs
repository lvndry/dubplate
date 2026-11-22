//! The window's side of track energy: asking for the set's tracks before the rest of the
//! library, taking the ratings as they arrive, and showing them in the library, the set and the
//! suggestions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::Context;

use super::DubplateApp;
use crate::energy::EnergyPriority;

/// How often the ratings that have arrived are taken while tracks are waiting.
const COLLECT_INTERVAL: Duration = Duration::from_millis(250);

impl DubplateApp {
    /// Asks for the energy of the set's tracks not rated yet, before anything else.
    pub(super) fn request_set_energy(&mut self, cx: &mut Context<'_, Self>) {
        let set_tracks: Vec<PathBuf> = self
            .set_editor
            .as_ref()
            .map(|editor| {
                let set = editor.set();
                set.entries
                    .iter()
                    .map(|entry| entry.track_path.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.energy
            .request(set_tracks.iter().map(PathBuf::as_path), EnergyPriority::Set);
        cx.notify();
        self.collect_energy_while_waiting(cx);
    }

    /// Asks for the energy of every other track of the library not rated yet.
    pub(super) fn request_library_energy(&mut self, cx: &mut Context<'_, Self>) {
        self.energy.request(
            self.catalogue
                .tracks
                .iter()
                .map(|track| track.track_path.as_path()),
            EnergyPriority::Library,
        );
        cx.notify();
        self.collect_energy_while_waiting(cx);
    }

    /// Gives a freshly built catalogue the energies already rated.
    pub(super) fn restore_energy(&mut self) {
        let catalogue = Arc::make_mut(&mut self.catalogue);
        for (path, reading) in self.energy.readings() {
            catalogue.set_energy(path, reading.rating);
        }
    }

    fn collect_energy_while_waiting(&mut self, cx: &mut Context<'_, Self>) {
        if self.energy_collecting || self.energy.waiting() == 0 {
            return;
        }
        self.energy_collecting = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(COLLECT_INTERVAL).await;
                let waiting = this.update(cx, |this, cx| {
                    this.collect_energy(cx);
                    let waiting = this.energy.waiting() > 0;
                    this.energy_collecting = waiting;
                    waiting
                });
                if !matches!(waiting, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn collect_energy(&mut self, cx: &mut Context<'_, Self>) {
        let answered = self.energy.collect();
        if answered.is_empty() {
            return;
        }
        let catalogue = Arc::make_mut(&mut self.catalogue);
        for (path, reading) in &answered {
            if let Some(reading) = reading {
                catalogue.set_energy(path, reading.rating);
            }
        }
        let ratings = answered
            .into_iter()
            .map(|(path, reading)| (path, reading.map(|reading| reading.rating)))
            .collect();
        self.browser
            .update(cx, |browser, cx| browser.add_energies(ratings, cx));
        cx.notify();
    }

    /// How many tracks are waiting for their energy, for the header.
    pub(super) fn energy_waiting(&self) -> usize {
        self.energy.waiting()
    }
}
