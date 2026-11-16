//! 파일 위치 · 저장 · 기록. 데이터 = %APPDATA%\Tempo\data.json
use crate::model::Data;
use std::io::Write;
use std::path::PathBuf;
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

pub fn data_path() -> PathBuf {
    dir().join("data.json")
}

pub fn log_path() -> PathBuf {
    dir().join("tempo.log")
}

pub fn load() -> (Data, Option<String>) {
    match std::fs::read_to_string(data_path()) {
        Ok(t) => match serde_json::from_str::<Data>(t.trim_start_matches('\u{feff}')) {
            Ok(d) => (d, None),
            Err(e) => {
                // 깨진 파일은 옆에 보관하고 새로 시작
                let bad = dir().join(format!("data.broken-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                let _ = std::fs::rename(data_path(), &bad);
                (Data::seed(), Some(format!("data.json 읽기 실패 ({e}) → {} 로 보관, 새로 시작", bad.display())))
            }
        },
        Err(_) => (Data::seed(), Some("first run: seeded data".into())),
    }
}

pub fn save(d: &Data) {
    if let Ok(s) = serde_json::to_string_pretty(d) {
        let tmp = dir().join("data.json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, data_path());
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
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let st = if on {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        std::process::Command::new("reg")
            .args(["add", key, "/v", "Tempo", "/t", "REG_SZ", "/d", &format!("\"{exe}\" --tray"), "/f"])
            .creation_flags(NO_WINDOW)
            .status()
    } else {
        std::process::Command::new("reg").args(["delete", key, "/v", "Tempo", "/f"]).creation_flags(NO_WINDOW).status()
    };
    st.map(|s| s.success()).unwrap_or(false)
}

pub fn open_path(p: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe").arg(p).spawn();
}

/// 바로가기 만들기 (start_menu = 시작 메뉴, 아니면 바탕 화면). 성공하면 true
pub fn create_shortcut(start_menu: bool) -> bool {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let Ok(exe) = std::env::current_exe() else { return false };
    let q = |s: &str| s.replace('\'', "''");
    let folder = if start_menu { "[Environment]::GetFolderPath('Programs')" } else { "[Environment]::GetFolderPath('Desktop')" };
    let dir = exe.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let script = format!(
        "$w = New-Object -ComObject WScript.Shell; $l = $w.CreateShortcut((Join-Path {folder} 'Tempo.lnk')); $l.TargetPath = '{}'; $l.WorkingDirectory = '{}'; $l.Description = 'Tempo'; $l.Save()",
        q(&exe.display().to_string()),
        q(&dir)
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
