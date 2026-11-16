//! 창 관리. 루트 뷰포트 = 보이지 않는 1px 호스트(늘 그려짐) — 메인 창·카드·알림 창은 모두 그 아래 자식 창.
//! (메인 창을 닫아 트레이로 보내도 카드와 알림이 계속 갱신되게 하려는 구조)
pub mod card;
pub mod drag;
pub mod editor;
pub mod focus;
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
}

pub struct App {
    core: Arc<Core>,
    main_open: bool,
    main: main_win::MainState,
    cards: HashMap<Id, card::CardState>,
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
        if o.new_alarm {
            main.page = main_win::Page::Alarms;
            main.alarm_edit = Some(editor::AlarmEdit::new(crate::model::AlarmDef::new(), true));
        }
        App { core, main_open: o.show_main, main, cards: HashMap::new(), _tray: tray, theme: None, titlebar: None, last_frame: None }
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
            if !card::card_visible(def, v) || core.card_hidden(def.id) {
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

        ring::show(ctx, &core, &p);

        let busy = views.iter().any(|v| v.phase == Phase::Running) || !core.rings().is_empty() || self.main.sw.running();
        if bench {
            core.update_ms.lock().unwrap().push(t0.elapsed().as_secs_f32() * 1000.0);
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(if busy { Duration::from_millis(100) } else { Duration::from_millis(500) });
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
