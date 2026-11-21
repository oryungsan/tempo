//! Microsoft To Do 연동 (Microsoft Graph) — 기기 코드 로그인 · 양방향 동기화.
//!   로그인: 화면에 뜬 코드를 https://microsoft.com/devicelogin 에 입력 (Azure 앱 등록의 클라이언트 ID 필요)
//!   토큰: Windows DPAPI 로 암호화해 %APPDATA%\Tempo\todo.token (이 PC·이 Windows 계정에서만 풀림)
//!   동기화: 2분마다 + 변경 직후. 고른 목록 하나 ↔ Tempo 리마인더
//!     · To Do 에 새로 생긴(완료 안 된) 작업 → 리마인더로 가져옴 (알림 시각 = 미리 알림, 없으면 기한일 09:00)
//!     · Tempo 에서 만든 리마인더 → To Do 작업으로 올림
//!     · 양쪽 수정 → 나중에 바뀐 쪽이 이김 · 완료/삭제도 양방향
use crate::core::{fmt_due, Core};
use crate::engine::unix_now;
use crate::model::{Reminder, Repeat};
use crate::store::{self, log};
use chrono::{Local, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const SCOPE: &str = "Tasks.ReadWrite User.Read offline_access";
const AUTH: &str = "https://login.microsoftonline.com/common/oauth2/v2.0";
const GRAPH: &str = "https://graph.microsoft.com/v1.0";
/// 클라이언트 ID 를 비워 두면 쓰는 Microsoft 공용 앱 (Microsoft Graph 명령줄 도구 — 개인 계정 기기 코드 로그인 지원).
/// 개인 계정은 디렉터리가 없으면 자체 앱 등록이 막혀 있어서(2026-11 Entra 안내) 기본값으로 둠
const DEFAULT_CLIENT: &str = "14d82eec-204b-4c2f-b7e8-296a70dab67e";

pub fn client_id_of(s: &str) -> String {
    if s.trim().is_empty() {
        DEFAULT_CLIENT.to_string()
    } else {
        s.trim().to_string()
    }
}

#[derive(Clone, Debug, Default)]
pub struct TodoState {
    pub connected: bool,
    /// 로그인 중: (코드, 주소)
    pub pending: Option<(String, String)>,
    pub lists: Vec<(String, String)>,
    pub last_sync: Option<chrono::DateTime<Local>>,
    pub last_error: Option<String>,
    pub busy: bool,
}

pub enum Msg {
    Connect,
    Disconnect,
    Sync,
}

fn tx() -> &'static Mutex<Option<Sender<Msg>>> {
    static TX: OnceLock<Mutex<Option<Sender<Msg>>>> = OnceLock::new();
    TX.get_or_init(|| Mutex::new(None))
}

pub fn send(m: Msg) {
    if let Some(t) = tx().lock().unwrap().as_ref() {
        let _ = t.send(m);
    }
}

/// 리마인더가 바뀌었음 → 곧 동기화
pub fn nudge() {
    send(Msg::Sync);
}

// ── DPAPI ───────────────────────────────────────────────

fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let inb = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out: CRYPT_INTEGER_BLOB = std::mem::zeroed();
        let ok = if protect {
            CryptProtectData(&inb, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out)
        } else {
            CryptUnprotectData(&inb, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out)
        };
        if ok == 0 {
            return None;
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(v)
    }
}

fn token_path() -> std::path::PathBuf {
    store::dir().join("todo.token")
}

fn load_refresh() -> Option<String> {
    let enc = std::fs::read(token_path()).ok()?;
    String::from_utf8(dpapi(&enc, false)?).ok()
}

fn save_refresh(rt: &str) {
    if let Some(enc) = dpapi(rt.as_bytes(), true) {
        let _ = std::fs::write(token_path(), enc);
    }
}

// ── HTTP ────────────────────────────────────────────────

fn agent() -> ureq::Agent {
    let tls = native_tls::TlsConnector::new().expect("tls");
    ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).timeout(Duration::from_secs(25)).build()
}

fn err_body(e: ureq::Error) -> (u16, Value) {
    match e {
        ureq::Error::Status(code, r) => (code, r.into_json().unwrap_or(Value::Null)),
        ureq::Error::Transport(t) => (0, json!({ "error": "transport", "error_description": t.to_string() })),
    }
}

struct Session {
    agent: ureq::Agent,
    client_id: String,
    refresh: String,
    access: String,
    expires: Instant,
}

impl Session {
    fn token(&mut self) -> Result<String, String> {
        if Instant::now() < self.expires && !self.access.is_empty() {
            return Ok(self.access.clone());
        }
        let r = self
            .agent
            .post(&format!("{AUTH}/token"))
            .send_form(&[("client_id", &self.client_id), ("grant_type", "refresh_token"), ("refresh_token", &self.refresh), ("scope", SCOPE)]);
        let v: Value = match r {
            Ok(r) => r.into_json().map_err(|e| e.to_string())?,
            Err(e) => {
                let (c, b) = err_body(e);
                return Err(format!("토큰 갱신 실패 ({c}): {}", b["error_description"].as_str().or(b["error"].as_str()).unwrap_or("")));
            }
        };
        self.access = v["access_token"].as_str().unwrap_or("").to_string();
        if let Some(rt) = v["refresh_token"].as_str() {
            self.refresh = rt.to_string();
            save_refresh(rt);
        }
        self.expires = Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(120));
        Ok(self.access.clone())
    }

    fn call(&mut self, method: &str, url: &str, body: Option<Value>) -> Result<Value, String> {
        let tok = self.token()?;
        let req = self.agent.request(method, url).set("Authorization", &format!("Bearer {tok}"));
        let r = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match r {
            Ok(r) => {
                if r.status() == 204 {
                    Ok(Value::Null)
                } else {
                    Ok(r.into_json().unwrap_or(Value::Null))
                }
            }
            Err(e) => {
                let (c, b) = err_body(e);
                if c == 404 && method == "DELETE" {
                    return Ok(Value::Null);
                }
                Err(format!("{method} {} → {c} {}", url.trim_start_matches(GRAPH), b["error"]["message"].as_str().or(b["error_description"].as_str()).unwrap_or("")))
            }
        }
    }

    fn get_all(&mut self, url: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let mut next = Some(url.to_string());
        let mut guard = 0;
        while let Some(u) = next.take() {
            guard += 1;
            if guard > 20 {
                break;
            }
            let v = self.call("GET", &u, None)?;
            if let Some(a) = v["value"].as_array() {
                out.extend(a.iter().cloned());
            }
            next = v["@odata.nextLink"].as_str().map(|s| s.to_string());
        }
        Ok(out)
    }
}

// ── 시간 변환 ───────────────────────────────────────────

fn parse_graph_dt(v: &Value) -> Option<NaiveDateTime> {
    let s = v["dateTime"].as_str()?;
    let s = s.split('.').next()?;
    let n = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()?;
    if v["timeZone"].as_str().unwrap_or("UTC") == "UTC" {
        Some(Utc.from_utc_datetime(&n).with_timezone(&Local).naive_local())
    } else {
        Some(n)
    }
}

fn graph_dt(local: NaiveDateTime) -> Value {
    let utc = Local.from_local_datetime(&local).earliest().map(|d| d.with_timezone(&Utc).naive_utc()).unwrap_or(local);
    json!({ "dateTime": utc.format("%Y-%m-%dT%H:%M:%S").to_string(), "timeZone": "UTC" })
}

fn graph_unix(s: Option<&str>) -> i64 {
    s.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp()).unwrap_or(0)
}

/// To Do 작업 → (제목, 메모, 알림 시각, 완료, 중요)
fn from_task(t: &Value) -> (String, String, Option<String>, bool, bool) {
    let title = t["title"].as_str().unwrap_or("").to_string();
    let notes = if t["body"]["contentType"].as_str() == Some("text") { t["body"]["content"].as_str().unwrap_or("").trim().to_string() } else { String::new() };
    let due = if t["isReminderOn"].as_bool().unwrap_or(false) {
        parse_graph_dt(&t["reminderDateTime"])
    } else {
        // 기한만 있으면 그날 09:00 에 알림
        parse_graph_dt(&t["dueDateTime"]).map(|d| d.date().and_hms_opt(9, 0, 0).unwrap())
    };
    (title, notes, due.map(fmt_due), t["status"].as_str() == Some("completed"), t["importance"].as_str() == Some("high"))
}

fn to_task(r: &Reminder) -> Value {
    let mut v = json!({
        "title": r.title,
        "body": { "content": r.notes, "contentType": "text" },
        "importance": if r.important { "high" } else { "normal" },
        "status": if r.done { "completed" } else { "notStarted" },
    });
    match r.due.as_deref().and_then(crate::core::parse_due) {
        Some(d) => {
            v["isReminderOn"] = json!(true);
            v["reminderDateTime"] = graph_dt(d);
            v["dueDateTime"] = graph_dt(d.date().and_hms_opt(0, 0, 0).unwrap());
        }
        None => {
            v["isReminderOn"] = json!(false);
            v["dueDateTime"] = Value::Null;
            v["reminderDateTime"] = Value::Null;
        }
    }
    v
}

// ── 스레드 ──────────────────────────────────────────────

pub fn spawn(core: Arc<Core>) {
    let (t, rx) = channel::<Msg>();
    *tx().lock().unwrap() = Some(t);
    std::thread::Builder::new()
        .name("todo".into())
        .spawn(move || {
            let mut sess: Option<Session> = None;
            let mut next_sync = Instant::now() + Duration::from_secs(3);
            loop {
                let wait = next_sync.saturating_duration_since(Instant::now());
                let msg = match rx.recv_timeout(wait) {
                    Ok(m) => Some(m),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                let settings = core.data().settings.todo.clone();
                match msg {
                    Some(Msg::Connect) => {
                        sess = connect(&core, &client_id_of(&settings.client_id));
                        next_sync = Instant::now();
                        continue;
                    }
                    Some(Msg::Disconnect) => {
                        sess = None;
                        let _ = std::fs::remove_file(token_path());
                        core.update(|d| {
                            d.settings.todo.enabled = false;
                            d.settings.todo.account.clear();
                        });
                        *core.todo.lock().unwrap() = TodoState::default();
                        log("todo: disconnected");
                        core.repaint();
                        continue;
                    }
                    Some(Msg::Sync) => {
                        // 변경 직후 — 3초 모아서 한 번
                        next_sync = next_sync.min(Instant::now() + Duration::from_secs(3));
                        continue;
                    }
                    None => {}
                }
                next_sync = Instant::now() + Duration::from_secs(120);
                if !settings.enabled {
                    continue;
                }
                if sess.is_none() {
                    if let Some(rt) = load_refresh() {
                        sess = Some(Session { agent: agent(), client_id: client_id_of(&settings.client_id), refresh: rt, access: String::new(), expires: Instant::now() });
                    } else {
                        set_err(&core, "로그인 정보가 없습니다 — 다시 연결하세요");
                        continue;
                    }
                }
                let s = sess.as_mut().unwrap();
                // 맞출 때마다 새 연결 — 토큰 갱신(약 58분마다) · Graph(실패 뒤 4분) 처럼 오래 쉰 연결을 다시 쓰면
                // 서버가 이미 끊은 연결이라 「원격 호스트에 의해 강제로 끊겼습니다」(10054)로 실패했음 (매시 2번)
                s.agent = agent();
                core.todo.lock().unwrap().busy = true;
                core.repaint();
                let r = sync(&core, s);
                {
                    let mut st = core.todo.lock().unwrap();
                    st.busy = false;
                    st.connected = r.is_ok() || st.connected;
                    match &r {
                        Ok(()) => {
                            st.last_sync = Some(Local::now());
                            st.last_error = None;
                        }
                        Err(e) => st.last_error = Some(e.clone()),
                    }
                }
                if let Err(e) = r {
                    log(format!("todo sync failed: {e}"));
                }
                core.repaint();
            }
        })
        .expect("todo thread");
}

fn set_err(core: &Core, e: &str) {
    let mut st = core.todo.lock().unwrap();
    st.last_error = Some(e.to_string());
    st.pending = None;
    st.busy = false;
    drop(st);
    core.repaint();
}

/// 기기 코드 로그인
fn connect(core: &Arc<Core>, client_id: &str) -> Option<Session> {
    let client_id = client_id.trim().to_string();
    if client_id.is_empty() {
        set_err(core, "클라이언트 ID 를 먼저 입력하세요");
        return None;
    }
    let ag = agent();
    let v: Value = match ag.post(&format!("{AUTH}/devicecode")).send_form(&[("client_id", &client_id), ("scope", SCOPE)]) {
        Ok(r) => r.into_json().ok()?,
        Err(e) => {
            let (c, b) = err_body(e);
            set_err(core, &format!("로그인 시작 실패 ({c}): {}", b["error_description"].as_str().unwrap_or("")));
            return None;
        }
    };
    let code = v["user_code"].as_str().unwrap_or("").to_string();
    let uri = v["verification_uri"].as_str().unwrap_or("https://microsoft.com/devicelogin").to_string();
    let device = v["device_code"].as_str().unwrap_or("").to_string();
    let mut interval = v["interval"].as_u64().unwrap_or(5);
    let deadline = Instant::now() + Duration::from_secs(v["expires_in"].as_u64().unwrap_or(900));
    {
        let mut st = core.todo.lock().unwrap();
        st.pending = Some((code.clone(), uri.clone()));
        st.last_error = None;
    }
    core.repaint();
    log(format!("todo: device code issued ({uri})"));
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(interval));
        if core.todo.lock().unwrap().pending.is_none() {
            return None; // 취소
        }
        let r = ag.post(&format!("{AUTH}/token")).send_form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device.as_str()),
        ]);
        match r {
            Ok(r) => {
                let t: Value = r.into_json().ok()?;
                let refresh = t["refresh_token"].as_str().unwrap_or("").to_string();
                save_refresh(&refresh);
                let mut s = Session {
                    agent: ag,
                    client_id: client_id.clone(),
                    refresh,
                    access: t["access_token"].as_str().unwrap_or("").to_string(),
                    expires: Instant::now() + Duration::from_secs(t["expires_in"].as_u64().unwrap_or(3600).saturating_sub(120)),
                };
                let me = s.call("GET", &format!("{GRAPH}/me"), None).unwrap_or(Value::Null);
                let account = me["mail"].as_str().or(me["userPrincipalName"].as_str()).unwrap_or("").to_string();
                core.update(|d| {
                    d.settings.todo.enabled = true;
                    d.settings.todo.account = account.clone();
                });
                {
                    let mut st = core.todo.lock().unwrap();
                    st.pending = None;
                    st.connected = true;
                }
                log("todo: connected");
                core.repaint();
                return Some(s);
            }
            Err(e) => {
                let (_, b) = err_body(e);
                match b["error"].as_str().unwrap_or("") {
                    "authorization_pending" => {}
                    "slow_down" => interval += 5,
                    other => {
                        set_err(core, &format!("로그인 실패: {}", b["error_description"].as_str().unwrap_or(other)));
                        return None;
                    }
                }
            }
        }
    }
    set_err(core, "로그인 시간이 지났습니다 — 다시 연결하세요");
    None
}

fn sync(core: &Arc<Core>, s: &mut Session) -> Result<(), String> {
    // 목록
    let lists = s.get_all(&format!("{GRAPH}/me/todo/lists"))?;
    let pairs: Vec<(String, String)> = lists
        .iter()
        .map(|l| (l["id"].as_str().unwrap_or("").to_string(), l["displayName"].as_str().unwrap_or("").to_string()))
        .collect();
    let default = lists.iter().find(|l| l["wellknownListName"].as_str() == Some("defaultList")).and_then(|l| l["id"].as_str()).map(|s| s.to_string());
    core.todo.lock().unwrap().lists = pairs.clone();
    let (list_id, deleted) = {
        let d = core.data();
        let chosen = d.settings.todo.list_id.clone().filter(|id| pairs.iter().any(|p| &p.0 == id)).or(default.clone());
        (chosen, d.todo_deleted.clone())
    };
    let Some(list_id) = list_id else { return Err("To Do 목록을 찾지 못했습니다".into()) };
    let list_name = pairs.iter().find(|p| p.0 == list_id).map(|p| p.1.clone()).unwrap_or_default();
    core.update_quiet(|d| {
        d.settings.todo.list_id = Some(list_id.clone());
        d.settings.todo.list_name = list_name.clone();
    });

    // 1) Tempo 에서 지운 것
    for (l, t) in &deleted {
        s.call("DELETE", &format!("{GRAPH}/me/todo/lists/{l}/tasks/{t}"), None)?;
    }
    if !deleted.is_empty() {
        core.update_quiet(|d| d.todo_deleted.retain(|x| !deleted.contains(x)));
    }

    // 2) 가져오기
    let tasks = s.get_all(&format!("{GRAPH}/me/todo/lists/{list_id}/tasks?$top=100"))?;
    let now = unix_now();
    let (mut added, mut changed, mut removed) = (0, 0, 0);
    core.update_quiet(|d| {
        let remote_ids: Vec<&str> = tasks.iter().filter_map(|t| t["id"].as_str()).collect();
        for t in &tasks {
            let Some(tid) = t["id"].as_str() else { continue };
            let rmod = graph_unix(t["lastModifiedDateTime"].as_str());
            let (title, notes, due, done, important) = from_task(t);
            match d.reminders.iter_mut().find(|r| r.todo_id.as_deref() == Some(tid)) {
                None => {
                    if done {
                        continue; // 이미 끝난 옛 작업은 안 가져옴
                    }
                    let mut r = Reminder::new(&title, due);
                    r.notes = notes;
                    r.important = important;
                    r.todo_id = Some(tid.to_string());
                    r.todo_list = Some(list_id.clone());
                    if t["recurrence"].is_object() {
                        r.repeat = Repeat::None; // 반복은 To Do 가 새 작업으로 만들어 줌
                    }
                    r.modified = now;
                    r.synced = now;
                    d.reminders.push(r);
                    added += 1;
                }
                Some(r) => {
                    let local_changed = r.modified > r.synced;
                    let remote_changed = rmod > r.synced;
                    if remote_changed && (!local_changed || rmod >= r.modified) {
                        r.title = title;
                        r.notes = notes;
                        if r.due != due {
                            r.due = due;
                        }
                        if done && !r.done {
                            r.done_at = Some(Local::now().format("%Y-%m-%dT%H:%M").to_string());
                        }
                        r.done = done;
                        r.important = important;
                        r.modified = now;
                        r.synced = now;
                        changed += 1;
                    }
                }
            }
        }
        // To Do 에서 지워진 것 (Tempo 쪽 변경이 없을 때만)
        let before = d.reminders.len();
        d.reminders.retain(|r| {
            !(r.todo_list.as_deref() == Some(list_id.as_str())
                && r.todo_id.as_deref().map(|id| !remote_ids.contains(&id)).unwrap_or(false)
                && r.modified <= r.synced)
        });
        removed = before - d.reminders.len();
    });

    // 3) 올리기
    let (new_local, dirty): (Vec<Reminder>, Vec<Reminder>) = {
        let d = core.data();
        (
            d.reminders.iter().filter(|r| r.todo_id.is_none() && !r.done).cloned().collect(),
            d.reminders.iter().filter(|r| r.todo_id.is_some() && r.modified > r.synced).cloned().collect(),
        )
    };
    let mut pushed = 0;
    for r in &new_local {
        let v = s.call("POST", &format!("{GRAPH}/me/todo/lists/{list_id}/tasks"), Some(to_task(r)))?;
        if let Some(tid) = v["id"].as_str() {
            let t = unix_now();
            core.update_quiet(|d| {
                if let Some(x) = d.reminders.iter_mut().find(|x| x.id == r.id && x.modified == r.modified) {
                    x.todo_id = Some(tid.to_string());
                    x.todo_list = Some(list_id.clone());
                    x.modified = t;
                    x.synced = t;
                }
            });
            pushed += 1;
        }
    }
    for r in &dirty {
        let l = r.todo_list.clone().unwrap_or(list_id.clone());
        let tid = r.todo_id.clone().unwrap();
        s.call("PATCH", &format!("{GRAPH}/me/todo/lists/{l}/tasks/{tid}"), Some(to_task(r)))?;
        let t = unix_now();
        core.update_quiet(|d| {
            if let Some(x) = d.reminders.iter_mut().find(|x| x.id == r.id && x.modified == r.modified) {
                x.modified = t;
                x.synced = t;
            }
        });
        pushed += 1;
    }
    if added + changed + removed + pushed > 0 {
        log(format!("todo sync '{list_name}': +{added} ~{changed} -{removed} ↑{pushed}"));
    }
    core.repaint();
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn https_works() {
        let r = super::agent().get("https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration").call().unwrap();
        let v: serde_json::Value = r.into_json().unwrap();
        assert!(v["token_endpoint"].as_str().unwrap().contains("oauth2/v2.0/token"));
    }

    #[test]
    fn time_roundtrip() {
        let local = chrono::NaiveDateTime::parse_from_str("2026-11-20 09:00", "%Y-%m-%d %H:%M").unwrap();
        let g = super::graph_dt(local);
        assert_eq!(g["timeZone"], "UTC");
        assert_eq!(super::parse_graph_dt(&g), Some(local));
        let t = serde_json::json!({"title":"a","status":"notStarted","isReminderOn":false,"dueDateTime":{"dateTime":"2026-11-19T15:00:00.0000000","timeZone":"UTC"}});
        assert_eq!(super::from_task(&t).2.as_deref(), Some("2026-11-20T09:00"));
    }
}
