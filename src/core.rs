//! 앱 상태 한곳 — 데이터 · 타이머 실행 상태 · 알람 스케줄 · 울리는 중 목록. UI/단축키/API/트레이가 모두 이걸 공유.
use crate::alarm;
use crate::audio::{self, PlayOpts};
use crate::engine::{unix_now, Cmd, Engine, Event, Opts, Phase};
use crate::model::{AlarmDef, Data, Id, Reminder, Repeat, TimerDef};
use crate::store::{self, log};
use chrono::{DateTime, Local, TimeZone};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct TimerView {
    pub id: Id,
    pub name: String,
    pub color: String,
    pub phase: Phase,
    pub idx: usize,
    pub n: usize,
    pub step_name: String,
    pub next_name: Option<String>,
    pub step_rem: Duration,
    pub total_rem: Duration,
    pub total: Duration,
    pub frac: f32,
    pub step_secs: Vec<u32>,
    pub chain_start_unix: i64,
    /// 반복 회차 (0 = 첫 회)
    pub loops: u32,
    /// 단계마다 멈춤 — 다음 단계 시작 대기 중
    pub waiting: bool,
    pub epoch: u64,
    /// 끝난 지 얼마 (끝 상태일 때)
    pub done_for: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RingKind {
    Alarm(Id),
    Timer(Id),
    Reminder(Id),
    Countdown(Id),
    Dday(Id),
}

pub struct Ring {
    pub kind: RingKind,
    pub title: String,
    pub subtitle: String,
    pub at: DateTime<Local>,
    since: Instant,
    stop: Arc<AtomicBool>,
}

/// 하루 기록 (타이머별): 완료 횟수 · 진행한 시간 · 포모도로 집중 횟수/시간
#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct DayStat {
    pub completes: u32,
    pub secs: u32,
    pub focus_count: u32,
    pub focus_secs: u32,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Stats {
    /// "YYYY-MM-DD" → 타이머 id(문자열) → 기록
    days: std::collections::BTreeMap<String, HashMap<String, DayStat>>,
}

fn stats_path() -> std::path::PathBuf {
    store::dir().join("stats.json")
}

#[derive(Default)]
struct AlarmRt {
    next: Option<DateTime<Local>>,
    snooze: Option<DateTime<Local>>,
    /// 이 PC 에서 마지막으로 울린 시각 (연동: 다른 PC 가 먼저 울리고 끈 일회성 알람 판단)
    rang: Option<DateTime<Local>>,
}

pub struct Core {
    data: Mutex<Data>,
    rt: Mutex<HashMap<Id, Engine>>,
    alarm_rt: Mutex<HashMap<Id, AlarmRt>>,
    rings: Mutex<Vec<Ring>>,
    ctx: OnceLock<eframe::egui::Context>,
    preview: Mutex<Option<Arc<AtomicBool>>>,
    pub hotkey_tid: AtomicU32,
    /// 등록 실패한 단축키 (다른 프로그램이 사용 중)
    pub hotkey_busy: Mutex<HashSet<String>>,
    pub open_main: AtomicBool,
    pub quit: AtomicBool,
    /// 카드 메뉴 「편집」 → 메인 창에서 그 타이머 편집기 열기
    pub edit_request: Mutex<Option<Id>>,
    /// 카드 두 번 누름 → 메인 창에서 그 타이머 집중 화면
    pub focus_request: Mutex<Option<Id>>,
    /// 카운트다운 · 디데이 카드에서 → 메인 창에서 편집 (true = 디데이)
    pub clock_request: Mutex<Option<(bool, Id)>>,
    /// 자동 업데이트 상태 (설정 화면 표시용)
    pub update_state: Mutex<crate::update::UpdateState>,
    /// 화면 쪽이 바쁨 (편집 창 열림 · 스톱워치) — 업데이트 미룸
    pub ui_hold: AtomicBool,
    /// 메인 창이 열려 있음 — 업데이트 후 다시 열기
    pub main_visible: AtomicBool,
    /// 끝나기 전 화면 효과 미리 보기 (타이머, 시작, 효과들)
    pub fx_preview: Mutex<Option<(Id, Instant, Vec<crate::model::WarnFx>)>>,
    /// 디데이 「다시 알림」 — id → (울릴 시각, 알림 문구)
    dday_snooze: Mutex<HashMap<Id, (chrono::NaiveDateTime, String)>>,
    stats: Mutex<Stats>,
    /// 닫은 카드 (그 타이머를 다시 시작하면 다시 보임)
    hidden_cards: Mutex<std::collections::HashSet<Id>>,
    /// 직접 연 카드 (카드 설정이 「끄기」거나 대기 중이어도 보임, 닫을 때까지)
    opened_cards: Mutex<std::collections::HashSet<Id>>,
    /// 리마인더별로 이미 울린 due (같은 시각에 두 번 안 울리게)
    rem_fired: Mutex<HashMap<Id, String>>,
    /// To Do 연동 상태 (설정 화면 표시용)
    pub todo: Mutex<crate::todo::TodoState>,
    /// 성능 측정 (/api/debug?bench=1): 끝 시각 · 프레임 간격(ms) · update 시간(ms)
    pub bench_until: Mutex<Option<Instant>>,
    pub frame_ms: Mutex<Vec<f32>>,
    pub update_ms: Mutex<Vec<f32>>,
    pub work_area: (f32, f32, f32, f32),
    /// 모든 모니터를 합친 범위 (논리 좌표, 왼·위·오·아래)
    pub virtual_screen: (f32, f32, f32, f32),
}

impl Core {
    pub fn new(data: Data, work_area: (f32, f32, f32, f32), virtual_screen: (f32, f32, f32, f32)) -> Self {
        let c = Core {
            data: Mutex::new(data),
            rt: Mutex::new(HashMap::new()),
            alarm_rt: Mutex::new(HashMap::new()),
            rings: Mutex::new(Vec::new()),
            ctx: OnceLock::new(),
            preview: Mutex::new(None),
            hotkey_tid: AtomicU32::new(0),
            hotkey_busy: Mutex::new(HashSet::new()),
            open_main: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            edit_request: Mutex::new(None),
            focus_request: Mutex::new(None),
            clock_request: Mutex::new(None),
            dday_snooze: Mutex::new(HashMap::new()),
            fx_preview: Mutex::new(None),
            update_state: Mutex::new(Default::default()),
            ui_hold: AtomicBool::new(false),
            main_visible: AtomicBool::new(false),
            rem_fired: Mutex::new(HashMap::new()),
            hidden_cards: Mutex::new(HashSet::new()),
            opened_cards: Mutex::new(HashSet::new()),
            todo: Mutex::new(Default::default()),
            stats: Mutex::new(std::fs::read_to_string(stats_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()),
            bench_until: Mutex::new(None),
            frame_ms: Mutex::new(Vec::new()),
            update_ms: Mutex::new(Vec::new()),
            work_area,
            virtual_screen,
        };
        c.sync_runtime();
        c
    }

    /// 저장된 창 위치가 지금 화면들 안에 (제목 줄이 보일 만큼) 있나 — 모니터를 뺐거나 해상도가 바뀐 경우 대비
    pub fn on_screen(&self, x: f32, y: f32) -> bool {
        let (l, t, r, b) = self.virtual_screen;
        x >= l - 40.0 && y >= t - 10.0 && x + 80.0 <= r && y + 40.0 <= b
    }

    pub fn set_ctx(&self, ctx: eframe::egui::Context) {
        let _ = self.ctx.set(ctx);
    }

    pub fn repaint(&self) {
        if let Some(c) = self.ctx.get() {
            c.request_repaint();
        }
    }

    // ── 데이터 ─────────────────────────────────────────────

    pub fn data(&self) -> MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 데이터 바꾸기 → 저장 · 실행 상태 맞추기 · 단축키 다시 등록
    pub fn update(&self, f: impl FnOnce(&mut Data)) {
        let hk_before = self.hotkey_sig();
        let (before, after) = {
            let mut d = self.data();
            let before = crate::sync::items(&d);
            f(&mut d);
            store::save(&d);
            (before, crate::sync::items(&d))
        };
        crate::sync::local_diff(&before, &after);
        self.sync_runtime();
        if self.hotkey_sig() != hk_before {
            self.reload_hotkeys();
        }
        self.repaint();
    }

    /// 창 위치처럼 자주 바뀌는 값 — 저장만
    pub fn update_quiet(&self, f: impl FnOnce(&mut Data)) {
        let (before, after) = {
            let mut d = self.data();
            let before = crate::sync::items(&d);
            f(&mut d);
            store::save(&d);
            (before, crate::sync::items(&d))
        };
        crate::sync::local_diff(&before, &after);
    }

    // ── PC 연동 ───────────────────────────────────────────

    /// 연동용 실행 상태
    pub fn run_snap(&self, id: Id) -> Option<crate::engine::Snap> {
        self.rt.lock().unwrap().get(&id).map(|e| e.snapshot(Instant::now(), crate::sync::now_ms()))
    }

    /// 업데이트 직전: 모든 타이머 진행 상태 저장 (새 버전이 이어받음)
    pub fn save_runs_for_update(&self) -> bool {
        let now_ms = crate::sync::now_ms();
        let now = Instant::now();
        let runs: std::collections::BTreeMap<String, serde_json::Value> = self
            .rt
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| e.phase != Phase::Idle)
            .map(|(id, e)| (id.to_string(), serde_json::json!({ "snap": e.snapshot(now, now_ms), "remote": crate::sync::remote_run(*id), "rev": crate::sync::run_rev(*id) })))
            .collect();
        let j = serde_json::json!({ "saved_ms": now_ms, "runs": runs });
        let ok = std::fs::write(store::dir().join("update-runs.json"), j.to_string()).is_ok();
        log(format!("update: saved {} running timer(s) for handover", runs.len()));
        ok
    }

    /// 시작할 때: 업데이트(또는 되돌리기) 직전 진행 상태가 있으면 이어받음 (2분 안에 저장된 것만)
    pub fn restore_runs_after_update(&self) {
        let path = store::dir().join("update-runs.json");
        let Ok(t) = std::fs::read_to_string(&path) else { return };
        let _ = std::fs::remove_file(&path);
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { return };
        let now_ms = crate::sync::now_ms();
        if now_ms - v["saved_ms"].as_i64().unwrap_or(0) > 120_000 {
            log("update: handover file too old — ignored");
            return;
        }
        let now = Instant::now();
        let mut n = 0;
        if let Some(map) = v["runs"].as_object() {
            let mut rt = self.rt.lock().unwrap();
            for (k, x) in map {
                let (Ok(id), Ok(sn)) = (k.parse::<Id>(), serde_json::from_value::<crate::engine::Snap>(x["snap"].clone())) else { continue };
                if let Some(e) = rt.get_mut(&id) {
                    e.restore(&sn, now, now_ms);
                    crate::sync::set_run_meta(id, x["rev"].as_i64().unwrap_or(0), x["remote"].as_bool().unwrap_or(false));
                    n += 1;
                }
            }
        }
        log(format!("update: restored {n} running timer(s)"));
        self.repaint();
    }

    /// 다른 PC 에서 온 변경 — 다시 보내지 않음
    pub fn apply_remote(&self, items: &[(String, Option<serde_json::Value>)], runs: &[(Id, crate::engine::Snap)], now_ms: i64) -> Vec<String> {
        let hk_before = self.hotkey_sig();
        let alarms_before: Vec<AlarmDef> = self.data().alarms.clone();
        let mut failed = Vec::new();
        if !items.is_empty() {
            let mut d = self.data();
            failed = crate::sync::apply_items(&mut d, items);
            store::save(&d);
        }
        self.sync_runtime();
        if self.hotkey_sig() != hk_before {
            self.reload_hotkeys();
        }
        if !runs.is_empty() {
            let now = Instant::now();
            let mut stopped = Vec::new();
            {
                let mut rt = self.rt.lock().unwrap();
                for (id, sn) in runs {
                    if let Some(e) = rt.get_mut(id) {
                        let was = e.phase;
                        e.restore(sn, now, now_ms);
                        if e.phase.active() && !was.active() {
                            self.hidden_cards.lock().unwrap().remove(id);
                        }
                        if !e.phase.active() {
                            stopped.push(*id);
                        }
                    }
                }
            }
            // 다른 PC 에서 멈춤 · 처음으로 → 이 PC 의 완료 알림도 닫음
            for id in stopped {
                self.dismiss_remote(RingKind::Timer(id));
            }
            for (id, sn) in runs {
                log(format!("{} synced from other PC: {}", self.timer_name(*id), sn.phase));
            }
        }
        if !items.is_empty() {
            log(format!("sync: applied {} item(s) from other PC", items.len() - failed.len()));
            self.catch_remote_alarms(&alarms_before);
        }
        self.repaint();
        failed
    }

    /// 처음 합치기: 다른 PC(호스트)와 같은 항목은 이 PC 것을 빼고(호스트 것이 들어옴), 연동 타이머 · 단축키 맞춤
    pub fn merge_local(&self, remap: &[(String, Id, Id)], host_hotkeys: &[String]) {
        {
            let mut d = self.data();
            let path = store::dir().join(format!("data.before-sync-{}.json", Local::now().format("%Y%m%d-%H%M%S")));
            if let Ok(j) = serde_json::to_string_pretty(&*d) {
                let _ = std::fs::write(&path, j);
            }
            // 같은 항목 = 이 PC 것의 id 를 호스트 id 로 (카드 위치 같은 PC 별 값은 유지, 내용은 곧 호스트 것으로)
            let map = |p: &str, id: Id| remap.iter().find(|r| r.0 == p && r.1 == id).map(|r| r.2);
            for x in d.timers.iter_mut() {
                if let Some(n) = map("t", x.id) {
                    x.id = n;
                }
            }
            for x in d.alarms.iter_mut() {
                if let Some(n) = map("a", x.id) {
                    x.id = n;
                }
            }
            for x in d.reminders.iter_mut() {
                if let Some(n) = map("m", x.id) {
                    x.id = n;
                }
            }
            for x in d.countdowns.iter_mut() {
                if let Some(n) = map("c", x.id) {
                    x.id = n;
                }
            }
            for x in d.ddays.iter_mut() {
                if let Some(n) = map("d", x.id) {
                    x.id = n;
                }
            }
            for x in d.world.iter_mut() {
                if let Some(n) = map("w", x.id) {
                    x.id = n;
                }
            }
            for x in d.bed_presets.iter_mut() {
                if let Some(n) = map("p", x.id) {
                    x.id = n;
                }
            }
            if let Some(n) = d.settings.automation_timer.and_then(|a| map("t", a)) {
                d.settings.automation_timer = Some(n);
            }
            // 남은 이 PC 타이머의 단축키가 호스트 타이머와 겹치면 비움
            for t in d.timers.iter_mut().filter(|t| !remap.iter().any(|r| r.0 == "t" && r.2 == t.id)) {
                if host_hotkeys.contains(&t.hotkey_toggle) {
                    t.hotkey_toggle.clear();
                }
                if host_hotkeys.contains(&t.hotkey_reset) {
                    t.hotkey_reset.clear();
                }
            }
            store::save(&d);
            log(format!("sync merge: backup {}", path.display()));
        }
        self.sync_runtime();
    }

    /// 다른 PC 에서 닫은 알림
    pub fn dismiss_remote(&self, kind: RingKind) {
        self.dismiss_ex(kind, false);
    }

    fn hotkey_sig(&self) -> Vec<(Id, String, String)> {
        self.data().timers.iter().map(|t| (t.id, t.hotkey_toggle.clone(), t.hotkey_reset.clone())).collect()
    }

    pub fn reload_hotkeys(&self) {
        let tid = self.hotkey_tid.load(Ordering::Relaxed);
        if tid != 0 {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(tid, crate::hotkey::WM_RELOAD, 0, 0);
            }
        }
    }

    fn sync_runtime(&self) {
        let timers: Vec<(Id, Vec<Duration>, Opts)> = self
            .data()
            .timers
            .iter()
            .map(|t| {
                let o = Opts {
                    repeat: t.repeat,
                    warn: Duration::from_secs(t.warn_secs as u64),
                    warn_each: t.warn_each,
                    auto_advance: t.auto_advance,
                    hold: t.bed.is_some(),
                };
                (t.id, t.steps.iter().map(|s| Duration::from_secs(s.secs.max(1) as u64)).collect(), o)
            })
            .collect();
        // 잠금 순서 = data → rt (view() 와 같게). data 는 여기서 먼저 다 읽어 둔다
        let alarms: Vec<Id> = self.data().alarms.iter().map(|a| a.id).collect();
        let mut rt = self.rt.lock().unwrap();
        rt.retain(|id, _| timers.iter().any(|t| t.0 == *id));
        for (id, durs, o) in timers {
            match rt.get_mut(&id) {
                Some(e) => {
                    if e.durations != durs || e.opts != o {
                        e.set_steps(durs, o);
                    }
                }
                None => {
                    rt.insert(id, Engine::with_opts(durs, o));
                }
            }
        }
        drop(rt);
        let mut art = self.alarm_rt.lock().unwrap();
        art.retain(|id, _| alarms.contains(id));
        // 설정이 바뀌었을 수 있으니 다음 시각은 다시 계산
        for r in art.values_mut() {
            r.next = None;
        }
    }

    pub fn upsert_timer(&self, t: TimerDef) {
        self.update(|d| match d.timers.iter_mut().find(|x| x.id == t.id) {
            Some(x) => *x = t,
            None => d.timers.push(t),
        });
    }

    pub fn delete_timer(&self, id: Id) {
        self.dismiss(RingKind::Timer(id));
        self.update(|d| {
            d.timers.retain(|t| t.id != id);
            if d.settings.automation_timer == Some(id) {
                d.settings.automation_timer = None;
            }
        });
    }

    pub fn upsert_alarm(&self, a: AlarmDef) {
        self.update(|d| match d.alarms.iter_mut().find(|x| x.id == a.id) {
            Some(x) => *x = a,
            None => d.alarms.push(a),
        });
    }

    pub fn delete_alarm(&self, id: Id) {
        self.dismiss(RingKind::Alarm(id));
        self.update(|d| d.alarms.retain(|a| a.id != id));
    }

    // ── 타이머 ─────────────────────────────────────────────

    pub fn cmd(&self, id: Id, c: Cmd, src: &str) {
        let ev = match self.rt.lock().unwrap().get_mut(&id) {
            Some(e) => e.command(c, Instant::now()),
            None => return,
        };
        if c == Cmd::Reset || c == Cmd::Toggle || c == Cmd::Start {
            self.dismiss(RingKind::Timer(id));
        }
        if ev.is_empty() {
            log(format!("{} {c:?} ({src}) -> no change", self.timer_name(id)));
        } else if let Some(sn) = self.run_snap(id) {
            crate::sync::local_run(id, &sn);
        }
        self.handle(id, ev, src);
        self.repaint();
    }

    // ── 치료실 베드 ─────────────────────────────────────────

    /// 베드 수 맞추기 — 모자라면 만들고, 남으면 쉬고 있는 베드만 지움. 프로그램이 없으면 기본 프로그램
    pub fn ensure_beds(&self, n: u32) {
        self.update(|d| {
            if d.bed_presets.is_empty() {
                d.bed_presets = crate::model::BedPreset::defaults();
            }
            let have: Vec<u32> = d.timers.iter().filter_map(|t| t.bed.as_ref().map(|b| b.no)).collect();
            for no in 1..=n {
                if !have.contains(&no) {
                    let mut t = TimerDef::simple(&format!("{no}번 베드"), 15 * 60, crate::model::PALETTE[(no as usize - 1) % crate::model::PALETTE.len()]);
                    t.card = crate::model::CardMode::Off;
                    t.notify = true;
                    t.warn_secs = 30;
                    t.warn_sound = "none".into(); // 베드 = 소리 없이 화면 효과만, 끝에 소리 · 음성
                    t.bed = Some(crate::model::BedInfo { no, ..Default::default() });
                    d.timers.push(t);
                }
            }
            d.timers.retain(|t| t.bed.as_ref().map(|b| b.no <= n || !b.patient.is_empty() || !b.program.is_empty()).unwrap_or(true));
        });
    }

    /// 베드 시작 — 프로그램(단계들) · 환자
    pub fn bed_start(&self, id: Id, program: &str, steps: Vec<crate::model::Step>, patient: &str, src: &str) {
        if steps.is_empty() {
            return;
        }
        let (program, patient) = (program.to_string(), patient.trim().to_string());
        self.update(|d| {
            if let Some(t) = d.timers.iter_mut().find(|t| t.id == id) {
                t.steps = steps;
                if let Some(b) = t.bed.as_mut() {
                    b.program = program;
                    b.patient = patient;
                }
            }
        });
        self.dismiss(RingKind::Timer(id));
        if self.view(id).map(|v| v.phase != Phase::Idle).unwrap_or(false) {
            self.cmd(id, Cmd::Reset, src);
        }
        self.cmd(id, Cmd::Start, src);
    }

    /// 베드 비우기 — 처음으로 + 환자 · 프로그램 지움
    pub fn bed_clear(&self, id: Id, src: &str) {
        self.dismiss(RingKind::Timer(id));
        self.cmd(id, Cmd::Reset, src);
        self.update(|d| {
            if let Some(b) = d.timers.iter_mut().find(|t| t.id == id).and_then(|t| t.bed.as_mut()) {
                b.patient.clear();
                b.program.clear();
                b.since.clear();
                b.note.clear();
            }
        });
    }

    fn bed_info(&self, id: Id) -> Option<crate::model::BedInfo> {
        self.data().timers.iter().find(|t| t.id == id).and_then(|t| t.bed.clone())
    }

    /// 환자가 베드에 들어옴 (EMR 연결) — 시작은 안 함. 반환 = 한 일
    pub fn bed_set_patient(&self, id: Id, name: &str, since: &str, note: &str, src: &str) -> &'static str {
        let (Some(v), Some(b)) = (self.view(id), self.bed_info(id)) else { return "no bed" };
        let name = name.trim().to_string();
        let set = |me: &Self| {
            let (n, s, t) = (name.clone(), since.to_string(), note.to_string());
            me.update(|d| {
                if let Some(bb) = d.timers.iter_mut().find(|t| t.id == id).and_then(|t| t.bed.as_mut()) {
                    bb.patient = n;
                    bb.since = s;
                    bb.note = t;
                }
            });
        };
        match v.phase {
            Phase::Running | Phase::Paused => {
                if b.patient.is_empty() || b.patient == name {
                    set(self);
                    "updated"
                } else {
                    "busy" // 다른 환자 치료 중 — 그대로
                }
            }
            Phase::Done => {
                if b.patient == name {
                    "same"
                } else {
                    // 지난 치료 끝난 베드에 새 환자
                    self.bed_clear(id, src);
                    set(self);
                    "replaced"
                }
            }
            Phase::Idle => {
                if b.patient == name && b.since == since && b.note == note {
                    "same"
                } else {
                    set(self);
                    "set"
                }
            }
        }
    }

    /// 환자가 베드에서 나감 (EMR 연결) — 끝난 베드 · 시작 안 한 베드만 비움, 치료 중이면 그대로
    pub fn bed_vacate(&self, id: Id, name: &str, src: &str) -> &'static str {
        let (Some(v), Some(b)) = (self.view(id), self.bed_info(id)) else { return "no bed" };
        let matches = name.trim().is_empty() || b.patient == name.trim();
        match v.phase {
            Phase::Running | Phase::Paused => "treating",
            _ if !matches => "other patient",
            Phase::Done => {
                self.bed_clear(id, src);
                "cleared"
            }
            Phase::Idle => {
                if b.patient.is_empty() && b.program.is_empty() {
                    "empty"
                } else {
                    self.bed_clear(id, src);
                    "cleared"
                }
            }
        }
    }

    /// N분 더 — 진행 중이면 마지막 단계를 늘림, 끝났으면 「연장」 단계로 다시 시작
    pub fn bed_extend(&self, id: Id, mins: u32, src: &str) {
        let Some(v) = self.view(id) else { return };
        if v.phase == Phase::Done || v.phase == Phase::Idle {
            let (prog, pat) = self.data().timers.iter().find(|t| t.id == id).and_then(|t| t.bed.clone()).map(|b| (b.program, b.patient)).unwrap_or_default();
            let prog = if prog.is_empty() { format!("{mins}분") } else if prog.ends_with("연장") { prog } else { format!("{prog} · 연장") };
            self.bed_start(id, &prog, vec![crate::model::Step { name: "연장".into(), secs: mins * 60, sound: "chime".into() }], &pat, src);
        } else {
            self.update(|d| {
                if let Some(s) = d.timers.iter_mut().find(|t| t.id == id).and_then(|t| t.steps.last_mut()) {
                    s.secs += mins * 60;
                }
            });
            log(format!("{} +{mins}분 ({src})", self.timer_name(id)));
        }
    }

    /// 모래시계 뒤집기 — 대기·완료면 시작, 진행 중이면 지금 단계를 처음부터 (일시정지였으면 이어서)
    pub fn flip(&self, id: Id) {
        let Some(v) = self.view(id) else { return };
        match v.phase {
            Phase::Idle | Phase::Done => self.cmd(id, Cmd::Start, "flip"),
            Phase::Running => self.cmd(id, Cmd::Goto(v.idx), "flip"),
            Phase::Paused => {
                self.cmd(id, Cmd::Goto(v.idx), "flip");
                self.cmd(id, Cmd::Resume, "flip");
            }
        }
    }

    /// 떠 있는 카드 닫기 — 그 타이머를 다시 시작하면 다시 보임
    pub fn hide_card(&self, id: Id) {
        self.opened_cards.lock().unwrap().remove(&id);
        self.hidden_cards.lock().unwrap().insert(id);
        self.repaint();
    }

    /// 카드 다시 열기 (진행 중이든 아니든)
    pub fn open_card(&self, id: Id) {
        self.hidden_cards.lock().unwrap().remove(&id);
        self.opened_cards.lock().unwrap().insert(id);
        self.repaint();
    }

    /// 닫은 카드 모두 다시 열기 (트레이 메뉴)
    pub fn reopen_closed_cards(&self) -> usize {
        let ids: Vec<Id> = self.hidden_cards.lock().unwrap().drain().collect();
        let n = ids.len();
        self.opened_cards.lock().unwrap().extend(ids);
        self.repaint();
        n
    }

    pub fn card_opened(&self, id: Id) -> bool {
        self.opened_cards.lock().unwrap().contains(&id)
    }

    pub fn card_hidden(&self, id: Id) -> bool {
        self.hidden_cards.lock().unwrap().contains(&id)
    }

    fn timer_name(&self, id: Id) -> String {
        self.data().timers.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default()
    }

    pub fn view(&self, id: Id) -> Option<TimerView> {
        let d = self.data();
        let t = d.timers.iter().find(|t| t.id == id)?;
        let rt = self.rt.lock().unwrap();
        let e = rt.get(&id)?;
        Some(make_view(t, e, Instant::now()))
    }

    pub fn views(&self) -> Vec<TimerView> {
        let d = self.data();
        let rt = self.rt.lock().unwrap();
        let now = Instant::now();
        d.timers.iter().filter_map(|t| rt.get(&t.id).map(|e| make_view(t, e, now))).collect()
    }

    fn add_stat(&self, id: Id, f: impl FnOnce(&mut DayStat)) {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut s = self.stats.lock().unwrap();
        f(s.days.entry(today).or_default().entry(id.to_string()).or_default());
        while s.days.len() > 120 {
            let first = s.days.keys().next().cloned().unwrap();
            s.days.remove(&first);
        }
        if let Ok(j) = serde_json::to_string(&*s) {
            let _ = std::fs::write(stats_path(), j);
        }
    }

    pub fn today(&self, id: Id) -> DayStat {
        let today = Local::now().format("%Y-%m-%d").to_string();
        self.stats.lock().unwrap().days.get(&today).and_then(|m| m.get(&id.to_string())).copied().unwrap_or_default()
    }

    pub fn automation_id(&self) -> Option<Id> {
        let d = self.data();
        d.settings
            .automation_timer
            .filter(|id| d.timers.iter().any(|t| t.id == *id))
            .or_else(|| d.timers.first().map(|t| t.id))
    }

    fn handle(&self, id: Id, ev: Vec<Event>, src: &str) {
        let Some(t) = self.data().timers.iter().find(|t| t.id == id).cloned() else { return };
        let mut sound: Option<usize> = None;
        // 다른 PC 에서 시작한 타이머 — 이 PC 소리는 설정에 따라
        let muted = crate::sync::remote_run(id) && !self.data().settings.sync.remote_sound;
        for e in &ev {
            match e {
                Event::Started => {
                    self.hidden_cards.lock().unwrap().remove(&id);
                    let s = self.rt.lock().unwrap().get(&id).map(|e| e.chain_start_unix).unwrap_or(0);
                    log(format!("{} start ({src}) chain_start_unix={s}", t.name));
                }
                Event::StepEnded(i) => {
                    sound = Some(*i);
                    let secs = t.steps.get(*i).map(|s| s.secs).unwrap_or(0);
                    let focus = t.pomodoro.is_some() && t.steps.get(*i).map(|s| s.name.starts_with("집중")).unwrap_or(false);
                    self.add_stat(id, |st| {
                        st.secs += secs;
                        if focus {
                            st.focus_count += 1;
                            st.focus_secs += secs;
                        }
                    });
                    log(format!("{} step {}/{} done: {}", t.name, i + 1, t.steps.len(), t.steps.get(*i).map(|s| s.name.as_str()).unwrap_or("")));
                }
                Event::Finished => {
                    log(format!("{} finished", t.name));
                    self.add_stat(id, |st| st.completes += 1);
                    // 다른 PC 에서 시작한 타이머는 설정을 켠 PC 에서만 완료 창 (소리와 같은 설정)
                    if let Some(b) = &t.bed {
                        if !muted {
                            let mut sub = vec![];
                            if !b.patient.trim().is_empty() {
                                sub.push(b.patient.trim().to_string());
                            }
                            if !b.program.is_empty() {
                                sub.push(format!("{} 끝", b.program));
                            }
                            self.push_ring(RingKind::Timer(id), format!("{} 끝", t.name), if sub.is_empty() { "치료 끝".into() } else { sub.join(" · ") }, None);
                            if self.data().settings.bed_voice {
                                crate::tts::speak(&format!("{} 치료가 끝났습니다", t.name), self.data().settings.volume);
                            }
                        }
                    } else if t.notify && !muted {
                        self.push_ring(RingKind::Timer(id), "타이머 완료".into(), t.name.clone(), None);
                    }
                }
                Event::Looped => {
                    log(format!("{} loop", t.name));
                    self.add_stat(id, |st| st.completes += 1);
                }
                Event::Jumped(i) => log(format!("{} jump to step {} ({src})", t.name, i + 1)),
                Event::Warn(_, _) if muted => {}
                Event::Warn(i, s) => {
                    if *s == t.warn_secs {
                        log(format!("{} countdown {}s before end of step {}", t.name, s, i + 1));
                    }
                    // 옛 설정값(한 번 울리던 예고음)은 짧은 「삐」로
                    let base = match t.warn_sound.as_str() {
                        "" | "warn" | "countdown" => "tick".to_string(),
                        x => x.to_string(),
                    };
                    // 마지막 1초는 높은 음
                    let snd = if *s <= 1 && base.starts_with("tick") { format!("{base}-hi") } else { base };
                    let vol = self.data().settings.volume;
                    audio::play(&snd, PlayOpts { volume: vol, repeat: 1, looping: false, max: Duration::ZERO });
                }
                Event::Waiting(i) => log(format!("{} waiting to start step {}", t.name, i + 1)),
                Event::Paused => log(format!("{} pause ({src})", t.name)),
                Event::Resumed => log(format!("{} resume ({src})", t.name)),
                Event::Reset => log(format!("{} reset ({src})", t.name)),
                Event::Skipped(i) => log(format!("{} skip step {} ({src})", t.name, i + 1)),
            }
        }
        // 절전 복귀 등으로 여러 단계를 한 번에 넘으면 소리는 마지막 한 번만
        if let Some(i) = sound.filter(|_| !muted) {
            if let Some(s) = t.steps.get(i) {
                let vol = self.data().settings.volume;
                audio::play(&s.sound, PlayOpts { volume: vol, repeat: t.sound_repeat, looping: false, max: Duration::ZERO });
            }
        }
    }

    // ── 알람 · 울림 ───────────────────────────────────────

    pub fn next_alarm(&self, id: Id) -> Option<DateTime<Local>> {
        let a = self.data().alarms.iter().find(|a| a.id == id).cloned()?;
        if !a.enabled {
            return None;
        }
        let mut art = self.alarm_rt.lock().unwrap();
        let r = art.entry(id).or_default();
        if r.next.is_none() {
            r.next = alarm::next_fire(&a, Local::now());
        }
        match (r.next, r.snooze) {
            (Some(n), Some(s)) => Some(n.min(s)),
            (n, s) => n.or(s),
        }
    }

    fn push_ring(&self, kind: RingKind, title: String, subtitle: String, sound: Option<String>) {
        self.push_ring_ex(kind, title, subtitle, sound, true);
    }

    /// looping = 끌 때까지 계속 (최대 10분), false = 한 번만
    fn push_ring_ex(&self, kind: RingKind, title: String, subtitle: String, sound: Option<String>, looping: bool) {
        let vol = self.data().settings.volume;
        let stop = match sound {
            Some(s) => audio::play(&s, PlayOpts { volume: vol, repeat: 1, looping, max: Duration::from_secs(if looping { 600 } else { 0 }) }),
            None => Arc::new(AtomicBool::new(true)),
        };
        let mut rings = self.rings.lock().unwrap();
        if let Some(old) = rings.iter().position(|r| r.kind == kind) {
            rings[old].stop.store(true, Ordering::Relaxed);
            rings.remove(old);
        }
        rings.push(Ring { kind, title, subtitle, at: Local::now(), since: Instant::now(), stop });
        drop(rings);
        self.repaint();
    }

    pub fn rings(&self) -> Vec<(RingKind, String, String, DateTime<Local>)> {
        self.rings.lock().unwrap().iter().map(|r| (r.kind, r.title.clone(), r.subtitle.clone(), r.at)).collect()
    }

    pub fn dismiss(&self, kind: RingKind) {
        self.dismiss_ex(kind, true);
    }

    fn dismiss_ex(&self, kind: RingKind, broadcast: bool) {
        let mut rings = self.rings.lock().unwrap();
        let mut removed = false;
        if let Some(i) = rings.iter().position(|r| r.kind == kind) {
            rings[i].stop.store(true, Ordering::Relaxed);
            rings.remove(i);
            removed = true;
            if let RingKind::Alarm(id) = kind {
                log(format!("alarm {id} dismissed"));
            }
        }
        drop(rings);
        if removed && broadcast {
            crate::sync::local_ring_off(kind);
        }
        self.repaint();
    }

    pub fn snooze(&self, id: Id) {
        let min = self.data().alarms.iter().find(|a| a.id == id).map(|a| a.snooze_min.max(1)).unwrap_or(5);
        self.dismiss(RingKind::Alarm(id));
        self.alarm_rt.lock().unwrap().entry(id).or_default().snooze = Some(Local::now() + chrono::Duration::minutes(min as i64));
        log(format!("alarm {id} snoozed {min} min"));
    }

    // ── 미리 듣기 ─────────────────────────────────────────

    pub fn preview(&self, sound: &str) {
        self.stop_preview();
        let vol = self.data().settings.volume;
        *self.preview.lock().unwrap() =
            Some(audio::play(sound, PlayOpts { volume: vol, repeat: 1, looping: false, max: Duration::ZERO }));
    }

    pub fn stop_preview(&self) {
        if let Some(s) = self.preview.lock().unwrap().take() {
            s.store(true, Ordering::Relaxed);
        }
    }

    // ── 100ms 마다 ────────────────────────────────────────

    pub fn tick(&self) {
        let now = Instant::now();
        let evs: Vec<(Id, Vec<Event>)> = {
            let mut rt = self.rt.lock().unwrap();
            rt.iter_mut().map(|(id, e)| (*id, e.tick(now))).filter(|(_, v)| !v.is_empty()).collect()
        };
        let changed = !evs.is_empty();
        for (id, ev) in evs {
            self.handle(id, ev, "timer");
        }
        self.tick_alarms();
        self.tick_reminders();
        self.tick_countdowns();
        self.tick_ddays();
        // 10분 넘게 울린 건 정리
        let stale: Vec<RingKind> =
            self.rings.lock().unwrap().iter().filter(|r| r.since.elapsed() > Duration::from_secs(600)).map(|r| r.kind).collect();
        for k in stale {
            log(format!("{k:?} auto-dismissed after 10 min"));
            self.dismiss(k);
        }
        if changed {
            self.repaint();
        }
    }

    // ── 리마인더 ─────────────────────────────────────────

    pub fn upsert_reminder(&self, mut r: Reminder) {
        r.modified = unix_now();
        self.rem_fired.lock().unwrap().remove(&r.id);
        self.update(|d| match d.reminders.iter_mut().find(|x| x.id == r.id) {
            Some(x) => *x = r,
            None => d.reminders.push(r),
        });
        crate::todo::nudge();
    }

    pub fn delete_reminder(&self, id: Id) {
        self.dismiss(RingKind::Reminder(id));
        self.update(|d| {
            if let Some(r) = d.reminders.iter().find(|r| r.id == id) {
                if let (Some(l), Some(t)) = (r.todo_list.clone(), r.todo_id.clone()) {
                    d.todo_deleted.push((l, t));
                }
            }
            d.reminders.retain(|r| r.id != id);
        });
        crate::todo::nudge();
    }

    /// 완료 — 반복이면 다음 날짜로 넘김
    pub fn complete_reminder(&self, id: Id) {
        self.dismiss(RingKind::Reminder(id));
        let now = Local::now();
        self.update(|d| {
            if let Some(r) = d.reminders.iter_mut().find(|r| r.id == id) {
                r.modified = unix_now();
                match (r.repeat, r.due.as_deref().and_then(parse_due)) {
                    (rep, Some(due)) if rep != Repeat::None => {
                        let mut n = next_repeat(due, rep);
                        while n <= now.naive_local() {
                            n = next_repeat(n, rep);
                        }
                        r.due = Some(fmt_due(n));
                    }
                    _ => {
                        r.done = true;
                        r.done_at = Some(now.format("%Y-%m-%dT%H:%M").to_string());
                    }
                }
            }
        });
        log(format!("reminder {id} completed"));
        crate::todo::nudge();
    }

    pub fn uncomplete_reminder(&self, id: Id) {
        self.update(|d| {
            if let Some(r) = d.reminders.iter_mut().find(|r| r.id == id) {
                r.done = false;
                r.done_at = None;
                r.modified = unix_now();
            }
        });
        crate::todo::nudge();
    }

    pub fn snooze_reminder(&self, id: Id, mins: i64) {
        self.dismiss(RingKind::Reminder(id));
        let due = fmt_due((Local::now() + chrono::Duration::minutes(mins)).naive_local());
        self.update(|d| {
            if let Some(r) = d.reminders.iter_mut().find(|r| r.id == id) {
                r.due = Some(due.clone());
                r.modified = unix_now();
            }
        });
        log(format!("reminder {id} snoozed {mins} min"));
        crate::todo::nudge();
    }

    fn tick_reminders(&self) {
        let now = Local::now().naive_local();
        let due: Vec<Reminder> = self
            .data()
            .reminders
            .iter()
            .filter(|r| !r.done)
            .filter(|r| r.due.as_deref().and_then(parse_due).map(|d| d <= now).unwrap_or(false))
            .cloned()
            .collect();
        for r in due {
            let key = r.due.clone().unwrap_or_default();
            {
                let mut fired = self.rem_fired.lock().unwrap();
                if fired.get(&r.id) == Some(&key) {
                    continue;
                }
                fired.insert(r.id, key.clone());
            }
            let d = parse_due(&key).unwrap();
            // 컴퓨터가 꺼져 있던 동안 지난 건 12시간 안이면 울림, 더 오래면 조용히 넘김
            if now - d > chrono::Duration::hours(12) {
                continue;
            }
            log(format!("reminder '{}' ringing ({key})", r.title));
            let sub = if r.notes.trim().is_empty() { format!("리마인더 · {}", d.format("%H:%M")) } else { format!("리마인더 · {} · {}", d.format("%H:%M"), r.notes.lines().next().unwrap_or("")) };
            self.push_ring(RingKind::Reminder(r.id), r.title.clone(), sub, Some(r.sound.clone()));
        }
    }

    // ── 카운트다운 ───────────────────────────────────────

    pub fn upsert_countdown(&self, c: crate::model::Countdown) {
        self.update(|d| match d.countdowns.iter_mut().find(|x| x.id == c.id) {
            Some(x) => *x = c,
            None => d.countdowns.push(c),
        });
    }

    pub fn delete_countdown(&self, id: Id) {
        self.dismiss(RingKind::Countdown(id));
        self.update(|d| d.countdowns.retain(|c| c.id != id));
    }

    fn tick_countdowns(&self) {
        let now = Local::now().naive_local();
        let due: Vec<crate::model::Countdown> = self
            .data()
            .countdowns
            .iter()
            .filter(|c| !c.fired && c.target_dt().map(|t| t <= now).unwrap_or(false))
            .cloned()
            .collect();
        if due.is_empty() {
            return;
        }
        let ids: Vec<Id> = due.iter().map(|c| c.id).collect();
        self.update(|d| {
            for c in d.countdowns.iter_mut().filter(|c| ids.contains(&c.id)) {
                c.fired = true;
            }
        });
        for c in due {
            let t = c.target_dt().unwrap();
            // 컴퓨터가 꺼져 있던 동안 지난 건 12시간 안이면 울림, 더 오래면 조용히 넘김
            if !c.notify || now - t > chrono::Duration::hours(12) {
                log(format!("countdown '{}' reached ({}) — silent", c.title, c.target));
                continue;
            }
            log(format!("countdown '{}' reached ({}) ringing", c.title, c.target));
            let sub = format!("카운트다운 · {} {}", crate::dday::date_ko(t.date(), now.date()), t.format("%H:%M"));
            self.push_ring(RingKind::Countdown(c.id), c.title.clone(), sub, Some(c.sound.clone()));
        }
    }

    // ── 디데이 알림 ─────────────────────────────────────

    pub fn upsert_dday(&self, mut v: crate::model::Dday) {
        // 저장 시점에 이미 지난 알림은 울리지 않음 (오늘 15시에 만든 「오늘 09:00」 알림 등)
        let now = Local::now().naive_local();
        for a in crate::dday::due_alerts(&v, now) {
            if !v.fired.contains(&a.key) {
                v.fired.push(a.key);
            }
        }
        self.update(|d| match d.ddays.iter_mut().find(|x| x.id == v.id) {
            Some(x) => *x = v,
            None => d.ddays.push(v),
        });
    }

    pub fn has_dday_snooze(&self) -> bool {
        !self.dday_snooze.lock().unwrap().is_empty()
    }

    /// 디데이 알림 「N분 뒤 다시」
    pub fn snooze_dday(&self, id: Id, mins: i64) {
        let sub = self.rings().into_iter().find(|r| r.0 == RingKind::Dday(id)).map(|r| r.2).unwrap_or_default();
        self.dismiss(RingKind::Dday(id));
        let at = Local::now().naive_local() + chrono::Duration::minutes(mins);
        self.dday_snooze.lock().unwrap().insert(id, (at, sub));
        log(format!("dday {id} snoozed {mins} min"));
    }

    pub fn delete_dday(&self, id: Id) {
        self.dday_snooze.lock().unwrap().remove(&id);
        self.dismiss(RingKind::Dday(id));
        self.update(|d| d.ddays.retain(|x| x.id != id));
    }

    fn tick_ddays(&self) {
        let now = Local::now().naive_local();
        // 다시 알림
        let snoozed: Vec<(Id, String)> = {
            let mut sn = self.dday_snooze.lock().unwrap();
            let due: Vec<(Id, String)> = sn.iter().filter(|(_, v)| v.0 <= now).map(|(k, v)| (*k, v.1.clone())).collect();
            for (k, _) in &due {
                sn.remove(k);
            }
            due
        };
        for (id, sub) in snoozed {
            let Some(d) = self.data().ddays.iter().find(|x| x.id == id).cloned() else { continue };
            log(format!("dday '{}' snooze ringing", d.title));
            self.push_ring_ex(RingKind::Dday(id), d.title.clone(), sub, Some(d.sound.clone()), !d.ring_once);
        }
        let mut fire: Vec<(crate::model::Dday, crate::dday::Alert)> = Vec::new();
        for d in self.data().ddays.iter().filter(|d| d.notify) {
            for a in crate::dday::due_alerts(d, now) {
                if !d.fired.contains(&a.key) {
                    fire.push((d.clone(), a));
                }
            }
        }
        if fire.is_empty() {
            return;
        }
        self.update(|data| {
            for (d, a) in &fire {
                if let Some(x) = data.ddays.iter_mut().find(|x| x.id == d.id) {
                    x.fired.push(a.key.clone());
                    let n = x.fired.len();
                    if n > 20 {
                        x.fired.drain(..n - 20);
                    }
                }
            }
        });
        // 같은 디데이 알림이 겹치면 (예: 1일 전 + 그날) 가까운 것 하나만
        let mut seen: Vec<Id> = Vec::new();
        fire.sort_by_key(|(_, a)| a.before);
        for (d, a) in fire {
            if seen.contains(&d.id) {
                continue;
            }
            seen.push(d.id);
            let day = crate::dday::date_ko(a.date, now.date());
            let sub = match (a.before, d.mode) {
                (0, crate::model::DdayMode::Until) => format!("디데이 · 오늘이 그날 ({day})"),
                (0, crate::model::DdayMode::Since) => format!("디데이 · 오늘 {}", a.label),
                (0, crate::model::DdayMode::Yearly) => format!("디데이 · 오늘 {}", a.label),
                (n, crate::model::DdayMode::Until) => format!("디데이 · D-{n} ({day})"),
                (n, _) => format!("디데이 · {} D-{n} ({day})", a.label),
            };
            log(format!("dday '{}' alert {} ringing", d.title, a.key));
            self.push_ring_ex(RingKind::Dday(d.id), d.title.clone(), sub, Some(d.sound.clone()), !d.ring_once);
        }
    }

    fn ring_alarm(&self, a: &AlarmDef) {
        self.alarm_rt.lock().unwrap().entry(a.id).or_default().rang = Some(Local::now());
        let title = format!("{:02}:{:02}", a.hour, a.minute);
        let sub = if a.label.trim().is_empty() { "알람".to_string() } else { a.label.clone() };
        log(format!("alarm {title} '{sub}' ringing"));
        self.push_ring(RingKind::Alarm(a.id), title, sub, Some(a.sound.clone()));
    }

    /// 연동: 다른 PC 가 방금 울리고 끈 일회성 알람 — 이 PC 가 아직 안 울렸으면 같이 울림
    fn catch_remote_alarms(&self, before: &[AlarmDef]) {
        let now = Local::now();
        let after: Vec<AlarmDef> = self.data().alarms.clone();
        for a in after.iter().filter(|a| !a.enabled && !a.days.iter().any(|d| *d)) {
            let Some(old) = before.iter().find(|b| b.id == a.id) else { continue };
            if !old.enabled {
                continue;
            }
            // 오늘 그 시각이 지금부터 2분 안쪽으로 지났나
            let Some(t) = now.date_naive().and_hms_opt(a.hour, a.minute, 0).and_then(|t| Local.from_local_datetime(&t).earliest()) else { continue };
            let ago = now - t;
            if ago < chrono::Duration::zero() || ago > chrono::Duration::minutes(2) {
                continue;
            }
            let rang = self.alarm_rt.lock().unwrap().get(&a.id).and_then(|r| r.rang).map(|r| now - r < chrono::Duration::minutes(3)).unwrap_or(false);
            if !rang {
                log(format!("alarm {} rang on other PC — ringing here too", a.id));
                self.ring_alarm(a);
            }
        }
    }

    fn tick_alarms(&self) {
        let now = Local::now();
        let alarms: Vec<AlarmDef> = self.data().alarms.iter().filter(|a| a.enabled).cloned().collect();
        let mut fire: Vec<AlarmDef> = Vec::new();
        let mut disable: Vec<Id> = Vec::new();
        {
            let mut art = self.alarm_rt.lock().unwrap();
            for a in &alarms {
                let r = art.entry(a.id).or_default();
                if r.next.is_none() {
                    r.next = alarm::next_fire(a, now - chrono::Duration::seconds(1));
                }
                let mut due = false;
                if let Some(n) = r.next {
                    if n <= now {
                        // 절전 등으로 30분 넘게 지난 건 건너뜀
                        due = now - n < chrono::Duration::minutes(30);
                        if !due {
                            log(format!("alarm {} missed ({})", a.label, n.format("%m-%d %H:%M")));
                        }
                        r.next = alarm::next_fire(a, now);
                        if !a.days.iter().any(|d| *d) {
                            disable.push(a.id);
                        }
                    }
                }
                if let Some(s) = r.snooze {
                    if s <= now {
                        due = true;
                        r.snooze = None;
                    }
                }
                if due {
                    fire.push(a.clone());
                }
            }
        }
        for a in fire {
            self.ring_alarm(&a);
        }
        if !disable.is_empty() {
            self.update(|d| {
                for a in d.alarms.iter_mut().filter(|a| disable.contains(&a.id)) {
                    a.enabled = false;
                }
            });
        }
    }

    // ── API ──────────────────────────────────────────────

    pub fn status_json(&self, id: Option<Id>) -> serde_json::Value {
        let Some(v) = id.and_then(|i| self.view(i)) else {
            return serde_json::json!({ "error": "no timer", "version": env!("CARGO_PKG_VERSION") });
        };
        let hk = self.data().timers.iter().find(|t| t.id == v.id).map(|t| t.hotkey_toggle.clone()).unwrap_or_default();
        serde_json::json!({
            "id": v.id.to_string(),
            "name": v.name,
            "phase": v.phase.as_str(),
            "running": v.phase == Phase::Running,
            "step": if v.phase == Phase::Idle { 0 } else { v.idx + 1 },
            "step_count": v.n,
            "step_name": if v.phase == Phase::Idle { String::new() } else { v.step_name.clone() },
            "next_name": v.next_name,
            "step_remaining": (v.step_rem.as_secs_f32() * 10.0).round() / 10.0,
            "total_remaining": (v.total_rem.as_secs_f32() * 10.0).round() / 10.0,
            "chain_start_unix": v.chain_start_unix,
            "now_unix": unix_now(),
            "hotkeys": hk,
            "version": env!("CARGO_PKG_VERSION"),
        })
    }
}

fn make_view(t: &TimerDef, e: &Engine, now: Instant) -> TimerView {
    let n = t.steps.len().max(1);
    let idx = e.idx.min(n - 1);
    TimerView {
        id: t.id,
        name: t.name.clone(),
        color: t.color.clone(),
        phase: e.phase,
        idx,
        n,
        step_name: t.steps.get(idx).map(|s| s.name.clone()).unwrap_or_default(),
        next_name: if e.phase.active() { t.steps.get(idx + 1).map(|s| s.name.clone()) } else { None },
        step_rem: e.step_remaining(now),
        total_rem: e.total_remaining(now),
        total: e.total(),
        frac: e.step_fraction(now),
        step_secs: t.steps.iter().map(|s| s.secs.max(1)).collect(),
        chain_start_unix: e.chain_start_unix,
        loops: e.loops,
        waiting: e.waiting,
        epoch: e.epoch,
        done_for: e.done_elapsed(now),
    }
}

pub fn parse_due(s: &str) -> Option<chrono::NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").ok()
}

pub fn fmt_due(d: chrono::NaiveDateTime) -> String {
    d.format("%Y-%m-%dT%H:%M").to_string()
}

pub fn next_repeat(d: chrono::NaiveDateTime, rep: Repeat) -> chrono::NaiveDateTime {
    use chrono::Datelike;
    match rep {
        Repeat::None => d,
        Repeat::Daily => d + chrono::Duration::days(1),
        Repeat::Weekly => d + chrono::Duration::days(7),
        Repeat::Weekdays => {
            let mut n = d + chrono::Duration::days(1);
            while n.weekday().num_days_from_monday() >= 5 {
                n += chrono::Duration::days(1);
            }
            n
        }
        Repeat::Monthly => d.checked_add_months(chrono::Months::new(1)).unwrap_or(d + chrono::Duration::days(30)),
    }
}

#[cfg(test)]
mod rem_tests {
    use super::*;
    #[test]
    fn repeats() {
        let d = parse_due("2026-11-20T09:00").unwrap(); // 금요일
        assert_eq!(fmt_due(next_repeat(d, Repeat::Weekdays)), "2026-11-23T09:00");
        assert_eq!(fmt_due(next_repeat(d, Repeat::Daily)), "2026-11-21T09:00");
        assert_eq!(fmt_due(next_repeat(d, Repeat::Monthly)), "2026-12-20T09:00");
        let e = parse_due("2026-01-31T08:30").unwrap();
        assert_eq!(fmt_due(next_repeat(e, Repeat::Monthly)), "2026-02-28T08:30");
    }
}

#[cfg(test)]
mod bed_tests {
    use super::*;

    // TEMPO_DIR(프로세스 전체 환경 변수)를 쓰는 시험끼리 겹치지 않게
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn update_handover_keeps_running_timers() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("tempo-handover-{}", std::process::id()));
        std::env::set_var("TEMPO_DIR", &dir);
        let data = Data { timers: vec![TimerDef::simple("라면", 180, "#5B8DEF")], ..Data::seed() };
        let id = data.timers[0].id;
        let a = Core::new(data.clone(), (0.0, 0.0, 1920.0, 1080.0), (0.0, 0.0, 1920.0, 1080.0));
        a.cmd(id, Cmd::Start, "t");
        std::thread::sleep(Duration::from_millis(1200));
        let before = a.view(id).unwrap().total_rem;
        assert!(a.save_runs_for_update());
        // 새 버전 = 같은 데이터로 새로 켜짐 → 이어받음
        let b = Core::new(data, (0.0, 0.0, 1920.0, 1080.0), (0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(b.view(id).unwrap().phase, Phase::Idle);
        b.restore_runs_after_update();
        let v = b.view(id).unwrap();
        assert_eq!(v.phase, Phase::Running);
        assert!((v.total_rem.as_secs_f32() - before.as_secs_f32()).abs() < 0.5, "{:?} vs {:?}", v.total_rem, before);
        // 파일은 한 번만
        assert!(!dir.join("update-runs.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cards_can_be_reopened_while_running() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("tempo-cards-{}", std::process::id()));
        std::env::set_var("TEMPO_DIR", &dir);
        let mut t = TimerDef::simple("라면", 180, "#5B8DEF");
        t.card = crate::model::CardMode::WhenActive;
        let mut off = TimerDef::simple("숨김", 180, "#5B8DEF");
        off.card = crate::model::CardMode::Off;
        let (id, oid) = (t.id, off.id);
        let core = Core::new(Data { timers: vec![t, off], ..Data::seed() }, (0.0, 0.0, 1920.0, 1080.0), (0.0, 0.0, 1920.0, 1080.0));
        let shown = |i: Id| {
            let d = core.data().timers.iter().find(|x| x.id == i).cloned().unwrap();
            crate::ui::card::shown(&core, &d, &core.view(i).unwrap())
        };
        core.cmd(id, Cmd::Start, "t");
        assert!(shown(id));
        // 진행 중 닫기 → 안 보임 → 다시 열기 → 보임
        core.hide_card(id);
        assert!(!shown(id));
        core.open_card(id);
        assert!(shown(id));
        // 카드 설정이 「끄기」여도 직접 열면 보임, 닫으면 안 보임, 트레이 「닫은 카드 다시 열기」
        assert!(!shown(oid));
        core.open_card(oid);
        assert!(shown(oid));
        core.hide_card(oid);
        core.hide_card(id);
        assert!(!shown(oid) && !shown(id));
        assert_eq!(core.reopen_closed_cards(), 2);
        assert!(shown(oid) && shown(id));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn emr_patient_and_vacate_rules() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 시험 데이터 폴더 (실제 %APPDATA%\Tempo 를 건드리지 않게)
        let dir = std::env::temp_dir().join(format!("tempo-bedtest-{}", std::process::id()));
        std::env::set_var("TEMPO_DIR", &dir);
        let core = Core::new(Data { timers: vec![], ..Data::seed() }, (0.0, 0.0, 1920.0, 1080.0), (0.0, 0.0, 1920.0, 1080.0));
        core.ensure_beds(3);
        let id = |no: u32| core.data().timers.iter().find(|t| t.bed.as_ref().map(|b| b.no) == Some(no)).map(|t| t.id).unwrap();
        let b = |no: u32| core.data().timers.iter().find(|t| t.bed.as_ref().map(|b| b.no) == Some(no)).and_then(|t| t.bed.clone()).unwrap();
        // 들어옴 → 이름만, 시작 안 함
        assert_eq!(core.bed_set_patient(id(1), "가", "10:02", "허리 약침", "t"), "set");
        assert_eq!((b(1).patient.as_str(), b(1).since.as_str()), ("가", "10:02"));
        assert_eq!(core.view(id(1)).unwrap().phase, Phase::Idle);
        assert_eq!(core.bed_set_patient(id(1), "가", "10:02", "허리 약침", "t"), "same");
        // 치료 중엔 다른 환자로 안 바뀜, 나가도 안 비움
        let steps = vec![crate::model::Step { name: "침".into(), secs: 900, sound: "none".into() }];
        core.bed_start(id(1), "침 15분", steps, "가", "t");
        assert_eq!(core.bed_set_patient(id(1), "나", "10:20", "", "t"), "busy");
        assert_eq!(core.bed_vacate(id(1), "가", "t"), "treating");
        assert_eq!(b(1).patient, "가");
        // 시작 전에 나감 → 비움
        core.bed_set_patient(id(2), "다", "10:05", "", "t");
        assert_eq!(core.bed_vacate(id(2), "다", "t"), "cleared");
        assert!(b(2).patient.is_empty() && b(2).since.is_empty());
        // 다른 환자 이름으로 나감 신호 → 그대로
        core.bed_set_patient(id(3), "라", "10:06", "", "t");
        assert_eq!(core.bed_vacate(id(3), "마", "t"), "other patient");
        assert_eq!(b(3).patient, "라");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
