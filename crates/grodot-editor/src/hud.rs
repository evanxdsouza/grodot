//! In-game HUD drawing, shared by the editor's Play mode and the standalone player.

use grodot_render::canvas::{Canvas, Rect};
use grodot_render::font::FontId;
use grodot_render::raster;
use grodot_render::scene_render::SceneRenderer;
use grodot_render::texture::{draw_nine_slice, draw_texture};
use grodot_render::View;
use grodot_runtime::ui::{Cmd, UiState};
use grodot_runtime::World;

pub fn draw_hud(canvas: &mut Canvas, area: Rect, world: &World) {
    let old = canvas.clip;
    canvas.set_clip(area.intersect(&old));
    let mut y = area.y + 12;
    if !world.hud.lines.is_empty() {
        let w = world.hud.lines.iter().map(|l| canvas.measure(l, FontId::UiBold)).max().unwrap_or(0) + 24;
        let h = world.hud.lines.len() as i32 * 22 + 12;
        canvas.fill_round_rect(Rect::new(area.x + 10, y - 4, w, h), 0x000000, 110);
        for line in &world.hud.lines {
            canvas.text_shadow(area.x + 22, y + 2, line, 0xffffff, FontId::UiBold);
            y += 22;
        }
    }
    if let Some(msg) = &world.hud.message {
        let tw = canvas.measure(msg, FontId::Title);
        let cy = area.y + area.h / 2 - 40;
        let sub_w = world.hud.sub_message.as_ref().map(|s| canvas.measure(s, FontId::UiBold)).unwrap_or(0);
        let bw = tw.max(sub_w) + 80;
        canvas.fill_round_rect(Rect::new(area.x + (area.w - bw) / 2, cy - 18, bw, 100), 0x000000, 150);
        canvas.text_shadow(area.x + (area.w - tw) / 2, cy, msg, 0xffd23f, FontId::Title);
        if let Some(sub) = &world.hud.sub_message {
            canvas.text_shadow(area.x + (area.w - sub_w) / 2, cy + 50, sub, 0xffffff, FontId::UiBold);
        }
    }
    canvas.clip = old;
}

/// Immediate-mode widgets drawn by the game's scripts this frame.
pub fn draw_ui(canvas: &mut Canvas, area: Rect, ui: &UiState, renderer: &mut SceneRenderer) {
    let old = canvas.clip;
    canvas.set_clip(area.intersect(&old));
    let skin = ui.skin.as_ref().and_then(|s| renderer.texture(&s.path).map(|t| (t, s.border.round() as i32)));
    let place = |x: f32, y: f32, w: f32, h: f32| {
        Rect::new(area.x + x.round() as i32, area.y + y.round() as i32, w.round() as i32, h.round() as i32)
    };
    for cmd in &ui.commands {
        match cmd {
            Cmd::Panel { x, y, w, h } => {
                let r = place(*x, *y, *w, *h);
                match &skin {
                    Some((t, border)) => draw_nine_slice(canvas, r, t, *border),
                    None => canvas.fill_round_rect(r, 0x10151f, 215),
                }
            }
            Cmd::Label { x, y, text, color, large } => {
                let font = if *large { FontId::Title } else { FontId::UiBold };
                canvas.text_shadow(area.x + x.round() as i32, area.y + y.round() as i32, text, *color, font);
            }
            Cmd::Image { x, y, w, h, path, border } => {
                if let Some(tex) = renderer.texture(path) {
                    let r = place(*x, *y, *w, *h);
                    if *border > 0.0 {
                        draw_nine_slice(canvas, r, &tex, border.round() as i32);
                    } else {
                        draw_texture(canvas, r, &tex);
                    }
                }
            }
            Cmd::Button { x, y, w, h, label, focused, pressed } => {
                let r = place(*x, *y, *w, *h);
                match &skin {
                    Some((t, border)) => {
                        draw_nine_slice(canvas, r, t, *border);
                        if *pressed {
                            canvas.fill_round_rect(r, 0x000000, 90);
                        } else if *focused {
                            canvas.fill_round_rect(r, 0x3d6fb0, 70);
                        }
                    }
                    None => {
                        let fill = if *pressed {
                            0x1a2230
                        } else if *focused {
                            0x3d6fb0
                        } else {
                            0x2b3442
                        };
                        canvas.fill_round_rect(r, fill, 235);
                    }
                }
                if *focused {
                    canvas.stroke_rect(r, 0xffd23f);
                }
                let text_w = canvas.measure(label, FontId::UiBold);
                let tx = r.x + (r.w - text_w) / 2;
                let ty = r.y + (r.h - 16) / 2;
                let color = if *focused { 0xffd23f } else { 0xffffff };
                canvas.text_shadow(tx, ty, label, color, FontId::UiBold);
            }
        }
    }
    canvas.clip = old;
}

pub fn draw_debug_lines(canvas: &mut Canvas, area: Rect, view: &View, world: &World) {
    for (a, b, c) in &world.debug_lines {
        raster::draw_line_3d(canvas, area, view, *a, *b, c.to_u32(), false);
    }
}
