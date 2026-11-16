//! 소리 — 내장 합성음(외부 파일 불필요) + 사용자 파일(wav/mp3/flac/ogg).
//! 재생마다 기본 출력 장치를 새로 열어 장치가 바뀌어도 따라간다. 반환되는 플래그로 중지.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SR: u32 = 48_000;

/// (id, 이름)
pub const BUILTIN: &[(&str, &str)] = &[
    ("chime", "차임"),
    ("bell", "맑은 벨"),
    ("bell-rise", "벨 · 올라가는 두 음"),
    ("bell-sixth", "벨 · 미라"),
    ("bell-fall", "벨 · 내려가는 두 음"),
    ("bell-high", "벨 · 높은 두 음"),
    ("bell-triple", "벨 · 세 음"),
    ("bell-low", "벨 · 낮은 두 번"),
    ("arpeggio", "완료 아르페지오"),
    ("marimba", "마림바"),
    ("digital", "디지털 비프"),
    ("classic", "클래식 알람"),
    ("soft", "부드러운 알림"),
    ("tick", "카운트다운 · 삐"),
    ("tick-soft", "카운트다운 · 톡"),
    ("tick-wood", "카운트다운 · 우드"),
    ("warn", "짧은 두 번"),
    ("countdown", "삐삐삐"),
];

pub fn name_of(id: &str) -> String {
    if id == "none" || id.is_empty() {
        return "없음".into();
    }
    if let Some(p) = id.strip_prefix("file:") {
        return std::path::Path::new(p).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| p.into());
    }
    BUILTIN.iter().find(|(i, _)| *i == id).map(|(_, n)| n.to_string()).unwrap_or_else(|| id.into())
}

pub struct PlayOpts {
    pub volume: f32,
    pub repeat: u32,
    /// 멈출 때까지 반복 (알람) — 최대 max
    pub looping: bool,
    pub max: Duration,
}

/// 재생 시작 → 중지 플래그
pub fn play(sound: &str, o: PlayOpts) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    if sound == "none" || sound.is_empty() {
        return stop;
    }
    let s2 = stop.clone();
    let sound = sound.to_string();
    let _ = std::thread::Builder::new().name("sound".into()).spawn(move || {
        if let Err(e) = run(&sound, &o, &s2) {
            crate::store::log(format!("sound '{sound}' failed: {e}"));
        }
    });
    stop
}

fn run(sound: &str, o: &PlayOpts, stop: &AtomicBool) -> Result<(), String> {
    let (_stream, handle) = rodio::OutputStream::try_default().map_err(|e| e.to_string())?;
    let sink = rodio::Sink::try_new(&handle).map_err(|e| e.to_string())?;
    sink.set_volume(o.volume.clamp(0.0, 1.0));
    let t0 = Instant::now();
    let mut n = 0u32;
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if !o.looping && n >= o.repeat.max(1) {
            break;
        }
        if o.looping && t0.elapsed() > o.max {
            break;
        }
        if n > 0 {
            sink.append(rodio::buffer::SamplesBuffer::new(1, SR, vec![0.0f32; (SR as f32 * if o.looping { 0.5 } else { 0.15 }) as usize]));
        }
        if let Some(path) = sound.strip_prefix("file:") {
            let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
            let dec = rodio::Decoder::new(std::io::BufReader::new(f)).map_err(|e| format!("{path}: {e}"))?;
            sink.append(dec);
        } else {
            sink.append(rodio::buffer::SamplesBuffer::new(1, SR, synth(sound)));
        }
        n += 1;
        while !sink.empty() {
            if stop.load(Ordering::Relaxed) {
                sink.stop();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }
    sink.stop();
    Ok(())
}

const C5: f32 = 523.25;
const D5: f32 = 587.33;
const E5: f32 = 659.25;
const G5: f32 = 783.99;
const A5: f32 = 880.00;
const C6: f32 = 1046.50;
const D6: f32 = 1174.66;
const E6: f32 = 1318.51;
const G6: f32 = 1567.98;

#[derive(Clone, Copy)]
enum Timbre {
    Bell,
    Marimba,
    Square,
    Soft,
    /// 아주 짧은 톡 (카운트다운)
    Click,
}

fn notes(id: &str) -> (Timbre, Vec<(f32, f32)>) {
    use Timbre::*;
    match id {
        "chime" => (Bell, vec![(E6, 0.0), (C6, 0.28)]),
        "bell" => (Bell, vec![(A5, 0.0)]),
        "bell-rise" => (Bell, vec![(G5, 0.0), (C6, 0.22)]),
        "bell-sixth" => (Bell, vec![(E5, 0.0), (A5, 0.22)]),
        "bell-fall" => (Bell, vec![(C6, 0.0), (G5, 0.22)]),
        "bell-high" => (Bell, vec![(A5, 0.0), (E6, 0.22)]),
        "bell-triple" => (Bell, vec![(D6, 0.0), (A5, 0.18), (D6, 0.36)]),
        "bell-low" => (Bell, vec![(D5, 0.0), (D5, 0.26)]),
        "arpeggio" => (Bell, vec![(C5, 0.0), (E5, 0.16), (G5, 0.32), (C6, 0.48)]),
        "marimba" => (Marimba, vec![(C6, 0.0), (E6, 0.14), (G6, 0.28), (E6, 0.42), (C6, 0.56)]),
        "digital" => (Square, vec![(2093.0, 0.0), (2093.0, 0.16), (2093.0, 0.32), (2093.0, 0.48)]),
        "classic" => (Square, (0..8).map(|i| (1760.0, i as f32 * 0.11)).collect()),
        "soft" => (Soft, vec![(C5, 0.0), (G5, 0.35), (E5, 0.7)]),
        "warn" => (Marimba, vec![(E6, 0.0), (E6, 0.16)]),
        "tick" => (Square, vec![(1568.0, 0.0)]),
        "tick-hi" => (Square, vec![(2093.0, 0.0)]),
        "tick-soft" => (Click, vec![(1318.5, 0.0)]),
        "tick-soft-hi" => (Click, vec![(1760.0, 0.0)]),
        "tick-wood" => (Click, vec![(784.0, 0.0)]),
        "tick-wood-hi" => (Click, vec![(1046.5, 0.0)]),
        "countdown" => (Square, vec![(1318.5, 0.0), (1318.5, 1.0), (1318.5, 2.0)]),
        _ => (Bell, vec![(A5, 0.0)]),
    }
}

pub fn synth(id: &str) -> Vec<f32> {
    let (timbre, ns) = notes(id);
    let tail = match timbre {
        Timbre::Bell => 1.6,
        Timbre::Marimba => 0.7,
        Timbre::Square => 0.09,
        Timbre::Soft => 1.4,
        Timbre::Click => 0.12,
    };
    let len = ns.iter().map(|n| n.1).fold(0.0, f32::max) + tail;
    let total = (len * SR as f32) as usize;
    let mut buf = vec![0.0f32; total];
    for &(f, at) in &ns {
        let start = (at * SR as f32) as usize;
        let end = match timbre {
            Timbre::Square => (start + (0.07 * SR as f32) as usize).min(total),
            _ => total,
        };
        for i in start..end {
            let t = (i - start) as f32 / SR as f32;
            let w = std::f32::consts::TAU * f * t;
            let v = match timbre {
                Timbre::Bell => {
                    let a = (t / 0.004).min(1.0);
                    a * (1.0 * (-2.4 * t).exp() * w.sin()
                        + 0.32 * (-3.6 * t).exp() * (2.0 * w).sin()
                        + 0.16 * (-5.0 * t).exp() * (2.76 * w).sin()
                        + 0.05 * (-8.0 * t).exp() * (5.4 * w).sin())
                }
                Timbre::Marimba => {
                    let a = (t / 0.002).min(1.0);
                    a * ((-7.0 * t).exp() * w.sin() + 0.25 * (-18.0 * t).exp() * (4.0 * w).sin())
                }
                Timbre::Square => {
                    let a = (t / 0.003).min(1.0) * ((0.07 - t) / 0.005).clamp(0.0, 1.0);
                    0.35 * a * (w.sin() + 0.33 * (3.0 * w).sin() + 0.2 * (5.0 * w).sin())
                }
                Timbre::Soft => {
                    let a = (t / 0.03).min(1.0);
                    a * (-2.0 * t).exp() * (w.sin() + 0.12 * (2.0 * w).sin())
                }
                Timbre::Click => {
                    let a = (t / 0.002).min(1.0);
                    1.6 * a * (-38.0 * t).exp() * (w.sin() + 0.3 * (3.01 * w).sin())
                }
            };
            buf[i] += 0.28 * v;
        }
    }
    let fade = (0.01 * SR as f32) as usize;
    for k in 0..fade.min(total) {
        buf[total - 1 - k] *= k as f32 / fade as f32;
    }
    let peak = buf.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.95 {
        buf.iter_mut().for_each(|v| *v *= 0.95 / peak);
    }
    buf
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_builtins_synthesize() {
        for (id, _) in super::BUILTIN {
            let b = super::synth(id);
            assert!(b.len() > 1000, "{id}");
            assert!(b.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{id}");
        }
    }
}
