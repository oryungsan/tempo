//! 자동 업데이트 — GitHub 최신 릴리스 확인(켤 때 + 6시간마다) → tempo.exe 내려받기(SHA-256 검사)
//! → 타이머가 모두 쉬고 알림도 없을 때 교체 · 다시 시작.
//! 되돌리기: 옛 exe(tempo.old.exe)로 띄운 감시 프로세스(--update-watch)가 새 버전이 45초 안에
//! 로컬 API 로 자기 버전을 대답하지 않으면 옛 버전을 다시 놓고 띄운 뒤, 그 버전은 다시 받지 않음.
use crate::core::Core;
use crate::engine::Phase;
use crate::model::UpdateMode;
use crate::store::{self, log};
use chrono::{DateTime, Local};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_URL: &str = "https://api.github.com/repos/oryungsan/tempo/releases/latest";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Default, Debug)]
pub struct UpdateState {
    /// 확인한 최신 버전
    pub latest: Option<String>,
    pub last_check: Option<DateTime<Local>>,
    pub error: Option<String>,
    /// 내려받아 둔 새 버전 (교체 대기)
    pub ready: Option<String>,
    /// 받았지만 타이머·알림 때문에 기다리는 중
    pub waiting: bool,
    pub busy: bool,
}

static NUDGE: AtomicBool = AtomicBool::new(false);
static APPLY_NOW: AtomicBool = AtomicBool::new(false);

/// 「지금 확인」
pub fn nudge() {
    NUDGE.store(true, Ordering::Relaxed);
}

/// 「지금 업데이트」 (알림만 모드에서도 바로 — 단 타이머가 쉬고 있을 때)
pub fn apply_now() {
    APPLY_NOW.store(true, Ordering::Relaxed);
    nudge();
}

/// "1.4.0" / "v1.4.0" → (1, 4, 0)
pub fn ver(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().trim_start_matches(['v', 'V']).split('.').map(|x| x.trim().parse::<u32>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn newer(candidate: &str, current: &str) -> bool {
    match (ver(candidate), ver(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// 릴리스 설명에서 "SHA-256 (exe): `…`" 의 64자리 16진수
pub fn exe_sha(body: &str) -> Option<String> {
    let i = body.find("SHA-256 (exe)")?;
    let rest = &body[i..];
    let hex: String = rest.split(|c: char| !c.is_ascii_hexdigit()).find(|w| w.len() == 64)?.to_string();
    Some(hex.to_ascii_lowercase())
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn agent() -> ureq::Agent {
    let tls = native_tls::TlsConnector::new().expect("tls");
    ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).timeout(Duration::from_secs(60)).build()
}

fn url() -> String {
    // 시험용: 다른 주소(로컬 가짜 릴리스)
    std::env::var("TEMPO_UPDATE_URL").unwrap_or_else(|_| DEFAULT_URL.to_string())
}

fn exe_paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    Some((exe, dir.join("tempo.new.exe"), dir.join("tempo.old.exe")))
}

fn skip_path() -> PathBuf {
    store::dir().join("update-skip.txt")
}

pub fn spawn(core: Arc<Core>) {
    std::thread::Builder::new().name("update".into()).spawn(move || run(core)).expect("update thread");
}

fn set(core: &Core, f: impl FnOnce(&mut UpdateState)) {
    f(&mut core.update_state.lock().unwrap());
    core.repaint();
}

fn run(core: Arc<Core>) {
    // 켜진 직후엔 잠깐 쉼 (시작 부하 · 네트워크 준비)
    let started = Instant::now();
    let mut last: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_secs(3));
        let mode = core.data().settings.update_mode;
        let nudged = NUDGE.swap(false, Ordering::Relaxed);
        if mode == UpdateMode::Off && !nudged {
            continue;
        }
        let due = last.map(|t| t.elapsed() >= CHECK_EVERY).unwrap_or(started.elapsed() >= Duration::from_secs(40));
        if nudged || due {
            last = Some(Instant::now());
            check(&core);
        }
        let ready = core.update_state.lock().unwrap().ready.clone();
        if let Some(v) = ready {
            let now_req = APPLY_NOW.load(Ordering::Relaxed);
            if mode == UpdateMode::Auto || now_req {
                if try_apply(&core, &v) {
                    return;
                }
            }
        }
    }
}

fn check(core: &Core) {
    set(core, |s| {
        s.busy = true;
        s.error = None;
    });
    let r = check_inner(core);
    set(core, |s| {
        s.busy = false;
        s.last_check = Some(Local::now());
        if let Err(e) = &r {
            s.error = Some(e.clone());
        }
    });
    if let Err(e) = r {
        log(format!("update check failed: {e}"));
    }
}

fn check_inner(core: &Core) -> Result<(), String> {
    let a = agent();
    let v: serde_json::Value = a
        .get(&url())
        .set("User-Agent", &format!("Tempo/{CURRENT}"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("릴리스 확인 실패: {e}"))?
        .into_json()
        .map_err(|e| format!("릴리스 응답 이상: {e}"))?;
    let tag = v["tag_name"].as_str().ok_or("릴리스 응답에 버전 없음")?.trim_start_matches(['v', 'V']).to_string();
    set(core, |s| s.latest = Some(tag.clone()));
    if !newer(&tag, CURRENT) {
        return Ok(());
    }
    if std::fs::read_to_string(skip_path()).map(|s| s.trim() == tag).unwrap_or(false) {
        return Err(format!("{tag} 은 지난번 실행에 실패해 건너뜀"));
    }
    if core.update_state.lock().unwrap().ready.as_deref() == Some(tag.as_str()) {
        return Ok(()); // 이미 받아 둠
    }
    let asset = v["assets"]
        .as_array()
        .and_then(|l| l.iter().find(|x| x["name"].as_str() == Some("tempo.exe")))
        .and_then(|x| x["browser_download_url"].as_str())
        .ok_or_else(|| format!("{tag} 릴리스에 tempo.exe 가 없음"))?
        .to_string();
    let want = exe_sha(v["body"].as_str().unwrap_or("")).ok_or_else(|| format!("{tag} 릴리스 설명에 SHA-256 (exe) 없음"))?;
    log(format!("update: downloading {tag}"));
    let resp = a.get(&asset).set("User-Agent", &format!("Tempo/{CURRENT}")).call().map_err(|e| format!("내려받기 실패: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader().take(64 * 1024 * 1024).read_to_end(&mut buf).map_err(|e| format!("내려받기 실패: {e}"))?;
    if buf.len() < 200_000 || !buf.starts_with(b"MZ") {
        return Err(format!("받은 파일이 실행 파일이 아님 ({} 바이트)", buf.len()));
    }
    let got = sha256_hex(&buf);
    if got != want {
        return Err(format!("SHA-256 불일치 — 받은 파일 버림 ({}…)", &got[..12]));
    }
    let (_, new, _) = exe_paths().ok_or("실행 파일 위치를 모름")?;
    std::fs::write(&new, &buf).map_err(|e| format!("{} 에 쓸 수 없음 (폴더 권한): {e}", new.display()))?;
    log(format!("update: {tag} downloaded ({} bytes, sha ok)", buf.len()));
    set(core, |s| s.ready = Some(tag));
    Ok(())
}

/// 지금 바꿔도 되나 — 울리는 알림 없음, 편집 중 아님, 스톱워치 안 돎, 디데이 다시 알림 없음.
/// 타이머가 돌고 있어도 됨 (진행 상태를 저장해 새 버전이 이어받음)
fn idle(core: &Core) -> bool {
    // 끝나기 1초 전 같은 경계는 피함 (단계가 넘어가는 순간 교체 X)
    let edge = core.views().iter().any(|v| v.phase == Phase::Running && v.step_rem.as_secs_f32() < 1.5);
    !edge && core.rings().is_empty() && !core.ui_hold.load(Ordering::Relaxed) && !core.has_dday_snooze()
}

fn try_apply(core: &Arc<Core>, tag: &str) -> bool {
    if !idle(core) {
        set(core, |s| s.waiting = true);
        return false;
    }
    set(core, |s| s.waiting = false);
    APPLY_NOW.store(false, Ordering::Relaxed);
    match apply(core, tag) {
        Ok(()) => true,
        Err(e) => {
            log(format!("update apply failed: {e}"));
            set(core, |s| {
                s.error = Some(e);
                s.ready = None;
            });
            false
        }
    }
}

fn apply(core: &Arc<Core>, tag: &str) -> Result<(), String> {
    let (exe, new, old) = exe_paths().ok_or("실행 파일 위치를 모름")?;
    if !new.exists() {
        return Err("받아 둔 새 파일이 없음".into());
    }
    if old.exists() {
        std::fs::remove_file(&old).map_err(|e| format!("tempo.old.exe 를 지울 수 없음: {e}"))?;
    }
    // 실행 중인 exe 도 이름 바꾸기는 됨
    std::fs::rename(&exe, &old).map_err(|e| format!("현재 exe 이름 바꾸기 실패 (폴더 권한): {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("새 exe 놓기 실패: {e}"));
    }
    // 진행 중인 타이머 넘기기 (새 버전 · 되돌린 옛 버전 모두 시작할 때 읽음)
    core.save_runs_for_update();
    let me = std::process::id().to_string();
    let mut args: Vec<String> = vec!["--after-update".into(), me];
    if !core.main_visible.load(Ordering::Relaxed) {
        args.insert(0, "--tray".into());
    }
    let child = match std::process::Command::new(&exe).args(&args).spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::rename(&exe, &new);
            let _ = std::fs::rename(&old, &exe);
            return Err(format!("새 버전 실행 실패: {e}"));
        }
    };
    // 감시 = 옛 exe 로 (새 버전이 망가져도 돌아옴)
    let _ = std::process::Command::new(&old).args(["--update-watch", tag, &std::process::id().to_string(), &child.id().to_string()]).spawn();
    log(format!("update: {CURRENT} -> {tag}, restarting (new pid {})", child.id()));
    core.quit.store(true, Ordering::Relaxed);
    core.repaint();
    // 혹시 창 쪽이 안 닫히면
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(8));
        std::process::exit(0);
    });
    Ok(())
}

// ── 다른 프로세스 기다리기 · 끝내기 ───────────────────────

fn open(pid: u32, access: u32) -> Option<windows_sys::Win32::Foundation::HANDLE> {
    let h = unsafe { windows_sys::Win32::System::Threading::OpenProcess(access, 0, pid) };
    (!h.is_null()).then_some(h)
}

/// 그 프로세스가 끝날 때까지 (최대 ms)
pub fn wait_pid(pid: u32, ms: u32) {
    use windows_sys::Win32::System::Threading::{WaitForSingleObject, PROCESS_SYNCHRONIZE};
    if let Some(h) = open(pid, PROCESS_SYNCHRONIZE) {
        unsafe {
            WaitForSingleObject(h, ms);
            windows_sys::Win32::Foundation::CloseHandle(h);
        }
    }
}

fn kill_pid(pid: u32) {
    use windows_sys::Win32::System::Threading::{TerminateProcess, PROCESS_TERMINATE};
    if let Some(h) = open(pid, PROCESS_TERMINATE) {
        unsafe {
            TerminateProcess(h, 1);
            windows_sys::Win32::Foundation::CloseHandle(h);
        }
    }
}

fn api_version(port: u16) -> Option<String> {
    use std::io::Write;
    let mut s = std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    // /api/update 는 타이머가 없어도 늘 자기 버전(current)을 돌려줌
    s.write_all(b"GET /api/update HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").ok()?;
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    let body = buf.split("\r\n\r\n").nth(1)?;
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v["current"].as_str().map(|x| x.to_string())
}

/// --update-watch <새 버전> <옛 pid> <새 pid> — 옛 exe(tempo.old.exe)로 실행됨
pub fn watch(args: &[String]) {
    let tag = args.first().cloned().unwrap_or_default();
    let old_pid: u32 = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    let new_pid: u32 = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(0);
    let port = store::load().0.settings.api_port;
    wait_pid(old_pid, 15_000);
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(45) {
        std::thread::sleep(Duration::from_secs(2));
        if api_version(port).as_deref() == Some(tag.as_str()) {
            log(format!("update: {tag} running ok ({}s)", t0.elapsed().as_secs()));
            return;
        }
    }
    // 되돌리기
    log(format!("update: {tag} did not answer in 45s — rolling back"));
    kill_pid(new_pid);
    std::thread::sleep(Duration::from_millis(1500));
    let Ok(me) = std::env::current_exe() else { return };
    let Some(dir) = me.parent() else { return };
    let exe = dir.join("tempo.exe");
    let bad = dir.join("tempo.bad.exe");
    let _ = std::fs::remove_file(&bad);
    let _ = std::fs::rename(&exe, &bad);
    if let Err(e) = std::fs::copy(&me, &exe) {
        log(format!("update rollback: copy failed: {e}"));
        return;
    }
    let _ = std::fs::write(skip_path(), &tag);
    let _ = std::process::Command::new(&exe).arg("--tray").spawn();
    log(format!("update rollback: restored previous version, {tag} will be skipped"));
}

/// 설정 화면 · API 용 요약
pub fn status_json(core: &Core) -> serde_json::Value {
    let s = core.update_state.lock().unwrap().clone();
    serde_json::json!({
        "current": CURRENT,
        "latest": s.latest,
        "ready": s.ready,
        "waiting": s.waiting,
        "busy": s.busy,
        "error": s.error,
        "last_check": s.last_check.map(|t| t.to_rfc3339()),
        "mode": format!("{:?}", core.data().settings.update_mode).to_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("v1.4.0", "1.3.0"));
        assert!(newer("1.10.0", "1.9.9"));
        assert!(!newer("1.3.0", "1.3.0"));
        assert!(!newer("v1.2.9", "1.3.0"));
        assert!(!newer("garbage", "1.3.0"));
        let body = "받는 법 …\n\nSHA-256 (zip): `aaaa`\nSHA-256 (exe): `53C511A344CB7E7E29F3C531A13D31F7B967FC8B91087AE6904E35797DB1BB38`\n";
        assert_eq!(exe_sha(body).unwrap(), "53c511a344cb7e7e29f3c531a13d31f7b967fc8b91087ae6904e35797db1bb38");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
