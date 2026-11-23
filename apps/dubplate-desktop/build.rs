//! Renders the Dubplate app icon from `assets/logo/app-icon.svg` for the Dock on macOS, so the
//! SVG stays the only copy of the artwork.

use std::error::Error;
use std::path::{Path, PathBuf};

const ICON_SIZE: u32 = 1024;

fn main() -> Result<(), Box<dyn Error>> {
    // Cargo runs the build script from the package directory of the active checkout.
    let artwork = Path::new("../../assets/logo/app-icon.svg");
    println!("cargo::rerun-if-changed={}", artwork.display());
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return Ok(());
    }
    let svg = std::fs::read(artwork)?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE).ok_or("the icon size is zero")?;
    let scale = ICON_SIZE as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.save_png(PathBuf::from(std::env::var("OUT_DIR")?).join("app-icon.png"))?;
    Ok(())
}
