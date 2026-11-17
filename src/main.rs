#![windows_subsystem = "windows"]
//! Tempo — 타이머 · 알람 · 리마인더 · 스톱워치 (Windows)
//!   tempo.exe           창 열고 시작
//!   tempo.exe --tray    트레이로 조용히 시작 (Windows 자동 시작용)

mod alarm;
mod api;
mod audio;
mod core;
mod dday;
mod engine;
mod hotkey;
mod model;
mod store;
mod todo;
mod tray;
mod ui;
mod world;

use eframe::egui;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

fn already_running() -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let tag = if std::env::var_os("TEMPO_DIR").is_some() { "Test" } else { "" };
    let name: Vec<u16> = format!("Local\\TempoAlarmTimerSingleInstance{tag}\0").encode_utf16().collect();
    let h = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    !h.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS
}

/// 이미 켜져 있으면 그쪽 메인 창을 띄움
fn ask_existing_to_show(port: u16) {
    if let Ok(mut s) = std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = s.write_all(b"GET /api/show HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        let mut buf = [0u8; 256];
        let _ = s.read(&mut buf);
    }
}

/// DPI 인식 전(= 논리 좌표) 작업 영역 — winit 이 DPI 인식을 켜기 전에 불러야 함
fn work_area() -> (f32, f32, f32, f32) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    let mut r: RECT = unsafe { std::mem::zeroed() };
    if unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut _, 0) } != 0 {
        (r.left as f32, r.top as f32, r.right as f32, r.bottom as f32)
    } else {
        (0.0, 0.0, 1280.0, 720.0)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (data, note) = store::load();
    if already_running() {
        ask_existing_to_show(data.settings.api_port);
        return;
    }
    let first_run = !store::data_path().exists();
    if first_run {
        store::save(&data);
    }
    let wa = work_area();
    let vs = {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
        let (x, y, w, h) = unsafe { (GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79)) };
        if w > 0 && h > 0 { (x as f32, y as f32, (x + w) as f32, (y + h) as f32) } else { wa }
    };
    let core = Arc::new(core::Core::new(data, wa, vs));
    store::log(format!("started v{} (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()));
    if let Some(n) = note {
        store::log(n);
    }

    hotkey::spawn(core.clone());
    api::spawn(core.clone());
    todo::spawn(core.clone());
    {
        let core = core.clone();
        std::thread::Builder::new()
            .name("tick".into())
            .spawn(move || loop {
                core.tick();
                std::thread::sleep(Duration::from_millis(100));
            })
            .expect("tick thread");
    }

    let show_main = !args.iter().any(|a| a == "--tray");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Tempo Host")
            .with_inner_size([1.0, 1.0])
            .with_position([wa.0, wa.1])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_taskbar(false)
            .with_mouse_passthrough(true)
            .with_window_level(egui::WindowLevel::AlwaysOnBottom),
        renderer: eframe::Renderer::Glow,
        vsync: false,
        persist_window: false,
        ..Default::default()
    };
    // 시험용: --page timers|alarms|reminders|stopwatch|countdown|dday|world|settings · --edit <타이머 이름> · --new-alarm
    //         --open new-countdown|new-dday|city (편집 창 바로 열기)
    let arg_after = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let start = ui::StartOpts { show_main, page: arg_after("--page"), edit: arg_after("--edit"), new_alarm: args.iter().any(|a| a == "--new-alarm"), focus: arg_after("--focus"), open: arg_after("--open") };
    let c2 = core.clone();
    if let Err(e) = eframe::run_native("tempo", options, Box::new(move |cc| Ok(Box::new(ui::App::new(cc, c2, start))))) {
        store::log(format!("window failed: {e}"));
    }
    store::log("exit");
}
