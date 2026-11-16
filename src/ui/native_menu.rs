//! Windows 기본 팝업 메뉴 (TrackPopupMenu) — 화면 경계를 Windows 가 알아서 피해서 잘리지 않는다.
//! (egui 메뉴는 창 안에서만 그려져, 작은 카드에서는 창을 늘려도 화면 끝에서 잘렸다)
//! 메뉴 모달 루프는 전용 스레드의 숨은 창에서 돌린다 — egui(winit) 이벤트 처리 중에 띄우면 재진입으로
//! 다음 마우스 입력을 놓쳤다(저사양 PC 실측: 두 번째 우클릭 무반응). 이 방식이면 메뉴가 떠 있어도 카드는 계속 갱신.
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos, PostMessageW, RegisterClassW,
    SetForegroundWindow, TrackPopupMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

pub enum Item {
    /// (명령 번호, 글자, 체크, 사용 가능)
    Cmd(u32, String, bool, bool),
    Sep,
    Sub(String, Vec<Item>),
}

pub fn cmd(id: u32, text: impl Into<String>) -> Item {
    Item::Cmd(id, text.into(), false, true)
}

type Done = Box<dyn FnOnce(Option<u32>) + Send>;

struct Req {
    items: Vec<Item>,
    dark: bool,
    done: Done,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn build(items: &[Item]) -> HMENU {
    let m = CreatePopupMenu();
    for it in items {
        match it {
            Item::Cmd(id, text, checked, enabled) => {
                let mut f = MF_STRING;
                if *checked {
                    f |= MF_CHECKED;
                }
                if !*enabled {
                    f |= MF_GRAYED;
                }
                let w = wide(text);
                AppendMenuW(m, f, *id as usize, w.as_ptr());
            }
            Item::Sep => {
                AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            }
            Item::Sub(text, sub) => {
                let s = build(sub);
                let w = wide(text);
                AppendMenuW(m, MF_STRING | MF_POPUP, s as usize, w.as_ptr());
            }
        }
    }
    m
}

/// 메뉴 모양을 테마에 맞춤 (Windows 10 1903+ 의 비공개 uxtheme 함수, 없으면 그냥 밝은 메뉴)
fn set_menu_theme(dark: bool) {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    unsafe {
        let lib = LoadLibraryW(wide("uxtheme.dll").as_ptr());
        if lib.is_null() {
            return;
        }
        // 135 = SetPreferredAppMode(2 항상 어둡게 · 3 항상 밝게), 136 = FlushMenuThemes
        if let Some(f) = GetProcAddress(lib, 135usize as *const u8) {
            let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set(if dark { 2 } else { 3 });
        }
        if let Some(f) = GetProcAddress(lib, 136usize as *const u8) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

fn sender() -> &'static Mutex<Sender<Req>> {
    static TX: OnceLock<Mutex<Sender<Req>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Req>();
        std::thread::Builder::new()
            .name("menu".into())
            .spawn(move || unsafe {
                let hinst = GetModuleHandleW(std::ptr::null());
                let cls = wide("TempoMenuOwner");
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
                let owner = CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    cls.as_ptr(),
                    wide("Tempo menu").as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    hinst,
                    std::ptr::null(),
                );
                for req in rx {
                    set_menu_theme(req.dark);
                    let m = build(&req.items);
                    let mut p = POINT { x: 0, y: 0 };
                    GetCursorPos(&mut p);
                    // 포그라운드가 아니면 바깥을 눌러도 메뉴가 안 닫힘 (MS 문서의 알려진 요구)
                    let fg = SetForegroundWindow(owner);
                    let t0 = std::time::Instant::now();
                    let r = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, p.x, p.y, 0, owner, std::ptr::null());
                    if std::env::var_os("TEMPO_DEBUG").is_some() {
                        crate::store::log(format!("menu at {},{} fg={fg} -> {r} after {} ms", p.x, p.y, t0.elapsed().as_millis()));
                    }
                    PostMessageW(owner, WM_NULL, 0, 0);
                    DestroyMenu(m);
                    (req.done)((r > 0).then_some(r as u32));
                }
            })
            .expect("menu thread");
        Mutex::new(tx)
    })
}

/// 커서 위치에 메뉴를 띄움 — 고르면(또는 닫으면) `done` 이 메뉴 스레드에서 불림
pub fn popup(items: Vec<Item>, dark: bool, done: impl FnOnce(Option<u32>) + Send + 'static) {
    let _ = sender().lock().unwrap().send(Req { items, dark, done: Box::new(done) });
}
