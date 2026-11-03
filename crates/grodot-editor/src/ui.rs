//! A tiny immediate-mode UI toolkit drawn into the software canvas.

use grodot_render::canvas::{Canvas, Rect, Rgb};
use grodot_render::font::{line_height, FontId};

pub mod theme {
    use grodot_render::Rgb;
    pub const BG: Rgb = 0x1b1c1f;
    pub const PANEL: Rgb = 0x26282c;
    pub const PANEL_DARK: Rgb = 0x202124;
    pub const HEADER: Rgb = 0x2f3136;
    pub const BORDER: Rgb = 0x3a3d43;
    pub const TEXT: Rgb = 0xe2e3e6;
    pub const TEXT_DIM: Rgb = 0x9a9ea6;
    pub const ACCENT: Rgb = 0x5b8cff;
    pub const ACCENT_DARK: Rgb = 0x3d5fae;
    pub const SELECTED: Rgb = 0x34507f;
    pub const BUTTON: Rgb = 0x3a3d45;
    pub const BUTTON_HOVER: Rgb = 0x494d57;
    pub const BUTTON_ACTIVE: Rgb = 0x2c2f35;
    pub const FIELD: Rgb = 0x18191c;
    pub const RED: Rgb = 0xe5534b;
    pub const GREEN: Rgb = 0x57b65f;
    pub const YELLOW: Rgb = 0xe2b340;
    pub const AXIS_X: Rgb = 0xe0584f;
    pub const AXIS_Y: Rgb = 0x6cc04a;
    pub const AXIS_Z: Rgb = 0x4f86e0;
}

use theme::*;

pub fn id_of(parts: &[&str]) -> u64 {
    let mut h: u64 = 1469598103934665603;
    for p in parts {
        for b in p.bytes() {
            h = (h ^ b as u64).wrapping_mul(1099511628211);
        }
        h = (h ^ 0xff).wrapping_mul(1099511628211);
    }
    h
}

#[derive(Default, Clone)]
pub struct InputFrame {
    pub mouse: (f32, f32),
    pub mouse_delta: (f32, f32),
    pub left_down: bool,
    pub left_pressed: bool,
    pub left_released: bool,
    pub right_down: bool,
    pub right_pressed: bool,
    pub middle_down: bool,
    pub wheel: f32,
    /// Keys pressed this frame (including auto-repeats).
    pub keys: Vec<String>,
    /// Keys released this frame.
    pub keys_up: Vec<String>,
    pub focus_lost: bool,
    pub text: String,
    pub ctrl: bool,
    pub shift: bool,
    pub double_click: bool,
}

pub struct Ui {
    pub canvas: Canvas,
    pub input: InputFrame,
    /// Widget currently being dragged or pressed.
    pub active: Option<u64>,
    /// Widget with keyboard focus (text fields).
    pub focus: Option<u64>,
    pub edit_buffer: String,
    pub edit_cursor: usize,
    pub edit_select_all: bool,
    /// A text edit was committed this frame for this widget.
    committed: Option<u64>,
    drag_start_value: f32,
    drag_accum: f32,
    drag_moved: bool,
    /// Tooltip to show at the end of the frame.
    pub tooltip: Option<String>,
    pub cursor_blink: f32,
    /// Area that blocks input to widgets below (open popups).
    pub modal: Option<Rect>,
    pub modal_layer: bool,
    /// Set by the editor: is the mouse over the game viewport (for game mouse input).
    pub hovered_raw_viewport: bool,
}

impl Ui {
    pub fn new(w: usize, h: usize) -> Ui {
        Ui {
            canvas: Canvas::new(w, h),
            input: InputFrame::default(),
            active: None,
            focus: None,
            edit_buffer: String::new(),
            edit_cursor: 0,
            edit_select_all: false,
            committed: None,
            drag_start_value: 0.0,
            drag_accum: 0.0,
            drag_moved: false,
            tooltip: None,
            cursor_blink: 0.0,
            modal: None,
            modal_layer: false,
            hovered_raw_viewport: false,
        }
    }

    pub fn begin_frame(&mut self, input: InputFrame, dt: f32) {
        self.input = input;
        self.tooltip = None;
        self.cursor_blink = (self.cursor_blink + dt) % 1.0;
        self.committed = None;
        self.canvas.reset_clip();
    }

    pub fn end_frame(&mut self) {
        if !self.input.left_down && self.focus.is_none() {
            self.active = None;
        }
        if self.input.left_released {
            self.active = None;
        }
        if let Some(t) = self.tooltip.take() {
            let (mx, my) = (self.input.mouse.0 as i32 + 14, self.input.mouse.1 as i32 + 18);
            let w = self.canvas.measure(&t, FontId::Ui) + 12;
            let r = Rect::new(mx.min(self.canvas.width as i32 - w - 4), my, w, 22);
            self.canvas.fill_rect(r, 0x101113);
            self.canvas.stroke_rect(r, BORDER);
            self.canvas.text(r.x + 6, r.y + 3, &t, TEXT, FontId::Ui);
        }
    }

    /// Is the mouse over `r`, taking open popups into account?
    pub fn hovered(&self, r: Rect) -> bool {
        let (x, y) = (self.input.mouse.0 as i32, self.input.mouse.1 as i32);
        if let Some(m) = self.modal {
            if !self.modal_layer && m.contains(x, y) {
                return false;
            }
        }
        r.contains(x, y)
    }

    pub fn clicked(&self, r: Rect) -> bool {
        self.input.left_pressed && self.hovered(r)
    }

    pub fn key(&self, name: &str) -> bool {
        self.input.keys.iter().any(|k| k == name)
    }

    pub fn has_text_focus(&self) -> bool {
        self.focus.is_some()
    }

    pub fn panel(&mut self, r: Rect, color: Rgb) {
        self.canvas.fill_rect(r, color);
    }

    pub fn header(&mut self, r: Rect, title: &str) {
        self.canvas.fill_rect(r, HEADER);
        self.canvas.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), BORDER);
        let ty = r.y + (r.h - line_height(FontId::UiBold)) / 2;
        self.canvas.text(r.x + 10, ty, title, TEXT, FontId::UiBold);
    }

    pub fn label(&mut self, x: i32, y: i32, text: &str, color: Rgb) -> i32 {
        self.canvas.text(x, y, text, color, FontId::Ui)
    }

    pub fn label_in(&mut self, r: Rect, text: &str, color: Rgb) {
        let ty = r.y + (r.h - line_height(FontId::Ui)) / 2;
        let old = self.canvas.clip;
        self.canvas.set_clip(r.intersect(&old));
        self.canvas.text(r.x, ty, text, color, FontId::Ui);
        self.canvas.clip = old;
    }

    pub fn label_centered(&mut self, r: Rect, text: &str, color: Rgb, font: FontId) {
        let w = self.canvas.measure(text, font);
        let ty = r.y + (r.h - line_height(font)) / 2;
        self.canvas.text(r.x + (r.w - w) / 2, ty, text, color, font);
    }

    fn button_impl(&mut self, r: Rect, text: &str, bg: Rgb, hover_bg: Rgb, fg: Rgb) -> bool {
        let hov = self.hovered(r);
        let pressed = hov && self.input.left_down;
        let c = if pressed { BUTTON_ACTIVE } else if hov { hover_bg } else { bg };
        self.canvas.fill_round_rect(r, c, 255);
        self.label_centered(r, text, fg, FontId::Ui);
        self.clicked(r)
    }

    pub fn button(&mut self, r: Rect, text: &str) -> bool {
        self.button_impl(r, text, BUTTON, BUTTON_HOVER, TEXT)
    }

    pub fn button_accent(&mut self, r: Rect, text: &str) -> bool {
        self.button_impl(r, text, ACCENT_DARK, ACCENT, 0xffffff)
    }

    pub fn button_colored(&mut self, r: Rect, text: &str, bg: Rgb) -> bool {
        self.button_impl(r, text, bg, grodot_render::canvas::blend(bg, 0xffffff, 40), 0xffffff)
    }

    /// A toggle button that is highlighted when `on`.
    pub fn toggle(&mut self, r: Rect, text: &str, on: bool) -> bool {
        if on {
            self.button_impl(r, text, ACCENT_DARK, ACCENT, 0xffffff)
        } else {
            self.button(r, text)
        }
    }

    pub fn checkbox(&mut self, r: Rect, value: &mut bool) -> bool {
        let b = Rect::new(r.x, r.y + (r.h - 16) / 2, 16, 16);
        let hov = self.hovered(b.shrink(-2));
        self.canvas.fill_round_rect(b, if *value { ACCENT } else if hov { BUTTON_HOVER } else { FIELD }, 255);
        self.canvas.stroke_rect(b, if *value { ACCENT } else { BORDER });
        if *value {
            // check mark
            self.canvas.line(b.x as f32 + 3.5, b.y as f32 + 8.0, b.x as f32 + 6.5, b.y as f32 + 11.0, 0xffffff);
            self.canvas.line(b.x as f32 + 6.5, b.y as f32 + 11.0, b.x as f32 + 12.5, b.y as f32 + 4.5, 0xffffff);
            self.canvas.line(b.x as f32 + 3.5, b.y as f32 + 9.0, b.x as f32 + 6.5, b.y as f32 + 12.0, 0xffffff);
            self.canvas.line(b.x as f32 + 6.5, b.y as f32 + 12.0, b.x as f32 + 12.5, b.y as f32 + 5.5, 0xffffff);
        }
        if self.clicked(b.shrink(-2)) {
            *value = !*value;
            return true;
        }
        false
    }

    /// Single-line text field. Returns true when the user commits (Enter or focus loss).
    pub fn text_field(&mut self, id: u64, r: Rect, value: &mut String) -> bool {
        let focused = self.focus == Some(id);
        let hov = self.hovered(r);
        if self.input.left_pressed && hov && !focused {
            self.focus = Some(id);
            self.edit_buffer = value.clone();
            self.edit_cursor = self.edit_buffer.chars().count();
            self.edit_select_all = true;
            self.cursor_blink = 0.0;
        }
        let mut commit = false;
        if self.focus == Some(id) {
            if self.input.left_pressed && !hov {
                commit = true;
            }
            for k in self.input.keys.clone() {
                match k.as_str() {
                    "Enter" | "Tab" => commit = true,
                    "Escape" => {
                        self.focus = None;
                        return false;
                    }
                    "Backspace" => {
                        if self.edit_select_all {
                            self.edit_buffer.clear();
                            self.edit_cursor = 0;
                            self.edit_select_all = false;
                        } else if self.edit_cursor > 0 {
                            let mut chars: Vec<char> = self.edit_buffer.chars().collect();
                            chars.remove(self.edit_cursor - 1);
                            self.edit_buffer = chars.into_iter().collect();
                            self.edit_cursor -= 1;
                        }
                    }
                    "Delete" => {
                        let mut chars: Vec<char> = self.edit_buffer.chars().collect();
                        if self.edit_select_all {
                            chars.clear();
                            self.edit_cursor = 0;
                            self.edit_select_all = false;
                        } else if self.edit_cursor < chars.len() {
                            chars.remove(self.edit_cursor);
                        }
                        self.edit_buffer = chars.into_iter().collect();
                    }
                    "Left" => {
                        self.edit_select_all = false;
                        self.edit_cursor = self.edit_cursor.saturating_sub(1)
                    }
                    "Right" => {
                        self.edit_select_all = false;
                        self.edit_cursor = (self.edit_cursor + 1).min(self.edit_buffer.chars().count())
                    }
                    "Home" => self.edit_cursor = 0,
                    "End" => self.edit_cursor = self.edit_buffer.chars().count(),
                    _ => {}
                }
            }
            let typed = std::mem::take(&mut self.input.text);
            if !typed.is_empty() {
                if self.edit_select_all {
                    self.edit_buffer.clear();
                    self.edit_cursor = 0;
                    self.edit_select_all = false;
                }
                let mut chars: Vec<char> = self.edit_buffer.chars().collect();
                for c in typed.chars() {
                    chars.insert(self.edit_cursor.min(chars.len()), c);
                    self.edit_cursor += 1;
                }
                self.edit_buffer = chars.into_iter().collect();
            }
            if commit {
                *value = self.edit_buffer.clone();
                self.focus = None;
                self.committed = Some(id);
            }
        }
        // draw
        let focused = self.focus == Some(id);
        self.canvas.fill_rect(r, FIELD);
        self.canvas.stroke_rect(r, if focused { ACCENT } else if hov { BUTTON_HOVER } else { BORDER });
        let shown = if focused { self.edit_buffer.clone() } else { value.clone() };
        let inner = r.shrink(1);
        let old = self.canvas.clip;
        self.canvas.set_clip(inner.intersect(&old));
        let ty = r.y + (r.h - line_height(FontId::Ui)) / 2;
        if focused && self.edit_select_all && !shown.is_empty() {
            let w = self.canvas.measure(&shown, FontId::Ui);
            self.canvas.fill_rect(Rect::new(r.x + 5, r.y + 3, w + 1, r.h - 6), ACCENT_DARK);
        }
        self.canvas.text(r.x + 6, ty, &shown, TEXT, FontId::Ui);
        if focused && self.cursor_blink < 0.6 {
            let before: String = shown.chars().take(self.edit_cursor).collect();
            let cx = r.x + 6 + self.canvas.measure(&before, FontId::Ui);
            self.canvas.fill_rect(Rect::new(cx, r.y + 4, 1, r.h - 8), TEXT);
        }
        self.canvas.clip = old;
        commit
    }

    /// A number you can drag horizontally, or click to type.
    pub fn drag_number(&mut self, id: u64, r: Rect, value: &mut f32, speed: f32, accent: Option<Rgb>) -> bool {
        let mut changed = false;
        if self.focus == Some(id) {
            let mut s = self.edit_buffer.clone();
            let before_focus = self.focus;
            if self.text_field(id, r, &mut s) {
                if let Ok(v) = s.trim().parse::<f32>() {
                    if v.is_finite() {
                        *value = v;
                        changed = true;
                    }
                }
            }
            let _ = before_focus;
            return changed;
        }
        let hov = self.hovered(r);
        if self.input.left_pressed && hov {
            self.active = Some(id);
            self.drag_start_value = *value;
            self.drag_accum = 0.0;
            self.drag_moved = false;
        }
        if self.active == Some(id) {
            if self.input.left_down {
                let dx = self.input.mouse_delta.0;
                if dx != 0.0 {
                    self.drag_accum += dx;
                    if self.drag_accum.abs() > 2.0 {
                        self.drag_moved = true;
                    }
                    if self.drag_moved {
                        let mult = if self.input.shift { 0.1 } else { 1.0 };
                        let nv = self.drag_start_value + self.drag_accum * speed * mult;
                        let nv = (nv * 1000.0).round() / 1000.0;
                        if nv != *value {
                            *value = nv;
                            changed = true;
                        }
                    }
                }
            }
            if self.input.left_released {
                if !self.drag_moved {
                    // click: start typing
                    self.focus = Some(id);
                    self.edit_buffer = fmt_f32(*value);
                    self.edit_cursor = self.edit_buffer.chars().count();
                    self.edit_select_all = true;
                }
                self.active = None;
            }
        }
        let bg = if self.active == Some(id) { BUTTON_ACTIVE } else if hov { 0x222429 } else { FIELD };
        self.canvas.fill_rect(r, bg);
        self.canvas.stroke_rect(r, BORDER);
        if let Some(a) = accent {
            self.canvas.fill_rect(Rect::new(r.x, r.y, 2, r.h), a);
        }
        let text = fmt_f32(*value);
        let old = self.canvas.clip;
        self.canvas.set_clip(r.shrink(1).intersect(&old));
        self.label_in(Rect::new(r.x + 6, r.y, r.w - 8, r.h), &text, TEXT);
        self.canvas.clip = old;
        if hov && self.active.is_none() {
            self.tooltip = None;
        }
        changed
    }

    pub fn was_committed(&self, id: u64) -> bool {
        self.committed == Some(id)
    }

    /// A selectable list row. Returns true when clicked.
    pub fn list_row(&mut self, r: Rect, text: &str, selected: bool, indent: i32, icon: Option<(Rgb, &str)>) -> bool {
        let hov = self.hovered(r);
        if selected {
            self.canvas.fill_rect(r, SELECTED);
        } else if hov {
            self.canvas.fill_rect(r, 0x2f3238);
        }
        let mut x = r.x + 8 + indent;
        if let Some((c, glyph)) = icon {
            let ir = Rect::new(x, r.y + (r.h - 14) / 2, 14, 14);
            self.canvas.fill_round_rect(ir, c, 255);
            self.label_centered(ir, glyph, 0x111111, FontId::UiBold);
            x += 20;
        }
        let old = self.canvas.clip;
        self.canvas.set_clip(r.intersect(&old));
        self.label_in(Rect::new(x, r.y, r.right() - x, r.h), text, if selected { 0xffffff } else { TEXT });
        self.canvas.clip = old;
        self.clicked(r)
    }

    pub fn separator(&mut self, x: i32, y: i32, w: i32) {
        self.canvas.fill_rect(Rect::new(x, y, w, 1), BORDER);
    }

    pub fn color_swatch(&mut self, r: Rect, c: Rgb) -> bool {
        self.canvas.fill_rect(r, c);
        self.canvas.stroke_rect(r, BORDER);
        self.clicked(r)
    }

    /// Popup menu; returns the index chosen. Call every frame while open.
    pub fn popup_menu(&mut self, x: i32, y: i32, title: &str, items: &[String]) -> PopupResult {
        let row_h = 24;
        let w = items.iter().map(|s| self.canvas.measure(s, FontId::Ui)).max().unwrap_or(80).max(self.canvas.measure(title, FontId::UiBold)) + 40;
        let h = row_h * items.len() as i32 + 30;
        let x = x.min(self.canvas.width as i32 - w - 4).max(0);
        let y = y.min(self.canvas.height as i32 - h - 4).max(0);
        let r = Rect::new(x, y, w, h);
        self.modal = Some(r);
        self.modal_layer = true;
        self.canvas.fill_rect(Rect::new(r.x + 3, r.y + 3, r.w, r.h), 0x0c0c0e);
        self.canvas.fill_rect(r, 0x2b2e34);
        self.canvas.stroke_rect(r, BORDER);
        self.canvas.text(r.x + 10, r.y + 6, title, TEXT_DIM, FontId::UiBold);
        let mut result = PopupResult::Open;
        for (i, item) in items.iter().enumerate() {
            let rr = Rect::new(r.x + 2, r.y + 28 + i as i32 * row_h, r.w - 4, row_h);
            if self.list_row(rr, item, false, 0, None) {
                result = PopupResult::Chosen(i);
            }
        }
        if result == PopupResult::Open && (self.input.left_pressed && !r.contains(self.input.mouse.0 as i32, self.input.mouse.1 as i32) || self.key("Escape")) {
            result = PopupResult::Closed;
        }
        self.modal_layer = false;
        result
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupResult {
    Open,
    Chosen(usize),
    Closed,
}

pub fn fmt_f32(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e7 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.3}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s == "-0" {
            "0".into()
        } else {
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ui: &mut Ui, f: InputFrame) {
        ui.begin_frame(f, 0.016);
    }

    #[test]
    fn button_click() {
        let mut ui = Ui::new(200, 100);
        let r = Rect::new(10, 10, 80, 24);
        frame(&mut ui, InputFrame { mouse: (20.0, 20.0), left_pressed: true, left_down: true, ..Default::default() });
        assert!(ui.button(r, "OK"));
        ui.end_frame();
        frame(&mut ui, InputFrame { mouse: (150.0, 20.0), left_pressed: true, left_down: true, ..Default::default() });
        assert!(!ui.button(r, "OK"));
    }

    #[test]
    fn text_field_typing() {
        let mut ui = Ui::new(300, 100);
        let r = Rect::new(10, 10, 150, 24);
        let mut s = String::from("Cube");
        frame(&mut ui, InputFrame { mouse: (20.0, 20.0), left_pressed: true, left_down: true, ..Default::default() });
        ui.text_field(1, r, &mut s);
        ui.end_frame();
        frame(&mut ui, InputFrame { mouse: (20.0, 20.0), text: "Player".into(), ..Default::default() });
        ui.text_field(1, r, &mut s);
        ui.end_frame();
        frame(&mut ui, InputFrame { mouse: (20.0, 20.0), keys: vec!["Backspace".into(), "Enter".into()], ..Default::default() });
        assert!(ui.text_field(1, r, &mut s));
        assert_eq!(s, "Playe");
    }

    #[test]
    fn drag_number_drags() {
        let mut ui = Ui::new(300, 100);
        let r = Rect::new(10, 10, 80, 24);
        let mut v = 1.0;
        frame(&mut ui, InputFrame { mouse: (20.0, 20.0), left_pressed: true, left_down: true, ..Default::default() });
        ui.drag_number(7, r, &mut v, 0.1, None);
        ui.end_frame();
        frame(&mut ui, InputFrame { mouse: (40.0, 20.0), mouse_delta: (20.0, 0.0), left_down: true, ..Default::default() });
        assert!(ui.drag_number(7, r, &mut v, 0.1, None));
        assert!((v - 3.0).abs() < 1e-4);
    }

    #[test]
    fn number_formatting() {
        assert_eq!(fmt_f32(1.0), "1");
        assert_eq!(fmt_f32(0.25), "0.25");
        assert_eq!(fmt_f32(-0.0001), "0");
    }
}
