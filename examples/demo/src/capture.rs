//! Headless render for the demo: `SHOT=<w>:<h>:<out.rgba>` rasterizes the app to raw
//! RGBA through the framework's [`pebbles::shell::capture::capture_rgba`] (no window, no
//! display) so the demo can be screenshotted in CI or over SSH. The capture renders the
//! settled (reduced-motion) frame with the system fonts, like the app. Convert the dump
//! to PNG with any tool (e.g. PIL).

use pebbles::prelude::*;
use pebbles::shell::capture::capture_rgba;

/// `SHOT=<w>:<h>:<out.rgba>` — render the demo app once and dump tight RGBA.
pub fn shot(
    spec: &str,
    root: fn() -> AnyWidget,
    base: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let p: Vec<&str> = spec.split(':').collect();
    let [w, h, out] = p[..] else {
        return Err("SHOT must be <w>:<h>:<out.rgba>".into());
    };
    let (w, h) = (w.parse::<u32>()?, h.parse::<u32>()?);
    let px = capture_rgba(root(), w, h, base)?;
    std::fs::write(out, &px)?;
    println!("{w} {h} {out}");
    Ok(())
}
