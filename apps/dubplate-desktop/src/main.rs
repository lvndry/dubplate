//! The Dubplate desktop app: two decks, a mixer and the rekordbox collection in one window.

mod app;
mod assets;
mod browser;
mod collection;
mod commands;
mod controls;
mod deck;
mod effects;
mod fonts;
mod frame_stats;
mod hardware;
mod library;
mod loading;
#[cfg(target_os = "macos")]
mod macos_identity;
mod meter;
mod pads;
mod playhead;
mod prep;
mod recording;
mod search;
mod set;
mod settings;
mod shortcuts;
mod theme;
mod transport;
mod waveform;
mod widgets;

use gpui::prelude::*;
use gpui::{
    App, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBounds, WindowOptions, point,
    px, size,
};

use crate::app::DubplateApp;
use crate::assets::AppAssets;

gpui::actions!(
    dubplate,
    [
        /// Quits Dubplate.
        Quit
    ]
);

const APP_NAME: &str = "Dubplate";

fn main() {
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
