//! 끝나기 전 화면 효과 — 예고음(마지막 N초)이 울리는 동안 카드 · 메인 창 · 화면 테두리로 알려 줌
//! 박자: 남은 시간이 정수 초를 넘을 때마다(= 「삐」 소리) 번쩍. 끝에 가까울수록 세짐.
use super::theme::*;
use crate::core::{Core, TimerView};
use crate::engine::Phase;
use crate::model::{Id, TimerDef, WarnFx};
use eframe::egui::{self, emath::TSTransform, Color32, Painter, Pos2, Rect, Stroke};
use std::sync::Arc;
use std::time::Instant;

/// 미리 보기 길이 (초)
const PREVIEW: f32 = 5.0;

#[derive(Clone, Debug)]
pub struct Fx {
    pub kinds: Vec<WarnFx>,
    /// 마지막 「삐」 직후 번쩍임 세기 0..1
    pub beat: f32,
    /// 마지막 「삐」 뒤 지난 초 0..1
    pub since: f32,
    /// 끝에 가까운 정도 0..1
    pub urgency: f32,
    /// 숨 쉬듯 0..1 (테두리 빛)
    pub breath: f32,
    pub color: Color32,
}

impl Fx {
    pub fn has(&self, k: WarnFx) -> bool {
        self.kinds.contains(&k)
    }
}

/// 미리 보기 중인가 — (남은 초, 효과 목록)
fn preview(core: &Core, id: Id) -> Option<(f32, Vec<WarnFx>)> {
    let g = core.fx_preview.lock().unwrap();
    let (pid, at, kinds) = g.as_ref()?;
    let left = PREVIEW - at.elapsed().as_secs_f32();
    (*pid == id && left > 0.0).then(|| (left, kinds.clone()))
}

/// 지금 이 타이머가 「끝나기 전」 구간인가 — 엔진의 예고음 조건과 같게
pub fn warn_left(def: &TimerDef, v: &TimerView) -> Option<f32> {
    if v.phase != Phase::Running || def.warn_secs == 0 || def.warn_fx.is_empty() {
        return None;
    }
    let last = v.idx + 1 >= v.n;
    if !(def.warn_each || last) {
        return None;
    }
    let d = *v.step_secs.get(v.idx)?;
    if d <= def.warn_secs {
        return None; // 단계가 카운트다운보다 짧으면 (예고음도 안 울림)
    }
    let rem = v.step_rem.as_secs_f32();
    (rem > 0.0 && rem <= def.warn_secs as f32).then_some(rem)
}

pub fn active(core: &Core, def: &TimerDef, v: &TimerView, time: f64) -> Option<Fx> {
    let (rem, warn, kinds) = match preview(core, def.id) {
        Some((left, kinds)) => (left, PREVIEW, kinds),
        None => (warn_left(def, v)?, def.warn_secs as f32, def.warn_fx.clone()),
    };
    // 정수 초를 넘은 직후 = 「삐」
    let since = 1.0 - rem.fract();
    let since = if since >= 1.0 { 0.0 } else { since };
    let urgency = (1.0 - rem / warn.max(1.0)).clamp(0.0, 1.0);
    let beat = (1.0 - since / (0.32 + 0.15 * urgency)).clamp(0.0, 1.0);
    let breath = ((time as f32 * std::f32::consts::TAU * (1.2 + 1.3 * urgency)).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
    Some(Fx { kinds, beat, since, urgency, breath, color: color_of(&def.color) })
}

/// 카드를 꺼 뒀어도 지금 띄워야 하나 (「카드 띄우기」 · 미리 보기)
pub fn show_card(core: &Core, def: &TimerDef, v: &TimerView) -> bool {
    if preview(core, def.id).map(|p| p.1.contains(&WarnFx::ShowCard)).unwrap_or(false) {
        return true;
    }
    def.warn_fx.contains(&WarnFx::ShowCard) && warn_left(def, v).is_some()
}

/// 지금 효과 중인 타이머가 있나 (다시 그리기 간격)
pub fn any_live(core: &Core, defs: &[TimerDef], views: &[TimerView]) -> bool {
    core.fx_preview.lock().unwrap().as_ref().map(|p| p.1.elapsed().as_secs_f32() < PREVIEW).unwrap_or(false)
        || defs.iter().any(|d| views.iter().find(|v| v.id == d.id).and_then(|v| warn_left(d, v)).is_some())
}

pub enum Area {
    Round(Rect, f32),
    Circle(Pos2, f32),
}

/// 카드 위에 덧그리기 — 깜박임 · 점점 빨갛게 · 테두리 빛
pub fn overlay(p: &Painter, area: Area, fx: &Fx, pc: &Pal, k: f32) {
    let red = pc.red;
    let hot = lerp(fx.color, red, 0.55 * fx.urgency);
    let fill = |c: Color32| {
        match &area {
            Area::Round(r, rad) => {
                p.rect_filled(*r, cr(*rad), c);
            }
            Area::Circle(c0, rad) => {
                p.circle_filled(*c0, *rad, c);
            }
        }
    };
    if fx.has(WarnFx::Tint) {
        fill(red.gamma_multiply(0.10 + 0.30 * fx.urgency));
    }
    if fx.has(WarnFx::Blink) && fx.beat > 0.0 {
        fill(hot.gamma_multiply((0.30 + 0.30 * fx.urgency) * fx.beat));
    }
    if fx.has(WarnFx::Pulse) {
        let a = 0.35 + 0.65 * fx.breath.max(fx.beat);
        let w = (2.0 + 3.5 * fx.urgency) * k;
        for (i, alpha) in [(0.0, 1.0), (1.0, 0.45), (2.0, 0.2)] {
            let s = Stroke::new(w, hot.gamma_multiply(a * alpha));
            let inset = w * (0.5 + i * 1.1);
            match &area {
                Area::Round(r, rad) => {
                    p.rect_stroke(r.shrink(inset), cr((*rad - inset).max(0.0)), s, egui::StrokeKind::Middle);
                }
                Area::Circle(c0, rad) => {
                    p.circle_stroke(*c0, rad - inset, s);
                }
            }
        }
    }
}

/// 흔들림 · 튀어오름 — 카드 그림 전체를 움직임 (창 위치는 그대로)
pub fn transform(fx: &Fx, rect: Rect, k: f32) -> TSTransform {
    let mut t = TSTransform::IDENTITY;
    if fx.has(WarnFx::Pop) && fx.beat > 0.0 {
        let s = 1.0 + (0.035 + 0.03 * fx.urgency) * fx.beat;
        let c = rect.center().to_vec2();
        t = TSTransform::new(c - c * s, s);
    }
    if fx.has(WarnFx::Shake) {
        let decay = (1.0 - fx.since / 0.45).clamp(0.0, 1.0);
        let dx = (fx.since * 55.0).sin() * (3.0 + 3.0 * fx.urgency) * k * decay;
        t = TSTransform::from_translation(egui::vec2(dx, 0.0)) * t;
    }
    t
}

static EDGE_SHAPE: std::sync::Mutex<Option<super::drag::Shape>> = std::sync::Mutex::new(None);

/// 「화면 테두리」 — 모니터 가장자리 띠 (클릭은 통과, 다른 창 포커스 안 뺏음)
pub fn edge(ctx: &egui::Context, core: &Arc<Core>, defs: &[TimerDef], views: &[TimerView], pc: &Pal) {
    let time = ctx.input(|i| i.time);
    let best = defs
        .iter()
        .filter_map(|d| views.iter().find(|v| v.id == d.id).and_then(|v| active(core, d, v, time)))
        .filter(|f| f.has(WarnFx::Edge))
        .max_by(|a, b| a.urgency.partial_cmp(&b.urgency).unwrap_or(std::cmp::Ordering::Equal));
    let Some(fx) = best else {
        *EDGE_SHAPE.lock().unwrap() = None;
        return;
    };
    let (l, t, r, b) = core.work_area;
    let title = "Tempo 화면 테두리";
    let builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_position([l, t])
        .with_inner_size([r - l, b - t])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_active(false)
        .with_mouse_passthrough(true)
        .with_window_level(egui::WindowLevel::AlwaysOnTop);
    let pc = *pc;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("edge-fx"), builder, |ctx, _| {
        let ppp = ctx.pixels_per_point();
        let px = |v: f32| (v * ppp).round() as i32;
        let thick = 12.0 + 6.0 * fx.urgency;
        let shape = super::drag::Shape::Frame { w: px(r - l), h: px(b - t), t: px(thick) };
        let mut last = EDGE_SHAPE.lock().unwrap();
        if *last != Some(shape) && super::drag::set_shape(title, shape) {
            *last = Some(shape);
        }
        drop(last);
        egui::CentralPanel::default().frame(egui::Frame::default()).show(ctx, |ui| {
            let full = ui.max_rect();
            let hot = lerp(fx.color, pc.red, 0.55 * fx.urgency);
            let dim = lerp(Color32::BLACK, hot, 0.35);
            let a = fx.beat.max(0.45 * fx.breath);
            ui.painter().rect_filled(full, cr(0.0), lerp(dim, hot, a));
        });
    });
}

/// 편집 창 「미리 보기」
pub fn start_preview(core: &Core, id: Id, kinds: Vec<WarnFx>) {
    *core.fx_preview.lock().unwrap() = Some((id, Instant::now(), kinds));
    core.repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn view(phase: Phase, idx: usize, steps: &[u32], rem: f32) -> TimerView {
        TimerView {
            id: 1,
            name: "t".into(),
            color: "#5B8DEF".into(),
            phase,
            idx,
            n: steps.len(),
            step_name: String::new(),
            next_name: None,
            step_rem: Duration::from_secs_f32(rem),
            total_rem: Duration::from_secs_f32(rem),
            total: Duration::from_secs(steps.iter().map(|x| *x as u64).sum()),
            frac: 0.0,
            step_secs: steps.to_vec(),
            chain_start_unix: 0,
            loops: 0,
            waiting: false,
            epoch: 0,
            done_for: None,
        }
    }

    #[test]
    fn warn_window_matches_beep_rules() {
        let mut d = TimerDef::simple("t", 60, "#5B8DEF");
        d.warn_secs = 10;
        assert!(warn_left(&d, &view(Phase::Running, 0, &[60], 5.3)).is_some());
        assert!(warn_left(&d, &view(Phase::Running, 0, &[60], 12.0)).is_none());
        assert!(warn_left(&d, &view(Phase::Paused, 0, &[60], 5.0)).is_none());
        // 단계 여러 개: 「전체 끝」이면 마지막 단계만
        assert!(warn_left(&d, &view(Phase::Running, 0, &[60, 60], 5.0)).is_none());
        assert!(warn_left(&d, &view(Phase::Running, 1, &[60, 60], 5.0)).is_some());
        d.warn_each = true;
        assert!(warn_left(&d, &view(Phase::Running, 0, &[60, 60], 5.0)).is_some());
        // 카운트다운보다 짧은 단계 · 효과 없음 · 예고 끔
        assert!(warn_left(&d, &view(Phase::Running, 0, &[8, 60], 5.0)).is_none());
        d.warn_fx.clear();
        assert!(warn_left(&d, &view(Phase::Running, 1, &[60, 60], 5.0)).is_none());
        d.warn_fx = crate::model::default_fx();
        d.warn_secs = 0;
        assert!(warn_left(&d, &view(Phase::Running, 1, &[60, 60], 5.0)).is_none());
    }
}

