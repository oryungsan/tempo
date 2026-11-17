//! 알람이 울릴 때 / 타이머가 끝났을 때 뜨는 창 (화면 가운데, 항상 위)
use super::theme::*;
use crate::core::{Core, RingKind};
use crate::engine::Cmd;
use eframe::egui::{self, Align2, FontId, Pos2, Stroke};
use std::sync::Arc;

static SHAPE: std::sync::Mutex<Option<super::drag::Shape>> = std::sync::Mutex::new(None);

pub fn show(ctx: &egui::Context, core: &Arc<Core>, p: &Pal) {
    let rings = core.rings();
    if rings.is_empty() {
        *SHAPE.lock().unwrap() = None; // 다음에 새로 뜨는 창에 다시 적용
        return;
    }
    let w = 420.0;
    let h = 92.0 + rings.len() as f32 * 132.0;
    let (l, t, r, b) = core.work_area;
    let pos = egui::pos2((l + r) / 2.0 - w / 2.0, (t + b) / 2.0 - 260.0);
    let builder = egui::ViewportBuilder::default()
        .with_title("Tempo 알림")
        .with_inner_size([w, h])
        .with_position(pos)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_icon(app_icon());
    let p = *p;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("ring"), builder, |ctx, _| {
        let ppp = ctx.pixels_per_point();
        let px = |v: f32| (v * ppp).round() as i32;
        let shape = super::drag::Shape::RoundRect { w: px(w), h: px(h), r: px(18.0) };
        let mut last = SHAPE.lock().unwrap();
        if *last != Some(shape) && super::drag::set_shape("Tempo 알림", shape) {
            *last = Some(shape);
        }
        drop(last);
        egui::CentralPanel::default().frame(egui::Frame::default()).show(ctx, |ui| {
            let full = ui.max_rect().shrink(1.0);
            let pt = ui.painter().clone();
            pt.rect_filled(full, cr(18.0), p.card);
            pt.rect_stroke(full, cr(18.0), Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
            let drag = ui.interact(full, ui.id().with("drag"), egui::Sense::click());
            super::drag::on_press(ctx, &drag, "Tempo 알림", &Default::default());
            // 벨 + 퍼지는 고리
            let tsec = ctx.input(|i| i.time) as f32;
            let c = Pos2::new(full.left() + 46.0, full.top() + 46.0);
            for k in 0..2 {
                let ph = ((tsec * 0.8) + k as f32 * 0.5).fract();
                pt.circle_stroke(c, 16.0 + ph * 16.0, Stroke::new(2.0_f32, p.accent.gamma_multiply(1.0 - ph)));
            }
            pt.circle_filled(c, 17.0, p.accent);
            pt.text(c, Align2::CENTER_CENTER, "⏰", FontId::proportional(17.0), p.on_accent);
            pt.text(Pos2::new(full.left() + 80.0, full.top() + 46.0), Align2::LEFT_CENTER, "Tempo 알림", bold(16.0), p.text);
            ctx.request_repaint_after(std::time::Duration::from_millis(33));

            let mut y = full.top() + 92.0;
            for (kind, title, sub, at) in rings.iter().cloned() {
                let row = egui::Rect::from_min_size(Pos2::new(full.left() + 20.0, y), egui::vec2(full.width() - 40.0, 118.0));
                pt.rect_filled(row, cr(14.0), p.card_hi);
                let ttl_font = if matches!(kind, RingKind::Alarm(_)) { digits(40.0) } else { bold(22.0) };
                let tg = {
                    let mut job = egui::text::LayoutJob::single_section(title.clone(), egui::text::TextFormat { font_id: ttl_font, color: p.text, ..Default::default() });
                    job.wrap = egui::text::TextWrapping::truncate_at_width(row.width() - 110.0);
                    ui.fonts(|f| f.layout_job(job))
                };
                pt.galley(Pos2::new(row.left() + 18.0, row.top() + 14.0), tg, p.text);
                let sg = {
                    let mut job = egui::text::LayoutJob::single_section(sub.clone(), egui::text::TextFormat { font_id: FontId::proportional(14.0), color: p.muted, ..Default::default() });
                    // 알람은 큰 시각 아래 버튼 옆, 나머지는 제목 바로 아래 넓게
                    let is_alarm = matches!(kind, RingKind::Alarm(_));
                    job.wrap = egui::text::TextWrapping::truncate_at_width(row.width() - if is_alarm { 320.0 } else { 40.0 });
                    ui.fonts(|f| f.layout_job(job))
                };
                let sy = if matches!(kind, RingKind::Alarm(_)) { 66.0 } else { 47.0 };
                pt.galley(Pos2::new(row.left() + 18.0, row.top() + sy), sg, p.muted);
                pt.text(Pos2::new(row.right() - 18.0, row.top() + 18.0), Align2::RIGHT_TOP, at.format("%H:%M:%S").to_string(), FontId::proportional(12.0), p.faint);
                let br = egui::Rect::from_min_size(Pos2::new(row.right() - 290.0, row.bottom() - 46.0), egui::vec2(274.0, 34.0));
                let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::right_to_left(egui::Align::Center)));
                let ui = &mut bui;
                match kind {
                    RingKind::Alarm(id) => {
                        if primary(ui, &p, "  끄기  ").clicked() {
                            core.dismiss(kind);
                        }
                        let snooze = core.data().alarms.iter().find(|a| a.id == id).map(|a| a.snooze_min).unwrap_or(5);
                        if ghost(ui, &p, &format!("{snooze}분 뒤 다시")).clicked() {
                            core.snooze(id);
                        }
                    }
                    RingKind::Reminder(id) => {
                        if primary(ui, &p, "  완료  ").clicked() {
                            core.complete_reminder(id);
                        }
                        if ghost(ui, &p, "1시간 뒤").clicked() {
                            core.snooze_reminder(id, 60);
                        }
                        if ghost(ui, &p, "10분 뒤").clicked() {
                            core.snooze_reminder(id, 10);
                        }
                    }
                    RingKind::Dday(id) => {
                        if primary(ui, &p, "  닫기  ").clicked() {
                            core.dismiss(kind);
                        }
                        if ghost(ui, &p, "1시간 뒤").clicked() {
                            core.snooze_dday(id, 60);
                        }
                        if ghost(ui, &p, "10분 뒤").clicked() {
                            core.snooze_dday(id, 10);
                        }
                    }
                    RingKind::Countdown(_) => {
                        if primary(ui, &p, "  닫기  ").clicked() {
                            core.dismiss(kind);
                        }
                    }
                    RingKind::Timer(id) if core.data().timers.iter().any(|t| t.id == id && t.bed.is_some()) => {
                        if primary(ui, &p, "  비우기  ").clicked() {
                            core.bed_clear(id, "ring");
                        }
                        if ghost(ui, &p, "+5분").clicked() {
                            core.dismiss(kind);
                            core.bed_extend(id, 5, "ring");
                        }
                        if ghost(ui, &p, "닫기").clicked() {
                            core.dismiss(kind);
                        }
                    }
                    RingKind::Timer(id) => {
                        if primary(ui, &p, "  닫기  ").clicked() {
                            core.dismiss(kind);
                        }
                        if ghost(ui, &p, "다시 시작").clicked() {
                            core.dismiss(kind);
                            core.cmd(id, Cmd::Start, "ring");
                        }
                    }
                }
                y += 132.0;
            }
        });
    });
}
