//! The Dubplate desktop app: two decks, a mixer and the rekordbox collection in one window.

mod analysis;
mod app;
mod assets;
mod audio_waveform;
mod audition;
mod browser;
mod collection;
mod commands;
mod controls;
mod cue_marks;
mod deck;
mod effects;
mod energy;
mod folder_picker;
mod fonts;
mod frame_stats;
mod hardware;
mod library;
mod library_view;
mod loading;
#[cfg(target_os = "macos")]
mod macos_identity;
mod meter;
mod pads;
mod phrases;
mod playhead;
mod prep;
mod recording;
mod relink;
mod search;
mod set;
mod settings;
mod shortcuts;
mod single_instance;
mod space;
mod tap_tempo;
mod theme;
mod transport;
mod waveform;
mod waveform_cache;
mod widgets;

use gpui::prelude::*;
use gpui::{
    App, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds, WindowOptions, point,
    px, size,
};

use crate::app::DubplateApp;
use crate::assets::AppAssets;
use crate::settings::Settings;
use crate::single_instance::{InstanceError, InstanceLock};

gpui::actions!(
    dubplate,
    [
        /// Quits Dubplate.
        Quit
    ]
);

const APP_NAME: &str = "Dubplate";

fn main() {
    let _instance_lock = claim_instance();
    #[cfg(target_os = "macos")]
    macos_identity::set_app_name(APP_NAME);
    gpui::application()
        .with_assets(AppAssets)
        .run(|cx: &mut App| {
            gpui::init(cx);
            #[cfg(target_os = "macos")]
            {
                macos_identity::show_dock_icon();
                macos_identity::stay_responsive();
            }
            if let Err(error) = fonts::register(cx) {
                eprintln!("error: the built-in fonts could not be loaded: {error:#}");
            }
            theme::apply_to_components(cx);
            cx.bind_keys(shortcuts::key_bindings());
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus([Menu::new(APP_NAME).items([MenuItem::action("Quit Dubplate", Quit)])]);
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            if let Err(error) = open_window(cx) {
                eprintln!("error: {error:#}");
                cx.quit();
            }
        });
}

/// Makes this the one running Dubplate, or brings the running one to the front and quits. When
/// the lock cannot be taken at all, Dubplate starts anyway: the DJ needs the decks more than the
/// guard.
fn claim_instance() -> Option<InstanceLock> {
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

fn open_window(cx: &mut App) -> anyhow::Result<()> {
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
        app_id: Some("dubplate".into()),
        ..WindowOptions::default()
    };
    gpui::open_window(options, cx, |window, cx| {
        cx.new(|cx| DubplateApp::new(window, cx))
    })?;
    cx.activate(true);
    Ok(())
}
