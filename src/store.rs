//! 파일 위치 · 저장 · 기록. 데이터 = %APPDATA%\Tempo\data.json
use crate::model::Data;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub fn dir() -> PathBuf {
    // TEMPO_DIR = 시험용 별도 데이터 폴더
    let d = match std::env::var_os("TEMPO_DIR") {
        Some(x) => PathBuf::from(x),
        None => std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("Tempo"),
    };
    let _ = std::fs::create_dir_all(&d);
    d
}

/// 시험 모드 = `TEMPO_TEST=1` **이고** `TEMPO_DIR` 이 기본 데이터 폴더(%APPDATA%\Tempo)가 아닌 다른 폴더.
/// 시험 전용 인자(치료실 창 열기 · 자동 닫기 · 포커스 안 뺏기)는 이때만 — 실제 데이터로 도는 배포 실행엔 안 걸림
pub fn test_mode() -> bool {
    std::env::var("TEMPO_TEST").as_deref() == Ok("1") && test_dir()
}

/// `TEMPO_DIR` 이 기본 데이터 폴더(%APPDATA%\Tempo)가 아닌 다른 폴더 = 시험 인스턴스
pub fn test_dir() -> bool {
    let Some(t) = std::env::var_os("TEMPO_DIR").filter(|v| !v.is_empty()) else { return false };
    let norm = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p).to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    let real = std::env::var_os("APPDATA").map(|a| norm(PathBuf::from(a).join("Tempo")));
    Some(norm(PathBuf::from(t))) != real
}

/// 내부망으로 여는 소켓(PC 연동 호스트 · 찾기 응답 · 내부망 베드 보기)의 주소.
/// 운영 = `0.0.0.0`(그대로). 단위 테스트 · 시험 인스턴스(다른 `TEMPO_DIR`) = `127.0.0.1` 만 —
/// 시험 exe 는 경로마다 새 프로그램이라 0.0.0.0 에 열 때마다 Windows 방화벽 「액세스 허용」 창이 떴음.
/// `TEMPO_BIND`(예: 127.0.0.1 · 0.0.0.0)로 직접 정할 수 있음 (옛 이름 `TEMPO_SYNC_BIND` 도 읽음)
pub fn bind_ip() -> String {
    for k in ["TEMPO_BIND", "TEMPO_SYNC_BIND"] {
        if let Ok(v) = std::env::var(k) {
            if !v.trim().is_empty() {
                return v.trim().to_string();
            }
        }
    }
    if cfg!(test) || test_dir() {
        "127.0.0.1".into()
    } else {
        "0.0.0.0".into()
    }
}

/// 이 컴퓨터 안에서만 (bind_ip 가 loopback) — 내부망 주소 알아보기 · 브로드캐스트도 하지 않음
pub fn loopback_only() -> bool {
    bind_ip().parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

pub fn data_path() -> PathBuf {
    dir().join("data.json")
}

pub fn log_path() -> PathBuf {
    dir().join("tempo.log")
}

fn parse(t: &str) -> Result<Data, serde_json::Error> {
    serde_json::from_str::<Data>(t.trim_start_matches('\u{feff}'))
}

/// 깨진 data.json 을 대신할 것: 직전 저장 중이던 임시 파일 → 켤 때 만든 백업 순
fn recover() -> Option<(Data, &'static str)> {
    [("data.json.tmp", "저장 중이던 임시 파일"), ("data.json.bak", "지난 실행 때 백업")]
        .into_iter()
        .find_map(|(f, what)| std::fs::read_to_string(dir().join(f)).ok().and_then(|t| parse(&t).ok()).map(|d| (d, what)))
}

/// 복구도 못 할 때 — 예시 타이머 없이 (예시의 Ctrl+Alt+F9/F10 단축키가 원래 쓰던 단축키를 뺏거나,
/// PC 연동으로 예시 항목이 다른 PC 에 퍼지지 않게)
fn empty() -> Data {
    Data { timers: vec![], world: vec![], ..Data::seed() }
}

/// 데이터 읽기. 처음 실행 = 예시 데이터. 깨졌거나 **없어졌으면** 임시 파일 · 백업에서 복구
/// (깨진 파일은 옆에 보관). 복구할 것도 없으면: 정말 처음이면 예시, 아니면 예시 없이 빈 데이터.
/// 읽기 자체가 안 되면(권한 · 다른 프로그램이 잠금) 잠깐 다시 시도, 그래도 안 되면 **저장하지 않고**
/// 빈 데이터로 실행 (그대로 저장하면 원래 파일을 덮어써 잃음)
pub fn load() -> (Data, Option<String>) {
    let path = data_path();
    let mut tries = 0;
    let t = loop {
        match std::fs::read_to_string(&path) {
            Ok(t) => break t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 주 파일이 없음 — 저장 중 끊겼거나(임시 파일만 남음) 깨진 파일을 보관한 뒤 복구본 저장에 실패한 경우.
                // 예시 데이터로 시작하면 원래 데이터 대신 예시가 PC 연동으로 다른 PC 에 퍼질 수 있음
                if let Some((d, what)) = recover() {
                    return (d, Some(format!("data.json 없음 → {what} 에서 복구")));
                }
                if had_data() {
                    return (empty(), Some("data.json 없음 · 복구할 파일도 읽을 수 없음 — 예시 없이 빈 데이터로 시작".into()));
                }
                return (Data::seed(), Some("first run: seeded data".into()));
            }
            Err(e) => {
                tries += 1;
                if tries >= 15 {
                    NO_SAVE.store(true, Ordering::Relaxed);
                    SAVE_OK.store(false, Ordering::Relaxed);
                    return (empty(), Some(format!("data.json 을 읽을 수 없음 ({e}) — 이번 실행은 저장하지 않음 (원래 파일 보호)")));
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    };
    match parse(&t) {
        Ok(d) => {
            // 다음에 깨졌을 때 돌아갈 곳
            if let Err(e) = std::fs::copy(&path, dir().join("data.json.bak")) {
                log(format!("data.json.bak 만들기 실패: {e}"));
            }
            (d, None)
        }
        Err(e) => {
            let bad = dir().join(format!("data.broken-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
            let kept = match std::fs::rename(&path, &bad) {
                Ok(()) => format!("{} 로 보관", bad.display()),
                Err(re) => format!("보관 실패({re})"),
            };
            match recover() {
                Some((d, what)) => (d, Some(format!("data.json 읽기 실패 ({e}) → {kept}, {what} 에서 복구"))),
                None => (empty(), Some(format!("data.json 읽기 실패 ({e}) → {kept}, 복구할 파일이 없어 빈 데이터로 시작"))),
            }
        }
    }
}

/// 전에 데이터가 있었던 흔적 (임시 파일 · 백업 · 깨진 파일 보관) — 있으면 「처음 실행」이 아님
fn had_data() -> bool {
    let d = dir();
    d.join("data.json.tmp").exists()
        || d.join("data.json.bak").exists()
        || std::fs::read_dir(&d).map(|it| it.flatten().any(|e| e.file_name().to_string_lossy().starts_with("data.broken-"))).unwrap_or(false)
}

/// 저장이 안 되고 있음 (실패 중 · 읽기 보호로 저장 안 함) — 화면에 경고
pub fn save_failing() -> bool {
    !SAVE_OK.load(Ordering::Relaxed)
}

/// 다른 일 없이 읽기만 (업데이트 감시용 — 깨진 파일 보관 · 백업 같은 일은 본 프로세스가)
pub fn peek() -> Option<Data> {
    std::fs::read_to_string(data_path()).ok().and_then(|t| parse(&t).ok())
}

static NO_SAVE: AtomicBool = AtomicBool::new(false);
static SAVE_OK: AtomicBool = AtomicBool::new(true);

/// 저장 (임시 파일 → 이름 바꾸기). 실패하면 처음 한 번 기록, 다시 되면 기록 — 성공 여부 반환
pub fn save(d: &Data) -> bool {
    if NO_SAVE.load(Ordering::Relaxed) {
        return false;
    }
    let r = serde_json::to_string_pretty(d).map_err(|e| e.to_string()).and_then(|s| {
        let tmp = dir().join("data.json.tmp");
        std::fs::write(&tmp, s).map_err(|e| format!("임시 파일 쓰기: {e}"))?;
        std::fs::rename(&tmp, data_path()).map_err(|e| format!("이름 바꾸기: {e}"))
    });
    match r {
        Ok(()) => {
            if !SAVE_OK.swap(true, Ordering::Relaxed) {
                log("data.json 저장 다시 됨");
            }
            true
        }
        Err(e) => {
            if SAVE_OK.swap(false, Ordering::Relaxed) {
                log(format!("data.json 저장 실패: {e} (다시 될 때까지 이 기록은 한 번만)"));
            }
            false
        }
    }
}

static LOG: Mutex<()> = Mutex::new(());

pub fn log(s: impl AsRef<str>) {
    let _g = LOG.lock().unwrap_or_else(|e| e.into_inner());
    let p = log_path();
    if std::fs::metadata(&p).map(|m| m.len() > 2_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&p, dir().join("tempo.log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), s.as_ref());
    }
}

/// HKCU Run 에 등록/해제 (--tray = 창 없이 트레이로 시작)
pub fn set_autostart(on: bool) -> bool {
    use std::ffi::OsStr;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let reg = crate::proc::system_exe("reg.exe");
    let r = if on {
        let Ok(exe) = std::env::current_exe() else { return false };
        let mut val = std::ffi::OsString::from("\"");
        val.push(exe.as_os_str());
        val.push("\" --tray");
        crate::proc::run_hidden(&reg, &[OsStr::new("add"), OsStr::new(key), OsStr::new("/v"), OsStr::new("Tempo"), OsStr::new("/t"), OsStr::new("REG_SZ"), OsStr::new("/d"), &val, OsStr::new("/f")], 15_000)
    } else {
        crate::proc::run_hidden(&reg, &[OsStr::new("delete"), OsStr::new(key), OsStr::new("/v"), OsStr::new("Tempo"), OsStr::new("/f")], 15_000)
    };
    matches!(r, Ok(Some(0)))
}

pub fn open_path(p: &std::path::Path) {
    let _ = crate::proc::start(&crate::proc::windows_exe("explorer.exe"), &[p.as_os_str()], false);
}


/// 바로가기 만들기 (start_menu = 시작 메뉴, 아니면 바탕 화면). 성공하면 true
pub fn create_shortcut(start_menu: bool) -> bool {
    use std::ffi::OsStr;
    let Ok(exe) = std::env::current_exe() else { return false };
    let q = |s: &str| s.replace('\'', "''");
    let folder = if start_menu { "[Environment]::GetFolderPath('Programs')" } else { "[Environment]::GetFolderPath('Desktop')" };
    let dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let script = format!(
        "$w = New-Object -ComObject WScript.Shell; $l = $w.CreateShortcut((Join-Path {folder} 'Tempo.lnk')); $l.TargetPath = '{}'; $l.WorkingDirectory = '{}'; $l.Description = 'Tempo'; $l.Save()",
        q(&exe.display().to_string()),
        q(&dir)
    );
    let r = crate::proc::run_hidden(&crate::proc::powershell(), &[OsStr::new("-NoProfile"), OsStr::new("-NonInteractive"), OsStr::new("-Command"), OsStr::new(&script)], 30_000);
    matches!(r, Ok(Some(0)))
}
