//! 창 끌기 — 누르는 순간 잡은 지점을 기억하고, 전용 스레드가 4ms 마다 커서를 따라 창을 옮긴다.
//! (egui StartDrag = 끌기 문턱 + 프레임 끝 처리 뒤에 Windows 이동 모드로 넘어가서, 그 사이 커서가 간 만큼
//!  창이 처음부터 뒤처졌다 — 저사양 PC 실측 24~60px. 이 방식은 UI 프레임 속도와 무관)
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetCursorPos, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, SetWindowPos, SWP_ASYNCWINDOWPOS,
    SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn find(title: &str) -> HWND {
    struct F<'a> {
        pid: u32,
        title: &'a str,
        hwnd: HWND,
    }
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> i32 {
        let f = &mut *(l as *mut F);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == f.pid {
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(h, buf.as_mut_ptr(), 256);
            if String::from_utf16_lossy(&buf[..n.max(0) as usize]) == f.title {
                f.hwnd = h;
                return 0;
            }
        }
        1
    }
    let mut f = F { pid: std::process::id(), title, hwnd: std::ptr::null_mut() };
    unsafe { EnumWindows(Some(cb), &mut f as *mut F as LPARAM) };
    f.hwnd
}

/// `grab` = 창 안에서 누른 지점 (물리 px). `moved` = 실제로 움직였으면 true (클릭과 구분)
pub fn start(title: String, grab: (i32, i32), moved: Arc<AtomicBool>) {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    moved.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        let h = find(&title);
        if !h.is_null() {
            let mut r: RECT = unsafe { std::mem::zeroed() };
            unsafe { GetWindowRect(h, &mut r) };
            let start = (r.left, r.top);
            let mut last = start;
            loop {
                if unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } as u16 & 0x8000 == 0 {
                    break;
                }
                let mut p = POINT { x: 0, y: 0 };
                unsafe { GetCursorPos(&mut p) };
                let to = (p.x - grab.0, p.y - grab.1);
                let dist = (to.0 - start.0).abs().max((to.1 - start.1).abs());
                if to != last && (dist > 3 || moved.load(Ordering::Relaxed)) {
                    moved.store(true, Ordering::Relaxed);
                    unsafe {
                        SetWindowPos(h, std::ptr::null_mut(), to.0, to.1, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS)
                    };
                    last = to;
                }
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
        }
        ACTIVE.store(false, Ordering::SeqCst);
    });
}

/// 이 프레임에 `resp` 위에서 왼쪽 버튼을 눌렀으면 끌기 시작
pub fn on_press(ctx: &eframe::egui::Context, resp: &eframe::egui::Response, title: &str, moved: &Arc<AtomicBool>) {
    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    if pressed && (resp.hovered() || resp.is_pointer_button_down_on()) {
        if let Some(o) = ctx.input(|i| i.pointer.press_origin()) {
            let ppp = ctx.pixels_per_point();
            start(title.to_string(), ((o.x * ppp).round() as i32, (o.y * ppp).round() as i32), moved.clone());
        }
    }
}

/// 창 모양 (물리 px). 투명 창이 안 되는 그래픽(Intel HD 515 같은 일부 OpenGL — 모서리가 검게 나옴)에서도
/// 둥근 모서리·원형이 되도록 Windows 창 영역 자체를 잘라낸다
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shape {
    Full,
    RoundRect { w: i32, h: i32, r: i32 },
    Ellipse { w: i32, h: i32 },
    /// 원형 본체(지름 d) + 그 아래 펼친 목록 판 (전체 높이 h)
    RingList { w: i32, d: i32, h: i32, r: i32, gap: i32 },
}

/// 적용했으면 true (창을 아직 못 찾았으면 false → 다음 프레임에 다시)
pub fn set_shape(title: &str, shape: Shape) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{CreateEllipticRgn, CreateRoundRectRgn, SetWindowRgn};
    if std::env::var_os("TEMPO_NO_SHAPE").is_some() {
        return true;
    }
    let h = find(title);
    if h.is_null() {
        return false;
    }
    unsafe {
        let rgn = match shape {
            Shape::Full => std::ptr::null_mut(),
            Shape::RoundRect { w, h, r } => CreateRoundRectRgn(0, 0, w + 1, h + 1, r * 2, r * 2),
            Shape::Ellipse { w, h } => CreateEllipticRgn(0, 0, w + 1, h + 1),
            Shape::RingList { w, d, h, r, gap } => {
                use windows_sys::Win32::Graphics::Gdi::{CombineRgn, DeleteObject, RGN_OR};
                let a = CreateEllipticRgn(0, 0, w + 1, d + 1);
                let b = CreateRoundRectRgn(0, d + gap, w + 1, h + 1, r * 2, r * 2);
                CombineRgn(a, a, b, RGN_OR);
                DeleteObject(b);
                a
            }
        };
        // 성공하면 영역은 시스템 소유 (DeleteObject 하지 않음)
        SetWindowRgn(h, rgn, 1);
    }
    true
}

/// 화면 커서 (물리 px)
pub fn cursor() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

pub fn lbutton_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000 != 0 }
}
