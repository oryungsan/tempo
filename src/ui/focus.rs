//! 집중 화면 — 타이머 하나를 크게. 포모도로면 집중/짧은 휴식/긴 휴식 탭 · 세션 · 사이클 · 오늘 기록
use super::styles::{self, Disp};
use super::theme::*;
use crate::core::{Core, TimerView};
use crate::engine::{Cmd, Phase};
use crate::model::{Style, TimerDef};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui};
use std::sync::Arc;

pub enum Action {
    None,
    Back,
    Edit,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Work,
    Short,
    Long,
}

fn kind_of(name: &str) -> Kind {
    if name.starts_with("집중") {
        Kind::Work
    } else if name.starts_with("긴 휴식") {
        Kind::Long
    } else {
        Kind::Short
    }
}

/// 큰 버튼 (그림 아이콘 + 글자)
fn big_button(ui: &mut Ui, rect: Rect, fill: Color32, hover: Color32, fg: Color32, label: &str, icon: Option<bool>) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(("big", label)), Sense::click());
    let p = ui.painter();
    if fill != Color32::TRANSPARENT && fill.r() as u32 + fill.g() as u32 + fill.b() as u32 > 0 {
        // 그림자
        p.rect_filled(rect.translate(egui::vec2(0.0, 6.0)).shrink2(egui::vec2(10.0, 0.0)), cr(18.0), fill.gamma_multiply(0.18));
    }
    p.rect_filled(rect, cr(18.0), if resp.hovered() { hover } else { fill });
    let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), bold(19.0), fg));
    let icon_w = if icon.is_some() { 26.0 } else { 0.0 };
    let x0 = rect.center().x - (g.size().x + icon_w) / 2.0;
    if let Some(running) = icon {
        let c = Pos2::new(x0 + 8.0, rect.center().y);
        if running {
            paint_pause(p, c, 7.0, fg);
        } else {
            paint_play(p, c, 7.0, fg);
        }
    }
    p.galley(Pos2::new(x0 + icon_w, rect.center().y - g.size().y / 2.0), g, fg);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn square_button(ui: &mut Ui, rect: Rect, p: &Pal, id: &str, draw: impl FnOnce(&egui::Painter, Pos2, Color32)) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    ui.painter().rect_filled(rect, cr(18.0), if resp.hovered() { p.card_hi } else { p.input });
    draw(ui.painter(), rect.center(), p.text);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn ui(ui: &mut Ui, core: &Arc<Core>, p: &Pal, def: &TimerDef, v: &TimerView, style: Style) -> Action {
    let mut act = Action::None;
    let d = styles::disp(def, v, p);
    let color = d.color;
    // 위 줄
    ui.horizontal(|ui| {
        if ghost(ui, p, "←  타이머 목록").clicked() {
            act = Action::Back;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ghost(ui, p, "편집").clicked() {
                act = Action::Edit;
            }
            let mut s = style;
            egui::ComboBox::from_id_salt(("focus-style", def.id)).selected_text(format!("디자인 · {}", s.label())).width(140.0).show_ui(ui, |ui| {
                for x in Style::ALL {
                    ui.selectable_value(&mut s, x, x.label());
                }
            });
            if s != style {
                let id = def.id;
                core.update(|dd| {
                    if let Some(t) = dd.timers.iter_mut().find(|t| t.id == id) {
                        t.style = Some(s);
                    }
                });
            }
        });
    });
    ui.add_space(10.0);

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let avail = ui.available_width();
        let w = avail.min(600.0);
        let x0 = ui.cursor().min.x + (avail - w) / 2.0;
        let mut y = ui.cursor().min.y;
        let pt = ui.painter().clone();

        // 이름
        pt.text(Pos2::new(x0 + w / 2.0, y + 12.0), Align2::CENTER_CENTER, &def.name, bold(17.0), p.muted);
        y += 34.0;

        // 포모도로 탭
        if def.pomodoro.is_some() {
            let tabs = [(Kind::Work, "집중"), (Kind::Short, "짧은 휴식"), (Kind::Long, "긴 휴식")];
            let bar = Rect::from_min_size(Pos2::new(x0, y), egui::vec2(w, 60.0));
            pt.rect_filled(bar, cr(18.0), p.input);
            let cur = def.steps.get(v.idx).map(|s| kind_of(&s.name)).unwrap_or(Kind::Work);
            let tw = (w - 12.0) / 3.0;
            for (i, (k, label)) in tabs.iter().enumerate() {
                let r = Rect::from_min_size(Pos2::new(bar.left() + 6.0 + i as f32 * tw, bar.top() + 6.0), egui::vec2(tw, 48.0));
                let resp = ui.interact(r, ui.id().with(("tab", i)), Sense::click());
                let sel = *k == cur;
                if sel {
                    pt.rect_filled(r, cr(14.0), p.bg);
                } else if resp.hovered() {
                    pt.rect_filled(r, cr(14.0), p.card_hi);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                pt.text(r.center(), Align2::CENTER_CENTER, *label, bold(16.0), if sel { color } else { p.muted });
                if resp.clicked() && !sel {
                    // 지금 위치 이후 그 종류의 첫 단계, 없으면 처음부터
                    let n = def.steps.len();
                    let target = (0..n).map(|o| (v.idx + o) % n).find(|j| kind_of(&def.steps[*j].name) == *k);
                    if let Some(j) = target {
                        core.cmd(def.id, Cmd::Goto(j), "focus");
                    }
                }
            }
            y += 84.0;
        }

        // 큰 그림
        let vis_h = match style {
            Style::Ring | Style::Classic => 400.0,
            Style::Minimal => 260.0,
            Style::Digital => 250.0,
            Style::Fill => 280.0,
            Style::Hourglass => 528.0,
        };
        let vis = Rect::from_min_size(Pos2::new(x0, y), egui::vec2(w, vis_h));
        paint_big(&pt, ui.ctx(), vis, style, p, &d, def, v);
        if style == Style::Hourglass {
            // 모래시계를 누르면 뒤집기
            let hit = Rect::from_center_size(Pos2::new(vis.center().x, vis.top() + 165.0), egui::vec2(220.0, 330.0));
            let r = ui.interact(hit, ui.id().with("glass"), Sense::click()).on_hover_text("눌러서 뒤집기 — 이 단계를 처음부터");
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.clicked() {
                core.flip(def.id);
            }
        }
        y += vis_h + 16.0;

        // 현재 단계 상자
        let bx = Rect::from_min_size(Pos2::new(x0, y), egui::vec2(w, 78.0));
        pt.rect_filled(bx, cr(18.0), p.card);
        pt.rect_stroke(bx, cr(18.0), Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
        pt.circle_filled(Pos2::new(bx.left() + 30.0, bx.center().y), 7.0, color);
        let (lbl, main) = match v.phase {
            Phase::Idle => ("준비", def.steps.first().map(|s| s.name.clone()).unwrap_or_default()),
            Phase::Done => ("완료", "수고하셨습니다".to_string()),
            _ if v.waiting => ("다음 단계 대기 — ▶ 로 시작", v.step_name.clone()),
            _ => ("현재 단계", v.step_name.clone()),
        };
        pt.text(Pos2::new(bx.left() + 52.0, bx.center().y - 13.0), Align2::LEFT_CENTER, lbl, FontId::proportional(13.0), p.muted);
        pt.text(Pos2::new(bx.left() + 52.0, bx.center().y + 12.0), Align2::LEFT_CENTER, main, bold(17.0), p.text);
        if let Some(next) = def.steps.get(v.idx + 1).filter(|_| v.phase.active()) {
            pt.text(Pos2::new(bx.right() - 22.0, bx.center().y - 13.0), Align2::RIGHT_CENTER, "다음", FontId::proportional(12.5), p.faint);
            pt.text(Pos2::new(bx.right() - 22.0, bx.center().y + 12.0), Align2::RIGHT_CENTER, format!("{} · {}", next.name, short_dur(next.secs)), FontId::proportional(14.0), p.muted);
        }
        y += 78.0 + 22.0;

        // 버튼 줄
        let bh = 76.0;
        let left_b = Rect::from_min_size(Pos2::new(x0, y), egui::vec2(bh, bh));
        let right_b = Rect::from_min_size(Pos2::new(x0 + w - bh, y), egui::vec2(bh, bh));
        let mid = Rect::from_min_max(Pos2::new(left_b.right() + 18.0, y), Pos2::new(right_b.left() - 18.0, y + bh));
        if square_button(ui, left_b, p, "f-reset", |pp, c, col| paint_reset(pp, c, 9.0, col)).on_hover_text("처음으로").clicked() {
            core.cmd(def.id, Cmd::Reset, "focus");
        }
        let running = v.phase == Phase::Running;
        let label = match v.phase {
            Phase::Running => "일시정지",
            Phase::Paused => if v.waiting { "시작" } else { "이어서" },
            _ => "시작",
        };
        let (fill, fg) = if running { (p.input, p.text) } else { (color, Color32::WHITE) };
        if big_button(ui, mid, fill, if running { p.card_hi } else { color.gamma_multiply(1.1) }, fg, label, Some(running)).clicked() {
            core.cmd(def.id, Cmd::Toggle, "focus");
        }
        if square_button(ui, right_b, p, "f-skip", |pp, c, col| {
            paint_play(pp, Pos2::new(c.x - 7.0, c.y), 7.5, col);
            paint_play(pp, Pos2::new(c.x + 6.0, c.y), 7.5, col);
        })
        .on_hover_text("다음 단계로")
        .clicked()
        {
            if v.phase.active() {
                core.cmd(def.id, Cmd::Skip, "focus");
            } else if def.steps.len() > 1 {
                core.cmd(def.id, Cmd::Goto(1), "focus");
            }
        }
        y += bh + 26.0;
        pt.line_segment([Pos2::new(x0, y), Pos2::new(x0 + w, y)], Stroke::new(1.0_f32, p.border));
        y += 22.0;

        // 아래: 사이클 · 오늘 기록
        let today = core.today(def.id);
        if let Some(pm) = def.pomodoro {
            pt.text(Pos2::new(x0, y), Align2::LEFT_TOP, "긴 휴식까지 사이클", FontId::proportional(13.0), p.muted);
            let (sess, n) = styles::pomo_session(def, v.idx).unwrap_or((1, pm.every));
            let works_done = def.steps.iter().take(if v.phase == Phase::Idle { 0 } else { v.idx }).filter(|s| s.name.starts_with("집중")).count() as u32;
            for i in 0..n {
                let c = Pos2::new(x0 + 10.0 + i as f32 * 28.0, y + 38.0);
                if i < works_done {
                    pt.circle_filled(c, 9.0, color);
                } else if i + 1 == sess && v.phase.active() {
                    pt.circle_stroke(c, 8.0, Stroke::new(2.5_f32, color));
                } else {
                    pt.circle_filled(c, 9.0, p.track);
                }
            }
            pt.text(Pos2::new(x0 + w, y), Align2::RIGHT_TOP, "오늘의 집중 기록", FontId::proportional(13.0), p.muted);
            pt.text(Pos2::new(x0 + w, y + 24.0), Align2::RIGHT_TOP, format!("{}회 · {}", today.focus_count, human_dur(today.focus_secs)), bold(19.0), p.text);
        } else {
            pt.text(Pos2::new(x0, y), Align2::LEFT_TOP, if def.steps.len() > 1 { "단계" } else { "진행" }, FontId::proportional(13.0), p.muted);
            if def.steps.len() > 1 {
                let n = def.steps.len().min(16);
                for i in 0..n {
                    let c = Pos2::new(x0 + 10.0 + i as f32 * 26.0, y + 38.0);
                    let done = v.phase == Phase::Done || (v.phase.active() && i < v.idx);
                    if done {
                        pt.circle_filled(c, 8.0, color);
                    } else if v.phase.active() && i == v.idx {
                        pt.circle_stroke(c, 7.0, Stroke::new(2.5_f32, color));
                    } else {
                        pt.circle_filled(c, 8.0, p.track);
                    }
                }
            } else {
                pt.text(Pos2::new(x0, y + 26.0), Align2::LEFT_TOP, format!("{}%", (d.done_total * 100.0).round()), bold(19.0), p.text);
            }
            pt.text(Pos2::new(x0 + w, y), Align2::RIGHT_TOP, "오늘의 기록", FontId::proportional(13.0), p.muted);
            pt.text(Pos2::new(x0 + w, y + 24.0), Align2::RIGHT_TOP, format!("완료 {}회 · {}", today.completes, human_dur(today.secs)), bold(19.0), p.text);
        }
        y += 80.0;
        ui.allocate_rect(Rect::from_min_max(Pos2::new(x0, ui.cursor().min.y), Pos2::new(x0 + w, y)), Sense::hover());
    });
    act
}

#[allow(clippy::too_many_arguments)]
fn paint_big(pt: &egui::Painter, ctx: &egui::Context, vis: Rect, style: Style, p: &Pal, d: &Disp, def: &TimerDef, v: &TimerView) {
    let c = vis.center();
    let col = if v.phase == Phase::Done { p.green } else { d.color };
    let pill = |pt: &egui::Painter, center: Pos2, text: &str, tc: Color32| {
        let g = ctx.fonts(|f| f.layout_no_wrap(text.to_string(), bold(14.0), tc));
        let r = Rect::from_center_size(center, g.size() + egui::vec2(28.0, 14.0));
        pt.rect_filled(r, cr(r.height() / 2.0), tc.gamma_multiply(0.16));
        pt.galley(r.center() - g.size() / 2.0, g, tc);
    };
    let big_digits = |pt: &egui::Painter, center_bottom: Pos2, size: f32, color: Color32| {
        let f = digits(if d.text.len() > 5 { size * 0.75 } else { size });
        let w = digits_width(ctx, &d.text, &f);
        draw_digits(pt, ctx, Pos2::new(center_bottom.x - w / 2.0, center_bottom.y), false, &d.text, f, color);
    };
    match style {
        Style::Ring => {
            let r = vis.height() / 2.0 - 14.0;
            styles::ring(pt, c, r, 14.0, d.rem_frac, col, p.track);
            pill(pt, Pos2::new(c.x, c.y - 92.0), &d.status, d.status_col);
            big_digits(pt, Pos2::new(c.x, c.y + 50.0), 104.0, d.dcol);
            pt.text(Pos2::new(c.x, c.y + 86.0), Align2::CENTER_CENTER, &d.sub, FontId::proportional(17.0), p.muted);
        }
        Style::Classic => {
            pill(pt, Pos2::new(c.x, vis.top() + 70.0), &d.status, d.status_col);
            big_digits(pt, Pos2::new(c.x, c.y + 60.0), 140.0, d.dcol);
            let bar = Rect::from_center_size(Pos2::new(c.x, c.y + 110.0), egui::vec2(vis.width() * 0.86, 10.0));
            let seg = match v.phase {
                Phase::Idle => SegState::Idle,
                Phase::Done => SegState::Done,
                _ => SegState::Active,
            };
            let frac = if def.steps.len() > 1 { v.frac } else { d.done_total };
            paint_segments(pt, bar, &v.step_secs, &[d.color], v.idx, frac, seg, p.track, p.green);
            pt.text(Pos2::new(c.x, c.y + 146.0), Align2::CENTER_CENTER, &d.sub, FontId::proportional(16.0), p.muted);
        }
        Style::Minimal => {
            pt.text(Pos2::new(c.x, vis.top() + 24.0), Align2::CENTER_CENTER, &d.status, FontId::proportional(15.0), d.status_col);
            big_digits(pt, Pos2::new(c.x, c.y + 78.0), 180.0, d.dcol);
            let y = vis.bottom() - 20.0;
            let l = c.x - vis.width() * 0.4;
            let r = c.x + vis.width() * 0.4;
            pt.line_segment([Pos2::new(l, y), Pos2::new(r, y)], Stroke::new(1.0_f32, p.track));
            pt.line_segment([Pos2::new(l, y), Pos2::new(l + (r - l) * d.done_total, y)], Stroke::new(3.0_f32, col));
        }
        Style::Digital => {
            let panel = vis.shrink2(egui::vec2(10.0, 14.0));
            pt.rect_filled(panel, cr(20.0), styles::lcd_panel(p));
            let lc = match v.phase {
                Phase::Paused if !v.waiting => p.amber,
                Phase::Done => p.green,
                _ => styles::lcd_color(d.color),
            };
            pt.text(Pos2::new(panel.left() + 24.0, panel.top() + 22.0), Align2::LEFT_CENTER, d.status.to_uppercase(), bold(14.0), lc.gamma_multiply(0.8));
            pt.text(Pos2::new(panel.right() - 24.0, panel.top() + 22.0), Align2::RIGHT_CENTER, &d.sub, FontId::proportional(13.0), Color32::from_gray(150));
            let size = if d.text.len() > 5 { 96.0 } else { 128.0 };
            let _ = size;
            let dh = if d.text.len() > 5 { 92.0 } else { 118.0 };
            styles::lcd(pt, Pos2::new(c.x, c.y + 4.0), dh, &d.text, lc, styles::lcd_off(p));
            let n = 20;
            let bw = (panel.width() - 48.0) / n as f32;
            for i in 0..n {
                let on = (i as f32 + 0.5) / n as f32 <= d.done_total;
                pt.rect_filled(Rect::from_min_size(Pos2::new(panel.left() + 24.0 + i as f32 * bw, panel.bottom() - 22.0), egui::vec2(bw - 4.0, 6.0)), cr(1.5), if on { lc } else { styles::lcd_tick_off(p) });
            }
        }
        Style::Hourglass => {
            let (top, falling, angle) = styles::glass_state(v, d);
            if angle > 0.0 || falling {
                ctx.request_repaint_after(std::time::Duration::from_millis(if angle > 0.0 { 16 } else { 40 }));
            }
            let gc = styles::glass_colors(p, d.color);
            styles::hourglass(pt, Pos2::new(c.x, vis.top() + 165.0), 320.0, top, falling, angle, ctx.input(|i| i.time), &gc);
            pill(pt, Pos2::new(c.x, vis.top() + 356.0), &d.status, d.status_col);
            big_digits(pt, Pos2::new(c.x, vis.top() + 484.0), 84.0, d.dcol);
            pt.text(Pos2::new(c.x, vis.top() + 508.0), Align2::CENTER_CENTER, &d.sub, FontId::proportional(16.0), p.muted);
        }
        Style::Fill => {
            let r = vis.shrink2(egui::vec2(4.0, 10.0));
            pt.rect_filled(r, cr(28.0), p.card);
            pt.rect_stroke(r, cr(28.0), Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
            styles::fill(pt, r.shrink(1.0), 27.0, d.done_total, col.gamma_multiply(if p.dark { 0.38 } else { 0.28 }));
            pt.text(Pos2::new(r.left() + 30.0, r.top() + 34.0), Align2::LEFT_CENTER, &d.status, bold(16.0), d.status_col);
            pt.text(Pos2::new(r.right() - 30.0, r.top() + 34.0), Align2::RIGHT_CENTER, format!("{}%", (d.done_total * 100.0).round()), digits(22.0), p.muted);
            big_digits(pt, Pos2::new(c.x, c.y + 62.0), 140.0, d.dcol);
            pt.text(Pos2::new(c.x, r.bottom() - 30.0), Align2::CENTER_CENTER, &d.sub, FontId::proportional(15.0), p.muted);
        }
    }
}
