//! 타이머 하나의 실행 상태 (단계 체인). 시간 = Instant 기준, 넘친 시간은 다음 단계로 이월.
//! 옵션: 끝나기 전 예고(warn) · 단계마다 멈추고 기다리기(auto_advance=false) · 반복(repeat, 회차 loops)
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Running,
    Paused,
    Done,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Running => "running",
            Phase::Paused => "paused",
            Phase::Done => "done",
        }
    }
    pub fn active(self) -> bool {
        matches!(self, Phase::Running | Phase::Paused)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// 대기·완료 → 시작, 실행 → 일시정지, 일시정지 → 이어서
    Toggle,
    /// 대기·완료일 때만 시작 (자동화용)
    Start,
    Pause,
    Resume,
    Reset,
    Skip,
    /// 그 단계로 이동. 대기 중이면 그 단계에서 「대기」(▶ 로 시작)
    Goto(usize),
}

impl Cmd {
    pub fn parse(s: &str) -> Option<Cmd> {
        Some(match s {
            "toggle" => Cmd::Toggle,
            "start" => Cmd::Start,
            "pause" => Cmd::Pause,
            "resume" => Cmd::Resume,
            "reset" => Cmd::Reset,
            "skip" => Cmd::Skip,
            _ => return None,
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Started,
    Paused,
    Resumed,
    Reset,
    StepEnded(usize),
    Skipped(usize),
    Finished,
    Looped,
    /// 끝나기 N초 전 (단계 idx)
    /// 끝나기 전 카운트다운 — (단계, 남은 초). 마지막 N초 동안 1초마다
    Warn(usize, u32),
    /// 단계마다 멈춤 — 다음 단계(idx) 시작 대기
    Waiting(usize),
    Jumped(usize),
}

pub const DONE_HOLD: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opts {
    pub repeat: bool,
    /// 0 = 끔
    pub warn: Duration,
    /// true = 단계마다, false = 타이머 전체 끝나기 전(마지막 단계)만
    pub warn_each: bool,
    pub auto_advance: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { repeat: false, warn: Duration::ZERO, warn_each: false, auto_advance: true }
    }
}

pub struct Engine {
    pub durations: Vec<Duration>,
    pub opts: Opts,
    pub phase: Phase,
    pub idx: usize,
    acc: Duration,
    since: Option<Instant>,
    done_at: Option<Instant>,
    /// 카운트다운에서 마지막으로 울린 남은 초 (같은 초에 두 번 안 울리게)
    last_tick: Option<u32>,
    /// 단계마다 멈춤으로 다음 단계 시작을 기다리는 중
    pub waiting: bool,
    /// 반복 회차 (처음 = 0)
    pub loops: u32,
    /// 마지막으로 처음부터 시작한 시각 (unix 초)
    pub chain_start_unix: i64,
    /// 단계에 들어갈 때마다 +1 (모래시계 뒤집기 애니메이션 신호)
    pub epoch: u64,
}

/// PC 연동용 실행 상태 — Instant 대신 실제 시각(ms)으로
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Snap {
    pub phase: String,
    pub idx: usize,
    pub acc_ms: u64,
    /// 지금 단계가 (다시) 흐르기 시작한 시각 — 일시정지면 None
    pub since_ms: Option<i64>,
    pub done_ms: Option<i64>,
    pub waiting: bool,
    pub loops: u32,
    pub chain_start: i64,
}

impl Phase {
    pub fn parse(s: &str) -> Phase {
        match s {
            "running" => Phase::Running,
            "paused" => Phase::Paused,
            "done" => Phase::Done,
            _ => Phase::Idle,
        }
    }
}

pub fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Engine {
    pub fn new(durations: Vec<Duration>, repeat: bool) -> Self {
        Self::with_opts(durations, Opts { repeat, ..Default::default() })
    }

    pub fn with_opts(durations: Vec<Duration>, opts: Opts) -> Self {
        Engine {
            durations: if durations.is_empty() { vec![Duration::from_secs(60)] } else { durations },
            opts,
            phase: Phase::Idle,
            idx: 0,
            acc: Duration::ZERO,
            since: None,
            done_at: None,
            last_tick: None,
            waiting: false,
            loops: 0,
            chain_start_unix: 0,
            epoch: 0,
        }
    }

    /// 편집 반영. 실행 중이면 지금 단계 위치는 유지(범위 밖이면 마지막 단계로)
    pub fn set_steps(&mut self, durations: Vec<Duration>, opts: Opts) {
        if durations.is_empty() {
            return;
        }
        self.durations = durations;
        self.opts = opts;
        if self.idx >= self.durations.len() {
            self.idx = self.durations.len() - 1;
        }
    }

    pub fn snapshot(&self, now: Instant, now_ms: i64) -> Snap {
        let to_ms = |t: Instant| now_ms - now.saturating_duration_since(t).as_millis() as i64;
        Snap {
            phase: self.phase.as_str().into(),
            idx: self.idx,
            acc_ms: self.acc.as_millis() as u64,
            since_ms: self.since.map(to_ms),
            done_ms: self.done_at.map(to_ms),
            waiting: self.waiting,
            loops: self.loops,
            chain_start: self.chain_start_unix,
        }
    }

    /// 다른 PC 의 실행 상태로 맞춤 (시각은 이미 이 PC 시계로 바꿔서 줌)
    pub fn restore(&mut self, s: &Snap, now: Instant, now_ms: i64) {
        let to_inst = |t: i64| now.checked_sub(Duration::from_millis((now_ms - t).max(0) as u64)).unwrap_or(now);
        self.phase = Phase::parse(&s.phase);
        self.epoch += 1;
        self.idx = s.idx.min(self.durations.len() - 1);
        self.acc = Duration::from_millis(s.acc_ms);
        self.since = if self.phase == Phase::Running { Some(s.since_ms.map(to_inst).unwrap_or(now)) } else { None };
        self.done_at = if self.phase == Phase::Done { Some(s.done_ms.map(to_inst).unwrap_or(now)) } else { None };
        self.waiting = s.waiting;
        self.loops = s.loops;
        self.chain_start_unix = s.chain_start;
        self.last_tick = None;
    }

    pub fn step_elapsed(&self, now: Instant) -> Duration {
        self.acc + self.since.map(|s| now.saturating_duration_since(s)).unwrap_or_default()
    }

    pub fn step_remaining(&self, now: Instant) -> Duration {
        match self.phase {
            Phase::Idle => self.durations[0],
            Phase::Done => Duration::ZERO,
            _ => self.durations[self.idx].saturating_sub(self.step_elapsed(now)),
        }
    }

    pub fn total(&self) -> Duration {
        self.durations.iter().sum()
    }

    pub fn total_remaining(&self, now: Instant) -> Duration {
        match self.phase {
            Phase::Idle => self.total(),
            Phase::Done => Duration::ZERO,
            _ => self.step_remaining(now) + self.durations[self.idx + 1..].iter().sum::<Duration>(),
        }
    }

    pub fn step_fraction(&self, now: Instant) -> f32 {
        match self.phase {
            Phase::Idle => 0.0,
            Phase::Done => 1.0,
            _ => {
                let d = self.durations[self.idx].as_secs_f32().max(0.001);
                (self.step_elapsed(now).as_secs_f32() / d).clamp(0.0, 1.0)
            }
        }
    }

    fn enter_step(&mut self, idx: usize, since: Option<Instant>) {
        self.epoch += 1;
        self.idx = idx;
        self.acc = Duration::ZERO;
        self.since = since;
        self.last_tick = None;
    }

    fn begin(&mut self, now: Instant) {
        self.phase = Phase::Running;
        self.enter_step(0, Some(now));
        self.done_at = None;
        self.waiting = false;
        self.loops = 0;
        self.chain_start_unix = unix_now();
    }

    pub fn command(&mut self, cmd: Cmd, now: Instant) -> Vec<Event> {
        let mut ev = Vec::new();
        match (cmd, self.phase) {
            (Cmd::Toggle | Cmd::Start, Phase::Idle | Phase::Done) => {
                self.begin(now);
                ev.push(Event::Started);
            }
            (Cmd::Toggle | Cmd::Pause, Phase::Running) => {
                self.acc = self.step_elapsed(now);
                self.since = None;
                self.phase = Phase::Paused;
                ev.push(Event::Paused);
            }
            (Cmd::Toggle | Cmd::Resume, Phase::Paused) => {
                self.since = Some(now);
                self.phase = Phase::Running;
                self.waiting = false;
                ev.push(Event::Resumed);
            }
            (Cmd::Reset, p) if p != Phase::Idle => {
                self.phase = Phase::Idle;
                self.enter_step(0, None);
                self.done_at = None;
                self.waiting = false;
                ev.push(Event::Reset);
            }
            (Cmd::Goto(i), p) if i < self.durations.len() => {
                match p {
                    Phase::Running => self.enter_step(i, Some(now)),
                    Phase::Paused => self.enter_step(i, None),
                    Phase::Idle | Phase::Done => {
                        self.enter_step(i, None);
                        self.phase = Phase::Paused;
                        self.waiting = true;
                        self.done_at = None;
                        self.loops = 0;
                    }
                }
                ev.push(Event::Jumped(i));
            }
            (Cmd::Skip, Phase::Running | Phase::Paused) => {
                ev.push(Event::Skipped(self.idx));
                if self.idx + 1 >= self.durations.len() {
                    self.finish(now, Duration::ZERO, &mut ev);
                } else {
                    let since = if self.phase == Phase::Running { Some(now) } else { None };
                    self.enter_step(self.idx + 1, since);
                }
            }
            _ => {}
        }
        ev
    }

    fn finish(&mut self, now: Instant, carry: Duration, ev: &mut Vec<Event>) {
        if self.opts.repeat {
            self.loops += 1;
            ev.push(Event::Looped);
            if self.opts.auto_advance {
                self.enter_step(0, Some(now - carry));
                self.phase = Phase::Running;
            } else {
                self.enter_step(0, None);
                self.phase = Phase::Paused;
                self.waiting = true;
                ev.push(Event::Waiting(0));
            }
            return;
        }
        self.phase = Phase::Done;
        self.enter_step(self.durations.len() - 1, None);
        self.done_at = Some(now);
        ev.push(Event::Finished);
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        let mut ev = Vec::new();
        if self.phase == Phase::Running {
            let mut guard = 0;
            while self.phase == Phase::Running && guard < 10_000 {
                guard += 1;
                let el = self.step_elapsed(now);
                let d = self.durations[self.idx];
                if el < d {
                    break;
                }
                ev.push(Event::StepEnded(self.idx));
                let carry = el - d;
                if self.idx + 1 >= self.durations.len() {
                    self.finish(now, carry, &mut ev);
                } else if self.opts.auto_advance {
                    self.enter_step(self.idx + 1, Some(now - carry));
                } else {
                    self.enter_step(self.idx + 1, None);
                    self.phase = Phase::Paused;
                    self.waiting = true;
                    ev.push(Event::Waiting(self.idx));
                }
            }
            // 끝나기 전 예고
            let last = self.idx + 1 == self.durations.len();
            if self.phase == Phase::Running && !self.opts.warn.is_zero() && (self.opts.warn_each || last) {
                let d = self.durations[self.idx];
                // 단계가 카운트다운보다 짧으면 건너뜀 (단계 내내 삐삐대지 않게)
                if d > self.opts.warn {
                    let s = self.step_remaining(now).as_secs_f32().ceil() as u32;
                    let w = self.opts.warn.as_secs() as u32;
                    if s >= 1 && s <= w && self.last_tick != Some(s) {
                        self.last_tick = Some(s);
                        ev.push(Event::Warn(self.idx, s));
                    }
                }
            }
        } else if self.phase == Phase::Done {
            if let Some(t) = self.done_at {
                if now.saturating_duration_since(t) >= DONE_HOLD {
                    self.phase = Phase::Idle;
                    self.enter_step(0, None);
                    self.done_at = None;
                    ev.push(Event::Reset);
                }
            }
        }
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &[u64]) -> Vec<Duration> {
        s.iter().map(|x| Duration::from_secs(*x)).collect()
    }
    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn chain_runs_through_with_carry() {
        let mut e = Engine::new(d(&[90, 90, 60]), false);
        let t0 = Instant::now();
        e.command(Cmd::Start, t0);
        assert_eq!(e.tick(at(t0, 90_250)), vec![Event::StepEnded(0)]);
        assert!(e.step_elapsed(at(t0, 90_250)) >= Duration::from_millis(249));
        assert_eq!(e.tick(at(t0, 241_000)), vec![Event::StepEnded(1), Event::StepEnded(2), Event::Finished]);
        assert_eq!(e.phase, Phase::Done);
        assert_eq!(e.tick(at(t0, 250_000)), vec![Event::Reset]);
        assert_eq!(e.phase, Phase::Idle);
    }

    #[test]
    fn start_is_idempotent_and_pause_keeps_time() {
        let mut e = Engine::new(d(&[90, 90, 60]), false);
        let t0 = Instant::now();
        e.command(Cmd::Start, t0);
        let s = e.chain_start_unix;
        assert!(e.command(Cmd::Start, at(t0, 5_000)).is_empty());
        assert_eq!(e.chain_start_unix, s);
        e.command(Cmd::Toggle, at(t0, 10_000));
        assert!(e.command(Cmd::Start, at(t0, 20_000)).is_empty());
        assert_eq!(e.step_remaining(at(t0, 500_000)), Duration::from_secs(80));
        e.command(Cmd::Toggle, at(t0, 30_000));
        assert_eq!(e.total_remaining(at(t0, 40_000)), Duration::from_secs(70 + 150));
    }

    #[test]
    fn repeat_loops_and_counts() {
        let mut e = Engine::new(d(&[10, 5]), true);
        let t0 = Instant::now();
        e.command(Cmd::Start, t0);
        assert_eq!(e.tick(at(t0, 16_000)), vec![Event::StepEnded(0), Event::StepEnded(1), Event::Looped]);
        assert_eq!((e.phase, e.idx, e.loops), (Phase::Running, 0, 1));
        assert_eq!(e.step_remaining(at(t0, 16_000)), Duration::from_secs(9));
    }

    #[test]
    fn skip_and_edit() {
        let mut e = Engine::new(d(&[90, 90, 60]), false);
        let t0 = Instant::now();
        e.command(Cmd::Start, t0);
        e.command(Cmd::Skip, at(t0, 3_000));
        e.command(Cmd::Skip, at(t0, 4_000));
        assert_eq!(e.idx, 2);
        e.set_steps(d(&[30]), Opts::default());
        assert_eq!(e.idx, 0);
        e.command(Cmd::Reset, t0);
        assert_eq!(e.total_remaining(t0), Duration::from_secs(30));
    }

    #[test]
    fn warn_last_only_and_each() {
        let o = Opts { warn: Duration::from_secs(10), ..Default::default() };
        let mut e = Engine::with_opts(d(&[30, 30]), o);
        let t0 = Instant::now();
        e.command(Cmd::Start, t0);
        assert!(e.tick(at(t0, 25_000)).is_empty()); // 1단계 끝 전 — 마지막 단계만이라 없음
        assert_eq!(e.tick(at(t0, 30_100)), vec![Event::StepEnded(0)]);
        assert!(e.tick(at(t0, 49_900)).is_empty());
        assert_eq!(e.tick(at(t0, 50_050)), vec![Event::Warn(1, 10)]);
        assert!(e.tick(at(t0, 50_500)).is_empty()); // 같은 초에는 한 번
        assert_eq!(e.tick(at(t0, 51_050)), vec![Event::Warn(1, 9)]);
        assert_eq!(e.tick(at(t0, 59_050)), vec![Event::Warn(1, 1)]);
        assert_eq!(e.tick(at(t0, 60_050)), vec![Event::StepEnded(1), Event::Finished]); // 끝은 끝 소리
        let mut e = Engine::with_opts(d(&[30, 30, 5]), Opts { warn_each: true, ..o });
        e.command(Cmd::Start, t0);
        assert_eq!(e.tick(at(t0, 20_050)), vec![Event::Warn(0, 10)]);
        assert_eq!(e.tick(at(t0, 50_050)), vec![Event::StepEnded(0), Event::Warn(1, 10)]);
        assert_eq!(e.tick(at(t0, 60_000)), vec![Event::StepEnded(1)]); // 5초 단계 < 예고 10초 → 건너뜀
    }

    #[test]
    fn goto_from_idle_waits_without_new_start() {
        let mut e = Engine::new(d(&[10, 5, 20]), true);
        let t0 = Instant::now();
        assert_eq!(e.command(Cmd::Goto(2), t0), vec![Event::Jumped(2)]);
        assert_eq!((e.phase, e.idx, e.waiting, e.chain_start_unix), (Phase::Paused, 2, true, 0));
        e.command(Cmd::Toggle, at(t0, 1_000));
        assert_eq!(e.step_remaining(at(t0, 6_000)), Duration::from_secs(15));
        e.command(Cmd::Goto(0), at(t0, 6_000));
        assert_eq!((e.phase, e.idx), (Phase::Running, 0));
        assert!(e.command(Cmd::Goto(9), t0).is_empty());
    }

    #[test]
    fn wait_between_steps() {
        let o = Opts { auto_advance: false, repeat: true, ..Default::default() };
        let mut e = Engine::with_opts(d(&[10, 5]), o);
        let t0 = Instant::now();
        e.command(Cmd::Start, t0);
        assert_eq!(e.tick(at(t0, 12_000)), vec![Event::StepEnded(0), Event::Waiting(1)]);
        assert_eq!((e.phase, e.idx, e.waiting), (Phase::Paused, 1, true));
        assert_eq!(e.step_remaining(at(t0, 99_000)), Duration::from_secs(5)); // 기다리는 동안 안 줄어듦
        e.command(Cmd::Toggle, at(t0, 20_000));
        assert!(!e.waiting);
        assert_eq!(e.tick(at(t0, 25_000)), vec![Event::StepEnded(1), Event::Looped, Event::Waiting(0)]);
        assert_eq!((e.idx, e.loops, e.phase), (0, 1, Phase::Paused));
    }

    #[test]
    fn snapshot_roundtrip_wall_clock() {
        let t0 = Instant::now();
        let mut a = Engine::with_opts(vec![Duration::from_secs(60), Duration::from_secs(30)], Opts::default());
        a.command(Cmd::Start, t0);
        let now = t0 + Duration::from_secs(20);
        let snap = a.snapshot(now, 1_000_000);
        assert_eq!(snap.since_ms, Some(1_000_000 - 20_000));
        // 다른 PC: 같은 실제 시각이면 남은 시간이 같음
        let mut b = Engine::with_opts(vec![Duration::from_secs(60), Duration::from_secs(30)], Opts::default());
        let nb = Instant::now();
        b.restore(&snap, nb, 1_000_000);
        assert_eq!(b.phase, Phase::Running);
        assert_eq!(b.step_remaining(nb).as_secs(), 40);
        // 일시정지 상태도
        a.command(Cmd::Pause, now);
        let s2 = a.snapshot(now + Duration::from_secs(5), 1_005_000);
        b.restore(&s2, nb, 1_005_000);
        assert_eq!((b.phase, b.step_remaining(nb + Duration::from_secs(100)).as_secs()), (Phase::Paused, 40));
    }

}
