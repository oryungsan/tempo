//! Windows 종료 · 로그오프 · 다시 시작 알아채기 — 숨은 최상위 창이 WM_QUERYENDSESSION / WM_ENDSESSION 을 받아
//! 마지막으로 저장하고 기록. (eframe/winit 은 이 메시지를 앱에 넘기지 않아, OS 종료 때 기록 없이 강제 종료되던 것
//! — 매일 PC 를 꺼도 로그에 안 남아 PC 꺼짐 · 앱 죽음을 구분할 수 없었음)
use crate::core::Core;
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, MSG, WM_ENDSESSION, WM_QUERYENDSESSION,
    WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

static CORE: OnceLock<Arc<Core>> = OnceLock::new();

const ENDSESSION_CLOSEAPP: isize = 0x0000_0001;
const ENDSESSION_CRITICAL: isize = 0x4000_0000;
const ENDSESSION_LOGOFF: isize = 0x8000_0000u32 as i32 as isize;

fn why(l: LPARAM) -> &'static str {
    if l & ENDSESSION_CLOSEAPP != 0 {
        "앱 닫기 요청(설치 · 업데이트 프로그램)"
    } else if l & ENDSESSION_LOGOFF != 0 {
        "로그오프"
    } else if l & ENDSESSION_CRITICAL != 0 {
        "강제 종료"
    } else {
        "종료 · 다시 시작"
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match m {
        // 막지 않음 (그대로 종료되게)
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if w != 0 {
                if let Some(core) = CORE.get() {
                    let ok = crate::store::save(&core.data());
                    crate::store::log(format!("windows session ending ({}) — saved {}", why(l), if ok { "ok" } else { "FAILED" }));
                }
            }
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}

pub fn spawn(core: Arc<Core>) {
    let _ = CORE.set(core);
    let _ = std::thread::Builder::new().name("session".into()).spawn(|| unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let cls: Vec<u16> = "TempoSession\0".encode_utf16().collect();
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
        };
        RegisterClassW(&wc);
        // 메시지 전용 창(HWND_MESSAGE)은 종료 알림을 못 받음 → 보이지 않는 보통 최상위 창
        let title: Vec<u16> = "Tempo session\0".encode_utf16().collect();
        let h = CreateWindowExW(WS_EX_TOOLWINDOW, cls.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
        if h.is_null() {
            crate::store::log("session window failed — Windows 종료를 기록하지 못함");
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
