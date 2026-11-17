//! PC 연동 — 같은 내부망(또는 Tailscale)의 Tempo 끼리 타이머 · 알람 · 리마인더 · 카운트다운 · 디데이 · 월드타임과
//! 타이머 실행 상태(시작 · 일시정지 · 단계)를 맞춘다.
//!
//! 구조: 한 PC = 호스트(TCP 47840 대기), 나머지 = 참여(호스트에 연결). 메시지 = 줄 단위 JSON. 연동 코드로 서로 HMAC 확인.
//! 맞추는 방식: 항목(타이머 1개, 알람 1개 …)마다 rev = 마지막으로 바뀐 실제 시각(ms) — 더 최근 것이 이김. 지운 건 묘비.
//! 시각은 메시지마다 보낸 쪽 「지금」을 함께 보내 받는 쪽 시계로 바꿈(두 PC 시계가 조금 달라도 남은 시간이 같게).
//! PC 마다 따로 두는 값: 카드 표시 · 위치 · 크기 · 항상 위, 사이드바 고정, 알림이 이미 울렸는지(각 PC 가 스스로 울림).
//! 처음 연결(참여 쪽 기준 그 호스트와 처음) = 합치기: 이름이 같은 타이머 등은 호스트 것 하나로, 나머지는 양쪽에 추가.
use crate::core::{Core, RingKind};
use crate::model::{Data, Id, SyncMode, SyncSettings};
use crate::store::{self, log};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// 이보다 작은 rev 는 「처음부터 있던 것」(시계 변환 안 함)
const REAL_MS: i64 = 1_000_000_000_000;

// ── 항목 ───────────────────────────────────────────────────

const LOCAL_TIMER: &[&str] = &["card", "card_pos", "card_scale", "card_on_top"];
const LOCAL_EVENT: &[&str] = &["card", "card_pos", "fired", "pinned"];

fn coll<T: serde::Serialize>(m: &mut BTreeMap<String, Value>, p: &str, list: &[T], id: impl Fn(&T) -> Id, strip: &[&str]) {
    let mut order = Vec::new();
    for x in list {
        let mut v = serde_json::to_value(x).unwrap_or(Value::Null);
        if let Value::Object(o) = &mut v {
            for k in strip {
                o.remove(*k);
            }
        }
        m.insert(format!("{p}:{}", id(x)), v);
        order.push(Value::String(id(x).to_string()));
    }
    m.insert(format!("o:{p}"), Value::Array(order));
}

/// 맞출 항목 전체 (PC 별 값은 뺌)
pub fn items(d: &Data) -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    coll(&mut m, "t", &d.timers, |x| x.id, LOCAL_TIMER);
    coll(&mut m, "a", &d.alarms, |x| x.id, &[]);
    coll(&mut m, "m", &d.reminders, |x| x.id, &[]);
    coll(&mut m, "c", &d.countdowns, |x| x.id, LOCAL_EVENT);
    coll(&mut m, "d", &d.ddays, |x| x.id, LOCAL_EVENT);
    coll(&mut m, "w", &d.world, |x| x.id, &[]);
    m
}

/// 항목 하나 넣기/바꾸기/지우기 — PC 별 값은 지금 것 유지
fn upsert<T: serde::Serialize + serde::de::DeserializeOwned>(list: &mut Vec<T>, id: Id, idf: impl Fn(&T) -> Id, v: Option<&Value>, keep: &[&str]) -> bool {
    let pos = list.iter().position(|x| idf(x) == id);
    let Some(v) = v else {
        if let Some(i) = pos {
            list.remove(i);
        }
        return true;
    };
    let mut nv = v.clone();
    if let (Some(i), Value::Object(o)) = (pos, &mut nv) {
        if let Ok(Value::Object(old)) = serde_json::to_value(&list[i]) {
            for k in keep {
                if let Some(x) = old.get(*k) {
                    o.insert((*k).to_string(), x.clone());
                }
            }
        }
    }
    match serde_json::from_value::<T>(nv) {
        Ok(t) => {
            match pos {
                Some(i) => list[i] = t,
                None => list.push(t),
            }
            true
        }
        Err(e) => {
            log(format!("sync: item {id} 읽기 실패: {e}"));
            false
        }
    }
}

fn reorder<T>(list: &mut Vec<T>, order: &Value, idf: impl Fn(&T) -> Id) {
    let Some(arr) = order.as_array() else { return };
    let rank: HashMap<Id, usize> = arr.iter().enumerate().filter_map(|(i, x)| x.as_str().and_then(|s| s.parse().ok()).map(|id| (id, i))).collect();
    // 순서 목록에 없는 건 뒤에 (원래 순서 유지)
    let mut keyed: Vec<(usize, usize, T)> = list.drain(..).enumerate().map(|(j, x)| (*rank.get(&idf(&x)).unwrap_or(&usize::MAX), j, x)).collect();
    keyed.sort_by_key(|k| (k.0, k.1));
    list.extend(keyed.into_iter().map(|k| k.2));
}

/// 받은 항목들을 데이터에 반영 (순서 항목은 맨 뒤에). 반환 = 못 읽은 항목 키
pub fn apply_items(d: &mut Data, list: &[(String, Option<Value>)]) -> Vec<String> {
    let mut failed = Vec::new();
    let mut orders = Vec::new();
    for (k, v) in list {
        let Some((p, rest)) = k.split_once(':') else { continue };
        if p == "o" {
            orders.push((rest.to_string(), v.clone()));
            continue;
        }
        let Ok(id) = rest.parse::<Id>() else { continue };
        let v = v.as_ref();
        let ok = match p {
            "t" => upsert(&mut d.timers, id, |x| x.id, v, LOCAL_TIMER),
            "a" => upsert(&mut d.alarms, id, |x| x.id, v, &[]),
            "m" => {
                // 다른 PC 에서 지운 리마인더 — To Do 쪽도 지우게 기록 (이 PC 가 To Do 와 연결돼 있으면)
                if v.is_none() {
                    if let Some(r) = d.reminders.iter().find(|r| r.id == id) {
                        if let (Some(l), Some(t)) = (r.todo_list.clone(), r.todo_id.clone()) {
                            d.todo_deleted.push((l, t));
                        }
                    }
                }
                upsert(&mut d.reminders, id, |x| x.id, v, &[])
            }
            "c" => upsert(&mut d.countdowns, id, |x| x.id, v, LOCAL_EVENT),
            "d" => upsert(&mut d.ddays, id, |x| x.id, v, LOCAL_EVENT),
            "w" => upsert(&mut d.world, id, |x| x.id, v, &[]),
            _ => true,
        };
        if !ok {
            failed.push(k.clone());
        }
    }
    for (p, v) in orders {
        let Some(v) = v else { continue };
        match p.as_str() {
            "t" => reorder(&mut d.timers, &v, |x| x.id),
            "a" => reorder(&mut d.alarms, &v, |x| x.id),
            "m" => reorder(&mut d.reminders, &v, |x| x.id),
            "c" => reorder(&mut d.countdowns, &v, |x| x.id),
            "d" => reorder(&mut d.ddays, &v, |x| x.id),
            "w" => reorder(&mut d.world, &v, |x| x.id),
            _ => {}
        }
    }
    failed
}

/// 처음 합칠 때 「같은 것」으로 볼 기준
fn natural_key(p: &str, v: &Value) -> Option<String> {
    let g = |k: &str| v.get(k).map(|x| x.to_string()).unwrap_or_default();
    Some(match p {
        "t" => g("name"),
        "a" => format!("{}|{}|{}|{}", g("hour"), g("minute"), g("label"), g("days")),
        "m" => format!("{}|{}", g("title"), g("due")),
        "c" => format!("{}|{}", g("title"), g("target")),
        "d" => format!("{}|{}|{}", g("title"), g("date"), g("mode")),
        "w" => g("tz"),
        _ => return None,
    })
}

// ── 저장되는 연동 기록 (sync.json) ─────────────────────────

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Store {
    pc_id: String,
    revs: BTreeMap<String, i64>,
    tombs: BTreeMap<String, i64>,
    /// 이미 합친 호스트들 (다음부터는 rev 로만 맞춤)
    paired: Vec<String>,
}

fn store_path() -> std::path::PathBuf {
    store::dir().join("sync.json")
}

struct RunMeta {
    rev: i64,
    remote: bool,
}

struct Peer {
    cid: u64,
    name: String,
    addr: String,
    tx: mpsc::Sender<String>,
    since: chrono::DateTime<chrono::Local>,
}

#[derive(Clone, Default, Debug)]
pub struct Status {
    /// 사람이 읽는 상태
    pub state: String,
    pub error: Option<String>,
    pub last_rx: Option<chrono::DateTime<chrono::Local>>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
static RUNS: Mutex<Option<HashMap<Id, RunMeta>>> = Mutex::new(None);
static PEERS: Mutex<Vec<Peer>> = Mutex::new(Vec::new());
static STATUS: Mutex<Option<Status>> = Mutex::new(None);
static NEXT_CID: AtomicU64 = AtomicU64::new(1);

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        let mut s: Store = std::fs::read_to_string(store_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if s.pc_id.is_empty() {
            s.pc_id = format!("{:016x}", crate::model::new_id());
        }
        *g = Some(s);
    }
    f(g.as_mut().unwrap())
}

fn save_store() {
    with_store(|s| {
        if let Ok(j) = serde_json::to_string(s) {
            let tmp = store_path().with_extension("json.tmp");
            if std::fs::write(&tmp, j).is_ok() {
                let _ = std::fs::rename(&tmp, store_path());
            }
        }
    });
}

fn with_runs<R>(f: impl FnOnce(&mut HashMap<Id, RunMeta>) -> R) -> R {
    let mut g = RUNS.lock().unwrap();
    f(g.get_or_insert_with(HashMap::new))
}

fn set_status(f: impl FnOnce(&mut Status)) {
    let mut g = STATUS.lock().unwrap();
    f(g.get_or_insert_with(Default::default));
}

pub fn status() -> Status {
    STATUS.lock().unwrap().clone().unwrap_or_default()
}

/// 연결된 PC 이름들
pub fn peers() -> Vec<(String, String, chrono::DateTime<chrono::Local>)> {
    PEERS.lock().unwrap().iter().map(|p| (p.name.clone(), p.addr.clone(), p.since)).collect()
}

pub fn pc_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

/// 이 PC 의 내부망 주소 (기본 경로 쪽)
pub fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    s.local_addr().ok().map(|a| a.ip().to_string())
}

pub fn new_code() -> String {
    // 헷갈리는 글자(0 O 1 I) 뺀 8자리
    const A: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let h = sha_hex(format!("{}{}{:?}", crate::model::new_id(), std::process::id(), Instant::now()).as_bytes());
    h.as_bytes().chunks(2).take(8).map(|c| A[(u8::from_str_radix(std::str::from_utf8(c).unwrap_or("0"), 16).unwrap_or(0) as usize) % A.len()] as char).collect()
}

// ── 인증 (HMAC-SHA256) ─────────────────────────────────────

fn sha_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

pub fn hmac(key: &[u8], msg: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::new().chain_update(ipad).chain_update(msg).finalize();
    Sha256::new().chain_update(opad).chain_update(inner).finalize().iter().map(|x| format!("{x:02x}")).collect()
}

fn norm_code(c: &str) -> String {
    c.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

// ── 이 PC 에서 바뀜 → 보내기 ───────────────────────────────

fn send_all(msg: &str, except: Option<u64>) {
    let peers = PEERS.lock().unwrap();
    for p in peers.iter().filter(|p| Some(p.cid) != except) {
        let _ = p.tx.send(msg.to_string());
    }
}

fn items_msg(list: &[(String, i64, Option<Value>)], full: bool) -> String {
    let mut m = Map::new();
    for (k, r, v) in list {
        m.insert(k.clone(), json!({ "r": r, "v": v }));
    }
    json!({ "t": "items", "now": now_ms(), "full": full, "items": m }).to_string()
}

/// Core::update 가 부름 — 바뀐 항목 rev 올리고 연결된 PC 로
pub fn local_diff(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) {
    let now = now_ms();
    let mut out: Vec<(String, i64, Option<Value>)> = Vec::new();
    for (k, v) in after {
        if before.get(k) != Some(v) {
            out.push((k.clone(), now, Some(v.clone())));
        }
    }
    for k in before.keys().filter(|k| !after.contains_key(*k)) {
        out.push((k.clone(), now, None));
    }
    if out.is_empty() {
        return;
    }
    with_store(|s| {
        for (k, r, v) in &out {
            if v.is_some() {
                s.revs.insert(k.clone(), *r);
                s.tombs.remove(k);
            } else {
                s.revs.remove(k);
                s.tombs.insert(k.clone(), *r);
            }
        }
        // 묘비는 30일 지나면 정리
        let old = now - 30 * 86_400_000;
        s.tombs.retain(|_, r| *r < REAL_MS || *r > old);
    });
    save_store();
    send_all(&items_msg(&out, false), None);
}

/// Core::cmd 가 부름 — 이 PC 에서 시작 · 일시정지 · 단계 이동 등
pub fn local_run(id: Id, snap: &crate::engine::Snap) {
    let now = now_ms();
    with_runs(|r| {
        r.insert(id, RunMeta { rev: now, remote: false });
    });
    let v = serde_json::to_value(snap).ok();
    send_all(&items_msg(&[(format!("r:{id}"), now, v)], false), None);
}

/// 이 타이머를 마지막으로 움직인 게 다른 PC 인가 (소리 끌지 판단)
pub fn remote_run(id: Id) -> bool {
    with_runs(|r| r.get(&id).map(|m| m.remote).unwrap_or(false))
}

fn ring_key(k: RingKind) -> String {
    match k {
        RingKind::Alarm(i) => format!("a:{i}"),
        RingKind::Timer(i) => format!("t:{i}"),
        RingKind::Reminder(i) => format!("m:{i}"),
        RingKind::Countdown(i) => format!("c:{i}"),
        RingKind::Dday(i) => format!("d:{i}"),
    }
}

fn ring_parse(s: &str) -> Option<RingKind> {
    let (p, id) = s.split_once(':')?;
    let id: Id = id.parse().ok()?;
    Some(match p {
        "a" => RingKind::Alarm(id),
        "t" => RingKind::Timer(id),
        "m" => RingKind::Reminder(id),
        "c" => RingKind::Countdown(id),
        "d" => RingKind::Dday(id),
        _ => return None,
    })
}

/// 이 PC 에서 알림 창을 닫음 → 다른 PC 도 닫게
pub fn local_ring_off(k: RingKind) {
    send_all(&json!({ "t": "off", "k": ring_key(k) }).to_string(), None);
}

// ── 받기 ───────────────────────────────────────────────────

fn full_list(core: &Core) -> Vec<(String, i64, Option<Value>)> {
    let it = items(&core.data());
    let mut out = Vec::new();
    with_store(|s| {
        for (k, v) in it {
            let r = *s.revs.entry(k.clone()).or_insert(1);
            out.push((k, r, Some(v)));
        }
        for (k, r) in &s.tombs {
            out.push((k.clone(), *r, None));
        }
    });
    let runs: Vec<(Id, i64)> = with_runs(|r| r.iter().map(|(k, m)| (*k, m.rev)).collect());
    for (id, rev) in runs {
        if let Some(sn) = core.run_snap(id) {
            out.push((format!("r:{id}"), rev, serde_json::to_value(sn).ok()));
        }
    }
    out
}

/// 받은 items — 더 최근 것만 반영. 반환 = 반영한 것 (호스트는 다른 PC 로 다시 보냄)
fn on_items(core: &Core, msg: &Value) -> Vec<(String, i64, Option<Value>)> {
    let offset = now_ms() - msg["now"].as_i64().unwrap_or_else(now_ms);
    let conv = |r: i64| if r >= REAL_MS { r + offset } else { r };
    let Some(map) = msg["items"].as_object() else { return vec![] };
    // 이미 같은 값이면 rev 만 맞추고 반영은 건너뜀 (다시 연결할 때 헛반영 줄이기)
    let current = items(&core.data());
    let mut data_items: Vec<(String, Option<Value>)> = Vec::new();
    let mut runs: Vec<(Id, crate::engine::Snap)> = Vec::new();
    let mut accepted = Vec::new();
    for (k, e) in map {
        let rev = conv(e["r"].as_i64().unwrap_or(0));
        let v = e.get("v").filter(|v| !v.is_null()).cloned();
        if let Some(id) = k.strip_prefix("r:").and_then(|x| x.parse::<Id>().ok()) {
            let local = with_runs(|r| r.get(&id).map(|m| m.rev).unwrap_or(0));
            if rev > local {
                if let Some(mut sn) = v.clone().and_then(|v| serde_json::from_value::<crate::engine::Snap>(v).ok()) {
                    sn.since_ms = sn.since_ms.map(|t| t + offset);
                    sn.done_ms = sn.done_ms.map(|t| t + offset);
                    with_runs(|r| {
                        r.insert(id, RunMeta { rev, remote: true });
                    });
                    runs.push((id, sn));
                    accepted.push((k.clone(), rev, v));
                }
            }
            continue;
        }
        let local = with_store(|s| s.revs.get(k).copied().unwrap_or(0).max(s.tombs.get(k).copied().unwrap_or(0)));
        if rev > local {
            let same = match &v {
                Some(x) => current.get(k) == Some(x),
                None => !current.contains_key(k),
            };
            with_store(|s| {
                if v.is_some() {
                    s.revs.insert(k.clone(), rev);
                    s.tombs.remove(k);
                } else {
                    s.revs.remove(k);
                    s.tombs.insert(k.clone(), rev);
                }
            });
            if !same {
                data_items.push((k.clone(), v.clone()));
            }
            accepted.push((k.clone(), rev, v));
        }
    }
    if !data_items.is_empty() || !runs.is_empty() {
        save_store();
        let failed = core.apply_remote(&data_items, &runs, now_ms());
        if !failed.is_empty() {
            // 못 읽은 건 받은 것으로 치지 않음 (다음 연결 때 다시)
            with_store(|s| {
                for k in &failed {
                    s.revs.remove(k);
                }
            });
            save_store();
            accepted.retain(|a| !failed.contains(&a.0));
        }
        set_status(|s| s.last_rx = Some(chrono::Local::now()));
    }
    accepted
}

/// 처음 합치기 (참여 쪽): 호스트에 같은 게 있으면 이 PC 것은 빼고 호스트 것으로
fn merge_first(core: &Core, host_items: &Map<String, Value>) {
    let mut host_keys: HashMap<(String, String), Id> = HashMap::new();
    let mut host_hotkeys: Vec<String> = Vec::new();
    for (k, e) in host_items {
        let Some((p, id)) = k.split_once(':') else { continue };
        let (Some(v), Ok(id)) = (e.get("v").filter(|v| !v.is_null()), id.parse::<Id>()) else { continue };
        if let Some(nk) = natural_key(p, v) {
            host_keys.insert((p.to_string(), nk), id);
        }
        if p == "t" {
            for hk in ["hotkey_toggle", "hotkey_reset"] {
                if let Some(s) = v[hk].as_str().filter(|s| !s.is_empty()) {
                    host_hotkeys.push(s.to_string());
                }
            }
        }
    }
    let mine = items(&core.data());
    let mut remap: Vec<(String, Id, Id)> = Vec::new();
    for (k, v) in &mine {
        let Some((p, id)) = k.split_once(':') else { continue };
        if p == "o" {
            continue;
        }
        let Ok(id) = id.parse::<Id>() else { continue };
        if let Some(nk) = natural_key(p, v) {
            if let Some(hid) = host_keys.get(&(p.to_string(), nk)) {
                if *hid != id {
                    remap.push((p.to_string(), id, *hid));
                }
            }
        }
    }
    log(format!("sync: 처음 합치기 — 같은 항목 {}개는 호스트 것으로, 나머지는 양쪽에", remap.len()));
    core.merge_local(&remap, &host_hotkeys);
    with_store(|s| {
        for (p, old, new) in &remap {
            s.revs.remove(&format!("{p}:{old}"));
            // 호스트 값이 들어오게
            s.revs.remove(&format!("{p}:{new}"));
        }
        // 순서도 호스트 것 먼저 받음 (끝나면 이 PC 것을 덧붙인 순서를 다시 보냄)
        s.revs.retain(|k, _| !k.starts_with("o:"));
    });
}

// ── 연결 ───────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
struct Cfg {
    s: SyncSettings,
    api_port: u16,
}

pub fn spawn(core: Arc<Core>) {
    // 처음 실행: 지금 항목들 rev 기본값
    let it = items(&core.data());
    with_store(|s| {
        for k in it.keys() {
            s.revs.entry(k.clone()).or_insert(1);
        }
    });
    save_store();
    std::thread::Builder::new().name("sync".into()).spawn(move || supervisor(core)).expect("sync thread");
}

fn supervisor(core: Arc<Core>) {
    let mut running: Option<(Cfg, Arc<AtomicBool>)> = None;
    loop {
        let cfg = {
            // 잠금은 한 번만 (같은 문장에서 두 번 잡으면 스스로 멈춤)
            let d = core.data();
            Cfg { s: d.settings.sync.clone(), api_port: d.settings.api_port }
        };
        let changed = running.as_ref().map(|r| r.0 != cfg).unwrap_or(true);
        if changed {
            if let Some((_, stop)) = running.take() {
                stop.store(true, Ordering::Relaxed);
                // 연결 끊기 (읽기 대기 중인 스레드가 빨리 나오게)
                PEERS.lock().unwrap().clear();
                std::thread::sleep(Duration::from_millis(1500));
            }
            let stop = Arc::new(AtomicBool::new(false));
            match cfg.s.mode {
                SyncMode::Off => set_status(|s| {
                    s.state = "꺼짐".into();
                    s.error = None;
                }),
                SyncMode::Host => {
                    let (c, st, p) = (core.clone(), stop.clone(), cfg.s.clone());
                    std::thread::Builder::new().name("sync-host".into()).spawn(move || host_loop(c, p, st)).ok();
                }
                SyncMode::Join => {
                    let (c, st, p) = (core.clone(), stop.clone(), cfg.s.clone());
                    std::thread::Builder::new().name("sync-join".into()).spawn(move || join_loop(c, p, st)).ok();
                }
            }
            running = Some((cfg, stop));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn host_loop(core: Arc<Core>, s: SyncSettings, stop: Arc<AtomicBool>) {
    if norm_code(&s.code).len() < 6 {
        set_status(|x| {
            x.state = "연동 코드가 필요합니다".into();
            x.error = None;
        });
        return;
    }
    let l = loop {
        // 시험용: TEMPO_SYNC_BIND=127.0.0.1 (방화벽 확인 창 없이)
        let bind = std::env::var("TEMPO_SYNC_BIND").unwrap_or_else(|_| "0.0.0.0".into());
        match TcpListener::bind((bind.as_str(), s.port)) {
            Ok(l) => break l,
            Err(e) => {
                set_status(|x| x.error = Some(format!("포트 {} 열기 실패: {e}", s.port)));
                for _ in 0..50 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
    };
    let _ = l.set_nonblocking(true);
    log(format!("sync: host listening on 0.0.0.0:{}", s.port));
    set_status(|x| {
        x.state = "호스트 — 다른 PC 를 기다리는 중".into();
        x.error = None;
    });
    while !stop.load(Ordering::Relaxed) {
        match l.accept() {
            Ok((st, addr)) => {
                let (c, s2, stp) = (core.clone(), s.clone(), stop.clone());
                std::thread::spawn(move || {
                    let _ = st.set_nonblocking(false);
                    if let Err(e) = conn(&c, st, addr.to_string(), &s2, true, &stp) {
                        log(format!("sync: {addr} 연결 끝: {e}"));
                    }
                    host_state();
                });
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    log("sync: host stopped");
}

fn host_state() {
    let n = PEERS.lock().unwrap().len();
    set_status(|x| x.state = if n == 0 { "호스트 — 다른 PC 를 기다리는 중".into() } else { format!("호스트 — {n}대 연결됨") });
}

fn join_loop(core: Arc<Core>, s: SyncSettings, stop: Arc<AtomicBool>) {
    let host = s.host.trim().to_string();
    if host.is_empty() || norm_code(&s.code).len() < 6 {
        set_status(|x| {
            x.state = "호스트 주소와 연동 코드를 넣어 주세요".into();
            x.error = None;
        });
        return;
    }
    let target = if host.contains(':') { host.clone() } else { format!("{host}:{}", s.port) };
    while !stop.load(Ordering::Relaxed) {
        set_status(|x| x.state = format!("{target} 에 연결 중…"));
        let addr = target.to_socket_addrs().ok().and_then(|mut a| a.next());
        let r = match addr {
            None => Err(format!("{target} 주소를 찾을 수 없음")),
            Some(a) => match TcpStream::connect_timeout(&a, Duration::from_secs(4)) {
                Ok(st) => conn(&core, st, target.clone(), &s, false, &stop),
                Err(e) => Err(format!("연결 안 됨: {e}")),
            },
        };
        if let Err(e) = r {
            set_status(|x| {
                x.error = Some(e.clone());
                x.state = "연결 끊김 — 5초 뒤 다시 시도".into();
            });
        }
        for _ in 0..25 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

fn send_line(w: &mut TcpStream, s: &str) -> std::io::Result<()> {
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

/// 줄 하나 읽기 (타임아웃이면 None — 버퍼는 이어서)
fn read_line(r: &mut BufReader<TcpStream>, buf: &mut String) -> std::io::Result<Option<String>> {
    match r.read_line(buf) {
        Ok(0) => Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "상대가 연결을 끊음")),
        Ok(_) => {
            if buf.ends_with('\n') {
                let l = buf.trim_end().to_string();
                buf.clear();
                Ok(Some(l))
            } else {
                Ok(None)
            }
        }
        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Ok(None),
        Err(e) => Err(e),
    }
}

fn read_json(r: &mut BufReader<TcpStream>, buf: &mut String, secs: u64) -> Result<Value, String> {
    let t0 = Instant::now();
    loop {
        if let Some(l) = read_line(r, buf).map_err(|e| e.to_string())? {
            return serde_json::from_str(&l).map_err(|e| format!("메시지 이상: {e}"));
        }
        if t0.elapsed() > Duration::from_secs(secs) {
            return Err("응답 없음".into());
        }
    }
}

fn conn(core: &Core, st: TcpStream, addr: String, s: &SyncSettings, host: bool, stop: &AtomicBool) -> Result<(), String> {
    let _ = st.set_nodelay(true);
    let _ = st.set_read_timeout(Some(Duration::from_secs(1)));
    let mut w = st.try_clone().map_err(|e| e.to_string())?;
    let mut r = BufReader::new(st);
    let mut buf = String::new();
    let code = norm_code(&s.code);
    let my_pc = with_store(|x| x.pc_id.clone());
    let nonce = sha_hex(format!("{}{:?}", crate::model::new_id(), Instant::now()).as_bytes());
    // 인사 · 확인
    let (peer_pc, peer_name) = if host {
        send_line(&mut w, &json!({ "t": "hi", "pc": my_pc, "name": pc_name(), "nonce": nonce, "ver": crate::update::CURRENT }).to_string()).map_err(|e| e.to_string())?;
        let a = read_json(&mut r, &mut buf, 10)?;
        if a["t"] != "auth" || a["mac"].as_str() != Some(hmac(code.as_bytes(), nonce.as_bytes()).as_str()) {
            let _ = send_line(&mut w, &json!({ "t": "no", "why": "연동 코드가 다릅니다" }).to_string());
            return Err("연동 코드가 다른 PC 가 연결을 시도함".into());
        }
        let n2 = a["nonce"].as_str().unwrap_or("");
        send_line(&mut w, &json!({ "t": "ok", "mac": hmac(code.as_bytes(), n2.as_bytes()) }).to_string()).map_err(|e| e.to_string())?;
        (a["pc"].as_str().unwrap_or("").to_string(), a["name"].as_str().unwrap_or("?").to_string())
    } else {
        let h = read_json(&mut r, &mut buf, 10)?;
        if h["t"] != "hi" {
            return Err("Tempo 호스트가 아님".into());
        }
        let hn = h["nonce"].as_str().unwrap_or("");
        send_line(&mut w, &json!({ "t": "auth", "pc": my_pc, "name": pc_name(), "mac": hmac(code.as_bytes(), hn.as_bytes()), "nonce": nonce }).to_string()).map_err(|e| e.to_string())?;
        let ok = read_json(&mut r, &mut buf, 10)?;
        if ok["t"] != "ok" {
            return Err(ok["why"].as_str().unwrap_or("호스트가 거절함").to_string());
        }
        if ok["mac"].as_str() != Some(hmac(code.as_bytes(), nonce.as_bytes()).as_str()) {
            return Err("호스트의 연동 코드가 다름".into());
        }
        (h["pc"].as_str().unwrap_or("").to_string(), h["name"].as_str().unwrap_or("?").to_string())
    };
    if peer_pc == my_pc {
        return Err("자기 자신에게 연결함".into());
    }
    // 보내는 스레드
    let (tx, rx) = mpsc::channel::<String>();
    let cid = NEXT_CID.fetch_add(1, Ordering::Relaxed);
    let alive = Arc::new(AtomicBool::new(true));
    {
        let alive = alive.clone();
        let mut w2 = w.try_clone().map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            while let Ok(m) = rx.recv() {
                if send_line(&mut w2, &m).is_err() {
                    break;
                }
            }
            alive.store(false, Ordering::Relaxed);
            let _ = w2.shutdown(std::net::Shutdown::Both);
        });
    }
    log(format!("sync: connected {} {peer_name} ({addr})", if host { "from" } else { "to" }));
    // 호스트는 바로 전체를 보냄. 참여는 호스트 전체를 먼저 받고(처음이면 합치기) 자기 전체를 보냄
    if host {
        let _ = tx.send(items_msg(&full_list(core), true));
    }
    PEERS.lock().unwrap().push(Peer { cid, name: peer_name.clone(), addr: addr.clone(), tx: tx.clone(), since: chrono::Local::now() });
    if host {
        host_state();
    } else {
        set_status(|x| {
            x.state = format!("연결됨 — {peer_name}");
            x.error = None;
        });
    }
    let mut got_full = host;
    let mut last_rx = Instant::now();
    let mut last_ping = Instant::now();
    let result = loop {
        if stop.load(Ordering::Relaxed) {
            break Ok(());
        }
        if !alive.load(Ordering::Relaxed) {
            break Err("보내기 실패".to_string());
        }
        if !PEERS.lock().unwrap().iter().any(|p| p.cid == cid) {
            break Ok(()); // 설정이 바뀌어 끊음
        }
        if last_ping.elapsed() > Duration::from_secs(15) {
            last_ping = Instant::now();
            let _ = tx.send(json!({ "t": "ping" }).to_string());
        }
        if last_rx.elapsed() > Duration::from_secs(45) {
            break Err("45초 동안 응답 없음".to_string());
        }
        let line = match read_line(&mut r, &mut buf) {
            Ok(Some(l)) => l,
            Ok(None) => continue,
            Err(e) => break Err(e.to_string()),
        };
        last_rx = Instant::now();
        let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
        match m["t"].as_str() {
            Some("items") => {
                if !got_full && m["full"].as_bool() == Some(true) {
                    got_full = true;
                    let first = !with_store(|x| x.paired.contains(&peer_pc));
                    if first {
                        if let Some(map) = m["items"].as_object() {
                            merge_first(core, map);
                        }
                    }
                    on_items(core, &m);
                    if first {
                        // 합친 순서(호스트 순서 + 이 PC 에만 있던 것)를 최신으로 → 호스트도 같은 순서
                        let now = now_ms();
                        with_store(|x| {
                            x.paired.push(peer_pc.clone());
                            for p in ["t", "a", "m", "c", "d", "w"] {
                                x.revs.insert(format!("o:{p}"), now);
                            }
                        });
                        save_store();
                    }
                    let _ = tx.send(items_msg(&full_list(core), true));
                } else {
                    let acc = on_items(core, &m);
                    // 호스트 = 다른 참여 PC 들에게도 전달
                    if host && !acc.is_empty() {
                        send_all(&items_msg(&acc, false), Some(cid));
                    }
                }
            }
            Some("off") => {
                if let Some(k) = m["k"].as_str().and_then(ring_parse) {
                    core.dismiss_remote(k);
                    if host {
                        send_all(&line, Some(cid));
                    }
                }
            }
            _ => {}
        }
    };
    PEERS.lock().unwrap().retain(|p| p.cid != cid);
    drop(tx);
    let _ = w.shutdown(std::net::Shutdown::Both);
    log(format!("sync: disconnected {peer_name} ({addr})"));
    result
}

/// 설정 화면 · API
pub fn status_json(core: &Core) -> Value {
    let st = status();
    let s = core.data().settings.sync.clone();
    json!({
        "mode": format!("{:?}", s.mode).to_lowercase(),
        "state": st.state,
        "error": st.error,
        "host": s.host,
        "port": s.port,
        "pc": pc_name(),
        "lan_ip": lan_ip(),
        "remote_sound": s.remote_sound,
        "peers": peers().into_iter().map(|(n, a, t)| json!({ "name": n, "addr": a, "since": t.to_rfc3339() })).collect::<Vec<_>>(),
        "last_rx": st.last_rx.map(|t| t.to_rfc3339()),
        "items": with_store(|x| x.revs.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Data, TimerDef};

    #[test]
    fn hmac_rfc4231() {
        assert_eq!(hmac(b"Jefe", b"what do ya want for nothing?"), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
        assert_eq!(norm_code(" ab-cd 23ef "), "ABCD23EF");
        assert_eq!(new_code().len(), 8);
    }

    #[test]
    fn items_strip_local_and_apply_keeps_local() {
        let mut a = Data::seed();
        a.timers[0].card_pos = Some([1.0, 2.0]);
        let it = items(&a);
        let k = format!("t:{}", a.timers[0].id);
        assert!(it[&k].get("card_pos").is_none());
        assert!(it.contains_key("o:t"));
        // 다른 PC 에서 이름을 바꿈 → 카드 위치는 이 PC 것 유지
        let mut v = it[&k].clone();
        v["name"] = json!("바뀐 이름");
        apply_items(&mut a, &[(k.clone(), Some(v))]);
        assert_eq!((a.timers[0].name.as_str(), a.timers[0].card_pos), ("바뀐 이름", Some([1.0, 2.0])));
        // 새 타이머 + 순서
        let t = TimerDef::simple("새 것", 60, "#5B8DEF");
        let nk = format!("t:{}", t.id);
        let mut order: Vec<Value> = vec![json!(t.id.to_string())];
        order.extend(a.timers.iter().map(|x| json!(x.id.to_string())));
        apply_items(&mut a, &[("o:t".into(), Some(Value::Array(order))), (nk.clone(), Some(serde_json::to_value(&t).unwrap()))]);
        assert_eq!(a.timers[0].name, "새 것");
        // 지우기
        apply_items(&mut a, &[(nk, None)]);
        assert!(a.timers.iter().all(|x| x.name != "새 것"));
    }

    #[test]
    fn natural_keys() {
        let v = json!({ "name": "추나" });
        assert_eq!(natural_key("t", &v), Some("\"추나\"".into()));
        assert!(natural_key("o", &v).is_none());
    }
}
