//! 타이머 · 알람 편집 창 (모달)
use super::theme::*;
use crate::audio;
use crate::core::Core;
use crate::model::{AlarmDef, CardMode, Step, TimerDef, PALETTE};
use eframe::egui::{self, FontId, Pos2, RichText, Sense, Stroke, Ui};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PickTarget {
    Step(usize),
    Warn,
    Alarm,
}

pub type PickSlot = Arc<Mutex<Option<(PickTarget, String)>>>;

pub fn pick_file(slot: &PickSlot, target: PickTarget) {
    let slot = slot.clone();
    std::thread::spawn(move || {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("소리 파일 선택")
            .add_filter("소리 파일", &["wav", "mp3", "flac", "ogg"])
            .pick_file()
        {
            *slot.lock().unwrap() = Some((target, format!("file:{}", p.display())));
        }
    });
}

pub enum Outcome {
    Keep,
    Close,
}

// ── 공용 조각 ──────────────────────────────────────────────

fn field_label(ui: &mut Ui, p: &Pal, text: &str) {
    ui.label(RichText::new(text).color(p.muted).size(12.5));
}

/// 소리 선택 콤보 + 미리 듣기
pub fn sound_picker(ui: &mut Ui, p: &Pal, core: &Arc<Core>, id: impl std::hash::Hash, sound: &mut String, width: f32, slot: &PickSlot, target: PickTarget) {
    egui::ComboBox::from_id_salt(id)
        .width(width)
        .wrap_mode(egui::TextWrapMode::Truncate)
        .selected_text(audio::name_of(sound))
        .height(360.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(sound, "none".to_string(), "없음");
            ui.separator();
            for (sid, name) in audio::BUILTIN {
                ui.selectable_value(sound, sid.to_string(), *name);
            }
            ui.separator();
            if sound.starts_with("file:") {
                let cur = sound.clone();
                ui.selectable_value(sound, cur.clone(), audio::name_of(&cur));
            }
            if ui.selectable_label(false, "파일에서 고르기…").clicked() {
                pick_file(slot, target);
            }
        });
    if icon_btn(ui, p, "▶", "미리 듣기").clicked() {
        core.preview(sound);
    }
}

/// 단축키 입력 칸 — 누르면 다음 키 조합을 받음 (Esc 취소 · Backspace 지우기)
pub fn hotkey_field(ui: &mut Ui, p: &Pal, value: &mut String, capturing: &mut bool, busy: bool) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(190.0, 32.0), Sense::click());
    if resp.clicked() {
        *capturing = !*capturing;
    }
    if *capturing {
        let mut done = false;
        ui.ctx().input(|i| {
            for e in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                    match key {
                        egui::Key::Escape => done = true,
                        egui::Key::Backspace | egui::Key::Delete if !modifiers.any() => {
                            value.clear();
                            done = true;
                        }
                        k => {
                            let name = k.name();
                            let fkey = name.len() >= 2 && name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit());
                            if (modifiers.ctrl || modifiers.alt || fkey) && crate::hotkey::parse(name).is_some() {
                                let mut s = String::new();
                                if modifiers.ctrl {
                                    s += "Ctrl+";
                                }
                                if modifiers.alt {
                                    s += "Alt+";
                                }
                                if modifiers.shift {
                                    s += "Shift+";
                                }
                                s += name;
                                *value = s;
                                done = true;
                            }
                        }
                    }
                }
            }
        });
        if done {
            *capturing = false;
        }
    }
    let stroke = if *capturing { Stroke::new(1.5_f32, p.accent) } else if busy { Stroke::new(1.0_f32, p.red) } else { Stroke::NONE };
    ui.painter().rect_filled(rect, cr(8.0), p.input);
    if stroke.width > 0.0 {
        ui.painter().rect_stroke(rect, cr(8.0), stroke, egui::StrokeKind::Inside);
    }
    let (text, col) = if *capturing {
        ("키 조합을 누르세요…".to_string(), p.accent)
    } else if value.is_empty() {
        ("없음 — 눌러서 지정".to_string(), p.faint)
    } else {
        (value.clone(), p.text)
    };
    ui.painter().text(Pos2::new(rect.left() + 12.0, rect.center().y), egui::Align2::LEFT_CENTER, text, FontId::proportional(13.5), col);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if busy && !*capturing {
        ui.label(RichText::new("다른 프로그램이 사용 중").color(p.red).size(12.0));
    } else if !value.is_empty() && !*capturing && icon_btn(ui, p, "×", "단축키 지우기").clicked() {
        value.clear();
    }
}

pub fn color_swatches(ui: &mut Ui, p: &Pal, color: &mut String) {
    ui.horizontal(|ui| {
        for hex in PALETTE {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), Sense::click());
            let c = color_of(hex);
            ui.painter().circle_filled(rect.center(), 10.0, c);
            if color.eq_ignore_ascii_case(hex) {
                ui.painter().circle_stroke(rect.center(), 12.5, Stroke::new(2.0_f32, p.text));
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                *color = hex.to_string();
            }
        }
    });
}

// ── 타이머 편집 ───────────────────────────────────────────

pub struct TimerEdit {
    pub def: TimerDef,
    pub is_new: bool,
    cap_toggle: bool,
    cap_reset: bool,
    confirm_delete: bool,
}

impl TimerEdit {
    pub fn new(def: TimerDef, is_new: bool) -> Self {
        TimerEdit { def, is_new, cap_toggle: false, cap_reset: false, confirm_delete: false }
    }

    pub fn apply_pick(&mut self, t: PickTarget, sound: String) {
        match t {
            PickTarget::Step(i) => {
                if let Some(s) = self.def.steps.get_mut(i) {
                    s.sound = sound;
                }
            }
            PickTarget::Warn => self.def.warn_sound = sound,
            PickTarget::Alarm => {}
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal, slot: &PickSlot) -> Outcome {
        let mut out = Outcome::Keep;
        let busy = core.hotkey_busy.lock().unwrap().clone();
        let original_toggle = core.data().timers.iter().find(|t| t.id == self.def.id).map(|t| (t.hotkey_toggle.clone(), t.hotkey_reset.clone()));
        egui::Modal::new(egui::Id::new("timer-edit")).show(ctx, |ui| {
            ui.set_width(700.0);
            ui.horizontal(|ui| {
                ui.heading(if self.is_new { "새 타이머" } else { "타이머 편집" });
            });
            ui.add_space(10.0);
            let max_h = (ctx.screen_rect().height() - 170.0).max(200.0);
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {

            field_label(ui, p, "이름");
            ui.add(egui::TextEdit::singleline(&mut self.def.name).desired_width(f32::INFINITY).font(FontId::proportional(15.0)).hint_text("예: 집중, 운동, 라면"));
            ui.add_space(8.0);
            field_label(ui, p, "색");
            color_swatches(ui, p, &mut self.def.color);
            ui.add_space(10.0);

            if let Some(mut pm) = self.def.pomodoro {
                ui.label(RichText::new("포모도로").font(bold(14.5)));
                ui.label(RichText::new("집중과 휴식을 번갈아 진행하고, 정한 횟수마다 긴 휴식을 합니다.").color(p.faint).size(12.0));
                ui.add_space(4.0);
                let before = pm;
                egui::Grid::new("pomo").num_columns(2).spacing([24.0, 10.0]).show(ui, |ui| {
                    field_label(ui, p, "집중");
                    ui.add(egui::DragValue::new(&mut pm.work_min).range(1..=180).speed(0.2).suffix(" 분"));
                    ui.end_row();
                    field_label(ui, p, "짧은 휴식");
                    ui.add(egui::DragValue::new(&mut pm.short_min).range(1..=60).speed(0.1).suffix(" 분"));
                    ui.end_row();
                    field_label(ui, p, "긴 휴식");
                    ui.add(egui::DragValue::new(&mut pm.long_min).range(1..=120).speed(0.1).suffix(" 분"));
                    ui.end_row();
                    field_label(ui, p, "긴 휴식 간격");
                    ui.add(egui::DragValue::new(&mut pm.every).range(1..=12).speed(0.05).suffix(" 번 집중마다"));
                    ui.end_row();
                });
                if pm != before {
                    self.def.pomodoro = Some(pm);
                    self.def.steps = pm.steps();
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("한 바퀴 {} · {}단계", human_dur(self.def.total_secs()), self.def.steps.len())).color(p.muted).size(12.5));
                    if ui.small_button("단계를 직접 편집").clicked() {
                        self.def.pomodoro = None;
                    }
                });
            } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("단계").font(bold(14.5)));
                ui.label(RichText::new(format!("{}단계 · 총 {}", self.def.steps.len(), human_dur(self.def.total_secs()))).color(p.muted).size(12.5));
            });
            ui.label(RichText::new("단계가 하나면 보통 카운트다운, 여러 개면 차례로 이어서 진행합니다. 각 단계가 끝날 때 고른 소리가 납니다.").color(p.faint).size(12.0));
            ui.add_space(4.0);
            let n = self.def.steps.len();
            let mut action: Option<(usize, i32)> = None; // (i, -1 위로 / 1 아래로 / 0 삭제)
            ui.vertical(|ui| {
                for i in 0..n {
                    let s = &mut self.def.steps[i];
                    egui::Frame::default().fill(p.input).corner_radius(cr(10.0)).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{}", i + 1)).color(p.faint).size(12.5));
                            ui.add(egui::TextEdit::singleline(&mut s.name).desired_width(210.0).hint_text("단계 이름"));
                            let mut m = s.secs / 60;
                            let mut sec = s.secs % 60;
                            let r1 = ui.add_sized([64.0, 28.0], egui::DragValue::new(&mut m).range(0..=999).speed(0.1).suffix(" 분"));
                            let r2 = ui.add_sized([58.0, 28.0], egui::DragValue::new(&mut sec).range(0..=59).speed(0.2).suffix(" 초"));
                            if r1.changed() || r2.changed() {
                                s.secs = (m * 60 + sec).max(1);
                            }
                            sound_picker(ui, p, core, ("step-sound", i), &mut s.sound, 150.0, slot, PickTarget::Step(i));
                            if icon_btn(ui, p, "↑", "위로").clicked() && i > 0 {
                                action = Some((i, -1));
                            }
                            if icon_btn(ui, p, "↓", "아래로").clicked() && i + 1 < n {
                                action = Some((i, 1));
                            }
                            if n > 1 && icon_btn(ui, p, "×", "단계 삭제").clicked() {
                                action = Some((i, 0));
                            }
                        });
                    });
                    ui.add_space(2.0);
                }
            });
            match action {
                Some((i, -1)) => self.def.steps.swap(i, i - 1),
                Some((i, 1)) => self.def.steps.swap(i, i + 1),
                Some((i, _)) => {
                    self.def.steps.remove(i);
                }
                None => {}
            }
            ui.horizontal(|ui| {
                if ghost(ui, p, "+ 단계 추가").clicked() {
                    let last = self.def.steps.last().cloned();
                    self.def.steps.push(Step {
                        name: format!("{}단계", self.def.steps.len() + 1),
                        secs: last.as_ref().map(|s| s.secs).unwrap_or(60),
                        sound: last.map(|s| s.sound).unwrap_or_else(|| "bell".into()),
                    });
                }
                ui.label(RichText::new("빠른 시간:").color(p.faint).size(12.0));
                for (label, secs) in [("1분", 60), ("3분", 180), ("5분", 300), ("10분", 600), ("15분", 900), ("30분", 1800)] {
                    if ui.small_button(label).on_hover_text("마지막 단계 시간").clicked() {
                        if let Some(s) = self.def.steps.last_mut() {
                            s.secs = secs;
                        }
                    }
                }
            });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            egui::Grid::new("timer-opts").num_columns(2).spacing([24.0, 12.0]).show(ui, |ui| {
                field_label(ui, p, "디자인");
                {
                    let cur = self.def.style.map(|s| s.label()).unwrap_or("설정 따름");
                    egui::ComboBox::from_id_salt("timer-style").selected_text(cur).width(160.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.def.style, None, "설정 따름");
                        for s in crate::model::Style::ALL {
                            ui.selectable_value(&mut self.def.style, Some(s), format!("{} — {}", s.label(), s.desc()));
                        }
                    });
                }
                ui.end_row();
                field_label(ui, p, "떠 있는 카드");
                segmented(ui, p, &mut self.def.card, &[(CardMode::Always, "항상 표시"), (CardMode::WhenActive, "실행 중에만"), (CardMode::Off, "끄기")]);
                ui.end_row();
                field_label(ui, p, "끝나면");
                segmented(ui, p, &mut self.def.repeat, &[(false, "멈춤"), (true, "처음부터 반복")]);
                ui.end_row();
                field_label(ui, p, "완료 알림 창");
                toggle(ui, p, &mut self.def.notify);
                ui.end_row();
                field_label(ui, p, "소리 반복");
                ui.add(egui::DragValue::new(&mut self.def.sound_repeat).range(1..=5).suffix(" 회"));
                ui.end_row();
                field_label(ui, p, "끝나기 전 카운트다운");
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [92.0, 28.0],
                        egui::DragValue::new(&mut self.def.warn_secs)
                            .range(0..=3600)
                            .speed(0.3)
                            .custom_formatter(|v, _| if v < 0.5 { "끔".into() } else { format!("마지막 {}초", v as u32) })
                            .custom_parser(|s| {
                                let s = s.trim();
                                if s == "끔" {
                                    Some(0.0)
                                } else {
                                    s.trim_start_matches("마지막").trim_end_matches("초 전").trim_end_matches('초').trim().parse::<f64>().ok()
                                }
                            }),
                    );
                    if self.def.warn_secs > 0 {
                        if self.def.steps.len() > 1 {
                            segmented(ui, p, &mut self.def.warn_each, &[(false, "전체 끝"), (true, "각 단계")]);
                        }
                        sound_picker(ui, p, core, "warn-sound", &mut self.def.warn_sound, 150.0, slot, PickTarget::Warn);
                    }
                });
                ui.end_row();
                if self.def.warn_secs > 0 {
                    field_label(ui, p, "끝나기 전 화면 효과");
                    ui.horizontal_wrapped(|ui| {
                        for fx in crate::model::WarnFx::ALL {
                            let on = self.def.warn_fx.contains(&fx);
                            if ui.selectable_label(on, fx.label()).on_hover_text(fx.desc()).clicked() {
                                if on {
                                    self.def.warn_fx.retain(|x| *x != fx);
                                } else {
                                    self.def.warn_fx.push(fx);
                                }
                            }
                        }
                        if ui.small_button("▶ 미리 보기").on_hover_text("5초 동안 고른 효과를 보여 줍니다 (카드는 저장된 타이머만)").clicked() {
                            super::fx::start_preview(core, self.def.id, self.def.warn_fx.clone());
                        }
                    });
                    ui.end_row();
                }
                if self.def.steps.len() > 1 || self.def.repeat {
                    field_label(ui, p, "단계 사이");
                    segmented(ui, p, &mut self.def.auto_advance, &[(true, "자동으로 이어서"), (false, "멈추고 ▶ 기다리기")]);
                    ui.end_row();
                }
                let own = |c: &str, which: usize| original_toggle.as_ref().map(|o| if which == 0 { o.0 == c } else { o.1 == c }).unwrap_or(false);
                field_label(ui, p, "시작 · 일시정지 단축키");
                ui.horizontal(|ui| {
                    let b = own(&self.def.hotkey_toggle, 0) && busy.contains(&self.def.hotkey_toggle);
                    hotkey_field(ui, p, &mut self.def.hotkey_toggle, &mut self.cap_toggle, b);
                });
                ui.end_row();
                field_label(ui, p, "처음으로 단축키");
                ui.horizontal(|ui| {
                    let b = own(&self.def.hotkey_reset, 1) && busy.contains(&self.def.hotkey_reset);
                    hotkey_field(ui, p, &mut self.def.hotkey_reset, &mut self.cap_reset, b);
                });
                ui.end_row();
            });
            if self.cap_toggle {
                self.cap_reset = false;
            }
            });

            ui.add_space(14.0);
            let name_ok = !self.def.name.trim().is_empty();
            let dup = core
                .data()
                .timers
                .iter()
                .filter(|t| t.id != self.def.id)
                .flat_map(|t| [t.hotkey_toggle.clone(), t.hotkey_reset.clone()])
                .filter(|h| !h.is_empty())
                .any(|h| h == self.def.hotkey_toggle || h == self.def.hotkey_reset)
                || (!self.def.hotkey_toggle.is_empty() && self.def.hotkey_toggle == self.def.hotkey_reset);
            ui.horizontal(|ui| {
                if !self.is_new {
                    if self.confirm_delete {
                        if danger(ui, p, "정말 삭제").clicked() {
                            core.delete_timer(self.def.id);
                            out = Outcome::Close;
                        }
                        if ghost(ui, p, "아니요").clicked() {
                            self.confirm_delete = false;
                        }
                    } else if danger(ui, p, "삭제").clicked() {
                        self.confirm_delete = true;
                    }
                }
                if dup {
                    ui.label(RichText::new("다른 타이머와 같은 단축키").color(p.red).size(12.5));
                } else if !name_ok {
                    ui.label(RichText::new("이름을 입력하세요").color(p.faint).size(12.5));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = ui.add_enabled_ui(name_ok && !dup, |ui| primary(ui, p, "  저장  ")).inner;
                    if save.clicked() {
                        self.def.name = self.def.name.trim().to_string();
                        core.upsert_timer(self.def.clone());
                        out = Outcome::Close;
                    }
                    if ghost(ui, p, "취소").clicked() {
                        out = Outcome::Close;
                    }
                });
            });
        });
        if matches!(out, Outcome::Close) {
            core.stop_preview();
        }
        out
    }
}

// ── 알람 편집 ─────────────────────────────────────────────

pub struct AlarmEdit {
    pub def: AlarmDef,
    pub is_new: bool,
    confirm_delete: bool,
}

impl AlarmEdit {
    pub fn new(def: AlarmDef, is_new: bool) -> Self {
        AlarmEdit { def, is_new, confirm_delete: false }
    }

    pub fn apply_pick(&mut self, t: PickTarget, sound: String) {
        if t == PickTarget::Alarm {
            self.def.sound = sound;
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal, slot: &PickSlot) -> Outcome {
        let mut out = Outcome::Keep;
        egui::Modal::new(egui::Id::new("alarm-edit")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.heading(if self.is_new { "새 알람" } else { "알람 편집" });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.style_mut().override_font_id = Some(digits(40.0));
                ui.spacing_mut().interact_size = egui::vec2(80.0, 56.0);
                ui.add(egui::DragValue::new(&mut self.def.hour).range(0..=23).speed(0.08).custom_formatter(|v, _| format!("{:02}", v as u32)));
                ui.label(RichText::new(":").font(digits(40.0)).color(p.muted));
                ui.add(egui::DragValue::new(&mut self.def.minute).range(0..=59).speed(0.12).custom_formatter(|v, _| format!("{:02}", v as u32)));
                ui.style_mut().override_font_id = None;
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new("숫자를 끌거나 눌러서 입력").color(p.faint).size(12.0));
                    let now = chrono::Local::now();
                    if let Some(t) = crate::alarm::next_fire(&self.def, now) {
                        ui.label(RichText::new(format!("{} ({})", crate::alarm::until_label(t, now), t.format("%m월 %d일 %H:%M"))).color(p.muted).size(12.5));
                    }
                });
            });
            ui.add_space(12.0);
            field_label(ui, p, "이름");
            ui.add(egui::TextEdit::singleline(&mut self.def.label).desired_width(f32::INFINITY).hint_text("예: 점심 약속, 운동, 회의"));
            ui.add_space(10.0);
            field_label(ui, p, "반복");
            ui.horizontal(|ui| {
                const N: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];
                for (i, n) in N.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::click());
                    let on = self.def.days[i];
                    ui.painter().circle_filled(rect.center(), 17.0, if on { p.accent } else { p.input });
                    let fg = if on { p.on_accent } else if i >= 5 { p.red.gamma_multiply(0.8) } else { p.muted };
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, *n, FontId::proportional(13.5), fg);
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.def.days[i] = !on;
                    }
                }
            });
            ui.horizontal(|ui| {
                for (label, d) in [
                    ("매일", [true; 7]),
                    ("평일", [true, true, true, true, true, false, false]),
                    ("주말", [false, false, false, false, false, true, true]),
                    ("한 번", [false; 7]),
                ] {
                    let sel = self.def.days == d;
                    if ui.selectable_label(sel, label).clicked() {
                        self.def.days = d;
                    }
                }
                ui.label(RichText::new(crate::alarm::days_label(&self.def.days)).color(p.faint).size(12.0));
            });
            ui.add_space(10.0);
            egui::Grid::new("alarm-opts").num_columns(2).spacing([24.0, 12.0]).show(ui, |ui| {
                field_label(ui, p, "소리");
                ui.horizontal(|ui| sound_picker(ui, p, core, "alarm-sound", &mut self.def.sound, 200.0, slot, PickTarget::Alarm));
                ui.end_row();
                field_label(ui, p, "다시 알림");
                ui.add(egui::DragValue::new(&mut self.def.snooze_min).range(1..=60).suffix(" 분 뒤"));
                ui.end_row();
            });
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if !self.is_new {
                    if self.confirm_delete {
                        if danger(ui, p, "정말 삭제").clicked() {
                            core.delete_alarm(self.def.id);
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
                    if primary(ui, p, "  저장  ").clicked() {
                        self.def.enabled = true;
                        self.def.label = self.def.label.trim().to_string();
                        core.upsert_alarm(self.def.clone());
                        out = Outcome::Close;
                    }
                    if ghost(ui, p, "취소").clicked() {
                        out = Outcome::Close;
                    }
                });
            });
        });
        if matches!(out, Outcome::Close) {
            core.stop_preview();
        }
        out
    }
}

pub fn new_timer_template(n: usize) -> TimerDef {
    let color = PALETTE[n % PALETTE.len()];
    TimerDef::simple("", 300, color)
}

pub fn duplicate(def: &TimerDef) -> TimerDef {
    let mut d = def.clone();
    d.id = crate::model::new_id();
    d.name = format!("{} 복사본", def.name);
    d.hotkey_toggle.clear();
    d.hotkey_reset.clear();
    d.card_pos = None;
    d
}

