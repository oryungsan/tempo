//! 치료실 베드 — 베드 판(메인 창 「치료실」 · 큰 보드 창), 베드를 누르면 시작 · 조작 창, 치료 프로그램 편집
use super::editor::Outcome;
use super::main_win::{empty, header_actions};
use super::theme::*;
use crate::core::{Core, TimerView};
use crate::engine::{Cmd, Phase};
use crate::model::{BedPreset, Id, Step, TimerDef, PALETTE};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui};
use std::sync::Arc;
use std::time::Duration;

/// 베드들 (번호순)
pub fn beds(core: &Core) -> Vec<(TimerDef, TimerView)> {
    let views = core.views();
    let mut v: Vec<(TimerDef, TimerView)> = core
        .data()
        .timers
        .iter()
        .filter(|t| t.bed.is_some())
        .filter_map(|t| views.iter().find(|v| v.id == t.id).map(|v| (t.clone(), v.clone())))
        .collect();
    v.sort_by_key(|(t, _)| t.bed.as_ref().map(|b| b.no).unwrap_or(0));
    v
}

fn hms(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

/// 요약 — (전체, 치료 중, 끝남)
pub fn counts(list: &[(TimerDef, TimerView)]) -> (usize, usize, usize) {
    let busy = list.iter().filter(|(t, v)| v.phase.active() || (v.phase == Phase::Idle && t.bed.as_ref().map(|b| !b.patient.is_empty()).unwrap_or(false))).count();
    let done = list.iter().filter(|(_, v)| v.phase == Phase::Done).count();
    (list.len(), busy, done)
}

fn trunc(ui: &Ui, text: &str, font: FontId, color: Color32, w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(w.max(10.0));
    ui.fonts(|f| f.layout_job(job))
}

/// 베드 타일 하나 — 메인 창 · 보드 공용 (k = 크기 배율)
pub fn tile(ui: &mut Ui, core: &Core, p: &Pal, def: &TimerDef, v: &TimerView, rect: Rect, k: f32) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(("bed", def.id)), Sense::click());
    let pt = ui.painter().clone();
    let b = def.bed.clone().unwrap_or_default();
    let col = color_of(&def.color);
    let rad = 16.0 * k;
    let time = ui.input(|i| i.time) as f32;
    let (l, t, r, btm) = (rect.left() + 16.0 * k, rect.top() + 12.0 * k, rect.right() - 16.0 * k, rect.bottom() - 12.0 * k);
    let w = r - l;
    match v.phase {
        Phase::Idle if !b.patient.is_empty() => {
            // 환자는 들어왔고 치료 시작 전 (EMR 연결)
            pt.rect_filled(rect, cr(rad), lerp(p.card, p.amber, if p.dark { 0.10 } else { 0.08 }));
            pt.rect_stroke(rect, cr(rad), Stroke::new(1.5 * k, p.amber.gamma_multiply(if resp.hovered() { 1.0 } else { 0.7 })), egui::StrokeKind::Inside);
            pt.text(Pos2::new(l, t), Align2::LEFT_TOP, &def.name, bold(17.0 * k), p.text);
            pt.text(Pos2::new(r, t + 2.0 * k), Align2::RIGHT_TOP, "치료 전", FontId::proportional(12.5 * k), p.amber);
            pt.galley(Pos2::new(l, t + 26.0 * k), trunc(ui, &b.patient, bold(20.0 * k), p.text, w), p.text);
            if !b.note.is_empty() {
                pt.galley(Pos2::new(l, t + 54.0 * k), trunc(ui, &b.note, FontId::proportional(13.0 * k), p.muted, w), p.muted);
            }
            let wait = chrono::NaiveTime::parse_from_str(&b.since, "%H:%M")
                .ok()
                .map(|s| (chrono::Local::now().time() - s).num_minutes())
                .filter(|m| *m >= 0)
                .map(|m| format!("입실 {} · {m}분째", b.since))
                .unwrap_or_default();
            if !wait.is_empty() {
                pt.text(Pos2::new(l, btm), Align2::LEFT_BOTTOM, wait, FontId::proportional(14.0 * k), p.amber);
            }
            pt.text(Pos2::new(r, btm), Align2::RIGHT_BOTTOM, "눌러서 시작", FontId::proportional(12.0 * k), p.faint);
        }
        Phase::Idle => {
            pt.rect_filled(rect, cr(rad), p.card);
            pt.rect_stroke(rect, cr(rad), Stroke::new(1.0_f32, if resp.hovered() { p.faint } else { p.border }), egui::StrokeKind::Inside);
            pt.text(Pos2::new(l, t), Align2::LEFT_TOP, &def.name, bold(17.0 * k), p.muted);
            pt.text(rect.center() + egui::vec2(0.0, 2.0 * k), Align2::CENTER_CENTER, "빈 베드", FontId::proportional(18.0 * k), p.faint);
            pt.text(Pos2::new(rect.center().x, btm), Align2::CENTER_BOTTOM, "눌러서 시작", FontId::proportional(12.0 * k), p.faint);
        }
        Phase::Done => {
            // 끝 = 붉게 · 초과 시간
            let pulse = (time * 2.2).sin() * 0.5 + 0.5;
            pt.rect_filled(rect, cr(rad), lerp(p.card, p.red, if p.dark { 0.22 + 0.10 * pulse } else { 0.14 + 0.08 * pulse }));
            pt.rect_stroke(rect, cr(rad), Stroke::new(2.0 * k, p.red), egui::StrokeKind::Inside);
            pt.text(Pos2::new(l, t), Align2::LEFT_TOP, &def.name, bold(17.0 * k), p.text);
            pt.text(Pos2::new(r, t), Align2::RIGHT_TOP, "끝", bold(17.0 * k), p.red);
            if !b.patient.is_empty() {
                pt.galley(Pos2::new(l, t + 26.0 * k), trunc(ui, &b.patient, bold(20.0 * k), p.text, w), p.text);
            }
            let over = format!("+{}", hms(v.done_for.unwrap_or_default()));
            pt.text(Pos2::new(l - 2.0 * k, btm - 16.0 * k), Align2::LEFT_BOTTOM, over, digits(40.0 * k), p.red);
            pt.galley(Pos2::new(l, btm - 14.0 * k), trunc(ui, &format!("{} · 눌러서 비우기", if b.program.is_empty() { "치료" } else { &b.program }), FontId::proportional(12.0 * k), p.muted, w), p.muted);
        }
        _ => {
            pt.rect_filled(rect, cr(rad), p.card);
            // 진행만큼 바탕이 차오름
            let total = v.total.as_secs_f32().max(1.0);
            let frac = (1.0 - v.total_rem.as_secs_f32() / total).clamp(0.0, 1.0);
            if frac > 0.0 {
                let fill = Rect::from_min_max(rect.min, Pos2::new(rect.left() + rect.width() * frac, rect.bottom()));
                pt.with_clip_rect(fill).rect_filled(rect, cr(rad), col.gamma_multiply(if p.dark { 0.22 } else { 0.16 }));
            }
            pt.rect_filled(Rect::from_min_size(rect.min + egui::vec2(0.0, 14.0 * k), egui::vec2(4.0 * k, rect.height() - 28.0 * k)), cr(2.0), col);
            pt.rect_stroke(rect, cr(rad), Stroke::new(1.0_f32, if resp.hovered() { p.faint } else { p.border }), egui::StrokeKind::Inside);
            let paused = v.phase == Phase::Paused;
            pt.text(Pos2::new(l, t), Align2::LEFT_TOP, &def.name, bold(17.0 * k), col);
            let st = if paused {
                if v.waiting { "다음 단계 대기".to_string() } else { "일시정지".to_string() }
            } else {
                let end = chrono::Local::now() + chrono::Duration::milliseconds(v.total_rem.as_millis() as i64);
                format!("끝 {}", end.format("%H:%M"))
            };
            pt.text(Pos2::new(r, t + 2.0 * k), Align2::RIGHT_TOP, st, FontId::proportional(12.5 * k), if paused { p.amber } else { p.muted });
            let mut y = t + 26.0 * k;
            if !b.patient.is_empty() {
                pt.galley(Pos2::new(l, y), trunc(ui, &b.patient, bold(20.0 * k), p.text, w), p.text);
                y += 28.0 * k;
            }
            let mut step = if v.n > 1 { format!("{} · {}/{} {}", b.program, v.idx + 1, v.n, v.step_name) } else { b.program.clone() };
            if !b.note.is_empty() {
                step = format!("{step} · {}", b.note);
            }
            pt.galley(Pos2::new(l, y), trunc(ui, &step, FontId::proportional(13.0 * k), p.muted, w), p.muted);
            // 남은 시간 = 이 단계 (여러 단계면 오른쪽에 전체)
            let rem = if v.n > 1 { v.step_rem } else { v.total_rem };
            let urgent = rem <= Duration::from_secs(60) && !paused;
            draw_digits(&pt, ui.ctx(), Pos2::new(l - 2.0 * k, btm), false, &hms(rem + Duration::from_millis(999)), digits(44.0 * k), if paused { p.amber } else if urgent { col } else { p.text });
            if v.n > 1 {
                pt.text(Pos2::new(r, btm - 4.0 * k), Align2::RIGHT_BOTTOM, format!("전체 {}", hms(v.total_rem + Duration::from_millis(999))), FontId::proportional(12.5 * k), p.faint);
            }
        }
    }
    if let Some(fx) = super::fx::active(core, def, v, ui.input(|i| i.time)) {
        super::fx::overlay(&pt, super::fx::Area::Round(rect, rad), &fx, p, k);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// 타일 배치 — 칸 수를 바꿔 가며 타일이 가장 크게 (가로:세로 ≈ 1.5)
fn layout(n: usize, area: Rect, gap: f32) -> Vec<Rect> {
    if n == 0 {
        return vec![];
    }
    let mut best = (0.0f32, 1usize);
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let w = (area.width() - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = (area.height() - gap * (rows as f32 - 1.0)) / rows as f32;
        let score = w.min(h * 1.5);
        if score > best.0 {
            best = (score, cols);
        }
    }
    let cols = best.1;
    let rows = n.div_ceil(cols);
    let w = (area.width() - gap * (cols as f32 - 1.0)) / cols as f32;
    let h = ((area.height() - gap * (rows as f32 - 1.0)) / rows as f32).min(w / 1.2);
    (0..n).map(|i| Rect::from_min_size(area.min + egui::vec2((i % cols) as f32 * (w + gap), (i / cols) as f32 * (h + gap)), egui::vec2(w, h))).collect()
}

// ── 메인 창 「치료실」 ──────────────────────────────────────

pub struct BedState {
    pub act: Option<BedAction>,
    pub presets: Option<PresetsEdit>,
    pub count: u32,
}

impl Default for BedState {
    fn default() -> Self {
        BedState { act: None, presets: None, count: 8 }
    }
}

impl BedState {
    /// 베드 조작 창 · 치료 프로그램 편집이 열려 있음 (업데이트 대기 — 입력 중인 이름 · 프로그램을 잃지 않게)
    pub fn editing(&self) -> bool {
        self.act.is_some() || self.presets.is_some()
    }
    /// 시험용 (--open bed:<번호> · bed-presets, 시험 모드에서만) — 연 창의 번호
    pub fn test_open(&mut self, core: &Core, def: Option<&TimerDef>) -> u64 {
        match def {
            Some(d) => {
                let a = BedAction::new(d);
                let g = a.gen;
                self.act = Some(a);
                g
            }
            None => {
                let e = PresetsEdit::new(core);
                let g = e.gen;
                self.presets = Some(e);
                g
            }
        }
    }
    /// 시험으로 연 **그 창**만 닫음 (그 사이 사람이 닫았다 다시 연 창 · 다른 창은 그대로)
    pub fn test_close(&mut self, gen: u64) -> bool {
        if self.act.as_ref().is_some_and(|a| a.gen == gen) {
            self.act = None;
            return true;
        }
        if self.presets.as_ref().is_some_and(|e| e.gen == gen) {
            self.presets = None;
            return true;
        }
        false
    }

    pub fn modals(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal) {
        if let Some(a) = self.act.as_mut() {
            if let Outcome::Close = a.ui(ctx, core, p) {
                self.act = None;
            }
        }
        if let Some(e) = self.presets.as_mut() {
            if let Outcome::Close = e.ui(ctx, core, p) {
                self.presets = None;
            }
        }
    }
}

pub fn page(ui: &mut Ui, core: &Arc<Core>, st: &mut BedState, p: &Pal) {
    let list = beds(core);
    let (n, busy, done) = counts(&list);
    if n == 0 {
        header_actions(ui, p, "치료실", "", &[]);
        empty(ui, p, "🏥", "치료실 베드를 만들어 보세요", "베드마다 치료 프로그램(침 15분, 핫팩 → 침 등)을 골라 시작하고, 끝나면 소리 · 음성 · 화면으로 알려 줍니다.");
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() / 2.0 - 140.0).max(0.0));
                ui.label(RichText::new("베드 수").size(14.0));
                ui.add(egui::DragValue::new(&mut st.count).range(1..=40).suffix(" 개"));
                if primary(ui, p, "  치료실 만들기  ").clicked() {
                    core.ensure_beds(st.count);
                }
            });
        });
        return;
    }
    let sub = format!("베드 {n} · 치료 중 {busy}{}", if done > 0 { format!(" · 끝남 {done}") } else { String::new() });
    match header_actions(ui, p, "치료실", &sub, &["치료실 보드 열기", "치료 프로그램", "베드 수"]) {
        Some(0) => core.update_quiet(|d| d.settings.bed_board = true),
        Some(1) => st.presets = Some(PresetsEdit::new(core)),
        Some(2) => {
            st.count = n as u32;
            st.act = Some(BedAction::count());
        }
        _ => {}
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let gap = 14.0;
        let aw = ui.available_width();
        let cols = (((aw + gap) / (230.0 + gap)).floor() as usize).max(1);
        let w = (aw - gap * (cols as f32 - 1.0)) / cols as f32;
        let h = 168.0;
        let rows = n.div_ceil(cols);
        let (area, _) = ui.allocate_exact_size(egui::vec2(aw, rows as f32 * (h + gap)), Sense::hover());
        for (i, (def, v)) in list.iter().enumerate() {
            let rect = Rect::from_min_size(area.min + egui::vec2((i % cols) as f32 * (w + gap), (i / cols) as f32 * (h + gap)), egui::vec2(w, h));
            if tile(ui, core, p, def, v, rect, 1.0).clicked() {
                st.act = Some(BedAction::new(def));
            }
        }
    });
}

// ── 베드 누르면 뜨는 창 ─────────────────────────────────────

pub struct BedAction {
    /// 창마다 고유 번호 (시험 자동 닫기가 자기가 연 창만 닫게)
    gen: u64,
    /// 0 = 베드 수 바꾸기 창
    id: Id,
    patient: String,
    patient_orig: String,
    mins: u32,
    count: u32,
    confirm_clear: bool,
}

impl BedAction {
    pub fn new(def: &TimerDef) -> Self {
        let pat = def.bed.as_ref().map(|b| b.patient.clone()).unwrap_or_default();
        BedAction { gen: next_gen(), id: def.id, patient: pat.clone(), patient_orig: pat, mins: 15, count: 0, confirm_clear: false }
    }

    fn count() -> Self {
        BedAction { gen: next_gen(), id: 0, patient: String::new(), patient_orig: String::new(), mins: 0, count: 0, confirm_clear: false }
    }

    pub fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal) -> Outcome {
        let mut out = Outcome::Keep;
        if self.id == 0 {
            // 베드 수
            let n = beds(core).len() as u32;
            if self.count == 0 {
                self.count = n.max(1);
            }
            egui::Modal::new(egui::Id::new("bed-count")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.heading("베드 수");
                ui.add_space(8.0);
                ui.add(egui::DragValue::new(&mut self.count).range(1..=40).suffix(" 개"));
                ui.label(RichText::new("줄이면 뒤 번호부터 지웁니다 (환자가 있는 베드는 남김)").color(p.faint).size(12.0));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary(ui, p, "  적용  ").clicked() {
                            core.ensure_beds(self.count);
                            out = Outcome::Close;
                        }
                        if ghost(ui, p, "취소").clicked() {
                            out = Outcome::Close;
                        }
                    });
                });
            });
            return out;
        }
        let Some((def, v)) = beds(core).into_iter().find(|(t, _)| t.id == self.id) else { return Outcome::Close };
        let id = def.id;
        let b = def.bed.clone().unwrap_or_default();
        let presets = core.data().bed_presets.clone();
        egui::Modal::new(egui::Id::new(("bed-act", id))).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&def.name).font(bold(24.0)).color(color_of(&def.color)));
                let (txt, c) = match v.phase {
                    Phase::Idle => ("빈 베드".to_string(), p.muted),
                    Phase::Running => (format!("치료 중 · {}", b.program), p.green),
                    Phase::Paused => (format!("일시정지 · {}", b.program), p.amber),
                    Phase::Done => (format!("끝 +{} · {}", hms(v.done_for.unwrap_or_default()), b.program), p.red),
                };
                ui.label(RichText::new(txt).color(c).size(14.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if super::card::shown(core, &def, &v) {
                        if ghost(ui, p, "카드 닫기").on_hover_text("이 베드의 떠 있는 카드 닫기").clicked() {
                            core.hide_card(id);
                        }
                    } else if ghost(ui, p, "카드 열기").on_hover_text("이 베드를 떠 있는 카드로 (닫을 때까지)").clicked() {
                        core.open_card(id);
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.patient).desired_width(300.0).font(FontId::proportional(16.0)).hint_text("환자 이름 (선택)").margin(egui::Margin::symmetric(10, 7)));
                if v.phase.active() && self.patient.trim() != self.patient_orig.trim() && ghost(ui, p, "이름 저장").clicked() {
                    let pat = self.patient.trim().to_string();
                    core.update(|d| {
                        if let Some(bb) = d.timers.iter_mut().find(|t| t.id == id).and_then(|t| t.bed.as_mut()) {
                            bb.patient = pat;
                        }
                    });
                    self.patient_orig = self.patient.clone();
                }
            });
            ui.add_space(10.0);
            match v.phase {
                Phase::Running | Phase::Paused => {
                    let rem = if v.n > 1 { v.step_rem } else { v.total_rem };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(hms(rem + Duration::from_millis(999))).font(digits(40.0)).color(if v.phase == Phase::Paused { p.amber } else { p.text }));
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            if v.n > 1 {
                                ui.label(RichText::new(format!("{}/{} 단계 · {}", v.idx + 1, v.n, v.step_name)).color(p.muted));
                                if let Some(nx) = &v.next_name {
                                    ui.label(RichText::new(format!("다음 · {nx}")).color(p.faint).size(12.5));
                                }
                            }
                            ui.label(RichText::new(format!("전체 남은 시간 {}", hms(v.total_rem + Duration::from_millis(999)))).color(p.faint).size(12.5));
                        });
                    });
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        let label = if v.phase == Phase::Running { "일시정지" } else if v.waiting { "다음 단계 시작" } else { "이어서" };
                        if primary(ui, p, &format!("  {label}  ")).clicked() {
                            core.cmd(id, Cmd::Toggle, "bed");
                        }
                        for m in [1u32, 5] {
                            if ghost(ui, p, &format!("+{m}분")).clicked() {
                                core.bed_extend(id, m, "bed");
                            }
                        }
                        if v.n > 1 && ghost(ui, p, "다음 단계").clicked() {
                            core.cmd(id, Cmd::Skip, "bed");
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if self.confirm_clear {
                            if danger(ui, p, "정말 끝내고 비우기").clicked() {
                                core.bed_clear(id, "bed");
                                out = Outcome::Close;
                            }
                            if ghost(ui, p, "아니요").clicked() {
                                self.confirm_clear = false;
                            }
                        } else if danger(ui, p, "끝내고 비우기").clicked() {
                            self.confirm_clear = true;
                        }
                    });
                }
                Phase::Done | Phase::Idle => {
                    if v.phase == Phase::Done {
                        ui.horizontal_wrapped(|ui| {
                            if primary(ui, p, "  비우기  ").clicked() {
                                core.bed_clear(id, "bed");
                                out = Outcome::Close;
                            }
                            if ghost(ui, p, "+5분 연장").clicked() {
                                core.bed_extend(id, 5, "bed");
                                out = Outcome::Close;
                            }
                            if ghost(ui, p, "같은 치료 다시").clicked() {
                                core.bed_start(id, &b.program, def.steps.clone(), &self.patient, "bed");
                                out = Outcome::Close;
                            }
                        });
                        ui.add_space(12.0);
                        ui.label(RichText::new("다른 치료로 시작").color(p.muted).size(12.5));
                    } else {
                        if !b.patient.is_empty() {
                            ui.horizontal(|ui| {
                                let mut info = String::new();
                                if !b.since.is_empty() {
                                    info += &format!("입실 {}", b.since);
                                }
                                if !b.note.is_empty() {
                                    info += &format!("  ·  {}", b.note);
                                }
                                ui.label(RichText::new(info).color(p.amber).size(13.0));
                                if ghost(ui, p, "비우기").clicked() {
                                    core.bed_clear(id, "bed");
                                    out = Outcome::Close;
                                }
                            });
                            ui.add_space(6.0);
                        }
                        ui.label(RichText::new("치료 프로그램을 누르면 바로 시작").color(p.muted).size(12.5));
                    }
                    ui.add_space(4.0);
                    // 프로그램 단추 (크게)
                    let cols = 3;
                    let gap = 8.0;
                    let bw = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
                    for chunk in presets.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            for pr in chunk {
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(bw, 58.0), Sense::click());
                                let c = color_of(&pr.color);
                                ui.painter().rect_filled(rect, cr(12.0), if resp.hovered() { c.gamma_multiply(0.30) } else { c.gamma_multiply(0.16) });
                                ui.painter().rect_stroke(rect, cr(12.0), Stroke::new(1.0_f32, c.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                                ui.painter().text(Pos2::new(rect.left() + 12.0, rect.center().y - 9.0), Align2::LEFT_CENTER, &pr.name, bold(15.0), p.text);
                                let detail = if pr.steps.len() > 1 { pr.steps.iter().map(|s| format!("{} {}", s.name, s.secs / 60)).collect::<Vec<_>>().join(" → ") } else { format!("{}분", pr.total_min()) };
                                ui.painter().galley(Pos2::new(rect.left() + 12.0, rect.center().y + 3.0), trunc(ui, &detail, FontId::proportional(12.0), p.muted, bw - 20.0), p.muted);
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if resp.clicked() {
                                    core.bed_start(id, &pr.name, pr.steps.clone(), &self.patient, "bed");
                                    out = Outcome::Close;
                                }
                            }
                        });
                        ui.add_space(gap - 8.0);
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("직접").color(p.muted));
                        ui.add(egui::DragValue::new(&mut self.mins).range(1..=240).suffix(" 분"));
                        for m in [5u32, 10, 15, 20, 30] {
                            if ui.selectable_label(self.mins == m, format!("{m}")).clicked() {
                                self.mins = m;
                            }
                        }
                        if ghost(ui, p, "시작").clicked() {
                            let m = self.mins.max(1);
                            core.bed_start(id, &format!("{m}분"), vec![Step { name: "치료".into(), secs: m * 60, sound: "chime".into() }], &self.patient, "bed");
                            out = Outcome::Close;
                        }
                    });
                }
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ghost(ui, p, "닫기").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        out = Outcome::Close;
                    }
                });
            });
        });
        out
    }
}

// ── 치료 프로그램 편집 ─────────────────────────────────────

pub struct PresetsEdit {
    gen: u64,
    list: Vec<BedPreset>,
}

impl PresetsEdit {
    pub fn new(core: &Core) -> Self {
        let mut list = core.data().bed_presets.clone();
        if list.is_empty() {
            list = BedPreset::defaults();
        }
        PresetsEdit { gen: next_gen(), list }
    }

    fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal) -> Outcome {
        let mut out = Outcome::Keep;
        let max_h = (ctx.screen_rect().height() - 180.0).max(240.0);
        egui::Modal::new(egui::Id::new("bed-presets")).show(ctx, |ui| {
            ui.set_width(640.0);
            ui.heading("치료 프로그램");
            ui.label(RichText::new("베드에서 고르는 치료 목록 — 단계를 여러 개 두면 차례로 진행합니다 (예: 핫팩 10분 → 침 15분). 연동된 PC 에도 같이 바뀝니다.").color(p.muted).size(12.5));
            ui.add_space(8.0);
            let mut del = None;
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                for (i, pr) in self.list.iter_mut().enumerate() {
                    card_frame(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let c = color_of(&pr.color);
                            let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::click());
                            ui.painter().circle_filled(r.center(), 9.0, c);
                            if resp.on_hover_text("눌러서 색 바꾸기").clicked() {
                                let k = PALETTE.iter().position(|x| x.eq_ignore_ascii_case(&pr.color)).map(|k| (k + 1) % PALETTE.len()).unwrap_or(0);
                                pr.color = PALETTE[k].to_string();
                            }
                            ui.add(egui::TextEdit::singleline(&mut pr.name).desired_width(170.0).font(FontId::proportional(15.0)).hint_text("이름"));
                            ui.label(RichText::new(format!("{}분", pr.total_min())).color(p.muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if icon_btn(ui, p, "×", "이 프로그램 지우기").clicked() {
                                    del = Some(i);
                                }
                            });
                        });
                        let mut sdel = None;
                        let n = pr.steps.len();
                        for (j, s) in pr.steps.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add_space(30.0);
                                ui.label(RichText::new(format!("{}.", j + 1)).color(p.faint));
                                ui.add(egui::TextEdit::singleline(&mut s.name).desired_width(110.0).hint_text("단계 이름"));
                                let mut m = (s.secs / 60).max(1);
                                if ui.add(egui::DragValue::new(&mut m).range(1..=240).suffix(" 분")).changed() {
                                    s.secs = m * 60;
                                }
                                if n > 1 && ui.small_button("빼기").clicked() {
                                    sdel = Some(j);
                                }
                            });
                        }
                        if let Some(j) = sdel {
                            pr.steps.remove(j);
                        }
                        ui.horizontal(|ui| {
                            ui.add_space(30.0);
                            if ui.small_button("+ 단계").clicked() {
                                pr.steps.push(Step { name: "다음".into(), secs: 600, sound: "chime".into() });
                            }
                        });
                        // 단계 끝 소리: 중간 = 올라가는 벨, 마지막 = 차임
                        let n = pr.steps.len();
                        for (j, s) in pr.steps.iter_mut().enumerate() {
                            s.sound = if j + 1 == n { "chime".into() } else { "bell-rise".into() };
                        }
                    });
                    ui.add_space(4.0);
                }
            });
            if let Some(i) = del {
                self.list.remove(i);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ghost(ui, p, "+ 새 프로그램").clicked() {
                    let k = self.list.len();
                    self.list.push(BedPreset::new("새 치료", &[("치료", 10)], PALETTE[k % PALETTE.len()]));
                }
                if ghost(ui, p, "기본값으로").clicked() {
                    self.list = BedPreset::defaults();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ok = self.list.iter().all(|x| !x.name.trim().is_empty() && !x.steps.is_empty());
                    if ui.add_enabled_ui(ok, |ui| primary(ui, p, "  저장  ")).inner.clicked() {
                        let list = self.list.clone();
                        core.update(|d| d.bed_presets = list);
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

// ── 큰 보드 창 (치료실 모니터 · TV) ─────────────────────────

#[derive(Default)]
pub struct Board {
    act: Option<BedAction>,
    full: bool,
}

impl Board {
    /// 보드 창의 베드 조작 창이 열려 있음
    pub fn editing(&self) -> bool {
        self.act.is_some()
    }
    /// 시험용 (--open board-bed:<번호>, 시험 모드에서만) — 연 창의 번호
    pub fn test_open(&mut self, def: &TimerDef) -> u64 {
        let a = BedAction::new(def);
        let g = a.gen;
        self.act = Some(a);
        g
    }
    /// 시험으로 연 그 창만 닫음
    pub fn test_close(&mut self, gen: u64) -> bool {
        if self.act.as_ref().is_some_and(|a| a.gen == gen) {
            self.act = None;
            return true;
        }
        false
    }
}

/// 창 고유 번호
fn next_gen() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// 보드 창 — 닫으면 false
pub fn board(ctx: &egui::Context, core: &Arc<Core>, st: &mut Board, p: &Pal) -> bool {
    let mut keep = true;
    let builder = egui::ViewportBuilder::default()
        .with_title("Tempo 치료실")
        .with_inner_size([1280.0, 760.0])
        .with_min_inner_size([520.0, 360.0])
        .with_icon(app_icon())
        .with_active(!super::test_inactive());
    let p = *p;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("bed-board"), builder, |ctx, _| {
        if ctx.input(|i| i.viewport().close_requested()) {
            keep = false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            st.full = !st.full;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(st.full));
        }
        egui::CentralPanel::default().frame(egui::Frame::default().fill(p.bg).inner_margin(egui::Margin::same(18))).show(ctx, |ui| {
            let list = beds(core);
            let (n, busy, done) = counts(&list);
            ui.horizontal(|ui| {
                ui.label(RichText::new("치료실").font(bold(28.0)).color(p.text));
                ui.add_space(12.0);
                let mut s = format!("치료 중 {busy} / {n}");
                if done > 0 {
                    s += &format!(" · 끝남 {done}");
                }
                ui.label(RichText::new(s).color(if done > 0 { p.red } else { p.muted }).size(17.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ghost(ui, &p, if st.full { "창으로 (F11)" } else { "전체 화면 (F11)" }).clicked() {
                        st.full = !st.full;
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(st.full));
                    }
                    ui.label(RichText::new(chrono::Local::now().format("%H:%M:%S").to_string()).font(digits(26.0)).color(p.muted));
                });
            });
            ui.add_space(10.0);
            if n == 0 {
                ui.label(RichText::new("베드가 없습니다 — 메인 창 「치료실」에서 만드세요").color(p.muted).size(16.0));
                return;
            }
            let area = ui.available_rect_before_wrap();
            let rects = layout(n, area, 14.0);
            for ((def, v), rect) in list.iter().zip(rects) {
                let k = (rect.height() / 168.0).min(rect.width() / 230.0).clamp(0.7, 3.5);
                if tile(ui, core, &p, def, v, rect, k).clicked() {
                    st.act = Some(BedAction::new(def));
                }
            }
            ui.allocate_rect(area, Sense::hover());
        });
        if let Some(a) = st.act.as_mut() {
            if let Outcome::Close = a.ui(ctx, core, &p) {
                st.act = None;
            }
        }
    });
    keep
}

/// API 용
/// 화면 기기용 상태 한 낱말 — empty(빈 베드) · waiting(치료 전) · running · paused(단계 사이 대기 포함) · done
pub fn bed_state(phase: Phase, waiting: bool, patient: &str) -> &'static str {
    match phase {
        Phase::Idle if patient.trim().is_empty() => "empty",
        Phase::Idle => "waiting",
        Phase::Running if waiting => "paused",
        Phase::Running => "running",
        Phase::Paused => "paused",
        Phase::Done => "done",
    }
}

pub fn beds_json(core: &Core) -> serde_json::Value {
    let list = beds(core);
    serde_json::Value::Array(
        list.iter()
            .map(|(t, v)| {
                let b = t.bed.clone().unwrap_or_default();
                serde_json::json!({
                    "no": b.no,
                    "id": t.id.to_string(),
                    "name": t.name,
                    "phase": v.phase.as_str(),
                    "patient": b.patient,
                    "program": b.program,
                    "since": b.since,
                    "note": b.note,
                    "step": if v.phase.active() { v.idx + 1 } else { 0 },
                    "step_count": v.n,
                    "step_name": v.step_name,
                    "next_step_name": if v.phase.active() { v.next_name.clone() } else { None },
                    "step_remaining": (v.step_rem.as_secs_f64() * 10.0).round() / 10.0,
                    "total_remaining": (v.total_rem.as_secs_f64() * 10.0).round() / 10.0,
                    "overtime": v.done_for.map(|d| d.as_secs()),
                    "state": bed_state(v.phase, v.waiting, &b.patient),
                    "step_secs": if v.phase.active() { v.step_secs.get(v.idx).copied().unwrap_or(0) } else { 0 },
                    "total_secs": v.total.as_secs(),
                })
            })
            .collect(),
    )
}
