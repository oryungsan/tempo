//! 월드타임 · 카운트다운 · 디데이 페이지와 편집 창
use super::editor::{color_swatches, sound_picker, Outcome, PickSlot, PickTarget};
use super::main_win::{empty, header_actions};
use super::theme::*;
use crate::core::Core;
use crate::dday::{self, date_ko};
use crate::model::{fmt_target, Countdown, Dday, DdayMode, Id, WorldClock, PALETTE};
use crate::world;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};
use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui};
use std::sync::Arc;

#[derive(Default)]
pub struct ClockState {
    /// 월드타임 「시간 미리 보기」 — 지금부터 몇 분 뒤 (0 = 지금)
    pub shift_min: i32,
    pub picker: Option<CityPicker>,
    pub rename: Option<(Id, String)>,
    pub cd_edit: Option<CountdownEdit>,
    pub dd_edit: Option<DdayEdit>,
}

impl ClockState {
    pub fn apply_pick(&mut self, t: PickTarget, sound: String) {
        if let Some(e) = self.cd_edit.as_mut() {
            if t == PickTarget::Alarm {
                e.c.sound = sound;
            }
        }
    }

    /// 열린 편집 창들 (메인 창 맨 위에 그림)
    pub fn modals(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal, slot: &PickSlot) {
        if let Some(e) = self.picker.as_mut() {
            if let Outcome::Close = e.ui(ctx, core, p) {
                self.picker = None;
            }
        }
        if let Some((id, name)) = self.rename.as_mut() {
            let mut close = false;
            egui::Modal::new(egui::Id::new("world-rename")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.heading("이름 바꾸기");
                ui.add_space(10.0);
                let te = ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY).hint_text("비우면 도시 이름").margin(egui::Margin::symmetric(10, 7)));
                te.request_focus();
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary(ui, p, "  저장  ").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let (id, label) = (*id, name.trim().to_string());
                            core.update(|d| {
                                if let Some(w) = d.world.iter_mut().find(|w| w.id == id) {
                                    w.label = label;
                                }
                            });
                            close = true;
                        }
                        if ghost(ui, p, "취소").clicked() {
                            close = true;
                        }
                    });
                });
            });
            if close {
                self.rename = None;
            }
        }
        if let Some(e) = self.cd_edit.as_mut() {
            if let Outcome::Close = e.ui(ctx, core, p, slot) {
                self.cd_edit = None;
                core.stop_preview();
            }
        }
        if let Some(e) = self.dd_edit.as_mut() {
            if let Outcome::Close = e.ui(ctx, core, p) {
                self.dd_edit = None;
            }
        }
    }
}

fn trunc(ui: &Ui, text: &str, font: FontId, color: Color32, w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(w.max(10.0));
    ui.fonts(|f| f.layout_job(job))
}

fn grid_cols(aw: f32, min_w: f32, gap: f32) -> (usize, f32) {
    let cols = (((aw + gap) / (min_w + gap)).floor() as usize).max(1);
    (cols, (aw - gap * (cols as f32 - 1.0)) / cols as f32)
}

// ── 월드타임 ───────────────────────────────────────────────

const DAY_FACE: Color32 = Color32::from_rgb(0xF4, 0xF5, 0xF7);
const DAY_INK: Color32 = Color32::from_rgb(0x1A, 0x1D, 0x23);
const NIGHT_FACE: Color32 = Color32::from_rgb(0x1B, 0x1F, 0x29);
const NIGHT_INK: Color32 = Color32::from_rgb(0xDD, 0xE3, 0xF0);

/// 아날로그 시계 — 낮엔 밝은 판, 밤엔 어두운 판
#[allow(clippy::too_many_arguments)]
fn analog(pt: &Painter, c: Pos2, r: f32, h: u32, m: u32, s: u32, show_sec: bool, accent: Color32, border: Color32) {
    let day = world::daytime(h);
    let (face, ink) = if day { (DAY_FACE, DAY_INK) } else { (NIGHT_FACE, NIGHT_INK) };
    pt.circle_filled(c, r, face);
    pt.circle_stroke(c, r, Stroke::new(1.0_f32, border));
    let dir = |deg: f32| {
        let a = deg.to_radians();
        egui::vec2(a.sin(), -a.cos())
    };
    for i in 0..12 {
        let v = dir(i as f32 * 30.0);
        let (a, b) = if i % 3 == 0 { (r * 0.74, r * 0.9) } else { (r * 0.8, r * 0.9) };
        pt.line_segment([c + v * a, c + v * b], Stroke::new(if i % 3 == 0 { 2.0_f32 } else { 1.0 }, lerp(face, ink, if i % 3 == 0 { 0.85 } else { 0.45 })));
    }
    sun_moon(pt, c + egui::vec2(0.0, r * 0.45), day, r / 44.0);
    let hd = ((h % 12) as f32 + m as f32 / 60.0) * 30.0;
    let md = (m as f32 + s as f32 / 60.0) * 6.0;
    pt.line_segment([c - dir(hd) * r * 0.1, c + dir(hd) * r * 0.5], Stroke::new(3.2_f32, ink));
    pt.line_segment([c - dir(md) * r * 0.12, c + dir(md) * r * 0.74], Stroke::new(2.2_f32, ink));
    if show_sec {
        let sd = s as f32 * 6.0;
        pt.line_segment([c - dir(sd) * r * 0.18, c + dir(sd) * r * 0.8], Stroke::new(1.2_f32, accent));
    }
    pt.circle_filled(c, 3.0, if show_sec { accent } else { ink });
}

/// 시계판 안 해 · 달 (6시 방향)
fn sun_moon(pt: &Painter, c: Pos2, day: bool, k: f32) {
    if day {
        let col = Color32::from_rgb(0xF2, 0xA9, 0x2C);
        pt.circle_filled(c, 3.4 * k, col);
        for i in 0..8 {
            let a = (i as f32 * 45.0).to_radians();
            let v = egui::vec2(a.cos(), a.sin());
            pt.line_segment([c + v * 5.0 * k, c + v * 6.8 * k], Stroke::new(1.3_f32 * k, col));
        }
    } else {
        pt.circle_filled(c, 5.2 * k, Color32::from_rgb(0xF2, 0xD7, 0x8A));
        pt.circle_filled(c + egui::vec2(2.7, -2.1) * k, 4.5 * k, NIGHT_FACE);
    }
}

pub fn world_page(ui: &mut Ui, core: &Arc<Core>, st: &mut ClockState, p: &Pal) {
    let list: Vec<WorldClock> = core.data().world.clone();
    let utc = Utc::now() + Duration::minutes(st.shift_min as i64);
    let lo = world::local_offset_min(utc);
    let sub = format!("{}  ·  내 PC {}", if list.is_empty() { "도시를 추가해 보세요".to_string() } else { format!("도시 {}곳", list.len()) }, world::utc_label(lo));
    if header_actions(ui, p, "월드타임", &sub, &["+  도시 추가"]) == Some(0) {
        st.picker = Some(CityPicker::default());
    }

    // 시간 미리 보기 (회의 시각 맞추기)
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("시간 미리 보기").size(14.0).color(p.text));
            ui.add_space(6.0);
            let w = (ui.available_width() - 330.0).clamp(160.0, 520.0);
            ui.spacing_mut().slider_width = w;
            ui.add(egui::Slider::new(&mut st.shift_min, -24 * 60..=24 * 60).step_by(15.0).show_value(false));
            let local = Local.from_utc_datetime(&utc.naive_utc());
            let txt = if st.shift_min == 0 {
                "지금".to_string()
            } else {
                format!("{}  →  내 PC {}", world::diff_label(st.shift_min).replace("같은 시각", "지금"), local.format("%H:%M"))
            };
            ui.label(RichText::new(txt).color(if st.shift_min == 0 { p.muted } else { p.accent }).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if st.shift_min != 0 && ghost(ui, p, "지금으로").clicked() {
                    st.shift_min = 0;
                }
                if ui.small_button("+1시간").clicked() {
                    st.shift_min = (st.shift_min + 60).min(24 * 60);
                }
                if ui.small_button("-1시간").clicked() {
                    st.shift_min = (st.shift_min - 60).max(-24 * 60);
                }
            });
        });
    });
    ui.add_space(14.0);

    let shifted = st.shift_min != 0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let gap = 16.0;
        let (cols, w) = grid_cols(ui.available_width(), 272.0, gap);
        // 첫 칸 = 내 PC
        let mut cards: Vec<Option<WorldClock>> = vec![None];
        cards.extend(list.iter().cloned().map(Some));
        for chunk in cards.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for wc in chunk {
                    world_card(ui, core, st, p, wc.as_ref(), utc, lo, w, shifted);
                }
            });
            ui.add_space(gap - 8.0);
        }
        if list.is_empty() {
            ui.add_space(10.0);
            ui.label(RichText::new("자주 보는 도시").color(p.muted).size(13.0));
            ui.horizontal_wrapped(|ui| {
                for tz in ["America/New_York", "America/Los_Angeles", "Europe/London", "Europe/Paris", "Asia/Tokyo", "Asia/Shanghai", "Asia/Singapore", "Australia/Sydney"] {
                    if ghost(ui, p, &format!("+ {}", world::display_name(tz, ""))).clicked() {
                        core.update(|d| d.world.push(WorldClock::new(tz)));
                    }
                }
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn world_card(ui: &mut Ui, core: &Arc<Core>, st: &mut ClockState, p: &Pal, wc: Option<&WorldClock>, utc: chrono::DateTime<Utc>, lo: i32, w: f32, shifted: bool) {
    let h = 150.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    let pt = ui.painter().clone();
    pt.rect_filled(rect, cr(16.0), p.card);
    pt.rect_stroke(rect, cr(16.0), Stroke::new(1.0_f32, if resp.hovered() { p.faint.gamma_multiply(0.6) } else { p.border }), egui::StrokeKind::Inside);
    let ct = match wc {
        Some(wc) => world::at(&wc.tz, utc, lo),
        None => {
            let l = Local.from_utc_datetime(&utc.naive_utc());
            Some(world::CityTime { hour: l.hour(), minute: l.minute(), second: l.second(), date: l.date_naive(), day_diff: 0, diff_min: 0, utc_min: lo })
        }
    };
    let Some(ct) = ct else {
        pt.text(rect.center(), Align2::CENTER_CENTER, format!("알 수 없는 시간대: {}", wc.map(|w| w.tz.as_str()).unwrap_or("")), FontId::proportional(13.0), p.red);
        return;
    };
    let (l, t, r) = (rect.left() + 18.0, rect.top(), rect.right() - 16.0);
    let cr_ = 44.0;
    let cc = Pos2::new(l + cr_, rect.center().y);
    analog(&pt, cc, cr_, ct.hour, ct.minute, ct.second, !shifted, p.accent, p.border);
    let x = cc.x + cr_ + 20.0;
    let tw = r - x;
    let (name, sub) = match wc {
        Some(wc) => {
            let mut sub = format!("{} · {}", world::day_label(ct.day_diff), world::diff_label(ct.diff_min));
            if ct.day_diff == 0 && ct.diff_min == 0 {
                sub = "오늘 · 같은 시각".into();
            }
            (world::display_name(&wc.tz, &wc.label), sub)
        }
        None => ("내 PC".to_string(), if shifted { "미리 보기".into() } else { "현재 시각".into() }),
    };
    pt.galley(Pos2::new(x, t + 18.0), trunc(ui, &name, bold(16.0), p.text, tw - 30.0), p.text);
    let sub_col = match ct.day_diff {
        0 => p.muted,
        _ => p.accent,
    };
    pt.galley(Pos2::new(x, t + 42.0), trunc(ui, &sub, FontId::proportional(12.5), sub_col, tw), sub_col);
    let tcol = if shifted { p.accent } else { p.text };
    let hm = format!("{:02}:{:02}", ct.hour, ct.minute);
    let big = digits(40.0);
    let bw = draw_digits(&pt, ui.ctx(), Pos2::new(x - 2.0, t + 110.0), false, &hm, big, tcol);
    if !shifted {
        draw_digits(&pt, ui.ctx(), Pos2::new(x + bw + 3.0, t + 104.0), false, &format!("{:02}", ct.second), digits(17.0), p.faint);
    }
    let foot = format!("{} · {}", date_ko(ct.date, Local::now().date_naive()), world::utc_label(ct.utc_min));
    pt.galley(Pos2::new(x, t + 118.0), trunc(ui, &foot, FontId::proportional(12.0), p.faint, tw), p.faint);

    // ⋯ 메뉴 (내 PC 칸은 없음)
    let Some(wc) = wc else { return };
    let id = wc.id;
    let mrect = Rect::from_center_size(Pos2::new(r - 8.0, t + 26.0), egui::vec2(28.0, 28.0));
    let mresp = ui.interact(mrect, ui.id().with(("wmenu", id)), Sense::click());
    if resp.hovered() || mresp.hovered() {
        if mresp.hovered() {
            pt.rect_filled(mrect, cr(8.0), p.card_hi);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        for dx in [-5.0f32, 0.0, 5.0] {
            pt.circle_filled(mrect.center() + egui::vec2(dx, 0.0), 1.8, if mresp.hovered() { p.text } else { p.muted });
        }
    }
    let _ = resp.on_hover_text(&wc.tz);
    let pop = ui.id().with(("wpop", id));
    if mresp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(pop));
    }
    egui::popup::popup_below_widget(ui, pop, &mresp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(150.0);
        if ui.button("이름 바꾸기…").clicked() {
            st.rename = Some((id, wc.label.clone()));
        }
        if ui.button("맨 앞으로").clicked() {
            core.update(|d| {
                if let Some(i) = d.world.iter().position(|w| w.id == id) {
                    let w = d.world.remove(i);
                    d.world.insert(0, w);
                }
            });
        }
        ui.separator();
        if ui.button(RichText::new("삭제").color(p.red)).clicked() {
            core.update(|d| d.world.retain(|w| w.id != id));
        }
    });
}

#[derive(Default)]
pub struct CityPicker {
    q: String,
}

impl CityPicker {
    fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal) -> Outcome {
        let mut out = Outcome::Keep;
        let have: Vec<String> = core.data().world.iter().map(|w| w.tz.clone()).collect();
        let utc = Utc::now();
        let lo = world::local_offset_min(utc);
        egui::Modal::new(egui::Id::new("city-picker")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.heading("도시 추가");
            ui.add_space(10.0);
            let te = ui.add(egui::TextEdit::singleline(&mut self.q).desired_width(f32::INFINITY).font(FontId::proportional(15.0)).hint_text("도시 · 나라 이름 (예: 파리, 미국, Reykjavik)").margin(egui::Margin::symmetric(12, 8)));
            if self.q.is_empty() {
                te.request_focus();
            }
            ui.add_space(8.0);
            let res = world::search(&self.q);
            egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                if res.is_empty() {
                    ui.label(RichText::new("찾는 도시가 없습니다 — 영문 시간대 이름으로도 찾을 수 있습니다 (예: Lagos, Karachi)").color(p.muted).size(12.5));
                }
                for (tz, name, country) in res {
                    let added = have.contains(&tz);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), if added { Sense::hover() } else { Sense::click() });
                    let pt = ui.painter();
                    if resp.hovered() && !added {
                        pt.rect_filled(rect, cr(10.0), p.card_hi);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let tc = if added { p.faint } else { p.text };
                    pt.text(Pos2::new(rect.left() + 12.0, rect.center().y - 8.0), Align2::LEFT_CENTER, &name, bold(14.5), tc);
                    pt.text(Pos2::new(rect.left() + 12.0, rect.center().y + 10.0), Align2::LEFT_CENTER, format!("{country} · {tz}"), FontId::proportional(11.5), p.faint);
                    if let Some(ct) = world::at(&tz, utc, lo) {
                        pt.text(Pos2::new(rect.right() - 12.0, rect.center().y - 8.0), Align2::RIGHT_CENTER, format!("{:02}:{:02}", ct.hour, ct.minute), digits(18.0), tc);
                        let s = if added { "추가됨".to_string() } else { format!("{} · {}", world::day_label(ct.day_diff), world::diff_label(ct.diff_min)) };
                        pt.text(Pos2::new(rect.right() - 12.0, rect.center().y + 11.0), Align2::RIGHT_CENTER, s, FontId::proportional(11.5), if added { p.green } else { p.muted });
                    }
                    if resp.clicked() && !added {
                        core.update(|d| d.world.push(WorldClock::new(&tz)));
                    }
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary(ui, p, "  닫기  ").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        out = Outcome::Close;
                    }
                });
            });
        });
        out
    }
}

// ── 카운트다운 ─────────────────────────────────────────────

/// 남은 밀리초 → 표시용 초 (올림 — 끝나기 직전까지 00:01)
fn rem_secs(target: NaiveDateTime, now: NaiveDateTime) -> i64 {
    let ms = (target - now).num_milliseconds();
    if ms > 0 {
        (ms + 999) / 1000
    } else {
        ms / 1000
    }
}

fn since_dt(unix: i64) -> Option<NaiveDateTime> {
    Local.timestamp_opt(unix, 0).single().map(|d| d.naive_local())
}

pub fn countdown_page(ui: &mut Ui, core: &Arc<Core>, st: &mut ClockState, p: &Pal) {
    let mut list: Vec<Countdown> = core.data().countdowns.clone();
    let now = Local::now().naive_local();
    list.sort_by_key(|c| {
        let t = c.target_dt().unwrap_or(now);
        if t > now {
            (0, (t - now).num_seconds())
        } else {
            (1, (now - t).num_seconds())
        }
    });
    let upcoming: Vec<&Countdown> = list.iter().filter(|c| c.target_dt().map(|t| t > now).unwrap_or(false)).collect();
    let sub = match upcoming.first() {
        Some(c) => format!("{}개 · 다음: {} {}", list.len(), c.title, crate::alarm::until_label(Local.from_local_datetime(&c.target_dt().unwrap()).earliest().unwrap_or_else(Local::now), Local::now())),
        None if list.is_empty() => String::new(),
        None => format!("{}개 · 모두 지남", list.len()),
    };
    if header_actions(ui, p, "카운트다운", &sub, &["+  새 카운트다운"]) == Some(0) {
        st.cd_edit = Some(CountdownEdit::new(Countdown::new("", now + Duration::hours(1), PALETTE[(list.len() + 2) % PALETTE.len()]), true));
    }
    if list.is_empty() {
        empty(ui, p, "⏲", "카운트다운이 없습니다", "정한 날짜·시각까지 남은 시간을 초 단위로 보여 주고, 그때가 되면 알려 드립니다.");
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                let today = now.date();
                let ny = NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap();
                let six = today.and_hms_opt(18, 0, 0).unwrap();
                let six = if six > now { six } else { six + Duration::days(1) };
                let presets = [("새해까지", ny, PALETTE[4]), ("퇴근까지", six, PALETTE[2]), ("1시간 뒤", now + Duration::hours(1), PALETTE[0])];
                let w: f32 = 3.0 * 110.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                for (name, at, col) in presets {
                    if ghost(ui, p, &format!("+ {name}")).clicked() {
                        let title = name.trim_end_matches("까지");
                        let title = if name == "1시간 뒤" { "카운트다운" } else { title };
                        st.cd_edit = Some(CountdownEdit::new(Countdown::new(title, at, col), true));
                    }
                }
            });
        });
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for c in &list {
            countdown_row(ui, st, p, c, now);
            ui.add_space(6.0);
        }
    });
}

fn countdown_row(ui: &mut Ui, st: &mut ClockState, p: &Pal, c: &Countdown, now: NaiveDateTime) {
    let h = 118.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    let pt = ui.painter().clone();
    let col = color_of(&c.color);
    let Some(target) = c.target_dt() else { return };
    let secs = rem_secs(target, now);
    let done = secs <= 0;
    pt.rect_filled(rect, cr(16.0), p.card);
    pt.rect_stroke(rect, cr(16.0), Stroke::new(1.0_f32, if resp.hovered() { p.faint.gamma_multiply(0.6) } else { p.border }), egui::StrokeKind::Inside);
    pt.rect_filled(Rect::from_min_size(rect.min + egui::vec2(0.0, 18.0), egui::vec2(3.0, h - 36.0)), cr(2.0), if done { p.green } else { col });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        st.cd_edit = Some(CountdownEdit::new(c.clone(), false));
    }
    let (l, t, r) = (rect.left() + 22.0, rect.top(), rect.right() - 22.0);
    let right_w = 330.0;
    pt.circle_filled(Pos2::new(l + 5.0, t + 28.0), 5.0, if done { p.green } else { col });
    pt.galley(Pos2::new(l + 18.0, t + 18.0), trunc(ui, &c.title, bold(16.5), p.text, r - l - right_w - 18.0), p.text);
    let when = format!("{} {}", date_ko(target.date(), now.date()), target.format(if target.second() == 0 { "%H:%M" } else { "%H:%M:%S" }));
    pt.galley(Pos2::new(l, t + 46.0), trunc(ui, &when, FontId::proportional(13.0), p.muted, r - l - right_w), p.muted);
    // 칩
    let mut cx = l;
    let mut chip_at = |text: &str, fg: Color32, bg: Color32| {
        let g = ui.fonts(|f| f.layout_no_wrap(text.to_string(), FontId::proportional(11.5), fg));
        let rr = Rect::from_min_size(Pos2::new(cx, t + 70.0), g.size() + egui::vec2(14.0, 6.0));
        pt.rect_filled(rr, cr(rr.height() / 2.0), bg);
        pt.galley(rr.min + egui::vec2(7.0, 3.0), g, fg);
        cx = rr.right() + 6.0;
    };
    if c.pinned {
        chip_at("사이드바 고정", p.accent, p.accent.gamma_multiply(0.14));
    }
    if !c.notify {
        chip_at("알림 끔", p.muted, p.input);
    }

    // 오른쪽: 남은 시간
    if done {
        pt.text(Pos2::new(r, t + 50.0), Align2::RIGHT_BOTTOM, "완료", bold(30.0), p.green);
        pt.text(Pos2::new(r, t + 76.0), Align2::RIGHT_BOTTOM, dday::ago_text(-secs), FontId::proportional(13.0), p.muted);
    } else {
        let (days, clock) = dday::remain_text(secs);
        let font = digits(44.0);
        let cw = digits_width(ui.ctx(), &clock, &font);
        let urgent = secs <= 60;
        let dc = if urgent { col } else { p.text };
        draw_digits(&pt, ui.ctx(), Pos2::new(r, t + 78.0), true, &clock, font, dc);
        if !days.is_empty() {
            pt.text(Pos2::new(r - cw - 12.0, t + 74.0), Align2::RIGHT_BOTTOM, &days, bold(28.0), col);
        }
        pt.text(Pos2::new(r, t + 26.0), Align2::RIGHT_BOTTOM, "남음", FontId::proportional(11.5), p.faint);
    }
    // 진행 막대 (만든 때 → 목표)
    let bar = Rect::from_min_size(Pos2::new(l, rect.bottom() - 20.0), egui::vec2(r - l, 5.0));
    pt.rect_filled(bar, cr(2.5), p.track);
    let frac = match since_dt(c.since) {
        _ if done => 1.0,
        Some(s) if target > s => ((now - s).num_milliseconds() as f32 / (target - s).num_milliseconds() as f32).clamp(0.0, 1.0),
        _ => 0.0,
    };
    if frac > 0.0 {
        pt.rect_filled(Rect::from_min_size(bar.min, egui::vec2(bar.width() * frac, bar.height())), cr(2.5), if done { p.green } else { col });
    }
}

/// 날짜 고르기 (년 · 월 · 일) + 요일
fn date_fields(ui: &mut Ui, p: &Pal, date: &mut NaiveDate, years: std::ops::RangeInclusive<i32>) {
    let mut y = date.year();
    let mut m = date.month();
    let mut d = date.day();
    let c1 = ui.add(egui::DragValue::new(&mut y).range(years).speed(0.05).suffix(" 년")).changed();
    let c2 = ui.add(egui::DragValue::new(&mut m).range(1..=12).speed(0.05).suffix(" 월")).changed();
    let c3 = ui.add(egui::DragValue::new(&mut d).range(1..=31).speed(0.1).suffix(" 일")).changed();
    if c1 || c2 || c3 {
        let last = (28..=31).rev().find(|dd| NaiveDate::from_ymd_opt(y, m, *dd).is_some()).unwrap_or(28);
        if let Some(nd) = NaiveDate::from_ymd_opt(y, m, d.min(last)) {
            *date = nd;
        }
    }
    ui.label(RichText::new(format!("({})", world::weekday_ko(*date))).color(p.muted));
}

pub struct CountdownEdit {
    pub c: Countdown,
    is_new: bool,
    date: NaiveDate,
    hour: u32,
    minute: u32,
    second: u32,
    orig_target: String,
    confirm_delete: bool,
}

impl CountdownEdit {
    pub fn new(c: Countdown, is_new: bool) -> Self {
        let t = c.target_dt().unwrap_or_else(|| Local::now().naive_local() + Duration::hours(1));
        CountdownEdit { date: t.date(), hour: t.hour(), minute: t.minute(), second: t.second(), orig_target: c.target.clone(), c, is_new, confirm_delete: false }
    }

    fn set(&mut self, t: NaiveDateTime) {
        self.date = t.date();
        self.hour = t.hour();
        self.minute = t.minute();
        self.second = t.second();
    }

    fn target(&self) -> NaiveDateTime {
        self.date.and_time(NaiveTime::from_hms_opt(self.hour, self.minute, self.second).unwrap_or_default())
    }

    fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal, slot: &PickSlot) -> Outcome {
        let mut out = Outcome::Keep;
        let now = Local::now().naive_local().with_nanosecond(0).unwrap();
        egui::Modal::new(egui::Id::new("cd-edit")).show(ctx, |ui| {
            ui.set_width(540.0);
            ui.heading(if self.is_new { "새 카운트다운" } else { "카운트다운 편집" });
            ui.add_space(12.0);
            ui.add(egui::TextEdit::singleline(&mut self.c.title).desired_width(f32::INFINITY).font(FontId::proportional(16.0)).hint_text("이름 — 예: 여행 출발, 퇴근, 새해").margin(egui::Margin::symmetric(12, 8)));
            ui.add_space(12.0);
            ui.label(RichText::new("언제까지").color(p.muted).size(12.5));
            ui.horizontal_wrapped(|ui| {
                for (l, d) in [("+10분", Duration::minutes(10)), ("+30분", Duration::minutes(30)), ("+1시간", Duration::hours(1)), ("+3시간", Duration::hours(3)), ("+1일", Duration::days(1)), ("+1주", Duration::weeks(1))] {
                    if ui.small_button(l).clicked() {
                        self.set(now + d);
                    }
                }
                ui.separator();
                let today = now.date();
                if ui.small_button("오늘 18:00").clicked() {
                    self.set(today.and_hms_opt(18, 0, 0).unwrap());
                }
                if ui.small_button("내일 09:00").clicked() {
                    self.set((today + Duration::days(1)).and_hms_opt(9, 0, 0).unwrap());
                }
                if ui.small_button("새해").clicked() {
                    self.set(NaiveDate::from_ymd_opt(today.year() + 1, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap());
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                date_fields(ui, p, &mut self.date, 2024..=2100);
                ui.add_space(14.0);
                ui.style_mut().override_font_id = Some(digits(22.0));
                ui.add(egui::DragValue::new(&mut self.hour).range(0..=23).speed(0.08).custom_formatter(|v, _| format!("{:02}", v as u32)));
                ui.label(RichText::new(":").font(digits(22.0)));
                ui.add(egui::DragValue::new(&mut self.minute).range(0..=59).speed(0.15).custom_formatter(|v, _| format!("{:02}", v as u32)));
                ui.label(RichText::new(":").font(digits(22.0)));
                ui.add(egui::DragValue::new(&mut self.second).range(0..=59).speed(0.2).custom_formatter(|v, _| format!("{:02}", v as u32)));
                ui.style_mut().override_font_id = None;
            });
            let target = self.target();
            let secs = (target - now).num_seconds();
            let (txt, col) = if secs > 0 {
                let (d, h, m, s) = dday::split(secs);
                let mut v = Vec::new();
                if d > 0 {
                    v.push(format!("{d}일"));
                }
                if h > 0 {
                    v.push(format!("{h}시간"));
                }
                if m > 0 {
                    v.push(format!("{m}분"));
                }
                if d == 0 && h == 0 && s > 0 {
                    v.push(format!("{s}초"));
                }
                (format!("지금부터 {} 뒤", v.join(" ")), p.accent)
            } else {
                ("이미 지난 시각입니다 — 저장하면 「완료」로 표시됩니다".to_string(), p.amber)
            };
            ui.label(RichText::new(txt).color(col).size(12.5));
            ui.add_space(10.0);
            egui::Grid::new("cd-opts").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                ui.label(RichText::new("색").color(p.muted).size(12.5));
                color_swatches(ui, p, &mut self.c.color);
                ui.end_row();
                ui.label(RichText::new("알림").color(p.muted).size(12.5));
                ui.horizontal(|ui| {
                    toggle(ui, p, &mut self.c.notify);
                    if self.c.notify {
                        ui.add_space(6.0);
                        sound_picker(ui, p, core, "cd-sound", &mut self.c.sound, 170.0, slot, PickTarget::Alarm);
                    } else {
                        ui.label(RichText::new("소리·알림 창 없이 표시만").color(p.faint).size(12.5));
                    }
                });
                ui.end_row();
                ui.label(RichText::new("사이드바").color(p.muted).size(12.5));
                ui.horizontal(|ui| {
                    toggle(ui, p, &mut self.c.pinned);
                    ui.label(RichText::new("왼쪽 아래에 남은 시간 늘 표시").color(p.faint).size(12.5));
                });
                ui.end_row();
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if !self.is_new {
                    if self.confirm_delete {
                        if danger(ui, p, "정말 삭제").clicked() {
                            core.delete_countdown(self.c.id);
                            out = Outcome::Close;
                        }
                        if ghost(ui, p, "아니요").clicked() {
                            self.confirm_delete = false;
                        }
                    } else if danger(ui, p, "삭제").clicked() {
                        self.confirm_delete = true;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = !self.c.title.trim().is_empty();
                    if ui.add_enabled_ui(ok, |ui| primary(ui, p, "  저장  ")).inner.clicked() {
                        self.c.title = self.c.title.trim().to_string();
                        self.c.target = fmt_target(target);
                        if self.is_new || self.c.target != self.orig_target {
                            // 목표를 바꾸면 진행 막대도 지금부터, 지난 시각이면 울리지 않음
                            self.c.since = crate::engine::unix_now();
                            self.c.fired = target <= now;
                            core.dismiss(crate::core::RingKind::Countdown(self.c.id));
                        }
                        core.upsert_countdown(self.c.clone());
                        out = Outcome::Close;
                    }
                    if ghost(ui, p, "취소").clicked() {
                        out = Outcome::Close;
                    }
                });
            });
        });
        out
    }
}

// ── 디데이 ─────────────────────────────────────────────────

/// 정렬 기준 — 가까운 날부터, 지난 「남은 날」은 뒤로
fn dday_key(i: &dday::DdayInfo, mode: DdayMode) -> (i32, i64) {
    match mode {
        DdayMode::Since => (0, i.next.as_ref().map(|n| n.2).unwrap_or(i.days.max(0))),
        _ if i.days >= 0 => (0, i.days),
        _ => (1, -i.days),
    }
}

pub fn dday_page(ui: &mut Ui, core: &Arc<Core>, st: &mut ClockState, p: &Pal) {
    let today = Local::now().date_naive();
    let mut list: Vec<(Dday, dday::DdayInfo)> = core.data().ddays.iter().filter_map(|d| dday::info(d, today).map(|i| (d.clone(), i))).collect();
    list.sort_by_key(|(d, i)| dday_key(i, d.mode));
    let todays: Vec<&str> = list
        .iter()
        .filter(|(d, i)| (d.mode != DdayMode::Since && i.days == 0) || i.next.as_ref().map(|n| n.2 == 0).unwrap_or(false))
        .map(|(d, _)| d.title.as_str())
        .collect();
    let sub = if list.is_empty() {
        String::new()
    } else if !todays.is_empty() {
        format!("{}개 · 오늘: {}", list.len(), todays.join(", "))
    } else {
        format!("{}개 · 오늘 {}", list.len(), date_ko(today, today))
    };
    let color = PALETTE[(list.len() + 4) % PALETTE.len()];
    if header_actions(ui, p, "디데이", &sub, &["+  새 디데이"]) == Some(0) {
        st.dd_edit = Some(DdayEdit::new(Dday::new("", today + Duration::days(30), DdayMode::Until, color), true));
    }
    if list.is_empty() {
        empty(ui, p, "📅", "디데이가 없습니다", "시험·여행까지 남은 날, 기념일부터 지난 날, 해마다 돌아오는 생일을 세어 드립니다.");
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - 360.0) / 2.0).max(0.0));
                for (label, title, mode, date) in [
                    ("+ 시험 · 여행 (남은 날)", "", DdayMode::Until, today + Duration::days(30)),
                    ("+ 기념일 (그날부터)", "", DdayMode::Since, today),
                    ("+ 생일 (매년)", "생일", DdayMode::Yearly, today),
                ] {
                    if ghost(ui, p, label).clicked() {
                        st.dd_edit = Some(DdayEdit::new(Dday::new(title, date, mode, color), true));
                    }
                }
            });
        });
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let gap = 16.0;
        let (cols, w) = grid_cols(ui.available_width(), 250.0, gap);
        for chunk in list.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (d, i) in chunk {
                    dday_card(ui, st, p, d, i, w, today);
                }
            });
            ui.add_space(gap - 8.0);
        }
    });
}

fn dday_card(ui: &mut Ui, st: &mut ClockState, p: &Pal, d: &Dday, i: &dday::DdayInfo, w: f32, today: NaiveDate) {
    let h = 176.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
    let pt = ui.painter().clone();
    let col = color_of(&d.color);
    let is_today = (d.mode != DdayMode::Since && i.days == 0) || i.next.as_ref().map(|n| n.2 == 0).unwrap_or(false);
    let past = d.mode == DdayMode::Until && i.days < 0;
    pt.rect_filled(rect, cr(16.0), if is_today { lerp(p.card, col, if p.dark { 0.16 } else { 0.10 }) } else { p.card });
    let stroke = if is_today { Stroke::new(2.0_f32, col) } else { Stroke::new(1.0_f32, if resp.hovered() { p.faint.gamma_multiply(0.6) } else { p.border }) };
    pt.rect_stroke(rect, cr(16.0), stroke, egui::StrokeKind::Inside);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        st.dd_edit = Some(DdayEdit::new(d.clone(), false));
    }
    let (l, t, r) = (rect.left() + 20.0, rect.top(), rect.right() - 18.0);
    pt.circle_filled(Pos2::new(l + 5.0, t + 26.0), 5.0, col);
    let mode_g = ui.fonts(|f| f.layout_no_wrap(d.mode.label().to_string(), FontId::proportional(11.0), p.muted));
    let mr = Rect::from_min_size(Pos2::new(r - mode_g.size().x - 12.0, t + 16.0), mode_g.size() + egui::vec2(12.0, 6.0));
    pt.rect_filled(mr, cr(mr.height() / 2.0), p.input);
    pt.galley(mr.min + egui::vec2(6.0, 3.0), mode_g, p.muted);
    pt.galley(Pos2::new(l + 18.0, t + 16.0), trunc(ui, &d.title, bold(15.5), p.text, mr.left() - l - 26.0), p.text);

    // 큰 글자
    let big_col = if past { p.muted } else { col };
    let bg = trunc(ui, &i.big, digits(if i.big.chars().count() > 6 { 38.0 } else { 46.0 }), big_col, r - l);
    let by = t + 46.0;
    let bh = bg.size().y;
    pt.galley(Pos2::new(l - 2.0, by), bg, big_col);
    if !i.small.is_empty() {
        pt.text(Pos2::new(r, by + bh - 10.0), Align2::RIGHT_BOTTOM, &i.small, FontId::proportional(13.0), p.muted);
    }
    // 날짜
    let date_line = match d.mode {
        DdayMode::Since => format!("{}부터", date_ko(i.target, today)),
        DdayMode::Yearly => {
            let o = d.date_nd().unwrap_or(i.target);
            format!("{}.{}.{} · 다음 {}", o.year(), o.month(), o.day(), date_ko(i.target, today))
        }
        DdayMode::Until => date_ko(i.target, today),
    };
    pt.galley(Pos2::new(l, t + 118.0), trunc(ui, &date_line, FontId::proportional(12.5), p.muted, r - l), p.muted);
    // 다음 기념일 · 오늘 표시
    let foot = match (&i.next, is_today) {
        (Some((n, _, 0)), _) => Some((format!("오늘 {n}!"), col)),
        (Some((n, dt, left)), _) => Some((format!("다음 {n} · D-{left} ({})", date_ko(*dt, today)), p.faint)),
        (None, true) => Some(("오늘이에요!".to_string(), col)),
        _ => None,
    };
    if let Some((txt, c)) = foot {
        pt.galley(Pos2::new(l, t + 142.0), trunc(ui, &txt, if is_today { bold(13.0) } else { FontId::proportional(12.5) }, c, r - l), c);
    } else if d.pinned {
        pt.galley(Pos2::new(l, t + 142.0), trunc(ui, "사이드바 고정", FontId::proportional(12.0), p.accent, r - l), p.accent);
    }
}

pub struct DdayEdit {
    pub d: Dday,
    is_new: bool,
    date: NaiveDate,
    confirm_delete: bool,
}

impl DdayEdit {
    pub fn new(d: Dday, is_new: bool) -> Self {
        let date = d.date_nd().unwrap_or_else(|| Local::now().date_naive());
        DdayEdit { d, is_new, date, confirm_delete: false }
    }

    fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal) -> Outcome {
        let mut out = Outcome::Keep;
        let today = Local::now().date_naive();
        egui::Modal::new(egui::Id::new("dd-edit")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.heading(if self.is_new { "새 디데이" } else { "디데이 편집" });
            ui.add_space(12.0);
            let hint = match self.d.mode {
                DdayMode::Until => "이름 — 예: 수능, 제주 여행, 프로젝트 마감",
                DdayMode::Since => "이름 — 예: 우리 만난 날, 금연, 입사",
                DdayMode::Yearly => "이름 — 예: 엄마 생신, 결혼기념일",
            };
            ui.add(egui::TextEdit::singleline(&mut self.d.title).desired_width(f32::INFINITY).font(FontId::proportional(16.0)).hint_text(hint).margin(egui::Margin::symmetric(12, 8)));
            ui.add_space(12.0);
            let opts: Vec<(DdayMode, &str)> = DdayMode::ALL.iter().map(|m| (*m, m.label())).collect();
            segmented(ui, p, &mut self.d.mode, &opts);
            ui.label(RichText::new(self.d.mode.desc()).color(p.faint).size(12.0));
            ui.add_space(10.0);
            ui.label(RichText::new(match self.d.mode {
                DdayMode::Until => "그날",
                DdayMode::Since => "시작한 날 (1일째)",
                DdayMode::Yearly => "처음 그날 (생일 = 태어난 날)",
            }).color(p.muted).size(12.5));
            ui.horizontal(|ui| {
                date_fields(ui, p, &mut self.date, 1900..=2100);
                ui.add_space(8.0);
                if ui.small_button("오늘").clicked() {
                    self.date = today;
                }
                if self.d.mode == DdayMode::Until {
                    for (l, n) in [("+100일", 100), ("+1년", 365)] {
                        if ui.small_button(l).clicked() {
                            self.date = today + Duration::days(n);
                        }
                    }
                }
            });
            // 미리 보기
            let mut probe = self.d.clone();
            probe.date = self.date.format("%Y-%m-%d").to_string();
            if let Some(i) = dday::info(&probe, today) {
                ui.add_space(8.0);
                card_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&i.big).font(digits(30.0)).color(color_of(&self.d.color)));
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&i.small).color(p.muted).size(13.0));
                            let extra = match &i.next {
                                Some((n, dt, left)) => format!("다음 {n} · D-{left} ({})", date_ko(*dt, today)),
                                None => date_ko(i.target, today),
                            };
                            ui.label(RichText::new(extra).color(p.faint).size(12.0));
                        });
                    });
                });
            }
            ui.add_space(10.0);
            egui::Grid::new("dd-opts").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                ui.label(RichText::new("색").color(p.muted).size(12.5));
                color_swatches(ui, p, &mut self.d.color);
                ui.end_row();
                ui.label(RichText::new("사이드바").color(p.muted).size(12.5));
                ui.horizontal(|ui| {
                    toggle(ui, p, &mut self.d.pinned);
                    ui.label(RichText::new("왼쪽 아래에 늘 표시").color(p.faint).size(12.5));
                });
                ui.end_row();
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if !self.is_new {
                    if self.confirm_delete {
                        if danger(ui, p, "정말 삭제").clicked() {
                            let id = self.d.id;
                            core.update(|d| d.ddays.retain(|x| x.id != id));
                            out = Outcome::Close;
                        }
                        if ghost(ui, p, "아니요").clicked() {
                            self.confirm_delete = false;
                        }
                    } else if danger(ui, p, "삭제").clicked() {
                        self.confirm_delete = true;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = !self.d.title.trim().is_empty();
                    if ui.add_enabled_ui(ok, |ui| primary(ui, p, "  저장  ")).inner.clicked() {
                        self.d.title = self.d.title.trim().to_string();
                        self.d.date = self.date.format("%Y-%m-%d").to_string();
                        let v = self.d.clone();
                        core.update(|d| match d.ddays.iter_mut().find(|x| x.id == v.id) {
                            Some(x) => *x = v,
                            None => d.ddays.push(v),
                        });
                        out = Outcome::Close;
                    }
                    if ghost(ui, p, "취소").clicked() {
                        out = Outcome::Close;
                    }
                });
            });
        });
        out
    }
}

// ── 사이드바 · API 용 요약 ──────────────────────────────────

/// 사이드바에 고정한 것들 — (색, 이름, 값)
pub fn pinned(core: &Core) -> Vec<(Color32, String, String)> {
    let now = Local::now().naive_local();
    let today = now.date();
    let d = core.data();
    let mut v = Vec::new();
    for c in d.countdowns.iter().filter(|c| c.pinned) {
        let Some(t) = c.target_dt() else { continue };
        let s = rem_secs(t, now);
        let val = if s <= 0 {
            "완료".to_string()
        } else {
            let (days, clock) = dday::remain_text(s);
            if days.is_empty() { clock } else { format!("{days} {}", &clock[..5]) }
        };
        v.push((color_of(&c.color), c.title.clone(), val));
    }
    for x in d.ddays.iter().filter(|x| x.pinned) {
        if let Some(i) = dday::info(x, today) {
            v.push((color_of(&x.color), x.title.clone(), i.big));
        }
    }
    v
}

/// 사이드바 숫자 — 다가오는 카운트다운
pub fn countdown_badge(core: &Core) -> usize {
    let now = Local::now().naive_local();
    core.data().countdowns.iter().filter(|c| c.target_dt().map(|t| t > now).unwrap_or(false)).count()
}

/// 사이드바 숫자 — 오늘인 디데이
pub fn dday_badge(core: &Core) -> usize {
    let today = Local::now().date_naive();
    core.data()
        .ddays
        .iter()
        .filter_map(|d| dday::info(d, today).map(|i| (d.mode, i)))
        .filter(|(m, i)| (*m != DdayMode::Since && i.days == 0) || i.next.as_ref().map(|n| n.2 == 0).unwrap_or(false))
        .count()
}
