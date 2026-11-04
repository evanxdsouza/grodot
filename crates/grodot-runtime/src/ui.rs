//! Immediate-mode UI for menus and HUDs.
//!
//! Scripts draw widgets from `update()` every frame, the way they call
//! `UI.text` today. A widget call returns its result in the same call: a
//! button returns true on the frame it is clicked or activated from the
//! keyboard or a pad. The host draws the recorded commands after the frame.
//!
//! Coordinates are pixels from the top-left corner of the game area (the window,
//! or the editor's viewport). `UI.screen_size()` gives the size, so layouts can
//! centre things.
//!
//! Focus: the arrow keys and the D-pad move focus between buttons in the order
//! they were drawn last frame. Enter, Space or pad A activates the focused
//! button. Moving the mouse over a button focuses it, and a click activates it.
//! Actions named `ui_up`, `ui_down` and `ui_confirm` in `grodot.toml` replace
//! these defaults.

use crate::world::InputState;

/// Default game-area size, used until the host sets the real one.
pub const DEFAULT_SCREEN: (f32, f32) = (1280.0, 720.0);

const UP_KEYS: &[&str] = &["Up", "PadDpadUp"];
const DOWN_KEYS: &[&str] = &["Down", "PadDpadDown"];
const CONFIRM_KEYS: &[&str] = &["Enter", "Space", "PadA"];

/// One drawn widget, in draw order.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Panel { x: f32, y: f32, w: f32, h: f32 },
    Label { x: f32, y: f32, text: String, color: u32, large: bool },
    Image { x: f32, y: f32, w: f32, h: f32, path: String, border: f32 },
    Button { x: f32, y: f32, w: f32, h: f32, label: String, focused: bool, pressed: bool },
}

/// A texture used for every panel and button, drawn as a nine-slice.
#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
    pub path: String,
    /// Border width in texels; the corners and edges of the skin keep this size.
    pub border: f32,
}

#[derive(Debug, Default)]
pub struct UiState {
    /// Size of the game area in pixels, set by the host each frame (0 = default).
    pub screen: (f32, f32),
    /// Widgets drawn this frame.
    pub commands: Vec<Cmd>,
    pub skin: Option<Skin>,
    /// Label of the focused button.
    pub focus: Option<String>,
    prev_buttons: Vec<String>,
    this_buttons: Vec<String>,
}

impl UiState {
    /// The size of the game area, with the default when the host has not set one.
    pub fn screen_size(&self) -> (f32, f32) {
        if self.screen.0 > 0.0 && self.screen.1 > 0.0 {
            self.screen
        } else {
            DEFAULT_SCREEN
        }
    }

    /// Forget focus and the button list (on a scene change).
    pub fn reset(&mut self) {
        self.focus = None;
        self.prev_buttons.clear();
        self.this_buttons.clear();
        self.skin = None;
    }

    /// Start a frame: clear the draw list and move focus with the navigation keys.
    pub fn begin_frame(&mut self, input: &InputState) {
        self.commands.clear();
        self.prev_buttons = std::mem::take(&mut self.this_buttons);
        let n = self.prev_buttons.len();
        if n == 0 {
            return;
        }
        let down = pressed_any(input, "ui_down", DOWN_KEYS);
        let up = pressed_any(input, "ui_up", UP_KEYS);
        if !down && !up {
            return;
        }
        let at = self.focus.as_ref().and_then(|f| self.prev_buttons.iter().position(|b| b == f));
        let next = match (at, down) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        self.focus = Some(self.prev_buttons[next].clone());
    }

    /// A button. Returns true when it was clicked or activated this frame.
    pub fn button(&mut self, input: &InputState, label: &str, x: f32, y: f32, w: f32, h: f32) -> bool {
        let hovered = inside(input.mouse_position, x, y, w, h);
        if hovered && input.mouse_delta != (0.0, 0.0) {
            self.focus = Some(label.to_string());
        }
        let focused = self.focus.as_deref() == Some(label);
        let clicked = hovered && input.pressed.contains("MouseLeft");
        let pressed = hovered && input.down.contains("MouseLeft");
        let activated = focused && pressed_any(input, "ui_confirm", CONFIRM_KEYS);
        self.this_buttons.push(label.to_string());
        self.commands.push(Cmd::Button { x, y, w, h, label: label.to_string(), focused, pressed: pressed && hovered });
        clicked || activated
    }
}

fn inside((mx, my): (f32, f32), x: f32, y: f32, w: f32, h: f32) -> bool {
    mx >= x && mx < x + w && my >= y && my < y + h
}

/// True if any key bound to `action` was pressed this frame. Projects can rebind
/// the navigation actions; otherwise the defaults apply.
fn pressed_any(input: &InputState, action: &str, defaults: &[&str]) -> bool {
    match input.actions.get(action) {
        Some(keys) => keys.iter().any(|k| input.pressed.contains(k)),
        None => defaults.iter().any(|k| input.pressed.contains(*k)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> InputState {
        InputState::default()
    }

    fn frame(ui: &mut UiState, i: &InputState, labels: &[&str]) -> Vec<bool> {
        ui.begin_frame(i);
        labels
            .iter()
            .map(|l| {
                ui.button(i, l, 100.0, 100.0 + 40.0 * (labels.iter().position(|x| x == l).unwrap() as f32), 200.0, 30.0)
            })
            .collect()
    }

    #[test]
    fn click_activates_the_button_under_the_mouse() {
        let mut ui = UiState::default();
        let mut i = input();
        i.mouse_position = (150.0, 145.0); // inside the second button
        frame(&mut ui, &i, &["Play", "Quit"]);
        i.mouse_delta = (1.0, 0.0); // moving over it focuses it
        i.pressed.insert("MouseLeft".into());
        i.down.insert("MouseLeft".into());
        let r = frame(&mut ui, &i, &["Play", "Quit"]);
        assert_eq!(r, vec![false, true]);
        assert!(matches!(&ui.commands[1], Cmd::Button { pressed: true, focused: true, .. }));
    }

    #[test]
    fn arrow_keys_move_focus_and_confirm_activates() {
        let mut ui = UiState::default();
        let mut i = input();
        frame(&mut ui, &i, &["Play", "Options", "Quit"]);
        // Down from nothing focused goes to the first button.
        i.pressed.insert("Down".into());
        frame(&mut ui, &i, &["Play", "Options", "Quit"]);
        i.pressed.clear();
        assert_eq!(ui.focus.as_deref(), Some("Play"));
        i.pressed.insert("Down".into());
        frame(&mut ui, &i, &["Play", "Options", "Quit"]);
        i.pressed.clear();
        assert_eq!(ui.focus.as_deref(), Some("Options"));
        // Wrap from the last button back to the first.
        i.pressed.insert("Up".into());
        frame(&mut ui, &i, &["Play", "Options", "Quit"]);
        i.pressed.clear();
        i.pressed.insert("Up".into());
        frame(&mut ui, &i, &["Play", "Options", "Quit"]);
        i.pressed.clear();
        assert_eq!(ui.focus.as_deref(), Some("Quit"));
        // Enter on the focused button activates it, and only it.
        i.pressed.insert("Enter".into());
        i.down.insert("Enter".into());
        let r = frame(&mut ui, &i, &["Play", "Options", "Quit"]);
        assert_eq!(r, vec![false, false, true]);
    }

    #[test]
    fn focus_is_forgotten_when_the_button_goes_away() {
        let mut ui = UiState::default();
        let mut i = input();
        frame(&mut ui, &i, &["Resume", "Quit"]);
        i.pressed.insert("Down".into());
        frame(&mut ui, &i, &["Resume", "Quit"]);
        i.pressed.clear();
        // The menu changes: the focused label is gone, so navigation starts over.
        frame(&mut ui, &i, &["Start"]);
        i.pressed.insert("Down".into());
        frame(&mut ui, &i, &["Start"]);
        assert_eq!(ui.focus.as_deref(), Some("Start"));
    }

    #[test]
    fn screen_size_defaults_until_the_host_sets_it() {
        let mut ui = UiState::default();
        assert_eq!(ui.screen_size(), DEFAULT_SCREEN);
        ui.screen = (640.0, 360.0);
        assert_eq!(ui.screen_size(), (640.0, 360.0));
    }
}
