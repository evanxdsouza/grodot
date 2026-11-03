//! In-game HUD drawing, shared by the editor's Play mode and the standalone player.

use grodot_render::canvas::{Canvas, Rect};
use grodot_render::font::FontId;
use grodot_render::raster;
use grodot_render::View;
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

pub fn draw_debug_lines(canvas: &mut Canvas, area: Rect, view: &View, world: &World) {
    for (a, b, c) in &world.debug_lines {
        raster::draw_line_3d(canvas, area, view, *a, *b, c.to_u32(), false);
    }
}
