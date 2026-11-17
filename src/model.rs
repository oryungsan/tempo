//! 저장되는 사용자 데이터 — 타이머 · 알람 · 설정
use serde::{Deserialize, Serialize};

pub type Id = u64;

pub fn new_id() -> Id {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(1);
    t.wrapping_mul(31).wrapping_add(N.fetch_add(1, Ordering::Relaxed))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub name: String,
    pub secs: u32,
    /// 단계 끝 소리 — audio::BUILTIN 의 id, "file:<경로>", "none"
    #[serde(default = "default_sound")]
    pub sound: String,
}

fn default_sound() -> String {
    "bell".into()
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CardMode {
    Always,
    WhenActive,
    Off,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TimerDef {
    pub id: Id,
    pub name: String,
    pub color: String,
    pub steps: Vec<Step>,
    /// 끝나면 처음부터 다시
    #[serde(default)]
    pub repeat: bool,
    #[serde(default = "when_active")]
    pub card: CardMode,
    /// 끝나면 알림 창
    #[serde(default)]
    pub notify: bool,
    #[serde(default = "one")]
    pub sound_repeat: u32,
    #[serde(default)]
    pub hotkey_toggle: String,
    #[serde(default)]
    pub hotkey_reset: String,
    #[serde(default)]
    pub card_pos: Option<[f32; 2]>,
    /// 끝나기 N초 전 예고음 (0 = 끔). 기존 데이터엔 10초로 켜짐
    #[serde(default = "ten")]
    pub warn_secs: u32,
    /// true = 단계마다, false = 타이머 전체 끝나기 전만
    #[serde(default)]
    pub warn_each: bool,
    #[serde(default = "warn_sound")]
    pub warn_sound: String,
    /// false = 단계가 끝날 때마다 멈추고 ▶ 를 기다림
    #[serde(default = "yes")]
    pub auto_advance: bool,
    /// 포모도로로 만든 타이머면 설정값 (단계는 이걸로 생성)
    #[serde(default)]
    pub pomodoro: Option<Pomo>,
    /// 이 타이머만 다른 디자인 (None = 설정 따름)
    #[serde(default)]
    pub style: Option<Style>,
    /// 이 카드만 다른 크기 (가장자리를 끌어 조절, None = 설정 따름)
    #[serde(default)]
    pub card_scale: Option<f32>,
    /// 이 카드만 항상 위 켬/끔 (None = 설정 따름)
    #[serde(default)]
    pub card_on_top: Option<bool>,
}

/// 타이머 모양 — 색 테마와 별개
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Style {
    Classic,
    Ring,
    Minimal,
    Digital,
    Fill,
    Hourglass,
}

impl Style {
    pub const ALL: [Style; 6] = [Style::Classic, Style::Ring, Style::Hourglass, Style::Minimal, Style::Digital, Style::Fill];
    pub fn label(self) -> &'static str {
        match self {
            Style::Classic => "클래식",
            Style::Ring => "원형",
            Style::Minimal => "미니멀",
            Style::Digital => "디지털",
            Style::Fill => "채우기",
            Style::Hourglass => "모래시계",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            Style::Classic => "큰 숫자 + 단계 막대",
            Style::Ring => "원형 고리 · 동그란 카드",
            Style::Minimal => "숫자만 크게, 얇은 선",
            Style::Digital => "LCD 패널 느낌",
            Style::Fill => "진행만큼 바탕이 차오름",
            Style::Hourglass => "떨어지는 모래 · 뒤집기",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pomo {
    pub work_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    /// 집중 몇 번마다 긴 휴식
    pub every: u32,
}

impl Default for Pomo {
    fn default() -> Self {
        Pomo { work_min: 25, short_min: 5, long_min: 15, every: 4 }
    }
}

impl Pomo {
    pub fn steps(&self) -> Vec<Step> {
        let mut v = Vec::new();
        let n = self.every.clamp(1, 12);
        for i in 1..=n {
            v.push(Step { name: format!("집중 {i}"), secs: self.work_min.max(1) * 60, sound: "chime".into() });
            if i < n {
                v.push(Step { name: format!("휴식 {i}"), secs: self.short_min.max(1) * 60, sound: "bell-rise".into() });
            } else {
                v.push(Step { name: "긴 휴식".into(), secs: self.long_min.max(1) * 60, sound: "arpeggio".into() });
            }
        }
        v
    }
}

fn when_active() -> CardMode {
    CardMode::WhenActive
}

fn one() -> u32 {
    1
}
fn ten() -> u32 {
    10
}
fn yes() -> bool {
    true
}
fn warn_sound() -> String {
    "tick".into()
}

impl TimerDef {
    pub fn total_secs(&self) -> u32 {
        self.steps.iter().map(|s| s.secs).sum()
    }
    pub fn simple(name: &str, secs: u32, color: &str) -> Self {
        TimerDef {
            id: new_id(),
            name: name.into(),
            color: color.into(),
            steps: vec![Step { name: name.into(), secs, sound: "chime".into() }],
            repeat: false,
            card: CardMode::WhenActive,
            notify: true,
            sound_repeat: 2,
            hotkey_toggle: String::new(),
            hotkey_reset: String::new(),
            card_pos: None,
            warn_secs: 10,
            warn_each: false,
            warn_sound: "tick".into(),
            auto_advance: true,
            pomodoro: None,
            style: None,
            card_scale: None,
            card_on_top: None,
        }
    }

    pub fn pomodoro(color: &str) -> Self {
        let p = Pomo::default();
        TimerDef {
            name: "포모도로".into(),
            steps: p.steps(),
            repeat: true,
            notify: false,
            warn_secs: 0,
            pomodoro: Some(p),
            style: Some(Style::Ring),
            ..TimerDef::simple("포모도로", 1500, color)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AlarmDef {
    pub id: Id,
    pub label: String,
    pub hour: u32,
    pub minute: u32,
    /// 월~일. 전부 false = 한 번만
    pub days: [bool; 7],
    pub enabled: bool,
    pub sound: String,
    pub snooze_min: u32,
}

impl AlarmDef {
    pub fn new() -> Self {
        AlarmDef {
            id: new_id(),
            label: String::new(),
            hour: 9,
            minute: 0,
            days: [true, true, true, true, true, false, false],
            enabled: true,
            sound: "classic".into(),
            snooze_min: 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    Midnight,
    Forest,
    Sunset,
    Ocean,
    Nordic,
    Mono,
    Rose,
    Sand,
}

impl Theme {
    pub const ALL: [Theme; 10] =
        [Theme::Dark, Theme::Midnight, Theme::Forest, Theme::Sunset, Theme::Ocean, Theme::Nordic, Theme::Mono, Theme::Light, Theme::Rose, Theme::Sand];
    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "다크",
            Theme::Light => "라이트",
            Theme::Midnight => "미드나잇",
            Theme::Forest => "포레스트",
            Theme::Sunset => "선셋",
            Theme::Ocean => "오션",
            Theme::Nordic => "노르딕",
            Theme::Mono => "모노",
            Theme::Rose => "로즈",
            Theme::Sand => "샌드",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub volume: f32,
    pub theme: Theme,
    pub card_scale: f32,
    pub autostart: bool,
    pub close_to_tray: bool,
    /// 로컬 API 의 /status · /start 가 가리키는 타이머 (자동화 연동)
    pub automation_timer: Option<Id>,
    pub api_port: u16,
    #[serde(default)]
    pub main_pos: Option<[f32; 2]>,
    #[serde(default)]
    pub main_size: Option<[f32; 2]>,
    #[serde(default = "classic")]
    pub style: Style,
    #[serde(default)]
    pub todo: TodoSettings,
    /// 메인 창 항상 위
    #[serde(default)]
    pub main_on_top: bool,
    /// 떠 있는 카드 항상 위 (기본 켬)
    #[serde(default = "yes")]
    pub cards_on_top: bool,
    /// 새 버전 자동 업데이트
    #[serde(default = "upd_auto")]
    pub update_mode: UpdateMode,
    /// PC 연동
    #[serde(default)]
    pub sync: SyncSettings,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncMode {
    Off,
    /// 이 PC 가 다른 PC 의 연결을 받음
    Host,
    /// 호스트 PC 에 연결
    Join,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SyncSettings {
    pub mode: SyncMode,
    /// 연동 코드 (양쪽 같아야 연결)
    #[serde(default)]
    pub code: String,
    /// 참여: 호스트 주소 (IP · 컴퓨터 이름 · Tailscale 이름)
    #[serde(default)]
    pub host: String,
    /// 참여: 연결했던 호스트 PC 의 고유 번호 (IP 가 바뀌면 찾아서 따라감)
    #[serde(default)]
    pub host_pc: String,
    #[serde(default = "sync_port")]
    pub port: u16,
    /// 다른 PC 에서 시작한 타이머의 소리도 이 PC 에서 냄
    #[serde(default)]
    pub remote_sound: bool,
}

fn sync_port() -> u16 {
    47840
}

impl Default for SyncSettings {
    fn default() -> Self {
        SyncSettings { mode: SyncMode::Off, code: String::new(), host: String::new(), host_pc: String::new(), port: sync_port(), remote_sound: false }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpdateMode {
    /// 받아서 타이머가 모두 쉴 때 바로 교체
    Auto,
    /// 받아 두고 알려만 줌 (「지금 업데이트」)
    Notify,
    Off,
}

fn upd_auto() -> UpdateMode {
    UpdateMode::Auto
}

fn classic() -> Style {
    Style::Classic
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            volume: 0.6,
            theme: Theme::Dark,
            card_scale: 1.0,
            autostart: false,
            close_to_tray: true,
            automation_timer: None,
            api_port: 47830,
            main_pos: None,
            main_size: None,
            style: Style::Classic,
            todo: TodoSettings::default(),
            main_on_top: false,
            cards_on_top: true,
            update_mode: UpdateMode::Auto,
            sync: SyncSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Data {
    pub version: u32,
    pub timers: Vec<TimerDef>,
    pub alarms: Vec<AlarmDef>,
    pub settings: Settings,
    #[serde(default)]
    pub reminders: Vec<Reminder>,
    /// To Do 에서도 지워야 할 (목록 id, 작업 id) — 다음 동기화 때 처리
    #[serde(default)]
    pub todo_deleted: Vec<(String, String)>,
    /// 월드타임 — 보는 도시들
    #[serde(default)]
    pub world: Vec<WorldClock>,
    /// 카운트다운 — 정한 날짜·시각까지
    #[serde(default)]
    pub countdowns: Vec<Countdown>,
    /// 디데이
    #[serde(default)]
    pub ddays: Vec<Dday>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldClock {
    pub id: Id,
    /// IANA 시간대 이름 (예: "America/New_York")
    pub tz: String,
    /// 보이는 이름 (비우면 도시 목록의 이름)
    #[serde(default)]
    pub label: String,
}

impl WorldClock {
    pub fn new(tz: &str) -> Self {
        WorldClock { id: new_id(), tz: tz.into(), label: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Countdown {
    pub id: Id,
    pub title: String,
    /// 로컬 시각 "YYYY-MM-DDTHH:MM:SS"
    pub target: String,
    /// 만든(또는 마지막으로 목표를 바꾼) 시각 unix 초 — 진행 막대의 시작점
    #[serde(default)]
    pub since: i64,
    pub color: String,
    /// 목표 시각에 알림 창 + 소리
    #[serde(default = "yes")]
    pub notify: bool,
    #[serde(default = "cd_sound")]
    pub sound: String,
    /// 이미 울렸음 (목표를 바꾸면 다시 false)
    #[serde(default)]
    pub fired: bool,
    /// 사이드바 아래에 고정
    #[serde(default)]
    pub pinned: bool,
    /// 바탕 화면에 떠 있는 카드
    #[serde(default)]
    pub card: bool,
    #[serde(default)]
    pub card_pos: Option<[f32; 2]>,
}

fn cd_sound() -> String {
    "arpeggio".into()
}

impl Countdown {
    pub fn new(title: &str, target: chrono::NaiveDateTime, color: &str) -> Self {
        Countdown {
            id: new_id(),
            title: title.into(),
            target: fmt_target(target),
            since: crate::engine::unix_now(),
            color: color.into(),
            notify: true,
            sound: cd_sound(),
            fired: false,
            pinned: false,
            card: false,
            card_pos: None,
        }
    }
    pub fn target_dt(&self) -> Option<chrono::NaiveDateTime> {
        parse_target(&self.target)
    }
}

pub fn parse_target(s: &str) -> Option<chrono::NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M"))
        .ok()
}

pub fn fmt_target(d: chrono::NaiveDateTime) -> String {
    d.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// 디데이 세는 방식
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DdayMode {
    /// 그날까지 남은 날 — D-23 · D-Day · D+5
    Until,
    /// 그날부터 — 첫날 = 1일째 (기념일 · 사귄 날 · 금연)
    Since,
    /// 해마다 돌아오는 날 — 생일 · 결혼기념일 (다음 그날까지 D-n)
    Yearly,
}

impl DdayMode {
    pub const ALL: [DdayMode; 3] = [DdayMode::Until, DdayMode::Since, DdayMode::Yearly];
    pub fn label(self) -> &'static str {
        match self {
            DdayMode::Until => "남은 날",
            DdayMode::Since => "그날부터",
            DdayMode::Yearly => "매년",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            DdayMode::Until => "시험 · 여행 · 마감 — D-23, 당일 D-Day, 지나면 D+5",
            DdayMode::Since => "기념일 · 금연 — 그날을 1일째로 세고 100일 · 1주년을 알려 줍니다",
            DdayMode::Yearly => "생일 · 결혼기념일 — 해마다 다음 그날까지 셉니다",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Dday {
    pub id: Id,
    pub title: String,
    /// "YYYY-MM-DD"
    pub date: String,
    pub mode: DdayMode,
    pub color: String,
    /// 사이드바 아래에 고정
    #[serde(default)]
    pub pinned: bool,
    /// 그날(매년 = 해마다, 그날부터 = 100일·주년마다) 알림
    #[serde(default)]
    pub notify: bool,
    /// 알림 시각 "HH:MM"
    #[serde(default = "nine")]
    pub alert_time: String,
    /// 미리 알림 — 며칠 전 (예: [7, 1])
    #[serde(default)]
    pub remind: Vec<u32>,
    #[serde(default = "rem_sound")]
    pub sound: String,
    /// 이미 울린 알림 ("그날/며칠 전") — 같은 알림 두 번 안 울리게
    #[serde(default)]
    pub fired: Vec<String>,
    /// 바탕 화면에 떠 있는 카드
    #[serde(default)]
    pub card: bool,
    #[serde(default)]
    pub card_pos: Option<[f32; 2]>,
    /// 알림 목록 — 며칠 전 · 시각 · 켬/끔 (비어 있으면 옛 alert_time + remind 로 만듦)
    #[serde(default)]
    pub alerts: Vec<AlertRule>,
    /// 그날부터: 몇 일째마다 알릴지
    #[serde(default = "every100")]
    pub day_step: DayStep,
    /// 그날부터: 주년(1주년 · 2주년 …)도 알림
    #[serde(default = "yes")]
    pub yearly_ms: bool,
    /// true = 소리 한 번만, false = 끌 때까지 계속 (최대 10분)
    #[serde(default)]
    pub ring_once: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AlertRule {
    /// 며칠 전 (0 = 당일)
    pub before: u32,
    /// "HH:MM"
    pub time: String,
    #[serde(default = "yes")]
    pub on: bool,
}

impl AlertRule {
    pub fn new(before: u32, time: &str) -> Self {
        AlertRule { before, time: time.into(), on: true }
    }
    pub fn naive(&self) -> chrono::NaiveTime {
        chrono::NaiveTime::parse_from_str(&self.time, "%H:%M").unwrap_or_else(|_| chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap())
    }
}

/// 그날부터: 기념일 간격
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DayStep {
    Every100,
    Every1000,
    Off,
}

impl DayStep {
    pub fn step(self) -> Option<i64> {
        match self {
            DayStep::Every100 => Some(100),
            DayStep::Every1000 => Some(1000),
            DayStep::Off => None,
        }
    }
}

fn every100() -> DayStep {
    DayStep::Every100
}

fn nine() -> String {
    "09:00".into()
}

impl Dday {
    pub fn new(title: &str, date: chrono::NaiveDate, mode: DdayMode, color: &str) -> Self {
        Dday {
            id: new_id(),
            title: title.into(),
            date: date.format("%Y-%m-%d").to_string(),
            mode,
            color: color.into(),
            pinned: false,
            notify: true,
            alert_time: nine(),
            remind: vec![],
            sound: rem_sound(),
            fired: vec![],
            card: false,
            card_pos: None,
            alerts: vec![AlertRule::new(0, "09:00")],
            day_step: DayStep::Every100,
            yearly_ms: true,
            ring_once: false,
        }
    }
    /// 실제 알림 목록 — 새 형식(alerts)이 비어 있으면 옛 형식(alert_time + remind)에서
    pub fn rules(&self) -> Vec<AlertRule> {
        if !self.alerts.is_empty() {
            return self.alerts.clone();
        }
        let mut v = vec![AlertRule::new(0, &self.alert_time)];
        v.extend(self.remind.iter().filter(|n| **n > 0).map(|n| AlertRule::new(*n, &self.alert_time)));
        v
    }
    pub fn date_nd(&self) -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").ok()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Repeat {
    None,
    Daily,
    Weekdays,
    Weekly,
    Monthly,
}

impl Repeat {
    pub const ALL: [Repeat; 5] = [Repeat::None, Repeat::Daily, Repeat::Weekdays, Repeat::Weekly, Repeat::Monthly];
    pub fn label(self) -> &'static str {
        match self {
            Repeat::None => "안 함",
            Repeat::Daily => "매일",
            Repeat::Weekdays => "평일",
            Repeat::Weekly => "매주",
            Repeat::Monthly => "매월",
        }
    }
}

fn rep_none() -> Repeat {
    Repeat::None
}
fn rem_sound() -> String {
    "chime".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Reminder {
    pub id: Id,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// 로컬 시각 "YYYY-MM-DDTHH:MM" (None = 날짜 없음)
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default = "rep_none")]
    pub repeat: Repeat,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub done_at: Option<String>,
    #[serde(default = "rem_sound")]
    pub sound: String,
    #[serde(default)]
    pub important: bool,
    /// Microsoft To Do 작업 id · 목록 id
    #[serde(default)]
    pub todo_id: Option<String>,
    #[serde(default)]
    pub todo_list: Option<String>,
    /// 마지막 수정 (unix 초)
    #[serde(default)]
    pub modified: i64,
    /// To Do 와 마지막으로 맞춘 시각 (unix 초)
    #[serde(default)]
    pub synced: i64,
}

impl Reminder {
    pub fn new(title: &str, due: Option<String>) -> Self {
        Reminder {
            id: new_id(),
            title: title.into(),
            notes: String::new(),
            due,
            repeat: Repeat::None,
            done: false,
            done_at: None,
            sound: "chime".into(),
            important: false,
            todo_id: None,
            todo_list: None,
            modified: crate::engine::unix_now(),
            synced: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct TodoSettings {
    #[serde(default)]
    pub enabled: bool,
    /// Azure 앱 등록의 애플리케이션(클라이언트) ID
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub list_id: Option<String>,
    #[serde(default)]
    pub list_name: String,
    #[serde(default)]
    pub account: String,
}

pub const PALETTE: [&str; 10] = [
    "#5B8DEF", "#8B6CF0", "#2FBF9F", "#E8A33D", "#E5677A", "#3FB5E8", "#D46CC4", "#8E97A6", "#B08463", "#7BC86C",
];

impl Data {
    /// 처음 실행 — 예시 타이머 몇 개 (지우거나 고쳐서 쓰면 됨)
    pub fn seed() -> Self {
        let mut ramen = TimerDef::simple("라면 3분", 180, "#E8A33D");
        ramen.steps[0].sound = "arpeggio".into();
        ramen.style = Some(Style::Hourglass);
        let mut five = TimerDef::simple("5분 타이머", 300, "#2FBF9F");
        five.hotkey_toggle = "Ctrl+Alt+F9".into();
        five.hotkey_reset = "Ctrl+Alt+F10".into();
        let pomo = TimerDef::pomodoro("#E5677A");
        // 단계 루틴 예시 — 단계마다 다른 끝 소리, 단계마다 마지막 5초 카운트다운
        let st = |name: &str, secs: u32, sound: &str| Step { name: name.into(), secs, sound: sound.into() };
        let mut stretch = TimerDef::simple("스트레칭 루틴", 30, "#3FB5E8");
        stretch.steps = vec![
            st("목 스트레칭", 30, "bell-rise"),
            st("어깨 돌리기", 30, "bell-sixth"),
            st("허리 숙이기", 30, "bell-high"),
            st("다리 스트레칭", 30, "bell-fall"),
            st("마무리 호흡", 20, "arpeggio"),
        ];
        stretch.warn_secs = 5;
        stretch.warn_each = true;
        stretch.notify = true;
        Data {
            version: 1,
            timers: vec![pomo, stretch, five, ramen],
            alarms: vec![],
            settings: Settings::default(),
            reminders: vec![],
            todo_deleted: vec![],
            world: ["America/New_York", "Europe/London", "Asia/Tokyo"].iter().map(|t| WorldClock::new(t)).collect(),
            countdowns: vec![],
            ddays: vec![],
        }
    }
}

pub fn parse_color(hex: &str) -> [u8; 3] {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return [(v >> 16) as u8, (v >> 8) as u8, v as u8];
        }
    }
    [0x5B, 0x8D, 0xEF]
}
