//! The Dubplate desktop app: two decks, a mixer and the rekordbox collection in one window.

mod analysis;
mod app;
mod assets;
mod audition;
mod auto_cue;
mod auto_gain;
mod browser;
mod collection;
mod commands;
mod controls;
mod cue_marks;
mod deck;
mod devices;
mod effects;
mod energy;
mod finder;
mod folder_picker;
mod fonts;
mod frame_stats;
mod glitches;
mod hardware;
mod history;
mod library;
mod library_match;
mod library_view;
mod loading;
#[cfg(target_os = "macos")]
mod macos_identity;
mod match_cell;
mod menus;
mod meter;
mod pad_fx;
mod pads;
mod phrases;
mod playhead;
mod prep;
mod recording;
mod relink;
mod search;
mod search_filter;
mod set;
mod settings;
mod shortcuts;
mod single_instance;
mod space;
mod stems;
mod tap_tempo;
mod theme;
mod track_edits;
mod track_menu;
mod track_metadata;
mod tracklist;
mod transport;
mod waveform;
mod waveform_cache;
mod widgets;

use std::process::ExitCode;

use gpui::prelude::*;
use gpui::{
    App, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};

use crate::app::DubplateApp;
use crate::assets::AppAssets;
use crate::devices::DeviceAccess;
use crate::settings::Settings;
use crate::single_instance::{InstanceError, InstanceLock};

gpui::actions!(
    dubplate,
    [
        /// Quits Dubplate.
        Quit
    ]
);

/// The app's name, in the menu bar and the Dock.
pub const APP_NAME: &str = "Dubplate";

fn main() -> ExitCode {
    if let Some(exit) = dubplate_stems::run_worker_if_requested() {
        return exit;
    }
    let devices = DeviceAccess::from_environment();
    if devices.is_offline() {
        eprintln!(
            "Offline devices: Dubplate plays to a silent output and opens no controller. Its settings and lock live under HOME."
        );
    }
    let _instance_lock = claim_instance(&devices);
    #[cfg(target_os = "macos")]
    macos_identity::set_app_name(APP_NAME);
    gpui::application()
        .with_assets(AppAssets)
        .run(move |cx: &mut App| {
            gpui::init(cx);
            #[cfg(target_os = "macos")]
            {
                if devices.is_offline() {
                    macos_identity::stay_in_background();
                } else {
                    macos_identity::show_dock_icon();
                }
                macos_identity::stay_responsive();
            }
            if let Err(error) = fonts::register(cx) {
                eprintln!("error: the built-in fonts could not be loaded: {error:#}");
            }
            theme::apply_to_components(cx);
            cx.bind_keys(shortcuts::key_bindings());
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            cx.bind_keys(menus::key_bindings());
            cx.on_action(|_: &Quit, cx| cx.quit());
            menus::register_app_actions(cx);
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            if let Err(error) = open_window(devices.clone(), cx) {
                eprintln!("error: {error:#}");
                cx.quit();
            }
        });
    ExitCode::SUCCESS
}

/// Makes this the one running Dubplate, or brings the running one to the front and quits. When
/// the lock cannot be taken at all, Dubplate starts anyway: the DJ needs the decks more than the
/// guard. A copy with offline devices leaves the running one where it is.
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        unused_variables,
        reason = "only macOS brings the running copy to the front"
    )
)]
fn claim_instance(devices: &DeviceAccess) -> Option<InstanceLock> {
    let path = match Settings::instance_lock_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("warning: Dubplate cannot check for another copy running: {error}");
            return None;
        }
    };
    match InstanceLock::acquire(&path) {
        Ok(lock) => Some(lock),
        Err(error @ InstanceError::AlreadyRunning { .. }) => {
            eprintln!(
                "error: {error}. Quit it before starting another: two would play through the same speakers and both answer the controller."
            );
            #[cfg(target_os = "macos")]
            if let InstanceError::AlreadyRunning {
                process_id: Some(process_id),
            } = error
                && !devices.is_offline()
            {
                macos_identity::bring_to_front(process_id);
            }
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("warning: Dubplate cannot check for another copy running: {error}");
            None
        }
    }
}

fn open_window(devices: DeviceAccess, cx: &mut App) -> anyhow::Result<()> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1440.0), px(900.0)),
            cx,
        ))),
        window_min_size: Some(size(px(1100.0), px(700.0))),
        titlebar: Some(TitlebarOptions {
            title: Some(APP_NAME.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.0), px(15.0))),
        }),
        app_owns_titlebar_drag: true,
        inactive_frame_interval: None,
        low_power_frame_interval: None,
        // The decks must scroll at the display's rate exactly when the Mac is busiest; a frame
        // of both lanes paints in about 3 ms, well inside a 120 Hz frame.
        thermal_frame_interval: None,
        app_id: Some("dubplate".into()),
        focus: !devices.is_offline(),
        ..WindowOptions::default()
    };
    let offline = devices.is_offline();
    gpui::open_window(options, cx, |window, cx| {
        cx.new(|cx| DubplateApp::new(devices, window, cx))
    })?;
    if !offline {
        cx.activate(true);
    }
    Ok(())
}
