//! 로컬 전용 HTTP API (127.0.0.1)
//!   자동화 연동 타이머(설정에서 지정) — 다른 프로그램·스크립트가 읽고 시작:
//!     GET /status · /start(대기·완료일 때만) · /toggle · /pause · /resume · /reset · /skip
//!   전체:
//!     GET /api/timers                       모든 타이머 상태
//!     GET /api/timer?id=<id>&do=<cmd>       특정 타이머 (id 대신 name=<이름> 가능)
//!     GET /api/show                         메인 창 열기
//!     GET /api/world · /api/countdowns · /api/ddays   월드타임 · 카운트다운 · 디데이
//!     GET /api/update[?do=check|apply]       자동 업데이트 상태 · 지금 확인 · 지금 업데이트
//!     GET /api/sync[?mode=host|join|off&code=&host=&sound=on|off&find=1]   PC 연동 상태 · 설정 · 주변 찾기
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
            "/api/world" => {
                let utc = chrono::Utc::now();
                let lo = crate::world::local_offset_min(utc);
                let list = core.data().world.clone();
                Some(serde_json::Value::Array(
                    list.iter()
                        .map(|w| match crate::world::at(&w.tz, utc, lo) {
                            Some(t) => serde_json::json!({
                                "tz": w.tz,
                                "name": crate::world::display_name(&w.tz, &w.label),
                                "time": format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second),
                                "date": t.date.to_string(),
                                "utc": crate::world::utc_label(t.utc_min),
                                "diff": crate::world::diff_label(t.diff_min),
                                "day": crate::world::day_label(t.day_diff),
                            }),
                            None => serde_json::json!({ "tz": w.tz, "error": "unknown time zone" }),
                        })
                        .collect(),
                ))
            }
            "/api/countdowns" => {
                let now = chrono::Local::now().naive_local();
                let list = core.data().countdowns.clone();
                Some(serde_json::Value::Array(
                    list.iter()
                        .map(|c| {
                            let rem = c.target_dt().map(|t| (t - now).num_seconds());
                            serde_json::json!({
                                "id": c.id.to_string(),
                                "title": c.title,
                                "target": c.target,
                                "remaining": rem,
                                "remaining_text": rem.filter(|s| *s > 0).map(|s| { let (d, cl) = crate::dday::remain_text(s); format!("{d} {cl}").trim().to_string() }),
                                "done": rem.map(|s| s <= 0).unwrap_or(false),
                                "fired": c.fired,
                            })
                        })
                        .collect(),
                ))
            }
            "/api/ddays" => {
                let today = chrono::Local::now().date_naive();
                let list = core.data().ddays.clone();
                Some(serde_json::Value::Array(
                    list.iter()
                        .map(|d| {
                            let i = crate::dday::info(d, today);
                            serde_json::json!({
                                "id": d.id.to_string(),
                                "title": d.title,
                                "date": d.date,
                                "mode": format!("{:?}", d.mode).to_lowercase(),
                                "label": i.as_ref().map(|i| i.big.clone()),
                                "detail": i.as_ref().map(|i| i.small.clone()),
                                "target": i.as_ref().map(|i| i.target.to_string()),
                                "days": i.as_ref().map(|i| i.days),
                                "next": i.as_ref().and_then(|i| i.next.as_ref()).map(|n| serde_json::json!({ "name": n.0, "date": n.1.to_string(), "days": n.2 })),
                            })
                        })
                        .collect(),
                ))
            }
            "/api/sync" => {
                // 설정 바꾸기: ?mode=host|join|off &code= &host= &port= &sound=on|off
                if get("find").is_some() {
                    let port = core.data().settings.sync.port;
                    crate::sync::discover_async(port);
                    std::thread::sleep(Duration::from_millis(1800));
                }
                let (mode, code, host, port, sound) = (get("mode"), get("code"), get("host"), get("port"), get("sound"));
                if mode.is_some() || code.is_some() || host.is_some() || port.is_some() || sound.is_some() {
                    core.update(|d| {
                        let s = &mut d.settings.sync;
                        match mode.as_deref() {
                            Some("host") => s.mode = crate::model::SyncMode::Host,
                            Some("join") => s.mode = crate::model::SyncMode::Join,
                            Some("off") => s.mode = crate::model::SyncMode::Off,
                            _ => {}
                        }
                        if let Some(c) = code {
                            s.code = c;
                        }
                        if let Some(h) = host {
                            s.host = h;
                        }
                        if let Some(p) = port.and_then(|p| p.parse().ok()) {
                            s.port = p;
                        }
                        if let Some(v) = sound {
                            s.remote_sound = v == "on" || v == "1" || v == "true";
                        }
                    });
                    std::thread::sleep(Duration::from_millis(2500));
                }
                Some(crate::sync::status_json(&core))
            }
            "/api/update" => {
                match get("do").as_deref() {
                    Some("check") => {
                        crate::update::nudge();
                        std::thread::sleep(Duration::from_secs(8));
                    }
                    Some("apply") => crate::update::apply_now(),
                    _ => {}
                }
                Some(crate::update::status_json(&core))
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
