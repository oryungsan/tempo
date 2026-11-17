//! 메인 창 — 사이드바 + 타이머 · 알람 · 스톱워치 · 설정
use super::editor::{self, AlarmEdit, Outcome, PickSlot, TimerEdit};
use super::theme::*;
use crate::core::{Core, TimerView};
use crate::engine::{Cmd, Phase};
use crate::model::{AlarmDef, CardMode, Id, Style, Theme, TimerDef};
use super::styles;
use crate::store;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Timers,
    Alarms,
    Reminders,
    Stopwatch,
    Countdown,
    Dday,
    World,
    Settings,
}

#[derive(Default)]
pub struct Stopwatch {
    since: Option<Instant>,
    acc: Duration,
    laps: Vec<Duration>,
}

impl Stopwatch {
    pub fn elapsed(&self) -> Duration {
        self.acc + self.since.map(|s| s.elapsed()).unwrap_or_default()
    }
    pub fn running(&self) -> bool {
        self.since.is_some()
    }
}

pub struct MainState {
    pub page: Page,
    pub timer_edit: Option<TimerEdit>,
    pub alarm_edit: Option<AlarmEdit>,
    pub sw: Stopwatch,
    pub slot: PickSlot,
    pub reset_cards: bool,
    /// 집중 화면으로 보는 타이머
    pub focus: Option<Id>,
    pub rem: super::reminders::RemState,
    pub clk: super::clocks::ClockState,
}

impl MainState {
    pub fn new() -> Self {
        MainState { page: Page::Timers, timer_edit: None, alarm_edit: None, sw: Stopwatch::default(), slot: Default::default(), reset_cards: false, focus: None, rem: Default::default(), clk: Default::default() }
    }

    /// 카운트다운 · 디데이 카드에서 「편집」 → 그 페이지 + 편집 창
    pub fn open_clock(&mut self, core: &Core, is_dday: bool, id: Id) {
        if is_dday {
            self.page = Page::Dday;
            if let Some(d) = core.data().ddays.iter().find(|x| x.id == id).cloned() {
                self.clk.dd_edit = Some(super::clocks::DdayEdit::new(d, false));
            }
        } else {
            self.page = Page::Countdown;
            if let Some(c) = core.data().countdowns.iter().find(|x| x.id == id).cloned() {
                self.clk.cd_edit = Some(super::clocks::CountdownEdit::new(c, false));
            }
        }
    }

    pub fn edit_timer(&mut self, core: &Core, id: Id) {
        if let Some(t) = core.data().timers.iter().find(|t| t.id == id).cloned() {
            self.page = Page::Timers;
            self.timer_edit = Some(TimerEdit::new(t, false));
        }
    }
}

fn sw_text(d: Duration) -> String {
    let cs = d.as_millis() / 10;
    let (h, m, s, c) = (cs / 360_000, (cs / 6000) % 60, (cs / 100) % 60, cs % 100);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{c:02}")
    } else {
        format!("{m:02}:{s:02}.{c:02}")
    }
}

pub fn ui(ctx: &egui::Context, core: &Arc<Core>, st: &mut MainState, p: &Pal) {
    // 파일 선택 결과 반영
    if let Some((t, s)) = st.slot.lock().unwrap().take() {
        if let Some(e) = st.timer_edit.as_mut() {
            e.apply_pick(t, s.clone());
        }
        if let Some(e) = st.alarm_edit.as_mut() {
            e.apply_pick(t, s.clone());
        }
        if let Some(e) = st.rem.edit.as_mut() {
            e.apply_pick(t, s.clone());
        }
        st.clk.apply_pick(t, s);
    }
    let views = core.views();
    sidebar(ctx, core, st, p, &views);
    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(p.bg).inner_margin(egui::Margin { left: 32, right: 32, top: 26, bottom: 20 }))
        .show(ctx, |ui| match st.page {
            Page::Timers => timers_page(ui, core, st, p, &views),
            Page::Alarms => alarms_page(ui, core, st, p),
            Page::Stopwatch => stopwatch_page(ui, st, p),
            Page::Settings => settings_page(ui, core, st, p),
            Page::Reminders => super::reminders::page(ui, core, &mut st.rem, p),
            Page::Countdown => super::clocks::countdown_page(ui, core, &mut st.clk, p),
            Page::Dday => super::clocks::dday_page(ui, core, &mut st.clk, p),
            Page::World => super::clocks::world_page(ui, core, &mut st.clk, p),
        });
    st.clk.modals(ctx, core, p, &st.slot);
    if let Some(e) = st.timer_edit.as_mut() {
        if let Outcome::Close = e.ui(ctx, core, p, &st.slot) {
            st.timer_edit = None;
        }
    }
    if let Some(e) = st.alarm_edit.as_mut() {
        if let Outcome::Close = e.ui(ctx, core, p, &st.slot) {
            st.alarm_edit = None;
        }
    }
    if let Some(e) = st.rem.edit.as_mut() {
        if let Outcome::Close = e.ui(ctx, core, p, &st.slot) {
            st.rem.edit = None;
        }
    }
}

// ── 사이드바 ───────────────────────────────────────────────

fn sidebar(ctx: &egui::Context, core: &Arc<Core>, st: &mut MainState, p: &Pal, views: &[TimerView]) {
    egui::SidePanel::left("nav")
        .exact_width(216.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::default().fill(p.side).inner_margin(egui::Margin { left: 14, right: 14, top: 22, bottom: 16 }))
        .show(ctx, |ui| {
            // 로고
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), Sense::hover());
                let c = r.center();
                ui.painter().circle_filled(c, 14.0, if p.dark { Color32::from_rgb(0x21, 0x25, 0x2D) } else { Color32::WHITE });
                ui.painter().add(egui::Shape::Path(egui::epaint::PathShape::line(
                    (0..=24).map(|i| {
                        let a = (-90.0f32 + i as f32 * 11.25).to_radians();
                        Pos2::new(c.x + 9.0 * a.cos(), c.y + 9.0 * a.sin())
                    }).collect(),
                    Stroke::new(2.6_f32, p.accent),
                )));
                ui.painter().line_segment([c, Pos2::new(c.x, c.y - 5.5)], Stroke::new(2.0_f32, p.text));
                ui.label(RichText::new("Tempo").font(bold(20.0)).color(p.text));
                // 항상 위 고정 핀
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let on = core.data().settings.main_on_top;
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
                    let bg = if on { p.accent.gamma_multiply(0.18) } else if resp.hovered() { p.card_hi } else { Color32::TRANSPARENT };
                    ui.painter().rect_filled(r, cr(8.0), bg);
                    let col = if on { p.accent } else { p.muted };
                    let c = r.center();
                    // 핀: 머리(둥근) + 몸통 + 바늘
                    ui.painter().circle_filled(c + egui::vec2(2.5, -3.0), 3.6, col);
                    ui.painter().line_segment([c + egui::vec2(-1.5, -6.0), c + egui::vec2(6.5, 2.0)], Stroke::new(2.2_f32, col));
                    ui.painter().line_segment([c + egui::vec2(1.0, 0.0), c + egui::vec2(-5.5, 6.5)], Stroke::new(1.6_f32, col));
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let tip = if on { "항상 위에 표시 중 — 누르면 해제" } else { "이 창을 항상 위에 표시" };
                    if resp.on_hover_text(tip).clicked() {
                        core.update(|d| d.settings.main_on_top = !on);
                    }
                });
            });
            ui.add_space(22.0);
            let running = views.iter().filter(|v| v.phase.active()).count();
            let alarms_on = core.data().alarms.iter().filter(|a| a.enabled).count();
            let sw_on = st.sw.running();
            for (page, icon, label, badge) in [
                (Page::Timers, "⏳", "타이머", if running > 0 { format!("{running}") } else { String::new() }),
                (Page::Alarms, "⏰", "알람", if alarms_on > 0 { format!("{alarms_on}") } else { String::new() }),
                (Page::Reminders, "☑", "리마인더", { let n = super::reminders::badge(core); if n > 0 { format!("{n}") } else { String::new() } }),
                (Page::Stopwatch, "⏱", "스톱워치", if sw_on { "●".into() } else { String::new() }),
                (Page::Countdown, "🏁", "카운트다운", { let n = super::clocks::countdown_badge(core); if n > 0 { format!("{n}") } else { String::new() } }),
                (Page::Dday, "📅", "디데이", { let n = super::clocks::dday_badge(core); if n > 0 { "오늘".into() } else { String::new() } }),
                (Page::World, "🌐", "월드타임", String::new()),
                (Page::Settings, "⚙", "설정", String::new()),
            ] {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), Sense::click());
                let sel = st.page == page;
                if sel {
                    ui.painter().rect_filled(rect, cr(10.0), if p.dark { p.card_hi } else { Color32::WHITE });
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, cr(10.0), p.card_hi.gamma_multiply(0.6));
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let col = if sel { p.text } else { p.muted };
                ui.painter().text(Pos2::new(rect.left() + 22.0, rect.center().y), Align2::CENTER_CENTER, icon, FontId::proportional(16.0), if sel { p.accent } else { p.muted });
                ui.painter().text(Pos2::new(rect.left() + 42.0, rect.center().y), Align2::LEFT_CENTER, label, if sel { bold(14.5) } else { FontId::proportional(14.5) }, col);
                if !badge.is_empty() {
                    let g = ui.fonts(|f| f.layout_no_wrap(badge.clone(), FontId::proportional(11.5), p.accent));
                    let br = Rect::from_center_size(Pos2::new(rect.right() - 20.0, rect.center().y), egui::vec2((g.size().x + 12.0).max(22.0), 20.0));
                    ui.painter().rect_filled(br, cr(10.0), p.accent.gamma_multiply(0.15));
                    ui.painter().galley(br.center() - g.size() / 2.0, g, p.accent);
                }
                if resp.clicked() {
                    st.page = page;
                }
                ui.add_space(-4.0);
            }
            // 아래: 시계 · 고정 항목 · 실행 중 요약 — 메뉴와 겹치지 않게 남은 높이만큼만
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                let row_h = 26.0;
                if ui.available_height() < 64.0 {
                    return;
                }
                let now = chrono::Local::now();
                const WD: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];
                let wd = WD[chrono::Datelike::weekday(&now).num_days_from_monday() as usize];
                ui.label(RichText::new(format!("{} {wd}요일", now.format("%m월 %d일"))).color(p.faint).size(12.0));
                ui.label(RichText::new(now.format("%H:%M").to_string()).font(digits(26.0)).color(p.muted));
                ui.add_space(6.0);
                let mut rows: Vec<(Color32, String, String, Color32)> = Vec::new();
                for v in views.iter().filter(|v| v.phase.active()).take(3) {
                    let rem = if v.n > 1 { v.step_rem } else { v.total_rem };
                    rows.push((color_of(&v.color), v.name.clone(), mmss(rem), if v.phase == Phase::Paused { p.amber } else { p.text }));
                }
                for (col, name, val) in super::clocks::pinned(core).into_iter().take(3) {
                    rows.push((col, name, val, p.text));
                }
                // 아래에서 위로 쌓음 → 실행 중 타이머가 시계 바로 위
                for (col, name, val, vcol) in rows {
                    if ui.available_height() < row_h + 8.0 {
                        break;
                    }
                    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::hover());
                    let pt = ui.painter();
                    pt.circle_filled(Pos2::new(r.left() + 4.0, r.center().y), 3.5, col);
                    let vg = ui.fonts(|f| f.layout_no_wrap(val, digits(13.0), vcol));
                    let vw = vg.size().x;
                    pt.galley(Pos2::new(r.right() - vw, r.center().y - vg.size().y / 2.0), vg, vcol);
                    let mut job = egui::text::LayoutJob::single_section(name, egui::TextFormat { font_id: FontId::proportional(12.5), color: p.muted, ..Default::default() });
                    job.wrap = egui::text::TextWrapping::truncate_at_width((r.width() - vw - 24.0).max(20.0));
                    let ng = ui.fonts(|f| f.layout_job(job));
                    pt.galley(Pos2::new(r.left() + 14.0, r.center().y - ng.size().y / 2.0), ng, p.muted);
                }
            });
        });
}

/// 오른쪽에 버튼 여러 개 (첫 번째 = 강조) — 누른 번호
pub(super) fn header_actions(ui: &mut Ui, p: &Pal, title: &str, sub: &str, actions: &[&str]) -> Option<usize> {
    let mut hit = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(bold(24.0)).color(p.text));
            if !sub.is_empty() {
                ui.label(RichText::new(sub).color(p.muted).size(13.0));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (i, a) in actions.iter().enumerate() {
                let r = if i == 0 { primary(ui, p, a) } else { ghost(ui, p, a) };
                if r.clicked() {
                    hit = Some(i);
                }
            }
        });
    });
    ui.add_space(18.0);
    hit
}

fn page_header(ui: &mut Ui, p: &Pal, title: &str, sub: &str, action: Option<&str>) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).font(bold(24.0)).color(p.text));
            if !sub.is_empty() {
                ui.label(RichText::new(sub).color(p.muted).size(13.0));
            }
        });
        if let Some(a) = action {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = primary(ui, p, a).clicked();
            });
        }
    });
    ui.add_space(18.0);
    clicked
}

// ── 타이머 ─────────────────────────────────────────────────

fn timers_page(ui: &mut Ui, core: &Arc<Core>, st: &mut MainState, p: &Pal, views: &[TimerView]) {
    let defs: Vec<TimerDef> = core.data().timers.clone();
    let global_style = core.data().settings.style;
    if let Some(fid) = st.focus {
        if let (Some(def), Some(v)) = (defs.iter().find(|t| t.id == fid), views.iter().find(|v| v.id == fid)) {
            match super::focus::ui(ui, core, p, def, v, styles::style_of(def, global_style)) {
                super::focus::Action::Back => st.focus = None,
                super::focus::Action::Edit => st.timer_edit = Some(TimerEdit::new(def.clone(), false)),
                super::focus::Action::None => {}
            }
            return;
        }
        st.focus = None;
    }
    let running = views.iter().filter(|v| v.phase.active()).count();
    let sub = if defs.is_empty() { String::new() } else { format!("{}개 · 실행 중 {}", defs.len(), running) };
    match header_actions(ui, p, "타이머", &sub, &["+  새 타이머", "포모도로"]) {
        Some(0) => st.timer_edit = Some(TimerEdit::new(editor::new_timer_template(defs.len()), true)),
        Some(1) => st.timer_edit = Some(TimerEdit::new(TimerDef::pomodoro(crate::model::PALETTE[4]), true)),
        _ => {}
    }
    if defs.is_empty() {
        empty(ui, p, "⏳", "아직 타이머가 없습니다", "「새 타이머」로 카운트다운이나 여러 단계 루틴을 만들어 보세요.");
        return;
    }
    // 「API」 표시는 설정에서 직접 지정한 경우만 (API 자체는 지정이 없으면 첫 타이머를 씀)
    let auto = core.data().settings.automation_timer;
    let busy = core.hotkey_busy.lock().unwrap().clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let aw = ui.available_width();
        let gap = 16.0;
        let cols = (((aw + gap) / (310.0 + gap)).floor() as usize).max(1);
        let w = (aw - gap * (cols as f32 - 1.0)) / cols as f32;
        for chunk in defs.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for def in chunk {
                    if let Some(v) = views.iter().find(|v| v.id == def.id) {
                        timer_card(ui, core, st, p, def, v, w, auto == Some(def.id), &busy, styles::style_of(def, global_style));
                    }
                }
            });
            ui.add_space(gap - 8.0);
        }
    });
}

pub(super) fn empty(ui: &mut Ui, p: &Pal, icon: &str, title: &str, sub: &str) {
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon).size(42.0).color(p.faint));
        ui.add_space(8.0);
        ui.label(RichText::new(title).font(bold(17.0)).color(p.text));
        ui.label(RichText::new(sub).color(p.muted).size(13.0));
    });
}

#[allow(clippy::too_many_arguments)]
fn timer_card(ui: &mut Ui, core: &Arc<Core>, st: &mut MainState, p: &Pal, def: &TimerDef, v: &TimerView, w: f32, is_auto: bool, busy: &std::collections::HashSet<String>, style: Style) {
    let h = 214.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
    let pt = ui.painter().clone();
    let color = color_of(&def.color);
    let multi = def.steps.len() > 1;
    pt.rect_filled(rect, cr(16.0), p.card);
    let d = styles::disp(def, v, p);
    if style == Style::Fill {
        styles::fill(&pt, rect.shrink(1.0), 15.0, d.done_total, (if v.phase == Phase::Done { p.green } else { color }).gamma_multiply(if p.dark { 0.32 } else { 0.24 }));
    }
    pt.rect_stroke(rect, cr(16.0), Stroke::new(1.0_f32, if resp.hovered() { p.faint.gamma_multiply(0.6) } else { p.border }), egui::StrokeKind::Inside);
    if v.phase.active() && style != Style::Fill {
        // 실행 중 = 왼쪽 색 띠
        pt.rect_filled(Rect::from_min_size(rect.min + egui::vec2(0.0, 18.0), egui::vec2(3.0, h - 36.0)), cr(2.0), color);
    }
    if resp.clicked() {
        st.focus = Some(def.id);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (l, r, t) = (rect.left() + 20.0, rect.right() - 20.0, rect.top());

    // 이름
    pt.circle_filled(Pos2::new(l + 5.0, t + 28.0), 5.0, color);
    let name_g = {
        let mut job = egui::text::LayoutJob::single_section(def.name.clone(), egui::TextFormat { font_id: bold(15.5), color: p.text, ..Default::default() });
        job.wrap = egui::text::TextWrapping::truncate_at_width(w - 110.0);
        ui.fonts(|f| f.layout_job(job))
    };
    pt.galley(Pos2::new(l + 18.0, t + 28.0 - name_g.size().y / 2.0), name_g, p.text);

    // ⋯ 메뉴
    let mrect = Rect::from_center_size(Pos2::new(r - 10.0, t + 28.0), egui::vec2(30.0, 30.0));
    let mresp = ui.interact(mrect, ui.id().with(("menu", def.id)), Sense::click());
    if mresp.hovered() {
        pt.rect_filled(mrect, cr(8.0), p.card_hi);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    for dx in [-5.5f32, 0.0, 5.5] {
        pt.circle_filled(mrect.center() + egui::vec2(dx, 0.0), 1.9, if mresp.hovered() { p.text } else { p.muted });
    }
    let pop = ui.id().with(("pop", def.id));
    if mresp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(pop));
    }
    egui::popup::popup_below_widget(ui, pop, &mresp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(190.0);
        if ui.button("편집…").clicked() {
            st.timer_edit = Some(TimerEdit::new(def.clone(), false));
        }
        if ui.button("복제").clicked() {
            core.upsert_timer(editor::duplicate(def));
        }
        ui.separator();
        ui.label(RichText::new("떠 있는 카드").color(p.faint).size(12.0));
        for (m, label) in [(CardMode::Always, "항상 표시"), (CardMode::WhenActive, "실행 중에만"), (CardMode::Off, "끄기")] {
            if ui.selectable_label(def.card == m, label).clicked() {
                let id = def.id;
                core.update(|d| {
                    if let Some(t) = d.timers.iter_mut().find(|t| t.id == id) {
                        t.card = m;
                    }
                });
            }
        }
        ui.separator();
        if ui.selectable_label(is_auto, "자동화 연동 타이머 (API)").clicked() {
            let id = def.id;
            core.update(|d| d.settings.automation_timer = Some(id));
        }
        ui.separator();
        if ui.button(RichText::new("삭제…").color(p.red)).clicked() {
            let mut e = TimerEdit::new(def.clone(), false);
            e.def = def.clone();
            st.timer_edit = Some(e);
        }
    });

    if style != Style::Classic {
        card_body(ui, &pt, p, def, v, &d, style, Rect::from_min_max(Pos2::new(l, t + 46.0), Pos2::new(r, t + 150.0)));
    } else {
    // 상태 줄
    let (status, scol) = match v.phase {
        Phase::Idle if multi => (format!("{}단계 · {}", v.n, human_dur(def.total_secs())), p.muted),
        Phase::Idle => (human_dur(def.total_secs()), p.muted),
        Phase::Running if multi && v.loops > 0 => (format!("{}회차 · {} / {} · {}", v.loops + 1, v.idx + 1, v.n, v.step_name), p.text),
        Phase::Running if multi => (format!("{} / {} · {}", v.idx + 1, v.n, v.step_name), p.text),
        Phase::Running => ("실행 중".into(), p.green),
        Phase::Paused if v.waiting => (format!("대기 · ▶ 를 누르면 {} 시작", v.step_name), p.amber),
        Phase::Paused if multi => (format!("일시정지 · {} / {} · {}", v.idx + 1, v.n, v.step_name), p.amber),
        Phase::Paused => ("일시정지".into(), p.amber),
        Phase::Done => ("완료".into(), p.green),
    };
    let sg = {
        let mut job = egui::text::LayoutJob::single_section(status, egui::TextFormat { font_id: FontId::proportional(13.0), color: scol, ..Default::default() });
        job.wrap = egui::text::TextWrapping::truncate_at_width(w - 40.0);
        ui.fonts(|f| f.layout_job(job))
    };
    pt.galley(Pos2::new(l, t + 50.0), sg, scol);

    // 숫자
    let rem = match v.phase {
        Phase::Idle => v.total,
        _ if multi => v.step_rem,
        _ => v.total_rem,
    };
    let dcol = match v.phase {
        Phase::Idle => p.muted,
        Phase::Paused => p.amber,
        Phase::Done => p.green,
        Phase::Running if rem <= Duration::from_secs(10) => color,
        Phase::Running => p.text,
    };
    let txt = if v.phase == Phase::Done { "00:00".to_string() } else { mmss(rem) };
    draw_digits(&pt, ui.ctx(), Pos2::new(l - 2.0, t + 128.0), false, &txt, digits(if txt.len() > 5 { 38.0 } else { 46.0 }), dcol);
    if v.phase == Phase::Running || (multi && v.phase.active()) {
        let (lbl, val) = if multi {
            ("전체".to_string(), mmss(v.total_rem))
        } else {
            let end = chrono::Local::now() + chrono::Duration::milliseconds(v.total_rem.as_millis() as i64);
            ("끝".to_string(), end.format("%H:%M").to_string())
        };
        pt.text(Pos2::new(r, t + 124.0), Align2::RIGHT_BOTTOM, val, digits(16.0), p.muted);
        pt.text(Pos2::new(r, t + 104.0), Align2::RIGHT_BOTTOM, lbl, FontId::proportional(11.5), p.faint);
    }

    // 막대
    let bar = Rect::from_min_size(Pos2::new(l, t + 140.0), egui::vec2(r - l, 6.0));
    let seg = match v.phase {
        Phase::Idle => SegState::Idle,
        Phase::Done => SegState::Done,
        _ => SegState::Active,
    };
    let frac = if multi { v.frac } else { 1.0 - v.total_rem.as_secs_f32() / v.total.as_secs_f32().max(0.001) };
    paint_segments(&pt, bar, &v.step_secs, &[color], v.idx, frac, seg, p.track, p.green);
    }

    // 아래: 칩 + 버튼
    let by = rect.bottom() - 30.0;
    let mut cx = l;
    let mut chip_at = |text: String, fg: Color32, bg: Color32, tip: &str| {
        let g = ui.fonts(|f| f.layout_no_wrap(text, FontId::proportional(11.5), fg));
        let cr_ = Rect::from_min_size(Pos2::new(cx, by - 11.0), g.size() + egui::vec2(14.0, 6.0));
        if cr_.right() < r - 130.0 {
            pt.rect_filled(cr_, cr(cr_.height() / 2.0), bg);
            pt.galley(cr_.min + egui::vec2(7.0, 3.0), g, fg);
            let _ = ui.interact(cr_, ui.id().with(("chip", tip, def.id)), Sense::hover()).on_hover_text(tip);
            cx = cr_.right() + 6.0;
        }
    };
    if !def.hotkey_toggle.is_empty() {
        let b = busy.contains(&def.hotkey_toggle);
        chip_at(def.hotkey_toggle.clone(), if b { p.red } else { p.muted }, if b { p.red.gamma_multiply(0.12) } else { p.input }, if b { "다른 프로그램이 사용 중인 단축키" } else { "시작 · 일시정지 단축키" });
    }
    if is_auto {
        chip_at("API".into(), p.accent, p.accent.gamma_multiply(0.14), "자동화 연동 타이머 — 127.0.0.1 /status · /start");
    }
    if def.repeat {
        chip_at("반복".into(), p.muted, p.input, "끝나면 처음부터 반복");
    }

    let big = 19.0;
    let c3 = Pos2::new(r - big, by);
    let running = v.phase == Phase::Running;
    let b = round_btn(ui, c3, big, ui.id().with(("play", def.id)), if running { p.input } else { color }, if running { p.card_hi } else { color.gamma_multiply(1.12) });
    if running {
        paint_pause(&pt, c3, 6.0, p.text);
    } else {
        paint_play(&pt, Pos2::new(c3.x + 1.0, c3.y), 6.0, Color32::WHITE);
    }
    if b.on_hover_text(match v.phase {
        Phase::Running => "일시정지",
        Phase::Paused => "이어서",
        _ => "시작",
    })
    .clicked()
    {
        core.cmd(def.id, Cmd::Toggle, "main");
    }
    let mut x = c3.x - big - 22.0;
    if v.phase != Phase::Idle {
        let c = Pos2::new(x, by);
        let b = round_btn(ui, c, 15.0, ui.id().with(("reset", def.id)), p.input, p.card_hi);
        paint_reset(&pt, c, 5.4, p.muted);
        if b.on_hover_text("처음으로").clicked() {
            core.cmd(def.id, Cmd::Reset, "main");
        }
        x -= 36.0;
    }
    if multi && v.phase.active() {
        let c = Pos2::new(x, by);
        let b = round_btn(ui, c, 15.0, ui.id().with(("skip", def.id)), p.input, p.card_hi);
        paint_next(&pt, c, 5.0, p.muted);
        if b.on_hover_text("다음 단계로").clicked() {
            core.cmd(def.id, Cmd::Skip, "main");
        }
    }
}

// ── 알람 ───────────────────────────────────────────────────

fn alarms_page(ui: &mut Ui, core: &Arc<Core>, st: &mut MainState, p: &Pal) {
    let mut alarms: Vec<AlarmDef> = core.data().alarms.clone();
    alarms.sort_by_key(|a| (a.hour, a.minute));
    let now = chrono::Local::now();
    let next = alarms.iter().filter_map(|a| core.next_alarm(a.id).map(|t| (t, a.clone()))).min_by_key(|(t, _)| *t);
    let sub = match &next {
        Some((t, a)) => format!("다음 알람 {} · {}", crate::alarm::until_label(*t, now), if a.label.is_empty() { t.format("%H:%M").to_string() } else { a.label.clone() }),
        None if alarms.is_empty() => String::new(),
        None => "켜진 알람 없음".into(),
    };
    if page_header(ui, p, "알람", &sub, Some("+  새 알람")) {
        st.alarm_edit = Some(AlarmEdit::new(AlarmDef::new(), true));
    }
    if alarms.is_empty() {
        empty(ui, p, "⏰", "알람이 없습니다", "정해진 시각·요일에 울리는 알람을 만들어 보세요.");
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for a in alarms {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 86.0), Sense::click());
            let pt = ui.painter().clone();
            pt.rect_filled(rect, cr(16.0), p.card);
            pt.rect_stroke(rect, cr(16.0), Stroke::new(1.0_f32, if resp.hovered() { p.faint.gamma_multiply(0.6) } else { p.border }), egui::StrokeKind::Inside);
            let on = a.enabled;
            let l = rect.left() + 24.0;
            let tw = draw_digits(&pt, ui.ctx(), Pos2::new(l, rect.center().y + 20.0), false, &format!("{:02}:{:02}", a.hour, a.minute), digits(40.0), if on { p.text } else { p.faint });
            let x2 = l + tw + 24.0;
            let title = if a.label.is_empty() { "알람".to_string() } else { a.label.clone() };
            pt.text(Pos2::new(x2, rect.center().y - 10.0), Align2::LEFT_CENTER, title, bold(15.0), if on { p.text } else { p.muted });
            let mut info = format!("{}  ·  {}", crate::alarm::days_label(&a.days), crate::audio::name_of(&a.sound));
            if on {
                if let Some(t) = core.next_alarm(a.id) {
                    info = format!("{info}  ·  {}", crate::alarm::until_label(t, now));
                }
            }
            pt.text(Pos2::new(x2, rect.center().y + 13.0), Align2::LEFT_CENTER, info, FontId::proportional(12.5), p.muted);
            let trect = Rect::from_center_size(Pos2::new(rect.right() - 44.0, rect.center().y), egui::vec2(40.0, 22.0));
            let mut en = a.enabled;
            let tr = toggle(&mut ui.new_child(egui::UiBuilder::new().max_rect(trect)), p, &mut en);
            if tr.changed() {
                let id = a.id;
                core.update(|d| {
                    if let Some(x) = d.alarms.iter_mut().find(|x| x.id == id) {
                        x.enabled = en;
                    }
                });
            } else if resp.clicked() {
                st.alarm_edit = Some(AlarmEdit::new(a.clone(), false));
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.add_space(4.0);
        }
    });
}

// ── 스톱워치 ───────────────────────────────────────────────

fn stopwatch_page(ui: &mut Ui, st: &mut MainState, p: &Pal) {
    page_header(ui, p, "스톱워치", "", None);
    let sw = &mut st.sw;
    ui.add_space(30.0);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 110.0), Sense::hover());
    let txt = sw_text(sw.elapsed());
    let font = digits(84.0);
    let tw = digits_width(ui.ctx(), &txt, &font);
    draw_digits(ui.painter(), ui.ctx(), Pos2::new(rect.center().x - tw / 2.0, rect.bottom()), false, &txt, font, if sw.running() { p.text } else { p.muted });
    ui.add_space(24.0);
    // 버튼
    let (brow, _) = ui.allocate_exact_size(egui::vec2(w, 70.0), Sense::hover());
    let c = brow.center();
    let main_b = round_btn(ui, c, 32.0, ui.id().with("sw-main"), if sw.running() { p.input } else { p.accent }, if sw.running() { p.card_hi } else { p.accent.gamma_multiply(1.12) });
    if sw.running() {
        paint_pause(ui.painter(), c, 9.0, p.text);
    } else {
        paint_play(ui.painter(), Pos2::new(c.x + 2.0, c.y), 10.0, p.on_accent);
    }
    if main_b.clicked() {
        if let Some(s) = sw.since.take() {
            sw.acc += s.elapsed();
        } else {
            sw.since = Some(Instant::now());
        }
    }
    let lc = Pos2::new(c.x - 84.0, c.y);
    let reset_b = round_btn(ui, lc, 24.0, ui.id().with("sw-reset"), p.input, p.card_hi);
    paint_reset(ui.painter(), lc, 7.5, p.muted);
    if reset_b.on_hover_text("초기화").clicked() {
        *sw = Stopwatch::default();
    }
    let rc = Pos2::new(c.x + 84.0, c.y);
    let lap_b = round_btn(ui, rc, 24.0, ui.id().with("sw-lap"), p.input, p.card_hi);
    ui.painter().text(rc, Align2::CENTER_CENTER, "랩", bold(14.0), if sw.running() { p.text } else { p.faint });
    if lap_b.on_hover_text("구간 기록").clicked() && sw.running() {
        sw.laps.push(sw.elapsed());
    }
    ui.add_space(20.0);
    if !sw.laps.is_empty() {
        let laps: Vec<Duration> = sw.laps.iter().enumerate().map(|(i, t)| *t - if i == 0 { Duration::ZERO } else { sw.laps[i - 1] }).collect();
        let best = laps.iter().min().copied();
        let worst = laps.iter().max().copied();
        let width = 460.0f32.min(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            card_frame(p).show(ui, |ui| {
                ui.set_width(width - 36.0);
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (i, lap) in laps.iter().enumerate().rev() {
                        let col = if laps.len() > 2 && Some(*lap) == best {
                            p.green
                        } else if laps.len() > 2 && Some(*lap) == worst {
                            p.red
                        } else {
                            p.text
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("랩 {}", i + 1)).color(p.muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(sw_text(sw.laps[i])).font(digits(15.0)).color(p.faint));
                                ui.add_space(24.0);
                                ui.label(RichText::new(sw_text(*lap)).font(digits(15.0)).color(col));
                            });
                        });
                    }
                });
            });
        });
    }
}

// ── 설정 ───────────────────────────────────────────────────

fn row(ui: &mut Ui, p: &Pal, title: &str, desc: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(14.5).color(p.text));
            if !desc.is_empty() {
                ui.label(RichText::new(desc).size(12.5).color(p.muted));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

fn section(ui: &mut Ui, p: &Pal, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(title).font(bold(13.0)).color(p.muted));
    ui.add_space(4.0);
    card_frame(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        body(ui);
    });
    ui.add_space(18.0);
}

fn settings_page(ui: &mut Ui, core: &Arc<Core>, st: &mut MainState, p: &Pal) {
    page_header(ui, p, "설정", "", None);
    let mut s = core.data().settings.clone();
    let before = s.clone();
    let mut reset_card_top = false;
    let timers: Vec<(Id, String)> = core.data().timers.iter().map(|t| (t.id, t.name.clone())).collect();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(ui.available_width().min(760.0));
        section(ui, p, "일반", |ui| {
            row(ui, p, "Windows 시작 시 자동 실행", "로그인하면 트레이에 조용히 켜집니다", |ui| {
                if toggle(ui, p, &mut s.autostart).changed() && !store::set_autostart(s.autostart) {
                    s.autostart = !s.autostart;
                }
            });
            ui.separator();
            row(ui, p, "바로가기 만들기", "이 Tempo 를 시작 메뉴·바탕 화면에서 바로 켤 수 있게", |ui| {
                if ghost(ui, p, "바탕 화면").clicked() {
                    let ok = store::create_shortcut(false);
                    store::log(format!("desktop shortcut: {ok}"));
                }
                if ghost(ui, p, "시작 메뉴").clicked() {
                    let ok = store::create_shortcut(true);
                    store::log(format!("start menu shortcut: {ok}"));
                }
            });
            ui.separator();
            row(ui, p, "창을 닫으면 트레이로", "끄면 창을 닫을 때 Tempo 가 종료됩니다 (타이머·알람도 멈춤)", |ui| {
                toggle(ui, p, &mut s.close_to_tray);
            });
            ui.separator();
            row(ui, p, "메인 창을 항상 위에", "다른 프로그램 창보다 앞에 표시 (왼쪽 위 핀 버튼과 같음)", |ui| {
                toggle(ui, p, &mut s.main_on_top);
            });
            ui.separator();
            row(ui, p, "떠 있는 카드를 항상 위에", "모든 카드 기본값 — 카드마다 오른쪽 클릭 메뉴 「항상 위에 표시」로 따로 정할 수 있습니다", |ui| {
                if toggle(ui, p, &mut s.cards_on_top).changed() {
                    // 전체 설정을 바꾸면 카드별 예외는 지움 (헷갈리지 않게)
                    reset_card_top = true;
                }
            });
            ui.separator();
            ui.label(RichText::new("디자인").size(14.5).color(p.text));
            ui.label(RichText::new("타이머·카드·집중 화면의 모양. 타이머마다 따로 정할 수도 있습니다 (편집 창 · 카드 메뉴).").size(12.5).color(p.muted));
            ui.add_space(4.0);
            style_grid(ui, p, &mut s.style);
            ui.add_space(10.0);
            ui.label(RichText::new("색 테마").size(14.5).color(p.text));
            ui.add_space(4.0);
            theme_grid(ui, p, &mut s.theme);
        });
        section(ui, p, "Microsoft To Do 연동", |ui| todo_section(ui, core, p, &mut s.todo));
        section(ui, p, "소리", |ui| {
            row(ui, p, "볼륨", "모든 타이머·알람 소리 (Windows 기본 출력 장치)", |ui| {
                if ghost(ui, p, "테스트").clicked() {
                    core.preview("chime");
                }
                ui.add(egui::Slider::new(&mut s.volume, 0.0..=1.0).show_value(false));
                ui.label(RichText::new(format!("{}%", (s.volume * 100.0).round())).color(p.muted));
            });
        });
        section(ui, p, "떠 있는 카드", |ui| {
            row(ui, p, "크기", "카드 가장자리(원형은 바깥 테두리)를 끌면 카드마다 따로 조절됩니다", |ui| {
                ui.add(egui::Slider::new(&mut s.card_scale, 0.6..=3.0).show_value(false));
                ui.label(RichText::new(format!("{}%", (s.card_scale * 100.0).round())).color(p.muted));
            });
            ui.separator();
            row(ui, p, "카드 위치 초기화", "모든 카드를 화면 오른쪽 위로 다시 모읍니다", |ui| {
                if ghost(ui, p, "초기화").clicked() {
                    core.update(|d| {
                        d.timers.iter_mut().for_each(|t| t.card_pos = None);
                        d.countdowns.iter_mut().for_each(|c| c.card_pos = None);
                        d.ddays.iter_mut().for_each(|x| x.card_pos = None);
                    });
                    st.reset_cards = true;
                }
            });
        });
        section(ui, p, "자동화 연동 (로컬 API)", |ui| {
            row(ui, p, "연동 타이머", "다른 프로그램이 /status · /start 로 이 타이머를 읽고 시작합니다", |ui| {
                let cur = s.automation_timer.and_then(|id| timers.iter().find(|t| t.0 == id)).map(|t| t.1.clone()).unwrap_or_else(|| "(첫 번째 타이머)".into());
                egui::ComboBox::from_id_salt("auto-timer").selected_text(cur).width(180.0).show_ui(ui, |ui| {
                    for (id, name) in &timers {
                        ui.selectable_value(&mut s.automation_timer, Some(*id), name);
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "http://127.0.0.1:{}  —  /status · /start · /toggle · /pause · /resume · /reset · /skip  ·  /api/timers · /api/timer?name=…&do=start",
                    s.api_port
                ))
                .color(p.faint)
                .size(12.0),
            );
        });
        section(ui, p, "데이터", |ui| {
            ui.horizontal(|ui| {
                if ghost(ui, p, "데이터 폴더 열기").clicked() {
                    store::open_path(&store::dir());
                }
                if ghost(ui, p, "기록 보기").clicked() {
                    let _ = std::process::Command::new("notepad.exe").arg(store::log_path()).spawn();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Tempo {}", env!("CARGO_PKG_VERSION"))).color(p.faint).size(12.5));
                });
            });
        });
    });
    if s != before {
        let vol_only = s.volume != before.volume && { let mut t = s.clone(); t.volume = before.volume; t == before };
        if vol_only {
            core.update_quiet(|d| d.settings = s);
        } else {
            core.update(|d| {
                d.settings = s;
                if reset_card_top {
                    for t in d.timers.iter_mut() {
                        t.card_on_top = None;
                    }
                }
            });
        }
    }
}

/// 테마 미리보기 타일
fn theme_grid(ui: &mut Ui, p: &Pal, theme: &mut Theme) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        for t in Theme::ALL {
            let tp = super::theme::pal(t);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(124.0, 82.0), Sense::click());
            let pt = ui.painter();
            pt.rect_filled(rect, cr(12.0), tp.bg);
            // 사이드바 · 카드 · 강조 막대
            pt.rect_filled(Rect::from_min_size(rect.min, egui::vec2(26.0, rect.height())), egui::CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 }, tp.side);
            let card = Rect::from_min_size(rect.min + egui::vec2(34.0, 10.0), egui::vec2(80.0, 38.0));
            pt.rect_filled(card, cr(7.0), tp.card);
            pt.rect_stroke(card, cr(7.0), Stroke::new(1.0_f32, tp.border), egui::StrokeKind::Inside);
            pt.circle_filled(card.min + egui::vec2(10.0, 11.0), 3.5, tp.accent);
            pt.text(card.min + egui::vec2(18.0, 11.0), Align2::LEFT_CENTER, "05:00", digits(12.0), tp.text);
            pt.rect_filled(Rect::from_min_size(card.min + egui::vec2(8.0, 26.0), egui::vec2(64.0, 4.0)), cr(2.0), tp.track);
            pt.rect_filled(Rect::from_min_size(card.min + egui::vec2(8.0, 26.0), egui::vec2(38.0, 4.0)), cr(2.0), tp.accent);
            pt.text(Pos2::new(rect.left() + 34.0, rect.bottom() - 16.0), Align2::LEFT_CENTER, t.label(), FontId::proportional(12.5), tp.text);
            let sel = *theme == t;
            pt.rect_stroke(rect, cr(12.0), if sel { Stroke::new(2.5_f32, p.accent) } else { Stroke::new(1.0_f32, p.border) }, egui::StrokeKind::Outside);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                *theme = t;
            }
        }
    });
}

/// 메인 창 타이머 카드 본문 (클래식 외 스타일)
#[allow(clippy::too_many_arguments)]
fn card_body(ui: &Ui, pt: &egui::Painter, p: &Pal, def: &TimerDef, v: &TimerView, d: &styles::Disp, style: Style, body: Rect) {
    let ctx = ui.ctx();
    let col = if v.phase == Phase::Done { p.green } else { d.color };
    let trunc = |text: &str, font: FontId, c: Color32, w: f32| {
        let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color: c, ..Default::default() });
        job.wrap = egui::text::TextWrapping::truncate_at_width(w);
        ctx.fonts(|f| f.layout_job(job))
    };
    match style {
        Style::Ring => {
            let r = 42.0;
            let c = Pos2::new(body.left() + r + 4.0, body.center().y);
            styles::ring(pt, c, r, 7.0, d.rem_frac, col, p.track);
            let f = digits(if d.text.len() > 5 { 15.0 } else { 20.0 });
            let w = digits_width(ctx, &d.text, &f);
            draw_digits(pt, ctx, Pos2::new(c.x - w / 2.0, c.y + 8.0), false, &d.text, f, d.dcol);
            let x = c.x + r + 22.0;
            let tw = body.right() - x;
            pt.galley(Pos2::new(x, c.y - 30.0), trunc(&d.status, bold(15.0), d.status_col, tw), d.status_col);
            pt.galley(Pos2::new(x, c.y - 4.0), trunc(&d.sub, FontId::proportional(13.0), p.muted, tw), p.muted);
            if let Some(nx) = &v.next_name {
                pt.galley(Pos2::new(x, c.y + 18.0), trunc(&format!("다음 · {nx}"), FontId::proportional(12.0), p.faint, tw), p.faint);
            }
        }
        Style::Minimal => {
            pt.galley(Pos2::new(body.left(), body.top() + 2.0), trunc(&d.status, FontId::proportional(12.5), d.status_col, body.width()), d.status_col);
            draw_digits(pt, ctx, Pos2::new(body.left() - 3.0, body.bottom() - 4.0), false, &d.text, digits(if d.text.len() > 5 { 50.0 } else { 66.0 }), d.dcol);
            pt.text(Pos2::new(body.right(), body.bottom() - 14.0), Align2::RIGHT_BOTTOM, &d.sub, FontId::proportional(12.0), p.faint);
            let y = body.bottom() + 2.0;
            pt.line_segment([Pos2::new(body.left(), y), Pos2::new(body.right(), y)], Stroke::new(1.0_f32, p.track));
            pt.line_segment([Pos2::new(body.left(), y), Pos2::new(body.left() + body.width() * d.done_total, y)], Stroke::new(2.0_f32, col));
        }
        Style::Digital => {
            pt.galley(Pos2::new(body.left(), body.top() + 2.0), trunc(&d.status.to_uppercase(), bold(12.0), d.status_col, body.width() * 0.6), d.status_col);
            pt.text(Pos2::new(body.right(), body.top() + 2.0), Align2::RIGHT_TOP, &d.sub, FontId::proportional(12.0), p.faint);
            let panel = Rect::from_min_max(Pos2::new(body.left(), body.top() + 24.0), body.max);
            pt.rect_filled(panel, cr(10.0), styles::lcd_panel(p));
            let lc = match v.phase {
                Phase::Paused if !v.waiting => p.amber,
                Phase::Done => p.green,
                _ => styles::lcd_color(d.color),
            };
            let size = if d.text.len() > 5 { 36.0 } else { 48.0 };
            let _ = size;
            let dh = if d.text.len() > 5 { 34.0 } else { 44.0 };
            styles::lcd(pt, Pos2::new(panel.center().x, panel.center().y - 3.0), dh, &d.text, lc, styles::lcd_off(p));
            let n = 16;
            let bw = (panel.width() - 24.0) / n as f32;
            for i in 0..n {
                let on = (i as f32 + 0.5) / n as f32 <= d.done_total;
                pt.rect_filled(Rect::from_min_size(Pos2::new(panel.left() + 12.0 + i as f32 * bw, panel.bottom() - 8.0), egui::vec2(bw - 3.0, 3.0)), cr(1.0), if on { lc } else { styles::lcd_tick_off(p) });
            }
        }
        Style::Fill => {
            pt.galley(Pos2::new(body.left(), body.top() + 2.0), trunc(&d.status, FontId::proportional(13.0), d.status_col, body.width()), d.status_col);
            draw_digits(pt, ctx, Pos2::new(body.left() - 2.0, body.bottom() - 18.0), false, &d.text, digits(if d.text.len() > 5 { 40.0 } else { 50.0 }), d.dcol);
            pt.text(Pos2::new(body.right(), body.bottom() - 22.0), Align2::RIGHT_BOTTOM, format!("{}%", (d.done_total * 100.0).round()), digits(20.0), p.muted);
            pt.text(Pos2::new(body.left(), body.bottom() - 2.0), Align2::LEFT_BOTTOM, &d.sub, FontId::proportional(12.0), p.faint);
        }
        Style::Hourglass => {
            let (top, falling, angle) = styles::glass_state(v, d);
            if angle > 0.0 || falling {
                ctx.request_repaint_after(std::time::Duration::from_millis(if angle > 0.0 { 16 } else { 60 }));
            }
            let gc = styles::glass_colors(p, d.color);
            let c = Pos2::new(body.left() + 40.0, body.center().y);
            styles::hourglass(pt, c, body.height() + 6.0, top, falling, angle, ctx.input(|i| i.time), &gc);
            let x = body.left() + 96.0;
            let tw = body.right() - x;
            pt.galley(Pos2::new(x, body.top() + 4.0), trunc(&d.status, bold(14.0), d.status_col, tw), d.status_col);
            draw_digits(pt, ctx, Pos2::new(x - 2.0, body.bottom() - 18.0), false, &d.text, digits(if d.text.len() > 5 { 34.0 } else { 44.0 }), d.dcol);
            pt.galley(Pos2::new(x, body.bottom() - 16.0), trunc(&d.sub, FontId::proportional(12.0), p.faint, tw), p.faint);
        }
        Style::Classic => {}
    }
}

/// 디자인 미리보기 타일
fn style_grid(ui: &mut Ui, p: &Pal, style: &mut Style) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        for s in Style::ALL {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(132.0, 118.0), Sense::click());
            let pv = Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::vec2(120.0, 70.0));
            ui.painter().rect_filled(rect, cr(12.0), p.input);
            styles::preview(ui.painter(), ui.ctx(), pv, s, p);
            ui.painter().text(Pos2::new(rect.left() + 10.0, rect.bottom() - 30.0), Align2::LEFT_CENTER, s.label(), bold(13.0), p.text);
            ui.painter().text(Pos2::new(rect.left() + 10.0, rect.bottom() - 12.0), Align2::LEFT_CENTER, s.desc(), FontId::proportional(10.5), p.muted);
            let sel = *style == s;
            ui.painter().rect_stroke(rect, cr(12.0), if sel { Stroke::new(2.5_f32, p.accent) } else { Stroke::new(1.0_f32, p.border) }, egui::StrokeKind::Outside);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                *style = s;
            }
        }
    });
}

/// 설정 — Microsoft To Do 연결
fn todo_section(ui: &mut Ui, core: &Arc<Core>, p: &Pal, s: &mut crate::model::TodoSettings) {
    let ts = core.todo.lock().unwrap().clone();
    ui.label(RichText::new("리마인더를 Microsoft To Do 의 목록 하나와 양방향으로 맞춥니다 (휴대폰 To Do 앱·Outlook 과 함께 보기).").color(p.muted).size(12.5));
    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("고급 — 직접 등록한 앱으로 연결").size(12.5).color(p.faint)).id_salt("todo-adv").show(ui, |ui| {
        row(ui, p, "앱 클라이언트 ID", "비워 두면 Microsoft 공용 앱(Graph 명령줄 도구)으로 로그인합니다", |ui| {
            ui.add(egui::TextEdit::singleline(&mut s.client_id).desired_width(300.0).hint_text("비워 두면 기본값"));
        });
    });
    ui.separator();
    if let Some((code, uri)) = &ts.pending {
        ui.label(RichText::new("브라우저에서 Microsoft 로그인 페이지를 열고 아래 코드를 입력하세요").size(14.0));
        ui.horizontal(|ui| {
            ui.label(RichText::new(code).font(digits(30.0)).color(p.accent));
            ui.add_space(12.0);
            if ghost(ui, p, "코드 복사").clicked() {
                ui.ctx().copy_text(code.clone());
            }
            if primary(ui, p, "로그인 페이지 열기").clicked() {
                let _ = std::process::Command::new("explorer.exe").arg(uri).spawn();
            }
            if ghost(ui, p, "취소").clicked() {
                core.todo.lock().unwrap().pending = None;
            }
        });
        ui.label(RichText::new(format!("{uri} — 로그인을 마치면 자동으로 연결됩니다").as_str()).color(p.faint).size(12.0));
    } else if s.enabled {
        row(ui, p, &format!("연결됨 · {}", if s.account.is_empty() { "Microsoft 계정" } else { &s.account }), &match (&ts.last_error, ts.last_sync) {
            (Some(e), _) => format!("오류: {e}"),
            (None, Some(t)) => format!("마지막 동기화 {}", t.format("%H:%M:%S")),
            (None, None) => if ts.busy { "동기화 중…".into() } else { "곧 동기화합니다".into() },
        }, |ui| {
            if ghost(ui, p, "연결 해제").clicked() {
                crate::todo::send(crate::todo::Msg::Disconnect);
            }
            if ghost(ui, p, if ts.busy { "동기화 중…" } else { "지금 동기화" }).clicked() {
                crate::todo::nudge();
            }
        });
        ui.separator();
        row(ui, p, "맞출 목록", "이 목록의 작업 ↔ Tempo 리마인더", |ui| {
            let cur = ts.lists.iter().find(|l| Some(&l.0) == s.list_id.as_ref()).map(|l| l.1.clone()).unwrap_or_else(|| s.list_name.clone());
            let before = s.list_id.clone();
            egui::ComboBox::from_id_salt("todo-list").selected_text(cur).width(200.0).show_ui(ui, |ui| {
                for (id, name) in &ts.lists {
                    ui.selectable_value(&mut s.list_id, Some(id.clone()), name);
                }
            });
            if s.list_id != before {
                crate::todo::nudge();
            }
        });
    } else {
        ui.horizontal(|ui| {
            if primary(ui, p, "Microsoft 계정 연결").clicked() {
                // 입력한 ID 를 먼저 저장해야 연결 스레드가 읽음
                let id = s.client_id.clone();
                core.update_quiet(|d| d.settings.todo.client_id = id);
                crate::todo::send(crate::todo::Msg::Connect);
            }
            if let Some(e) = &ts.last_error {
                ui.label(RichText::new(e).color(p.red).size(12.5));
            }
        });
    }
}
