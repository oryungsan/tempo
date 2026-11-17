//! 떠 있는 카운트다운 · 디데이 카드 — 끌어서 이동(위치 기억), 오른쪽 클릭 메뉴, 두 번 누르면 편집
use super::theme::*;
use crate::core::Core;
use crate::dday::{self, date_ko};
use crate::model::{Countdown, Dday, Id};
use chrono::{Local, NaiveDateTime, Timelike};
use eframe::egui::{self, text::LayoutJob, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextFormat};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const W: f32 = 264.0;
pub const H: f32 = 112.0;

#[derive(Clone)]
pub enum Ev {
    Cd(Countdown),
    Dd(Dday),
}

impl Ev {
    /// (디데이인가, id)
    pub fn key(&self) -> (bool, Id) {
        match self {
            Ev::Cd(c) => (false, c.id),
            Ev::Dd(d) => (true, d.id),
        }
    }
    fn title(&self) -> &str {
        match self {
            Ev::Cd(c) => &c.title,
            Ev::Dd(d) => &d.title,
        }
    }
    fn color(&self) -> Color32 {
        color_of(match self {
            Ev::Cd(c) => &c.color,
            Ev::Dd(d) => &d.color,
        })
    }
    fn pinned(&self) -> bool {
        match self {
            Ev::Cd(c) => c.pinned,
            Ev::Dd(d) => d.pinned,
        }
    }
    pub fn card_pos(&self) -> Option<[f32; 2]> {
        match self {
            Ev::Cd(c) => c.card_pos,
            Ev::Dd(d) => d.card_pos,
        }
    }
    /// 초 단위로 바뀌는 중인가 (다시 그리기 간격)
    pub fn live(&self, now: NaiveDateTime) -> bool {
        match self {
            Ev::Cd(c) => c.target_dt().map(|t| t > now).unwrap_or(false),
            Ev::Dd(d) => dday::info(d, now.date()).and_then(|i| dday::soon(d, &i, now)).is_some(),
        }
    }
    fn win_title(&self) -> String {
        match self {
            Ev::Cd(c) => format!("{} · 카운트다운 — Tempo", c.title),
            Ev::Dd(d) => format!("{} · 디데이 — Tempo", d.title),
        }
    }
}

/// 떠 있는 카드로 켠 것들 (카운트다운 먼저)
pub fn floating(core: &Core) -> Vec<Ev> {
    let d = core.data();
    let mut v: Vec<Ev> = d.countdowns.iter().filter(|c| c.card).cloned().map(Ev::Cd).collect();
    v.extend(d.ddays.iter().filter(|x| x.card).cloned().map(Ev::Dd));
    v
}

pub struct EvCardState {
    init_pos: Pos2,
    last_pos: Option<Pos2>,
    dirty_at: Option<Instant>,
    dragged: Arc<AtomicBool>,
    shape: Option<super::drag::Shape>,
}

impl EvCardState {
    pub fn new(init_pos: Pos2) -> Self {
        EvCardState { init_pos, last_pos: None, dirty_at: None, dragged: Default::default(), shape: None }
    }
}

fn set_ev(core: &Core, key: (bool, Id), f: impl FnOnce(Option<&mut Countdown>, Option<&mut Dday>)) {
    core.update(|d| {
        if key.0 {
            f(None, d.ddays.iter_mut().find(|x| x.id == key.1));
        } else {
            f(d.countdowns.iter_mut().find(|x| x.id == key.1), None);
        }
    });
}

fn open_editor(core: &Core, key: (bool, Id)) {
    *core.clock_request.lock().unwrap() = Some(key);
    core.open_main.store(true, Ordering::Relaxed);
    core.repaint();
}

fn trunc(ctx: &egui::Context, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(10.0));
    ctx.fonts(|f| f.layout_job(job))
}

pub fn show(ctx: &egui::Context, core: &Arc<Core>, ev: &Ev, st: &mut EvCardState, k: f32, pc: &Pal) {
    let k = k.clamp(0.5, 4.0);
    let title = ev.win_title();
    let on_top = core.data().settings.cards_on_top;
    let builder = egui::ViewportBuilder::default()
        .with_title(title.clone())
        .with_inner_size([W * k, H * k])
        .with_position(st.init_pos)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_window_level(if on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal })
        .with_icon(app_icon());
    let key = ev.key();
    let vid = egui::ViewportId::from_hash_of(("evcard", key));
    ctx.show_viewport_immediate(vid, builder, |ctx, _| {
        // 위치 기억 (움직임 멈추고 0.8초 뒤)
        if let Some(p) = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min) {
            if st.last_pos.map(|l| (l - p).length() > 0.5).unwrap_or(true) {
                if st.last_pos.is_some() {
                    st.dirty_at = Some(Instant::now());
                }
                st.last_pos = Some(p);
            }
        }
        if st.dirty_at.map(|t| t.elapsed() > Duration::from_millis(800)).unwrap_or(false) {
            st.dirty_at = None;
            if let Some(p) = st.last_pos {
                core.update_quiet(|d| {
                    if key.0 {
                        if let Some(x) = d.ddays.iter_mut().find(|x| x.id == key.1) {
                            x.card_pos = Some([p.x, p.y]);
                        }
                    } else if let Some(x) = d.countdowns.iter_mut().find(|x| x.id == key.1) {
                        x.card_pos = Some([p.x, p.y]);
                    }
                });
            }
        }
        let mut menu_req = false;
        egui::CentralPanel::default().frame(egui::Frame::default()).show(ctx, |ui| {
            menu_req = draw(ui, ctx, core, ev, st, k, pc, &title);
        });
        let ppp = ctx.pixels_per_point();
        let px = |v: f32| (v * ppp).round() as i32;
        let shape = super::drag::Shape::RoundRect { w: px(W * k), h: px(H * k), r: px(16.0 * k) };
        if st.shape != Some(shape) && super::drag::set_shape(&title, shape) {
            st.shape = Some(shape);
        }
        if menu_req {
            menu(core, ev, pc.dark);
        }
    });
}

fn draw(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, ev: &Ev, st: &mut EvCardState, k: f32, pc: &Pal, title: &str) -> bool {
    let card = Rect::from_min_size(ui.max_rect().min, egui::vec2(W * k, H * k)).shrink(1.0);
    let p = ui.painter().clone();
    p.rect_filled(card, cr(16.0 * k), pc.card);
    p.rect_stroke(card, cr(16.0 * k), Stroke::new(1.0_f32, pc.border), egui::StrokeKind::Inside);
    let key = ev.key();
    let bg = ui.interact(card, ui.id().with(("evcard", key)), Sense::click());
    super::drag::on_press(ctx, &bg, title, &st.dragged);
    let moved = st.dragged.load(Ordering::Relaxed);
    if bg.double_clicked() && !moved {
        open_editor(core, key);
    }
    let menu_req = bg.secondary_clicked();

    let now = Local::now().naive_local();
    let col = ev.color();
    let (l, r, t) = (card.left() + 16.0 * k, card.right() - 16.0 * k, card.top());
    // 제목 줄: 색 점 · 이름 · 종류
    p.circle_filled(Pos2::new(l + 4.0 * k, t + 22.0 * k), 4.5 * k, col);
    let kind = if key.0 { "디데이" } else { "카운트다운" };
    let kg = ctx.fonts(|f| f.layout_no_wrap(kind.to_string(), FontId::proportional(10.5 * k), pc.faint));
    let kw = kg.size().x;
    let name = trunc(ctx, ev.title(), bold(13.5 * k), pc.text, r - l - 16.0 * k - kw - 26.0 * k);
    p.galley(Pos2::new(l + 14.0 * k, t + 22.0 * k - name.size().y / 2.0), name, pc.text);
    let hovering = ctx.input(|i| i.pointer.hover_pos()).map(|q| card.contains(q)).unwrap_or(false);
    if !hovering {
        p.galley(Pos2::new(r - kw, t + 22.0 * k - kg.size().y / 2.0), kg, pc.faint);
    }

    match ev {
        Ev::Cd(c) => {
            let Some(target) = c.target_dt() else { return menu_req };
            let ms = (target - now).num_milliseconds();
            if ms > 0 {
                let secs = (ms + 999) / 1000;
                let (days, clock) = dday::remain_text(secs);
                let f = digits(32.0 * k);
                let cw = digits_width(ctx, &clock, &f);
                draw_digits(&p, ctx, Pos2::new(r, t + 78.0 * k), true, &clock, f, if secs <= 60 { col } else { pc.text });
                if !days.is_empty() {
                    p.text(Pos2::new(r - cw - 8.0 * k, t + 74.0 * k), Align2::RIGHT_BOTTOM, &days, bold(22.0 * k), col);
                }
                let fmt = if target.second() == 0 { "%H:%M" } else { "%H:%M:%S" };
                let foot = format!("{} {}까지", date_ko(target.date(), now.date()), target.format(fmt));
                p.galley(Pos2::new(l, t + 86.0 * k), trunc(ctx, &foot, FontId::proportional(11.5 * k), pc.muted, r - l), pc.muted);
            } else {
                p.text(Pos2::new(l - 1.0 * k, t + 76.0 * k), Align2::LEFT_BOTTOM, "완료", bold(28.0 * k), pc.green);
                p.text(Pos2::new(r, t + 74.0 * k), Align2::RIGHT_BOTTOM, dday::ago_text(-ms / 1000), FontId::proportional(12.0 * k), pc.muted);
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        Ev::Dd(d) => {
            let Some(i) = dday::info(d, now.date()) else { return menu_req };
            let past = d.mode == crate::model::DdayMode::Until && i.days < 0;
            let big_col = if past { pc.muted } else { col };
            let bg_ = trunc(ctx, &i.big, digits(if i.big.chars().count() > 6 { 30.0 * k } else { 36.0 * k }), big_col, r - l);
            let by = t + 74.0 * k - bg_.size().y + 6.0 * k;
            p.galley(Pos2::new(l - 2.0 * k, by), bg_, big_col);
            let soon = dday::soon(d, &i, now);
            let small = match &soon {
                Some((_, s)) => {
                    let (_, clock) = dday::remain_text(*s);
                    clock
                }
                None => i.small.clone(),
            };
            if !small.is_empty() {
                let (font, c) = if soon.is_some() { (digits(18.0 * k), col) } else { (FontId::proportional(12.5 * k), pc.muted) };
                p.text(Pos2::new(r, t + 70.0 * k), Align2::RIGHT_BOTTOM, small, font, c);
            }
            let foot = match (&soon, &i.next) {
                (Some((label, _)), _) => label.clone(),
                (None, Some((n, _, 0))) => format!("오늘 {n}!"),
                (None, Some((n, dt, left))) => format!("다음 {n} · D-{left} ({})", date_ko(*dt, now.date())),
                (None, None) if i.days == 0 && d.mode != crate::model::DdayMode::Since => "오늘이에요!".into(),
                _ => date_ko(i.target, now.date()),
            };
            p.galley(Pos2::new(l, t + 86.0 * k), trunc(ctx, &foot, FontId::proportional(11.5 * k), pc.muted, r - l), pc.muted);
            ctx.request_repaint_after(Duration::from_millis(if soon.is_some() { 250 } else { 1000 }));
        }
    }

    // 닫기 × (마우스 올렸을 때) = 카드 끄기
    if hovering {
        let xc = Pos2::new(card.right() - 13.0 * k, card.top() + 13.0 * k);
        let xr = 8.0 * k;
        let rx = round_btn(ui, xc, xr, ui.id().with(("evclose", key)), pc.input, pc.red.gamma_multiply(0.85));
        let s = xr * 0.38;
        let c = if rx.hovered() { Color32::WHITE } else { pc.muted };
        p.line_segment([xc + egui::vec2(-s, -s), xc + egui::vec2(s, s)], Stroke::new(1.6_f32 * k, c));
        p.line_segment([xc + egui::vec2(-s, s), xc + egui::vec2(s, -s)], Stroke::new(1.6_f32 * k, c));
        if rx.on_hover_text("카드 끄기 — 편집 창에서 다시 켤 수 있습니다").clicked() {
            set_ev(core, key, |c, d| {
                if let Some(c) = c {
                    c.card = false;
                }
                if let Some(d) = d {
                    d.card = false;
                }
            });
        }
    }
    menu_req
}

/// 오른쪽 클릭 메뉴 (Windows 기본 메뉴)
fn menu(core: &Arc<Core>, ev: &Ev, dark: bool) {
    use super::native_menu::{cmd, popup, Item};
    let key = ev.key();
    let pinned = ev.pinned();
    let items = vec![cmd(1, "편집…"), Item::Cmd(2, "사이드바에 고정".into(), pinned, true), Item::Sep, cmd(3, "카드 끄기")];
    let core = core.clone();
    popup(items, dark, move |r| match r {
        Some(1) => open_editor(&core, key),
        Some(2) => set_ev(&core, key, |c, d| {
            if let Some(c) = c {
                c.pinned = !pinned;
            }
            if let Some(d) = d {
                d.pinned = !pinned;
            }
        }),
        Some(3) => set_ev(&core, key, |c, d| {
            if let Some(c) = c {
                c.card = false;
            }
            if let Some(d) = d {
                d.card = false;
            }
        }),
        _ => {}
    });
}
