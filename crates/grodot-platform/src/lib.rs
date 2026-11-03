//! Linux platform layer: an X11 window for presenting frames and reading input,
//! and ALSA audio output. Both libraries are loaded with `dlopen` at runtime, so
//! Grodot builds without any system development packages and starts (headless)
//! even on machines without a display or sound card.

pub mod audio;
pub mod dl;
pub mod window;

pub use audio::{Audio, Clip, Mixer};
pub use window::{Event, MouseButton, PlatformWindow};

/// Sleep until `target` seconds have passed since `start` (frame pacing).
pub fn sleep_until(start: std::time::Instant, target: f64) {
    let elapsed = start.elapsed().as_secs_f64();
    if target > elapsed {
        std::thread::sleep(std::time::Duration::from_secs_f64(target - elapsed));
    }
}
