//! 다른 프로그램 실행 — **핸들 상속 없이** (CreateProcessW bInheritHandles = FALSE), 실행 파일은 **절대 경로**로.
//!
//! std `Command` 는 Windows 에서 늘 상속을 켜는데, 이 프로세스가 받아 둔 소켓(API keep-alive 연결 ·
//! PC 연동 연결)은 상속 가능 핸들이라 자식(업데이트한 새 Tempo · 메모장 · PowerShell …)에게 넘어감 →
//! 이 프로세스가 끝나거나 소켓을 닫아도 연결이 안 끊겨, 상대는 아무도 읽지 않는 연결에 묶임
//! (업데이트 뒤 API 무응답 · PC 연동 30초+ 끊김의 원인).
//! 실행 파일 이름만 주면 Windows 가 현재 폴더부터 찾으므로, 늘 신뢰할 수 있는 절대 경로를 씀
//! (이 exe 경로 · Windows 폴더 · System32). 경로는 UTF-16 원본 그대로 넘김.
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateProcessW, GetExitCodeProcess, WaitForSingleObject, PROCESS_INFORMATION, STARTUPINFOW};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn invalid(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, msg.to_string())
}

fn dir_of(f: unsafe extern "system" fn(*mut u16, u32) -> u32) -> PathBuf {
    let mut buf = vec![0u16; 520];
    let n = unsafe { f(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return PathBuf::from(r"C:\Windows");
    }
    use std::os::windows::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]))
}

/// Windows 폴더의 프로그램 (explorer.exe · notepad.exe)
pub fn windows_exe(name: &str) -> PathBuf {
    dir_of(windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW).join(name)
}

/// System32 의 프로그램 (reg.exe · WindowsPowerShell\v1.0\powershell.exe)
pub fn system_exe(rel: &str) -> PathBuf {
    dir_of(windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW).join(rel)
}

pub fn powershell() -> PathBuf {
    system_exe(r"WindowsPowerShell\v1.0\powershell.exe")
}

/// 명령줄 인자 하나 (CommandLineToArgvW 규칙: 공백 · 탭 · 따옴표가 있거나 비었으면 감싸고,
/// 따옴표 앞 역슬래시는 2n+1, 닫는 따옴표 앞은 2n)
fn quote(a: &[u16]) -> Vec<u16> {
    const Q: u16 = b'"' as u16;
    const BS: u16 = b'\\' as u16;
    if !a.is_empty() && !a.iter().any(|&c| c == b' ' as u16 || c == b'\t' as u16 || c == Q) {
        return a.to_vec();
    }
    let mut out = vec![Q];
    let mut slashes = 0; // 바로 앞에 이어진 역슬래시 수 (이미 한 번씩 씀)
    for &c in a {
        match c {
            BS => slashes += 1,
            Q => {
                out.extend(std::iter::repeat_n(BS, slashes + 1));
                slashes = 0;
            }
            _ => slashes = 0,
        }
        out.push(c);
    }
    out.extend(std::iter::repeat_n(BS, slashes));
    out.push(Q);
    out
}

/// 명령줄 — 첫 낱말(프로그램 경로)은 따옴표로만 감쌈 (argv0 규칙: 안쪽 역슬래시 그대로)
fn command_line(exe: &Path, args: &[&OsStr]) -> std::io::Result<Vec<u16>> {
    let e: Vec<u16> = exe.as_os_str().encode_wide().collect();
    if e.contains(&0) || e.contains(&(b'"' as u16)) {
        return Err(invalid("실행 파일 경로에 NUL · 따옴표"));
    }
    let mut line = vec![b'"' as u16];
    line.extend(&e);
    line.push(b'"' as u16);
    for a in args {
        let w: Vec<u16> = a.encode_wide().collect();
        if w.contains(&0) {
            return Err(invalid("인자에 NUL"));
        }
        line.push(b' ' as u16);
        line.extend(quote(&w));
    }
    if line.len() >= 32_767 {
        return Err(invalid("명령줄이 너무 김"));
    }
    line.push(0);
    Ok(line)
}

/// 실행 중인 자식 (프로세스 핸들 보유 — 끝나길 기다릴 때)
pub struct Child {
    pub pid: u32,
    h: HANDLE,
}

impl Child {
    /// 끝날 때까지 최대 ms 기다림 → 끝났으면 종료 코드
    pub fn wait(&self, ms: u32) -> Option<u32> {
        if unsafe { WaitForSingleObject(self.h, ms) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        (unsafe { GetExitCodeProcess(self.h, &mut code) } != 0).then_some(code)
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.h) };
    }
}

/// 실행 (핸들 상속 없음). `exe` 는 절대 경로, `hidden` = 콘솔 창 없이
pub fn start(exe: &Path, args: &[&OsStr], hidden: bool) -> std::io::Result<Child> {
    if !exe.is_absolute() {
        return Err(invalid("실행 파일은 절대 경로여야 함"));
    }
    let mut line = command_line(exe, args)?;
    let app: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let flags = if hidden { CREATE_NO_WINDOW } else { 0 };
    // lpApplicationName = 절대 경로 (검색 없음), bInheritHandles = FALSE, 환경 · 현재 폴더 = 이 프로세스 것
    let ok = unsafe { CreateProcessW(app.as_ptr(), line.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0, flags, std::ptr::null(), std::ptr::null(), &si, &mut pi) };
    if ok == 0 {
        return Err(std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
    }
    unsafe { CloseHandle(pi.hThread) };
    Ok(Child { pid: pi.dwProcessId, h: pi.hProcess })
}

/// 띄우고 잊기 — pid
pub fn spawn(exe: &Path, args: &[&str]) -> std::io::Result<u32> {
    let a: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    start(exe, &a, false).map(|c| c.pid)
}

/// 숨겨서 실행하고 끝날 때까지 (최대 ms) — 종료 코드 (시간 넘으면 None)
pub fn run_hidden(exe: &Path, args: &[&OsStr], ms: u32) -> std::io::Result<Option<u32>> {
    Ok(start(exe, args, true)?.wait(ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> String {
        String::from_utf16(&quote(&s.encode_utf16().collect::<Vec<_>>())).unwrap()
    }

    #[test]
    fn quotes_args() {
        assert_eq!(q("--tray"), "--tray");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q("a\"b"), "\"a\\\"b\"");
        assert_eq!(q("C:\\dir with space\\"), "\"C:\\dir with space\\\\\"");
        assert_eq!(q("x\\\"y z"), "\"x\\\\\\\"y z\"");
        // 첫 낱말 = 따옴표로만, NUL · 상대 경로 거부
        let l = command_line(Path::new(r"C:\Program Files\Tempo\tempo.exe"), &[OsStr::new("--tray")]).unwrap();
        assert_eq!(String::from_utf16(&l[..l.len() - 1]).unwrap(), "\"C:\\Program Files\\Tempo\\tempo.exe\" --tray");
        assert!(command_line(Path::new(r"C:\a.exe"), &[OsStr::new("a\0b")]).is_err());
        assert!(start(Path::new("notepad.exe"), &[], true).is_err());
        assert!(windows_exe("explorer.exe").is_absolute() && powershell().ends_with(r"WindowsPowerShell\v1.0\powershell.exe"));
    }

    #[test]
    fn child_gets_exact_args_and_no_window() {
        // 실제 자식이 받은 인자 (한글 · 공백 · 따옴표 · 끝 역슬래시)를 그대로 돌려받음
        let out = std::env::temp_dir().join(format!("tempo-proc-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let args = ["한글 경로\\", "a\"b", "", "x y"];
        let ps1 = std::env::temp_dir().join(format!("tempo-proc-{}.ps1", std::process::id()));
        std::fs::write(&ps1, format!("\u{feff}$args | ForEach-Object {{ '[' + $_ + ']' }} | Set-Content -Encoding UTF8 -LiteralPath '{}'", out.display())).unwrap();
        let mut f: Vec<&OsStr> = vec![OsStr::new("-NoProfile"), OsStr::new("-NonInteractive"), OsStr::new("-ExecutionPolicy"), OsStr::new("Bypass"), OsStr::new("-File"), ps1.as_os_str()];
        f.extend(args.iter().map(OsStr::new));
        let code = run_hidden(&powershell(), &f, 30_000).unwrap();
        assert_eq!(code, Some(0));
        let got = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = got.trim_start_matches('\u{feff}').lines().collect();
        assert_eq!(lines, vec!["[한글 경로\\]", "[a\"b]", "[]", "[x y]"]);
        let _ = (std::fs::remove_file(&out), std::fs::remove_file(&ps1));
    }
}
