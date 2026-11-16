//! 전역 단축키 — 전용 스레드에서 RegisterHotKey + 메시지 루프. 데이터가 바뀌면 WM_RELOAD 로 다시 등록.
//! 등록 실패(다른 프로그램이 점유)는 5초마다 재시도.
use crate::core::Core;
use crate::engine::Cmd;
use crate::model::Id;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, SetTimer, MSG, WM_APP, WM_HOTKEY, WM_TIMER};

pub const WM_RELOAD: u32 = WM_APP + 1;

pub fn parse(s: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut vk = None;
    for part in s.split('+').map(|p| p.trim().to_ascii_uppercase()) {
        let v = match part.as_str() {
            "CTRL" | "CONTROL" => {
                mods |= MOD_CONTROL;
                continue;
            }
            "ALT" => {
                mods |= MOD_ALT;
                continue;
            }
            "SHIFT" => {
                mods |= MOD_SHIFT;
                continue;
            }
            "WIN" => {
                mods |= MOD_WIN;
                continue;
            }
            "SPACE" => 0x20,
            "PAGEUP" => 0x21,
            "PAGEDOWN" => 0x22,
            "END" => 0x23,
            "HOME" => 0x24,
            "INSERT" => 0x2D,
            "DELETE" => 0x2E,
            k if k.len() >= 2 && k.starts_with('F') && k[1..].chars().all(|c| c.is_ascii_digit()) => {
                let n: u32 = k[1..].parse().ok()?;
                if !(1..=24).contains(&n) {
                    return None;
                }
                0x6F + n
            }
            k if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => k.as_bytes()[0] as u32,
            _ => return None,
        };
        vk = Some(v);
    }
    vk.map(|v| (mods, v))
}

pub fn spawn(core: Arc<Core>) {
    std::thread::Builder::new().name("hotkey".into()).spawn(move || run(core)).expect("hotkey thread");
}

struct Binding {
    combo: String,
    timer: Id,
    cmd: Cmd,
    ok: bool,
}

fn collect(core: &Core) -> Vec<Binding> {
    let d = core.data();
    let mut v = Vec::new();
    for t in &d.timers {
        for (combo, cmd) in [(&t.hotkey_toggle, Cmd::Toggle), (&t.hotkey_reset, Cmd::Reset)] {
            if !combo.trim().is_empty() {
                v.push(Binding { combo: combo.trim().to_string(), timer: t.id, cmd, ok: false });
            }
        }
    }
    v
}

fn register(core: &Core, b: &mut [Binding], log_fail: bool) {
    for (i, x) in b.iter_mut().enumerate() {
        if x.ok {
            continue;
        }
        let Some((mods, vk)) = parse(&x.combo) else { continue };
        x.ok = unsafe { RegisterHotKey(std::ptr::null_mut(), i as i32 + 1, mods | MOD_NOREPEAT, vk) } != 0;
        if x.ok {
            crate::store::log(format!("hotkey {} registered", x.combo));
        } else if log_fail {
            crate::store::log(format!("hotkey {} busy (다른 프로그램이 사용 중 또는 중복) — 5초마다 재시도", x.combo));
        }
    }
    *core.hotkey_busy.lock().unwrap() = b.iter().filter(|x| !x.ok).map(|x| x.combo.clone()).collect();
    core.repaint();
}

fn run(core: Arc<Core>) {
    core.hotkey_tid.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    let mut binds = collect(&core);
    register(&core, &mut binds, true);
    unsafe { SetTimer(std::ptr::null_mut(), 0, 5000, None) };
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        match msg.message {
            WM_HOTKEY => {
                if let Some(b) = binds.get((msg.wParam as usize).wrapping_sub(1)) {
                    core.cmd(b.timer, b.cmd, "hotkey");
                }
            }
            WM_RELOAD => {
                for (i, b) in binds.iter().enumerate() {
                    if b.ok {
                        unsafe { UnregisterHotKey(std::ptr::null_mut(), i as i32 + 1) };
                    }
                }
                binds = collect(&core);
                register(&core, &mut binds, true);
            }
            WM_TIMER if binds.iter().any(|b| !b.ok && parse(&b.combo).is_some()) => register(&core, &mut binds, false),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        assert_eq!(parse("Ctrl+Alt+F9"), Some((MOD_CONTROL | MOD_ALT, 0x78)));
        assert_eq!(parse("ctrl + alt + f10"), Some((MOD_CONTROL | MOD_ALT, 0x79)));
        assert_eq!(parse("Shift+A"), Some((MOD_SHIFT, b'A' as u32)));
        assert_eq!(parse("Ctrl+Space"), Some((MOD_CONTROL, 0x20)));
        assert_eq!(parse("Ctrl+Bogus"), None);
    }
}
