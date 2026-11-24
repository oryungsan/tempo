//! 내부망 베드 보기 (선택 기능, 기본 끔, 이 PC 설정)
//!
//! 같은 내부망의 화면 기기(베드 옆 보드 · 태블릿 등)가 베드 상태를 **읽기만** 하는 작은 HTTP 서버.
//! 로컬 API(127.0.0.1)와 다른 포트에 따로 열고, 여기엔 읽기 하나만 있다 — 시작 · 비우기 같은 조작은 없음.
//!
//!   GET /api/beds[?no=3]     Authorization: Bearer <토큰>  (또는 X-Tempo-Token: <토큰> · ?token=<토큰>)
//!   → { "source":"tempo", "version", "now_ms", "tz_min", "beds":[ { no, state, patient, since, program,
//!        step, step_count, step_name, step_secs, step_remaining, total_secs, total_remaining, overtime } ] }
//!
//! state = empty(빈 베드) · waiting(치료 전) · running(치료 중) · paused(일시정지) · done(끝, overtime = 초과 초).
//! 토큰이 틀리면 401, 다른 경로 404, GET 이 아니면 405. 접수 메모(note)는 보내지 않음.
use crate::core::Core;
use crate::model::LanBeds;
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn spawn(core: Arc<Core>) {
    std::thread::Builder::new().name("lan-beds".into()).spawn(move || run(core)).expect("lan thread");
}

/// 켤 수 있는 설정인가 — 켜짐 + 토큰 8자 이상
pub fn usable(c: &LanBeds) -> bool {
    c.enabled && c.token.trim().chars().count() >= 8 && c.port != 0
}

fn run(core: Arc<Core>) {
    // (포트, 토큰, 서버)
    let mut cur: Option<(u16, String, tiny_http::Server)> = None;
    let mut warned_port = 0u16;
    let mut retry_at = Instant::now();
    let mut guard = Guard::default();
    loop {
        let cfg = core.data().settings.lan_beds.clone();
        let want = usable(&cfg);
        let token = cfg.token.trim().to_string();
        if let Some((port, _, _)) = &cur {
            if !want || *port != cfg.port {
                let port = *port;
                stop(cur.take().map(|c| c.2), port);
                crate::store::log(format!("lan beds: stopped (port {port})"));
            } else if let Some(c) = cur.as_mut() {
                c.1 = token.clone(); // 토큰만 바뀌면 서버는 그대로
            }
        }
        if want && cur.is_none() && Instant::now() >= retry_at {
            let addr = format!("0.0.0.0:{}", cfg.port);
            match tiny_http::Server::http(&addr) {
                Ok(s) => {
                    crate::store::log(format!("lan beds: listening on {addr} (read only)"));
                    warned_port = 0;
                    cur = Some((cfg.port, token.clone(), s));
                }
                Err(e) => {
                    if warned_port != cfg.port {
                        crate::store::log(format!("lan beds: bind {addr} failed: {e} — 10초마다 재시도"));
                        warned_port = cfg.port;
                    }
                    retry_at = Instant::now() + Duration::from_secs(10);
                }
            }
        }
        match &cur {
            Some((_, tok, s)) => {
                if let Ok(Some(req)) = s.recv_timeout(Duration::from_millis(500)) {
                    serve(&core, req, tok, &mut guard);
                }
            }
            None => std::thread::sleep(Duration::from_millis(700)),
        }
    }
}

/// 서버 끄기 — tiny_http 는 drop 때 듣는 주소(0.0.0.0)로 접속해 accept 를 깨우는데 Windows 에선 0.0.0.0 접속이 안 됨
/// → 127.0.0.1 로 한 번 접속해 받는 스레드를 끝냄 (포트가 바로 풀리게)
fn stop(s: Option<tiny_http::Server>, port: u16) {
    drop(s);
    let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300));
}

/// 토큰 틀림이 몰리면 잠깐 거절 (1분에 30번 넘으면 그 1분 동안 429)
#[derive(Default)]
struct Guard {
    window: Option<Instant>,
    fails: u32,
    logged: bool,
}

impl Guard {
    fn blocked(&mut self, now: Instant) -> bool {
        if self.window.map(|w| now.duration_since(w) > Duration::from_secs(60)).unwrap_or(true) {
            self.window = Some(now);
            self.fails = 0;
            self.logged = false;
        }
        self.fails > 30
    }
    fn fail(&mut self) {
        self.fails += 1;
    }
}

#[derive(Debug, PartialEq)]
pub enum Route {
    Beds(Option<u32>),
    NotFound,
    Method,
}

/// 경로 판정 — 읽기 하나만. `count=` 같은 다른 값은 무시 (베드를 만들지 않음)
pub fn route(method: &str, url: &str) -> Route {
    let path = url.split('?').next().unwrap_or("").trim_end_matches('/').to_ascii_lowercase();
    if path != "/api/beds" {
        return Route::NotFound;
    }
    if !method.eq_ignore_ascii_case("GET") {
        return Route::Method;
    }
    Route::Beds(param(url, "no").and_then(|n| n.parse().ok()))
}

fn param(url: &str, key: &str) -> Option<String> {
    url.split_once('?')?.1.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
}

/// 요청에 실린 토큰 (Authorization: Bearer · X-Tempo-Token · ?token=)
pub fn given_token(headers: &[(String, String)], url: &str) -> Option<String> {
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("authorization") {
            if let Some(t) = v.trim().strip_prefix("Bearer ").or_else(|| v.trim().strip_prefix("bearer ")) {
                return Some(t.trim().to_string());
            }
        }
        if k.eq_ignore_ascii_case("x-tempo-token") {
            return Some(v.trim().to_string());
        }
    }
    param(url, "token")
}

/// 같은 길이 비교 시간이 내용과 무관하게
pub fn token_ok(given: Option<&str>, want: &str) -> bool {
    let (Some(g), w) = (given, want.trim()) else { return false };
    if w.is_empty() || g.len() != w.len() {
        return false;
    }
    g.bytes().zip(w.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// 보내는 내용 — 로컬 /api/beds 항목에서 접수 메모 · 내부 id 를 뺀 것
pub fn board_json(beds: &serde_json::Value, only: Option<u32>, now_ms: i64, tz_min: i32) -> serde_json::Value {
    let list: Vec<serde_json::Value> = beds
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|b| only.map(|n| b["no"].as_u64() == Some(n as u64)).unwrap_or(true))
                .map(|b| {
                    let mut o = b.clone();
                    if let Some(m) = o.as_object_mut() {
                        for k in ["note", "id", "name"] {
                            m.remove(k);
                        }
                    }
                    o
                })
                .collect()
        })
        .unwrap_or_default();
    serde_json::json!({
        "source": "tempo",
        "version": env!("CARGO_PKG_VERSION"),
        "now_ms": now_ms,
        "tz_min": tz_min,
        "beds": list,
    })
}

fn serve(core: &Arc<Core>, mut req: tiny_http::Request, token: &str, guard: &mut Guard) {
    let json = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap();
    let nocache = tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap();
    let url = req.url().to_string();
    let reply = |req: tiny_http::Request, code: u16, body: serde_json::Value| {
        let _ = req.respond(tiny_http::Response::from_string(body.to_string()).with_header(json.clone()).with_header(nocache.clone()).with_status_code(code));
    };
    // 본문은 안 읽음 (읽기 전용) — 큰 본문을 보내도 그냥 닫음
    let _ = req.as_reader();
    let only = match route(req.method().as_str(), &url) {
        Route::NotFound => return reply(req, 404, serde_json::json!({ "error": "not found" })),
        Route::Method => return reply(req, 405, serde_json::json!({ "error": "read only" })),
        Route::Beds(n) => n,
    };
    if guard.blocked(Instant::now()) {
        if !guard.logged {
            crate::store::log("lan beds: too many wrong tokens — 1분 동안 거절");
            guard.logged = true;
        }
        return reply(req, 429, serde_json::json!({ "error": "too many attempts" }));
    }
    let headers: Vec<(String, String)> = req.headers().iter().map(|h| (h.field.as_str().as_str().to_string(), h.value.as_str().to_string())).collect();
    if !token_ok(given_token(&headers, &url).as_deref(), token) {
        guard.fail();
        return reply(req, 401, serde_json::json!({ "error": "token" }));
    }
    let utc = chrono::Utc::now();
    let tz_min = chrono::Local::now().offset().local_minus_utc() / 60;
    let body = board_json(&crate::ui::beds::beds_json(core), only, utc.timestamp_millis(), tz_min);
    reply(req, 200, body);
}

/// 새 토큰 — 헷갈리는 글자(0 O 1 I) 뺀 12자리, OS 난수
pub fn new_token() -> String {
    const A: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut buf = [0u8; 12];
    use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    let ok = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) } == 0;
    if !ok {
        // 거의 없음 — 그래도 비지 않게
        let h = format!("{}{:?}", crate::model::new_id(), Instant::now());
        for (i, b) in h.bytes().enumerate() {
            buf[i % 12] ^= b;
        }
    }
    buf.iter().map(|b| A[(*b as usize) % A.len()] as char).collect()
}

/// 이 PC 의 내부망 주소 (설정에 「기기에 넣을 주소」로 보여 줌) — 10초마다 다시 봄
pub fn local_ip() -> Option<String> {
    static CACHE: std::sync::Mutex<Option<(Instant, Option<String>)>> = std::sync::Mutex::new(None);
    let mut g = CACHE.lock().unwrap();
    if let Some((t, ip)) = g.as_ref() {
        if t.elapsed() < Duration::from_secs(10) {
            return ip.clone();
        }
    }
    let ip = route_ip();
    *g = Some((Instant::now(), ip.clone()));
    ip
}

/// 패킷은 안 보냄 (UDP connect 는 나갈 경로만 고름)
fn route_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.0.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then(|| ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reading_beds_is_routed() {
        assert_eq!(route("GET", "/api/beds"), Route::Beds(None));
        assert_eq!(route("GET", "/api/beds/?no=3"), Route::Beds(Some(3)));
        assert_eq!(route("get", "/API/BEDS?no=x"), Route::Beds(None));
        // 조작 · 다른 API 는 여기 없음
        for u in ["/api/bed?no=3&do=clear", "/api/bed?no=3&do=start&min=5", "/start", "/status", "/api/timers", "/api/timer?id=1&do=reset", "/api/sync?mode=off", "/api/update?do=apply", "/", "/api/show"] {
            assert_eq!(route("GET", u), Route::NotFound, "{u}");
        }
        assert_eq!(route("POST", "/api/beds"), Route::Method);
        assert_eq!(route("DELETE", "/api/beds"), Route::Method);
    }

    #[test]
    fn token_is_required_and_compared_exactly() {
        let h = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
        let want = "ABCD2345EFGH";
        assert!(token_ok(given_token(&h("Authorization", "Bearer ABCD2345EFGH"), "/api/beds").as_deref(), want));
        assert!(token_ok(given_token(&h("x-tempo-token", "ABCD2345EFGH"), "/api/beds").as_deref(), want));
        assert!(token_ok(given_token(&[], "/api/beds?no=1&token=ABCD2345EFGH").as_deref(), want));
        assert!(!token_ok(given_token(&[], "/api/beds").as_deref(), want));
        assert!(!token_ok(Some("ABCD2345EFGX"), want));
        assert!(!token_ok(Some("ABCD2345EFG"), want));
        assert!(!token_ok(Some(""), ""));
        assert!(!token_ok(Some("x"), "  "));
    }

    #[test]
    fn stop_releases_port_right_away() {
        // 0.0.0.0 에 연 서버를 끄면 같은 포트를 바로 다시 열 수 있어야 함 (끄기 · 포트 바꾸기)
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let s = tiny_http::Server::http(format!("0.0.0.0:{port}")).expect("bind");
        stop(Some(s), port);
        let mut again = None;
        for _ in 0..20 {
            if let Ok(s) = tiny_http::Server::http(format!("0.0.0.0:{port}")) {
                again = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(again.is_some(), "port {port} still busy after stop");
        stop(again, port);
    }

    #[test]
    fn needs_enabled_and_long_token() {
        let mut c = LanBeds::default();
        assert!(!c.enabled, "기본 끔");
        assert!(!usable(&c));
        c.enabled = true;
        c.token = "short".into();
        assert!(!usable(&c));
        c.token = new_token();
        assert_eq!(c.token.len(), 12);
        assert!(usable(&c));
        assert_ne!(new_token(), new_token());
    }

    #[test]
    fn bed_states() {
        use crate::engine::Phase;
        use crate::ui::beds::bed_state;
        assert_eq!(bed_state(Phase::Idle, false, ""), "empty");
        assert_eq!(bed_state(Phase::Idle, false, "  "), "empty");
        assert_eq!(bed_state(Phase::Idle, false, "이름"), "waiting");
        assert_eq!(bed_state(Phase::Running, false, "이름"), "running");
        assert_eq!(bed_state(Phase::Running, true, "이름"), "paused");
        assert_eq!(bed_state(Phase::Paused, false, ""), "paused");
        assert_eq!(bed_state(Phase::Done, false, "이름"), "done");
    }

    #[test]
    fn board_json_drops_note_and_filters_bed() {
        let beds = serde_json::json!([
            { "no": 1, "id": "11", "name": "1번 베드", "state": "empty", "patient": "", "note": "" },
            { "no": 3, "id": "33", "name": "3번 베드", "state": "running", "patient": "홍길동", "note": "메모", "step": 2, "step_count": 3, "step_name": "침" },
        ]);
        let all = board_json(&beds, None, 1_700_000_000_000, 540);
        assert_eq!(all["source"], "tempo");
        assert_eq!(all["now_ms"], 1_700_000_000_000i64);
        assert_eq!(all["tz_min"], 540);
        assert_eq!(all["beds"].as_array().unwrap().len(), 2);
        let one = board_json(&beds, Some(3), 0, 0);
        let b = &one["beds"][0];
        assert_eq!(one["beds"].as_array().unwrap().len(), 1);
        assert_eq!(b["patient"], "홍길동");
        assert_eq!(b["step_name"], "침");
        assert!(b.get("note").is_none() && b.get("id").is_none() && b.get("name").is_none());
        assert_eq!(board_json(&beds, Some(9), 0, 0)["beds"].as_array().unwrap().len(), 0);
    }
}
