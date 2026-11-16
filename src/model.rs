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
