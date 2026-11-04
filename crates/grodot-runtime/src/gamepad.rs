//! Gamepads through the Linux joystick API (`/dev/input/js0` .. `js7`).
//!
//! No crates: the device is opened non-blocking and read as 8-byte `js_event`
//! records (`u32 time, i16 value, u8 type, u8 number`). Pad buttons feed the
//! ordinary input state under key names such as `PadA` or `PadDpadUp`, so
//! `grodot.toml` bindings like `jump = "Space, PadA"` and `Input.just_pressed`
//! work with no extra code. The left stick also drives the standard movement
//! actions (see [`Gamepad::move_axis`]).
//!
//! Button numbers follow the common xpad layout used by most Xbox-style pads
//! on Linux. Other pads may number their buttons differently, so bindings on
//! unusual hardware may need a `js` remap later.

use crate::world::InputState;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;

/// `O_NONBLOCK` is 0o4000 on Linux for every architecture Grodot targets.
const O_NONBLOCK: i32 = 0o4000;
/// Frames between attempts to find a pad when none is connected.
const RESCAN_FRAMES: u32 = 60;
/// Stick values inside this radius read as zero.
pub const STICK_DEADZONE: f32 = 0.15;
/// Raw joystick button slots we track.
const JS_BUTTONS: usize = 16;

/// One named pad button: its name in Groscript, the key name used in input
/// bindings, and the joystick button number it comes from (if any).
pub struct PadButton {
    pub name: &'static str,
    pub key: &'static str,
    pub js: Option<u8>,
}

/// Standard layout. D-pad buttons come from the hat axes, not from buttons.
pub const PAD_BUTTONS: &[PadButton] = &[
    PadButton { name: "a", key: "PadA", js: Some(0) },
    PadButton { name: "b", key: "PadB", js: Some(1) },
    PadButton { name: "x", key: "PadX", js: Some(2) },
    PadButton { name: "y", key: "PadY", js: Some(3) },
    PadButton { name: "lb", key: "PadLB", js: Some(4) },
    PadButton { name: "rb", key: "PadRB", js: Some(5) },
    PadButton { name: "back", key: "PadBack", js: Some(6) },
    PadButton { name: "start", key: "PadStart", js: Some(7) },
    PadButton { name: "guide", key: "PadGuide", js: Some(8) },
    PadButton { name: "ls", key: "PadLS", js: Some(9) },
    PadButton { name: "rs", key: "PadRS", js: Some(10) },
    PadButton { name: "dpad_up", key: "PadDpadUp", js: None },
    PadButton { name: "dpad_down", key: "PadDpadDown", js: None },
    PadButton { name: "dpad_left", key: "PadDpadLeft", js: None },
    PadButton { name: "dpad_right", key: "PadDpadRight", js: None },
];

/// The key name for a button name ("a" -> "PadA"), if it exists.
pub fn button_key(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    PAD_BUTTONS.iter().find(|b| b.name == lower).map(|b| b.key)
}

/// True for key names such as `PadA` that belong to a pad button.
pub fn is_pad_key(s: &str) -> bool {
    PAD_BUTTONS.iter().any(|b| b.key == s)
}

/// Axis names known to `Gamepad.axis`.
pub const AXIS_NAMES: &[&str] = &["left_x", "left_y", "right_x", "right_y", "left_trigger", "right_trigger"];

pub struct Gamepad {
    file: Option<File>,
    device: Option<String>,
    /// Raw joystick button states, by js button number.
    held: [bool; JS_BUTTONS],
    /// Raw joystick axes, -1..1 (xpad triggers rest at -1).
    axes: [f32; 8],
    /// D-pad state as (up, down, left, right).
    dpad: [bool; 4],
    rescan: u32,
    partial: Vec<u8>,
    /// Treated as connected without a device (tests and replay tools).
    virtual_pad: bool,
}

impl Default for Gamepad {
    fn default() -> Self {
        Gamepad::new()
    }
}

impl Gamepad {
    pub fn new() -> Gamepad {
        Gamepad {
            file: None,
            device: None,
            held: [false; JS_BUTTONS],
            axes: [0.0; 8],
            dpad: [false; 4],
            rescan: 0,
            partial: Vec::new(),
            virtual_pad: false,
        }
    }

    pub fn connected(&self) -> bool {
        self.file.is_some() || self.virtual_pad
    }

    /// Pretend a pad is connected, so events fed with [`Gamepad::feed_event`]
    /// count for the stick. Used by tests and replay tools; the real pad is
    /// found by `poll`.
    pub fn set_virtual(&mut self, on: bool) {
        self.virtual_pad = on;
    }

    /// Device path of the connected pad, e.g. `/dev/input/js0`.
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// Read whatever the pad has sent since the last frame, and look for a pad
    /// when none is connected. Call once per frame, before scripts run.
    pub fn poll(&mut self, input: &mut InputState) {
        if self.file.is_none() {
            if self.rescan == 0 {
                self.rescan = RESCAN_FRAMES;
                self.open();
            } else {
                self.rescan -= 1;
            }
        }
        let mut data = Vec::new();
        let alive = match self.file.as_mut() {
            None => return,
            Some(f) => read_available(f, &mut data),
        };
        self.partial.extend_from_slice(&data);
        while self.partial.len() >= 8 {
            let ev: [u8; 8] = self.partial[..8].try_into().unwrap();
            self.partial.drain(..8);
            self.feed_event(ev, input);
        }
        if !alive {
            self.disconnect(input);
        }
    }

    fn open(&mut self) {
        for i in 0..8 {
            let path = format!("/dev/input/js{}", i);
            if let Ok(f) = OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(&path) {
                self.file = Some(f);
                self.device = Some(path);
                return;
            }
        }
    }

    /// The pad was unplugged: release everything it was holding.
    fn disconnect(&mut self, input: &mut InputState) {
        self.file = None;
        self.device = None;
        self.partial.clear();
        for b in PAD_BUTTONS {
            if let Some(j) = b.js {
                if self.held[j as usize] {
                    input.release(b.key);
                }
            }
        }
        let dpad_keys = ["PadDpadUp", "PadDpadDown", "PadDpadLeft", "PadDpadRight"];
        for (down, key) in self.dpad.iter().zip(dpad_keys) {
            if *down {
                input.release(key);
            }
        }
        self.held = [false; JS_BUTTONS];
        self.dpad = [false; 4];
        self.axes = [0.0; 8];
    }

    /// Apply one raw `js_event`. Public so tests and tools can drive the pad.
    /// Initial-state events (sent when the device opens) set the state
    /// without reporting presses.
    pub fn feed_event(&mut self, ev: [u8; 8], input: &mut InputState) {
        let value = i16::from_le_bytes([ev[4], ev[5]]);
        let init = ev[6] & 0x80 != 0;
        let kind = ev[6] & !0x80;
        let number = ev[7] as usize;
        match kind {
            1 if number < JS_BUTTONS => {
                let down = value != 0;
                if self.held[number] != down {
                    self.held[number] = down;
                    if let Some(b) = PAD_BUTTONS.iter().find(|b| b.js == Some(number as u8)) {
                        set_key(input, b.key, down, !init);
                    }
                }
            }
            2 if number < 8 => {
                let v = (value as f32 / 32767.0).clamp(-1.0, 1.0);
                self.axes[number] = v;
                if number == 6 || number == 7 {
                    self.update_dpad(input, number, v, init);
                }
            }
            _ => {}
        }
    }

    /// Hat axes: 6 is left/right, 7 is up/down (negative is left and up).
    fn update_dpad(&mut self, input: &mut InputState, axis: usize, v: f32, init: bool) {
        let mut want = self.dpad;
        if axis == 6 {
            want[2] = v < -0.5; // left
            want[3] = v > 0.5; // right
        } else {
            want[0] = v < -0.5; // up
            want[1] = v > 0.5; // down
        }
        let keys = ["PadDpadUp", "PadDpadDown", "PadDpadLeft", "PadDpadRight"];
        for i in 0..4 {
            if want[i] != self.dpad[i] {
                set_key(input, keys[i], want[i], !init);
            }
        }
        self.dpad = want;
    }

    /// A stick or trigger value. Sticks run -1..1 with a deadzone; `left_y`
    /// and `right_y` are positive when pushed up. Triggers run 0..1.
    pub fn axis(&self, name: &str) -> f32 {
        if !self.connected() {
            return 0.0;
        }
        match name {
            "left_x" => deadzone(self.axes[0]),
            "left_y" => -deadzone(self.axes[1]),
            "right_x" => deadzone(self.axes[3]),
            "right_y" => -deadzone(self.axes[4]),
            "left_trigger" => (self.axes[2] + 1.0) * 0.5,
            "right_trigger" => (self.axes[5] + 1.0) * 0.5,
            _ => 0.0,
        }
    }

    /// Whether a pad button is held right now (by its name, e.g. "a").
    pub fn down(&self, input: &InputState, name: &str) -> Option<bool> {
        button_key(name).map(|k| input.down.contains(k))
    }

    /// The stick's contribution to one of the standard movement actions, or
    /// `None` when the pair is not a standard one or no pad is connected.
    /// The result has the same sign convention as `Input.axis`.
    pub fn move_axis(&self, neg: &str, pos: &str) -> Option<f32> {
        if !self.connected() {
            return None;
        }
        match (neg, pos) {
            ("move_left", "move_right") => Some(self.axis("left_x")),
            // Forward is the negative direction of move_forward/move_back.
            ("move_forward", "move_back") => Some(-self.axis("left_y")),
            _ => None,
        }
    }
}

/// Press or release a pad key. `edge` is false for initial-state events, which
/// set the held state without reporting a press or release.
fn set_key(input: &mut InputState, key: &str, down: bool, edge: bool) {
    if down {
        if edge {
            input.press(key);
        } else {
            input.down.insert(key.to_string());
        }
    } else if edge {
        input.release(key);
    } else {
        input.down.remove(key);
    }
}

fn deadzone(v: f32) -> f32 {
    let a = v.abs();
    if a < STICK_DEADZONE {
        0.0
    } else {
        v.signum() * (a - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)
    }
}

/// Read until the device would block. Returns false if the device is gone.
fn read_available(f: &mut File, out: &mut Vec<u8>) -> bool {
    let mut buf = [0u8; 8 * 64];
    loop {
        match f.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == ErrorKind::WouldBlock => return true,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: u8, number: u8, value: i16) -> [u8; 8] {
        let v = value.to_le_bytes();
        [0, 0, 0, 0, v[0], v[1], kind, number]
    }

    fn input() -> InputState {
        InputState::default()
    }

    #[test]
    fn buttons_report_edges_and_feed_bindings() {
        let mut pad = Gamepad::new();
        let mut i = input();
        pad.feed_event(ev(1, 0, 1), &mut i);
        assert!(i.pressed.contains("PadA") && i.down.contains("PadA"));
        i.end_frame();
        pad.feed_event(ev(1, 0, 1), &mut i); // repeat: no new edge
        assert!(i.pressed.is_empty());
        pad.feed_event(ev(1, 0, 0), &mut i);
        assert!(i.released.contains("PadA") && !i.down.contains("PadA"));
    }

    #[test]
    fn init_events_set_state_without_edges() {
        let mut pad = Gamepad::new();
        let mut i = input();
        pad.feed_event(ev(0x81, 7, 1), &mut i); // init: Start already held
        assert!(i.down.contains("PadStart"));
        assert!(i.pressed.is_empty());
    }

    #[test]
    fn sticks_have_a_deadzone_and_up_is_positive() {
        let mut pad = Gamepad::new();
        pad.set_virtual(true);
        let mut i = input();
        pad.feed_event(ev(2, 0, 3000), &mut i); // small push: inside the deadzone
        assert_eq!(pad.axis("left_x"), 0.0);
        pad.feed_event(ev(2, 0, 32767), &mut i);
        assert!((pad.axis("left_x") - 1.0).abs() < 1e-4);
        pad.feed_event(ev(2, 1, -32767), &mut i); // stick pushed up
        assert!((pad.axis("left_y") - 1.0).abs() < 1e-4);
    }

    #[test]
    fn triggers_rest_at_zero_and_press_to_one() {
        let mut pad = Gamepad::new();
        pad.set_virtual(true);
        let mut i = input();
        pad.feed_event(ev(2, 2, -32767), &mut i); // xpad trigger at rest
        assert_eq!(pad.axis("left_trigger"), 0.0);
        pad.feed_event(ev(2, 2, 32767), &mut i);
        assert!((pad.axis("left_trigger") - 1.0).abs() < 1e-4);
    }

    #[test]
    fn hat_axes_become_dpad_buttons() {
        let mut pad = Gamepad::new();
        let mut i = input();
        pad.feed_event(ev(2, 6, 32767), &mut i); // right
        assert!(i.pressed.contains("PadDpadRight"));
        pad.feed_event(ev(2, 6, 0), &mut i);
        assert!(i.released.contains("PadDpadRight"));
        pad.feed_event(ev(2, 7, -32767), &mut i); // up
        assert!(i.pressed.contains("PadDpadUp"));
    }

    #[test]
    fn move_axis_maps_the_left_stick_to_standard_actions() {
        let mut pad = Gamepad::new();
        assert_eq!(pad.move_axis("move_left", "move_right"), None, "no pad, no stick");
        pad.set_virtual(true);
        let mut i = input();
        pad.feed_event(ev(2, 1, -32767), &mut i); // stick up = forward
        assert!((pad.move_axis("move_forward", "move_back").unwrap() + 1.0).abs() < 1e-4);
        pad.feed_event(ev(2, 0, 32767), &mut i); // stick right
        assert!((pad.move_axis("move_left", "move_right").unwrap() - 1.0).abs() < 1e-4);
        assert_eq!(pad.move_axis("a", "b"), None);
    }

    #[test]
    fn names_and_keys_line_up() {
        assert_eq!(button_key("a"), Some("PadA"));
        assert_eq!(button_key("DPAD_UP"), Some("PadDpadUp"));
        assert!(is_pad_key("PadStart"));
        assert!(!is_pad_key("Space"));
    }
}
