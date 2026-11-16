//! 리마인더 페이지 · 편집 창
use super::editor::{sound_picker, Outcome, PickSlot, PickTarget};
use super::theme::*;
use crate::core::{fmt_due, parse_due, Core};
use crate::model::{Id, Reminder, Repeat};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui};
use std::sync::Arc;

const WD: [&str; 7] = ["월", "화", "수", "목", "금", "토", "일"];

pub struct RemState {
    pub edit: Option<ReminderEdit>,
    pub quick: String,
    pub quick_when: usize,
    pub show_done: bool,
}

impl Default for RemState {
    fn default() -> Self {
        RemState { edit: None, quick: String::new(), quick_when: 2, show_done: false }
    }
}

/// 빠른 추가 시각 (이름, 계산)
fn quick_options() -> Vec<(&'static str, Option<NaiveDateTime>)> {
    let now = Local::now().naive_local();
    let today = now.date();
    let at = |d: NaiveDate, h: u32, m: u32| d.and_hms_opt(h, m, 0).unwrap();
    let round5 = |t: NaiveDateTime| {
        let m = t.minute_of_day();
        let r = (m + 4) / 5 * 5;
        at(t.date(), 0, 0) + Duration::minutes(r as i64)
    };
    vec![
        ("시각 없음", None),
        ("30분 뒤", Some(round5(now + Duration::minutes(30)))),
        ("1시간 뒤", Some(round5(now + Duration::hours(1)))),
        ("오늘 18:00", Some(at(today, 18, 0))),
        ("내일 09:00", Some(at(today + Duration::days(1), 9, 0))),
    ]
}

trait MinuteOfDay {
    fn minute_of_day(&self) -> u32;
}
impl MinuteOfDay for NaiveDateTime {
    fn minute_of_day(&self) -> u32 {
        use chrono::Timelike;
        self.hour() * 60 + self.minute()
    }
}

pub fn due_label(d: NaiveDateTime) -> String {
    let today = Local::now().date_naive();
    let date = d.date();
    let t = d.format("%H:%M");
    let diff = (date - today).num_days();
    match diff {
        0 => format!("오늘 {t}"),
        1 => format!("내일 {t}"),
        -1 => format!("어제 {t}"),
        2..=6 => format!("{} {t}", WD[date.weekday().num_days_from_monday() as usize]),
        _ => format!("{}월 {}일 ({}) {t}", date.month(), date.day(), WD[date.weekday().num_days_from_monday() as usize]),
    }
}

pub fn page(ui: &mut Ui, core: &Arc<Core>, st: &mut RemState, p: &Pal) {
    let rems: Vec<Reminder> = core.data().reminders.clone();
    let now = Local::now().naive_local();
    let today = now.date();
    let open: Vec<&Reminder> = rems.iter().filter(|r| !r.done).collect();
    let overdue = open.iter().filter(|r| r.due.as_deref().and_then(parse_due).map(|d| d < now).unwrap_or(false)).count();
    let today_n = open.iter().filter(|r| r.due.as_deref().and_then(parse_due).map(|d| d.date() == today).unwrap_or(false)).count();

    // 머리
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("리마인더").font(bold(24.0)).color(p.text));
            let mut sub = format!("할 일 {}개 · 오늘 {today_n}", open.len());
            if overdue > 0 {
                sub += &format!(" · 지남 {overdue}");
            }
            ui.label(RichText::new(sub).color(p.muted).size(13.0));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary(ui, p, "+  새 리마인더").clicked() {
                st.edit = Some(ReminderEdit::new(Reminder::new("", quick_options()[4].1.map(fmt_due)), true));
            }
            let s = core.data().settings.todo.clone();
            let ts = core.todo.lock().unwrap().clone();
            let (txt, col) = if s.enabled {
                let when = ts.last_sync.map(|t| format!(" · {}", t.format("%H:%M"))).unwrap_or_default();
                if ts.last_error.is_some() {
                    ("To Do 동기화 오류".to_string(), p.red)
                } else if ts.busy {
                    ("To Do 동기화 중…".to_string(), p.accent)
                } else {
                    (format!("To Do · {}{when}", s.list_name), p.green)
                }
            } else {
                ("To Do 연결 안 됨 (설정)".to_string(), p.faint)
            };
            ui.label(RichText::new(txt).color(col).size(12.5));
        });
    });
    ui.add_space(14.0);

    // 빠른 추가
    let opts = quick_options();
    ui.horizontal(|ui| {
        let w = ui.available_width() - 200.0;
        let te = ui.add(
            egui::TextEdit::singleline(&mut st.quick).desired_width(w).font(FontId::proportional(15.0)).hint_text("할 일을 적고 Enter — 예: 약 먹기, 장보기, 보고서 제출").margin(egui::Margin::symmetric(12, 8)),
        );
        egui::ComboBox::from_id_salt("quick-when").selected_text(opts[st.quick_when.min(opts.len() - 1)].0).width(110.0).show_ui(ui, |ui| {
            for (i, (l, _)) in opts.iter().enumerate() {
                ui.selectable_value(&mut st.quick_when, i, *l);
            }
        });
        let add = ghost(ui, p, "추가").clicked() || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if add && !st.quick.trim().is_empty() {
            let due = opts[st.quick_when.min(opts.len() - 1)].1.map(fmt_due);
            core.upsert_reminder(Reminder::new(st.quick.trim(), due));
            st.quick.clear();
            te.request_focus();
        }
    });
    ui.add_space(12.0);

    if rems.is_empty() {
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("☑").size(40.0).color(p.faint));
            ui.label(RichText::new("리마인더가 없습니다").font(bold(17.0)).color(p.text));
            ui.label(RichText::new("위 칸에 할 일을 적으면 정한 시각에 알려 드립니다. Microsoft To Do 와 연결하면 휴대폰에서도 함께 봅니다.").color(p.muted).size(13.0));
        });
        return;
    }

    // 묶음
    let mut groups: Vec<(&str, Color32, Vec<&Reminder>)> = vec![
        ("지남", p.red, vec![]),
        ("오늘", p.accent, vec![]),
        ("내일", p.text, vec![]),
        ("이후", p.text, vec![]),
        ("날짜 없음", p.muted, vec![]),
    ];
    for r in &open {
        let g = match r.due.as_deref().and_then(parse_due) {
            Some(d) if d < now => 0,
            Some(d) if d.date() == today => 1,
            Some(d) if d.date() == today + Duration::days(1) => 2,
            Some(_) => 3,
            None => 4,
        };
        groups[g].2.push(r);
    }
    for g in groups.iter_mut() {
        g.2.sort_by_key(|r| r.due.clone().unwrap_or_else(|| "9999".into()));
    }
    let mut done: Vec<&Reminder> = rems.iter().filter(|r| r.done).collect();
    done.sort_by_key(|r| std::cmp::Reverse(r.done_at.clone().unwrap_or_default()));

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (name, col, list) in &groups {
            if list.is_empty() {
                continue;
            }
            ui.label(RichText::new(format!("{name}  {}", list.len())).font(bold(13.0)).color(*col));
            ui.add_space(2.0);
            for r in list {
                row(ui, core, st, p, r, now);
            }
            ui.add_space(12.0);
        }
        if !done.is_empty() {
            let h = ui.add(egui::Label::new(RichText::new(format!("{} 완료  {}", if st.show_done { "▾" } else { "▸" }, done.len())).font(bold(13.0)).color(p.muted)).sense(Sense::click()));
            if h.clicked() {
                st.show_done = !st.show_done;
            }
            if st.show_done {
                ui.add_space(2.0);
                for r in done.iter().take(30) {
                    row(ui, core, st, p, r, now);
                }
            }
        }
    });

    if let Some(e) = st.edit.as_mut() {
        let _ = e;
    }
}

fn row(ui: &mut Ui, core: &Arc<Core>, st: &mut RemState, p: &Pal, r: &Reminder, now: NaiveDateTime) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 52.0), Sense::click());
    let pt = ui.painter().clone();
    pt.rect_filled(rect, cr(12.0), if resp.hovered() { p.card_hi } else { p.card });
    // 체크
    let cc = Pos2::new(rect.left() + 24.0, rect.center().y);
    let cr_ = Rect::from_center_size(cc, egui::vec2(28.0, 28.0));
    let cresp = ui.interact(cr_, ui.id().with(("chk", r.id)), Sense::click());
    if r.done {
        pt.circle_filled(cc, 10.0, p.accent);
        paint_check(&pt, cc, 0.9, p.on_accent);
    } else {
        pt.circle_stroke(cc, 9.5, Stroke::new(1.8_f32, if cresp.hovered() { p.accent } else { p.faint }));
        if cresp.hovered() {
            paint_check(&pt, cc, 0.9, p.accent.gamma_multiply(0.6));
        }
    }
    if cresp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if cresp.clicked() {
        if r.done {
            core.uncomplete_reminder(r.id);
        } else {
            core.complete_reminder(r.id);
        }
    } else if resp.clicked() {
        st.edit = Some(ReminderEdit::new(r.clone(), false));
    }
    // 제목
    let x = rect.left() + 48.0;
    let mut fmt = egui::TextFormat { font_id: FontId::proportional(14.5), color: if r.done { p.faint } else { p.text }, ..Default::default() };
    if r.done {
        fmt.strikethrough = Stroke::new(1.0_f32, p.faint);
    }
    let mut title = r.title.clone();
    if r.important && !r.done {
        title = format!("★ {title}");
    }
    let right_w = 190.0;
    let mut job = egui::text::LayoutJob::single_section(title, fmt);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - x - right_w);
    let g = ui.fonts(|f| f.layout_job(job));
    let has_notes = !r.notes.trim().is_empty();
    let ty = if has_notes { rect.center().y - 9.0 } else { rect.center().y };
    pt.galley(Pos2::new(x, ty - g.size().y / 2.0), g, p.text);
    if has_notes {
        let mut job = egui::text::LayoutJob::single_section(r.notes.lines().next().unwrap_or("").to_string(), egui::TextFormat { font_id: FontId::proportional(12.0), color: p.faint, ..Default::default() });
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.right() - x - right_w);
        let g = ui.fonts(|f| f.layout_job(job));
        pt.galley(Pos2::new(x, rect.center().y + 2.0), g, p.faint);
    }
    // 오른쪽: 시각 · 반복 · To Do
    let mut rx = rect.right() - 16.0;
    if r.todo_id.is_some() {
        let g = ui.fonts(|f| f.layout_no_wrap("To Do".into(), FontId::proportional(11.0), p.accent));
        let br = Rect::from_min_size(Pos2::new(rx - g.size().x - 12.0, rect.center().y - 10.0), egui::vec2(g.size().x + 12.0, 20.0));
        pt.rect_filled(br, cr(10.0), p.accent.gamma_multiply(0.14));
        pt.galley(br.min + egui::vec2(6.0, 3.0), g, p.accent);
        rx = br.left() - 8.0;
    }
    if r.repeat != Repeat::None {
        let g = ui.fonts(|f| f.layout_no_wrap(format!("↻ {}", r.repeat.label()), FontId::proportional(12.0), p.muted));
        pt.galley(Pos2::new(rx - g.size().x, rect.center().y - g.size().y / 2.0), g.clone(), p.muted);
        rx -= g.size().x + 10.0;
    }
    if let Some(d) = r.due.as_deref().and_then(parse_due) {
        let col = if r.done {
            p.faint
        } else if d < now {
            p.red
        } else if d.date() == now.date() {
            p.accent
        } else {
            p.muted
        };
        pt.text(Pos2::new(rx, rect.center().y), Align2::RIGHT_CENTER, due_label(d), FontId::proportional(13.0), col);
    }
    ui.add_space(4.0);
}

// ── 편집 창 ────────────────────────────────────────────────

pub struct ReminderEdit {
    pub r: Reminder,
    pub is_new: bool,
    has_time: bool,
    date: NaiveDate,
    hour: u32,
    minute: u32,
    confirm_delete: bool,
}

impl ReminderEdit {
    pub fn new(r: Reminder, is_new: bool) -> Self {
        let d = r.due.as_deref().and_then(parse_due);
        let base = d.unwrap_or_else(|| Local::now().naive_local() + Duration::hours(1));
        use chrono::Timelike;
        ReminderEdit { has_time: d.is_some(), date: base.date(), hour: base.hour(), minute: base.minute(), r, is_new, confirm_delete: false }
    }

    pub fn apply_pick(&mut self, t: PickTarget, sound: String) {
        if t == PickTarget::Alarm {
            self.r.sound = sound;
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, core: &Arc<Core>, p: &Pal, slot: &PickSlot) -> Outcome {
        let mut out = Outcome::Keep;
        egui::Modal::new(egui::Id::new("rem-edit")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.heading(if self.is_new { "새 리마인더" } else { "리마인더 편집" });
            ui.add_space(12.0);
            ui.add(egui::TextEdit::singleline(&mut self.r.title).desired_width(f32::INFINITY).font(FontId::proportional(16.0)).hint_text("할 일").margin(egui::Margin::symmetric(12, 8)));
            ui.add_space(6.0);
            ui.add(egui::TextEdit::multiline(&mut self.r.notes).desired_width(f32::INFINITY).desired_rows(2).hint_text("메모 (선택)"));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                toggle(ui, p, &mut self.has_time);
                ui.label(RichText::new("알림 시각").size(14.5));
            });
            if self.has_time {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let today = Local::now().date_naive();
                    let mon = today + Duration::days((7 - today.weekday().num_days_from_monday() as i64) % 7 + if today.weekday().num_days_from_monday() == 0 { 7 } else { 0 });
                    for (l, d) in [("오늘", today), ("내일", today + Duration::days(1)), ("모레", today + Duration::days(2)), ("다음 주 월", mon)] {
                        if ui.selectable_label(self.date == d, l).clicked() {
                            self.date = d;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let mut y = self.date.year();
                    let mut m = self.date.month();
                    let mut d = self.date.day();
                    let c1 = ui.add(egui::DragValue::new(&mut y).range(2024..=2100).speed(0.05).suffix(" 년")).changed();
                    let c2 = ui.add(egui::DragValue::new(&mut m).range(1..=12).speed(0.05).suffix(" 월")).changed();
                    let c3 = ui.add(egui::DragValue::new(&mut d).range(1..=31).speed(0.1).suffix(" 일")).changed();
                    if c1 || c2 || c3 {
                        let last = (28..=31).rev().find(|dd| NaiveDate::from_ymd_opt(y, m, *dd).is_some()).unwrap_or(28);
                        if let Some(nd) = NaiveDate::from_ymd_opt(y, m, d.min(last)) {
                            self.date = nd;
                        }
                    }
                    ui.label(RichText::new(format!("({})", WD[self.date.weekday().num_days_from_monday() as usize])).color(p.muted));
                    ui.add_space(14.0);
                    ui.style_mut().override_font_id = Some(digits(22.0));
                    ui.add(egui::DragValue::new(&mut self.hour).range(0..=23).speed(0.08).custom_formatter(|v, _| format!("{:02}", v as u32)));
                    ui.label(RichText::new(":").font(digits(22.0)));
                    ui.add(egui::DragValue::new(&mut self.minute).range(0..=59).speed(0.15).custom_formatter(|v, _| format!("{:02}", v as u32)));
                    ui.style_mut().override_font_id = None;
                });
                ui.horizontal(|ui| {
                    for (h, m) in [(9, 0), (12, 0), (14, 0), (18, 0)] {
                        if ui.small_button(format!("{h:02}:{m:02}")).clicked() {
                            self.hour = h;
                            self.minute = m;
                        }
                    }
                });
                ui.add_space(8.0);
                egui::Grid::new("rem-opts").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                    ui.label(RichText::new("반복").color(p.muted).size(12.5));
                    let opts: Vec<(Repeat, &str)> = Repeat::ALL.iter().map(|r| (*r, r.label())).collect();
                    segmented(ui, p, &mut self.r.repeat, &opts);
                    ui.end_row();
                    ui.label(RichText::new("소리").color(p.muted).size(12.5));
                    ui.horizontal(|ui| sound_picker(ui, p, core, "rem-sound", &mut self.r.sound, 180.0, slot, PickTarget::Alarm));
                    ui.end_row();
                });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                toggle(ui, p, &mut self.r.important);
                ui.label(RichText::new("중요").size(14.0));
                if self.r.todo_id.is_some() {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Microsoft To Do 와 동기화됨").color(p.accent).size(12.5));
                }
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if !self.is_new {
                    if self.confirm_delete {
                        if danger(ui, p, "정말 삭제").clicked() {
                            core.delete_reminder(self.r.id);
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
                    let ok = !self.r.title.trim().is_empty();
                    if ui.add_enabled_ui(ok, |ui| primary(ui, p, "  저장  ")).inner.clicked() {
                        self.r.title = self.r.title.trim().to_string();
                        self.r.due = if self.has_time {
                            Some(fmt_due(self.date.and_time(NaiveTime::from_hms_opt(self.hour, self.minute, 0).unwrap())))
                        } else {
                            None
                        };
                        if !self.has_time {
                            self.r.repeat = Repeat::None;
                        }
                        core.upsert_reminder(self.r.clone());
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

/// 사이드바 숫자 (지남 + 오늘)
pub fn badge(core: &Core) -> usize {
    let now = Local::now().naive_local();
    core.data()
        .reminders
        .iter()
        .filter(|r| !r.done)
        .filter(|r| r.due.as_deref().and_then(parse_due).map(|d| d < now || d.date() == now.date()).unwrap_or(false))
        .count()
}

#[allow(dead_code)]
pub fn _id(_: Id) {}
