//! 로컬 전용 HTTP API (127.0.0.1)
//!   자동화 연동 타이머(설정에서 지정) — 다른 프로그램·스크립트가 읽고 시작:
//!     GET /status · /start(대기·완료일 때만) · /toggle · /pause · /resume · /reset · /skip
//!   전체:
//!     GET /api/timers                       모든 타이머 상태
//!     GET /api/timer?id=<id>&do=<cmd>       특정 타이머 (id 대신 name=<이름> 가능)
//!     GET /api/show                         메인 창 열기
use crate::core::Core;
use crate::engine::Cmd;
use crate::model::Id;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub fn spawn(core: Arc<Core>) {
    std::thread::Builder::new().name("api".into()).spawn(move || run(core)).expect("api thread");
}

fn query(url: &str) -> Vec<(String, String)> {
    url.split_once('?')
        .map(|(_, q)| {
            q.split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), decode(v)))
                .collect()
        })
        .unwrap_or_default()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn run(core: Arc<Core>) {
    let port = core.data().settings.api_port;
    let addr = format!("127.0.0.1:{port}");
    let mut warned = false;
    let server = loop {
        match tiny_http::Server::http(&addr) {
            Ok(s) => break s,
            Err(e) => {
                if !warned {
                    crate::store::log(format!("api bind {addr} failed: {e} — 10초마다 재시도"));
                    warned = true;
                }
                std::thread::sleep(Duration::from_secs(10));
            }
        }
    };
    crate::store::log(format!("api listening on {addr}"));
    let json = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap();
    for req in server.incoming_requests() {
        let url = req.url().to_string();
        let path = url.split('?').next().unwrap_or("").trim_end_matches('/').to_ascii_lowercase();
        let q = query(&url);
        let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        let body: Option<serde_json::Value> = match path.as_str() {
            "" | "/status" => Some(core.status_json(core.automation_id())),
            "/start" | "/toggle" | "/pause" | "/resume" | "/reset" | "/skip" => {
                let id = core.automation_id();
                if let (Some(id), Some(c)) = (id, Cmd::parse(&path[1..])) {
                    core.cmd(id, c, "api");
                }
                Some(core.status_json(id))
            }
            "/api/timers" => {
                let ids: Vec<Id> = core.data().timers.iter().map(|t| t.id).collect();
                Some(serde_json::Value::Array(ids.into_iter().map(|i| core.status_json(Some(i))).collect()))
            }
            "/api/timer" => {
                let id = get("id").and_then(|s| s.parse::<Id>().ok()).or_else(|| {
                    let n = get("name")?;
                    core.data().timers.iter().find(|t| t.name == n).map(|t| t.id)
                });
                if let (Some(id), Some(c)) = (id, get("do").and_then(|d| Cmd::parse(&d))) {
                    core.cmd(id, c, "api");
                }
                Some(core.status_json(id))
            }
            "/api/debug" => {
                if get("bench").is_some() {
                    core.frame_ms.lock().unwrap().clear();
                    core.update_ms.lock().unwrap().clear();
                    *core.bench_until.lock().unwrap() = Some(std::time::Instant::now() + Duration::from_secs(3));
                    core.repaint();
                    std::thread::sleep(Duration::from_millis(3300));
                }
                let stat = |v: &Vec<f32>| {
                    let n = v.len().max(1) as f32;
                    let mut s = v.clone();
                    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    serde_json::json!({
                        "n": v.len(),
                        "avg": (v.iter().sum::<f32>() / n * 10.0).round() / 10.0,
                        "p50": s.get(s.len() / 2).copied().unwrap_or(0.0),
                        "max": s.last().copied().unwrap_or(0.0),
                    })
                };
                let f = core.frame_ms.lock().unwrap().clone();
                let u = core.update_ms.lock().unwrap().clone();
                Some(serde_json::json!({ "frame_interval_ms": stat(&f), "update_ms": stat(&u), "fps": (f.len() as f32 / 3.0).round() }))
            }
            "/api/todo" => {
                match get("do").as_deref() {
                    Some("connect") => crate::todo::send(crate::todo::Msg::Connect),
                    Some("sync") => crate::todo::nudge(),
                    Some("disconnect") => crate::todo::send(crate::todo::Msg::Disconnect),
                    _ => {}
                }
                std::thread::sleep(Duration::from_millis(if get("do").as_deref() == Some("connect") { 2500 } else { 0 }));
                let st = core.todo.lock().unwrap().clone();
                let s = core.data().settings.todo.clone();
                Some(serde_json::json!({
                    "enabled": s.enabled,
                    "connected": st.connected,
                    "account": s.account,
                    "list": s.list_name,
                    "pending_code": st.pending.as_ref().map(|p| p.0.clone()),
                    "uri": st.pending.as_ref().map(|p| p.1.clone()),
                    "busy": st.busy,
                    "last_sync": st.last_sync.map(|t| t.to_rfc3339()),
                    "error": st.last_error,
                    "reminders": core.data().reminders.len(),
                }))
            }
            "/api/show" => {
                core.open_main.store(true, Ordering::Relaxed);
                core.repaint();
                Some(serde_json::json!({ "ok": true }))
            }
            _ => None,
        };
        let _ = match body {
            Some(b) => req.respond(tiny_http::Response::from_string(b.to_string()).with_header(json.clone())),
            None => req.respond(tiny_http::Response::from_string("not found").with_status_code(404)),
        };
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes() {
        assert_eq!(super::decode("%ED%83%80%EC%9D%B4%EB%A8%B8"), "타이머");
        assert_eq!(super::decode("a+b"), "a b");
        assert_eq!(super::decode("abc%"), "abc%");
    }
}
