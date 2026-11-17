//! 창 관리. 루트 뷰포트 = 보이지 않는 1px 호스트(늘 그려짐) — 메인 창·카드·알림 창은 모두 그 아래 자식 창.
//! (메인 창을 닫아 트레이로 보내도 카드와 알림이 계속 갱신되게 하려는 구조)
pub mod beds;
pub mod card;
pub mod clocks;
pub mod drag;
pub mod editor;
pub mod event_card;
pub mod focus;
pub mod fx;
pub mod styles;
pub mod main_win;
pub mod native_menu;
pub mod reminders;
pub mod ring;
pub mod theme;

use crate::core::Core;
use crate::engine::Phase;
use crate::model::{Id, Theme};
use eframe::egui::{self, ViewportCommand, ViewportId};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub struct StartOpts {
    pub show_main: bool,
    pub page: Option<String>,
    pub edit: Option<String>,
    pub new_alarm: bool,
    pub focus: Option<String>,
    pub open: Option<String>,
}

pub struct App {
    core: Arc<Core>,
    main_open: bool,
    main: main_win::MainState,
    cards: HashMap<Id, card::CardState>,
    ev_cards: HashMap<(bool, Id), event_card::EvCardState>,
    board: beds::Board,
    _tray: Option<tray_icon::TrayIcon>,
    theme: Option<Theme>,
    /// 메인 창 제목 표시줄 색을 맞춘 테마 (창이 새로 열리면 None)
    titlebar: Option<Theme>,
    last_frame: Option<std::time::Instant>,
}

fn main_vid() -> ViewportId {
    ViewportId::from_hash_of("tempo-main")
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, core: Arc<Core>, o: StartOpts) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        core.set_ctx(cc.egui_ctx.clone());
        let tray = crate::tray::build(core.clone());
        let mut main = main_win::MainState::new();
        main.page = match o.page.as_deref() {
            Some("alarms") => main_win::Page::Alarms,
            Some("stopwatch") => main_win::Page::Stopwatch,
            Some("settings") => main_win::Page::Settings,
            Some("reminders") => main_win::Page::Reminders,
            Some("beds") => main_win::Page::Beds,
            Some("countdown") => main_win::Page::Countdown,
            Some("dday") => main_win::Page::Dday,
            Some("world") => main_win::Page::World,
            _ => main_win::Page::Timers,
        };
        if let Some(name) = o.edit {
            let id = core.data().timers.iter().find(|t| t.name == name).map(|t| t.id);
            if let Some(id) = id {
                main.edit_timer(&core, id);
            }
        }
        if let Some(name) = o.focus {
            main.focus = core.data().timers.iter().find(|t| t.name == name).map(|t| t.id);
        }
        match o.open.as_deref() {
            Some("new-countdown") => {
                main.page = main_win::Page::Countdown;
                let t = chrono::Local::now().naive_local() + chrono::Duration::hours(1);
                main.clk.cd_edit = Some(clocks::CountdownEdit::new(crate::model::Countdown::new("", t, crate::model::PALETTE[0]), true));
            }
            Some("new-dday") => {
                main.page = main_win::Page::Dday;
                let d = chrono::Local::now().date_naive() + chrono::Duration::days(30);
                main.clk.dd_edit = Some(clocks::DdayEdit::new(crate::model::Dday::new("", d, crate::model::DdayMode::Until, crate::model::PALETTE[5]), true));
            }
            Some(x) if x.starts_with("dday:") => {
                let id = core.data().ddays.iter().find(|d| d.title == x[5..]).map(|d| d.id);
                if let Some(id) = id {
                    main.open_clock(&core, true, id);
                }
            }
            Some("city") => {
                main.page = main_win::Page::World;
                main.clk.picker = Some(Default::default());
            }
            _ => {}
        }
        if o.new_alarm {
            main.page = main_win::Page::Alarms;
            main.alarm_edit = Some(editor::AlarmEdit::new(crate::model::AlarmDef::new(), true));
        }
        App { core, main_open: o.show_main, main, cards: HashMap::new(), ev_cards: HashMap::new(), board: Default::default(), _tray: tray, theme: None, titlebar: None, last_frame: None }
    }

    fn default_card_pos(&self, stack_y: f32, k: f32, style: crate::model::Style) -> egui::Pos2 {
        let (_, t, r, _) = self.core.work_area;
        let (w, _) = card::card_size(style);
        egui::pos2(r - w * k - 18.0, t + 18.0 + stack_y)
    }
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now();
        let core = self.core.clone();
        let bench = core.bench_until.lock().unwrap().map(|t| t0 < t).unwrap_or(false);
        if bench {
            if let Some(l) = self.last_frame {
                core.frame_ms.lock().unwrap().push((t0 - l).as_secs_f32() * 1000.0);
            }
            self.last_frame = Some(t0);
        } else {
            self.last_frame = None;
        }
        if core.quit.load(Ordering::Relaxed) {
            crate::store::log("quit");
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
            return;
        }
        if core.open_main.swap(false, Ordering::Relaxed) {
            if self.main_open {
                ctx.send_viewport_cmd_to(main_vid(), ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd_to(main_vid(), ViewportCommand::Focus);
            }
            self.main_open = true;
        }
        if let Some(id) = core.edit_request.lock().unwrap().take() {
            self.main.edit_timer(&core, id);
        }
        if let Some((is_dday, id)) = core.clock_request.lock().unwrap().take() {
            self.main_open = true;
            self.main.open_clock(&core, is_dday, id);
        }
        if let Some(id) = core.focus_request.lock().unwrap().take() {
            self.main.page = main_win::Page::Timers;
            self.main.focus = Some(id);
        }
        let settings = core.data().settings.clone();
        if self.theme != Some(settings.theme) {
            theme::apply_visuals(ctx, &theme::pal(settings.theme));
            self.theme = Some(settings.theme);
        }
        let p = theme::pal(settings.theme);

        // 호스트(루트)는 아무것도 그리지 않음
        egui::CentralPanel::default().frame(egui::Frame::default()).show(ctx, |_| {});

        // 메인 창
        if self.main_open {
            let size = settings.main_size.unwrap_or([980.0, 680.0]);
            let mut b = egui::ViewportBuilder::default()
                .with_title("Tempo")
                .with_inner_size(size)
                .with_min_inner_size([780.0, 540.0])
                .with_window_level(if settings.main_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal })
                .with_icon(theme::app_icon());
            if let Some(pos) = settings.main_pos.filter(|p| core.on_screen(p[0], p[1])) {
                b = b.with_position(pos);
            }
            let mut close = false;
            let main = &mut self.main;
            ctx.show_viewport_immediate(main_vid(), b, |ctx, _| {
                main_win::ui(ctx, &core, main, &p);
                if ctx.input(|i| i.viewport().close_requested()) {
                    close = true;
                    let (pos, size) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
                    core.update_quiet(|d| {
                        if let Some(r) = pos {
                            d.settings.main_pos = Some([r.min.x, r.min.y]);
                        }
                        if let Some(r) = size {
                            d.settings.main_size = Some([r.width(), r.height()]);
                        }
                    });
                }
            });
            if self.titlebar != Some(settings.theme) && set_titlebar(p.dark) {
                self.titlebar = Some(settings.theme);
            }
            if close {
                self.titlebar = None;
                if settings.close_to_tray {
                    self.main_open = false;
                    core.stop_preview();
                } else {
                    core.quit.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                }
            }
        }
        if std::mem::take(&mut self.main.reset_cards) {
            self.cards.clear();
            self.ev_cards.clear();
        }

        // 떠 있는 카드
        let k = settings.card_scale.clamp(0.5, 3.0);
        let defs = core.data().timers.clone();
        let views = core.views();
        // 기본 위치 = 화면 오른쪽 위부터 카드 높이만큼 차례로 쌓기 (디자인마다 높이가 다름)
        let mut stack_y = 0.0;
        let mut shown = Vec::new();
        for def in &defs {
            let Some(v) = views.iter().find(|v| v.id == def.id) else { continue };
            // 「카드 띄우기」 효과 = 꺼 둔 카드도 끝나기 전엔 보임
            let fx_show = fx::show_card(&core, def, v);
            // 베드 = 설정 「베드를 떠 있는 카드로도 보기」 하나로 (치료 중 · 끝난 베드)
            let visible = if def.bed.is_some() { settings.bed_cards && v.phase != Phase::Idle } else { card::card_visible(def, v) };
            if !fx_show && (!visible || core.card_hidden(def.id)) {
                continue;
            }
            let style = styles::style_of(def, settings.style);
            let pos = def
                .card_pos
                .filter(|p| core.on_screen(p[0], p[1]))
                .map(|p| egui::pos2(p[0], p[1]))
                .unwrap_or_else(|| self.default_card_pos(stack_y, def.card_scale.unwrap_or(k), style));
            stack_y += card::card_size(style).1 * def.card_scale.unwrap_or(k) + 12.0;
            let stt = self.cards.entry(def.id).or_insert_with(|| card::CardState::new(pos));
            card::show(ctx, &core, def, v, stt, def.card_scale.unwrap_or(k), &p, style);
            shown.push(def.id);
        }
        // 안 보이는 카드는 상태도 버림 (다시 뜰 때 저장된 위치로)
        self.cards.retain(|id, _| shown.contains(id));

        // 떠 있는 카운트다운 · 디데이 카드 — 타이머 카드 아래로 이어서 쌓기
        let now = chrono::Local::now().naive_local();
        let evs = event_card::floating(&core);
        let mut ev_live = false;
        let mut ev_shown = Vec::new();
        for ev in &evs {
            let pos = ev.card_pos().filter(|p| core.on_screen(p[0], p[1])).map(|p| egui::pos2(p[0], p[1])).unwrap_or_else(|| {
                let (_, t, r, _) = core.work_area;
                egui::pos2(r - event_card::W * k - 18.0, t + 18.0 + stack_y)
            });
            stack_y += event_card::H * k + 12.0;
            let stt = self.ev_cards.entry(ev.key()).or_insert_with(|| event_card::EvCardState::new(pos));
            event_card::show(ctx, &core, ev, stt, k, &p);
            ev_live |= ev.live(now);
            ev_shown.push(ev.key());
        }
        self.ev_cards.retain(|key, _| ev_shown.contains(key));

        // 치료실 보드 창
        if settings.bed_board && main_win::page_visible(&core, main_win::Page::Beds) && !beds::board(ctx, &core, &mut self.board, &p) {
            core.update_quiet(|d| d.settings.bed_board = false);
        }
        ring::show(ctx, &core, &p);
        fx::edge(ctx, &core, &defs, &views, &p);
        let fx_live = fx::any_live(&core, &defs, &views);

        // 업데이트 판단용: 편집 창 · 스톱워치 · 메인 창
        let m = &self.main;
        let hold = m.sw.running() || m.timer_edit.is_some() || m.alarm_edit.is_some() || m.rem.edit.is_some() || m.clk.editing();
        core.ui_hold.store(hold, std::sync::atomic::Ordering::Relaxed);
        core.main_visible.store(self.main_open, std::sync::atomic::Ordering::Relaxed);
        // 초 단위로 바뀌는 화면 (카운트다운 · 월드타임) · 사이드바 고정 카운트다운
        let live_page = self.main_open && matches!(self.main.page, main_win::Page::Countdown | main_win::Page::World | main_win::Page::Dday);
        let pinned_cd = self.main_open && core.data().countdowns.iter().any(|c| c.pinned && !c.fired);
        let busy = views.iter().any(|v| v.phase == Phase::Running || v.done_for.is_some()) || !core.rings().is_empty() || self.main.sw.running() || live_page || pinned_cd || ev_live;
        if bench {
            core.update_ms.lock().unwrap().push(t0.elapsed().as_secs_f32() * 1000.0);
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(if fx_live { 16 } else if busy { 100 } else { 500 }));
        }
    }
}

/// 메인 창 제목 표시줄을 테마에 맞춤 (Windows 11 DWM). 창을 찾았으면 true
fn set_titlebar(dark: bool) -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId};
    struct Find {
        pid: u32,
        hwnd: HWND,
    }
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> i32 {
        let f = &mut *(l as *mut Find);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == f.pid {
            let mut buf = [0u16; 64];
            let n = GetWindowTextW(h, buf.as_mut_ptr(), 64);
            if String::from_utf16_lossy(&buf[..n.max(0) as usize]) == "Tempo" {
                f.hwnd = h;
                return 0;
            }
        }
        1
    }
    let mut f = Find { pid: std::process::id(), hwnd: std::ptr::null_mut() };
    unsafe { EnumWindows(Some(cb), &mut f as *mut Find as LPARAM) };
    if f.hwnd.is_null() {
        return false;
    }
    let v: i32 = dark as i32;
    // 20 = DWMWA_USE_IMMERSIVE_DARK_MODE
    unsafe { DwmSetWindowAttribute(f.hwnd, 20, &v as *const i32 as *const _, 4) };
    true
}
