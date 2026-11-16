//! 디자인 스타일 공용 그림 — 원형 고리 · LCD 숫자 · 채우기 · 표시값 계산
use super::theme::*;
use crate::core::TimerView;
use crate::engine::Phase;
use crate::model::{Style, TimerDef};
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape, Stroke};
use std::time::Duration;

/// 실제로 쓸 스타일 (타이머 지정 > 설정)
pub fn style_of(def: &TimerDef, global: Style) -> Style {
    def.style.unwrap_or(global)
}

/// 화면에 보일 값들
pub struct Disp {
    pub text: String,
    pub dcol: Color32,
    /// 지금 단위(여러 단계면 지금 단계, 하나면 전체)의 남은 비율 1→0
    pub rem_frac: f32,
    /// 전체 진행 0→1
    pub done_total: f32,
    pub status: String,
    pub status_col: Color32,
    /// 「세션 1 / 4」 · 「단계 2 / 7」 · 「끝 15:05」
    pub sub: String,
    pub color: Color32,
}

pub fn pomo_session(def: &TimerDef, idx: usize) -> Option<(u32, u32)> {
    let pm = def.pomodoro?;
    let works_before = def.steps.iter().take(idx).filter(|s| s.name.starts_with("집중")).count() as u32;
    let cur_is_work = def.steps.get(idx).map(|s| s.name.starts_with("집중")).unwrap_or(false);
    Some(((works_before + cur_is_work as u32).max(1), pm.every.max(1)))
}

pub fn disp(def: &TimerDef, v: &TimerView, p: &Pal) -> Disp {
    let multi = def.steps.len() > 1;
    let color = color_of(&def.color);
    let unit_rem = if multi { v.step_rem } else { v.total_rem };
    let unit_total = if multi { Duration::from_secs(*v.step_secs.get(v.idx).unwrap_or(&1) as u64) } else { v.total };
    let rem_frac = match v.phase {
        Phase::Idle => 1.0,
        Phase::Done => 0.0,
        _ => (unit_rem.as_secs_f32() / unit_total.as_secs_f32().max(0.001)).clamp(0.0, 1.0),
    };
    let done_total = match v.phase {
        Phase::Idle => 0.0,
        Phase::Done => 1.0,
        _ => (1.0 - v.total_rem.as_secs_f32() / v.total.as_secs_f32().max(0.001)).clamp(0.0, 1.0),
    };
    let idle_show = if def.pomodoro.is_some() { Duration::from_secs(*v.step_secs.first().unwrap_or(&0) as u64) } else { v.total };
    let text = match v.phase {
        Phase::Idle => mmss(idle_show),
        Phase::Done => "00:00".into(),
        _ => mmss(unit_rem),
    };
    let dcol = match v.phase {
        Phase::Idle => p.text.gamma_multiply(0.72),
        Phase::Done => p.green,
        Phase::Paused if v.waiting => p.text,
        Phase::Paused => p.amber,
        Phase::Running if unit_rem <= Duration::from_secs(10) => color,
        Phase::Running => p.text,
    };
    let (status, status_col) = match v.phase {
        Phase::Idle if def.pomodoro.is_some() => ("집중 시간".to_string(), color),
        Phase::Idle if multi => (format!("{}단계 · {}", v.n, human_dur(def.total_secs())), p.muted),
        Phase::Idle => (human_dur(def.total_secs()), p.muted),
        Phase::Paused if v.waiting => (format!("대기 · {}", v.step_name), p.amber),
        Phase::Paused => (if multi { format!("일시정지 · {}", v.step_name) } else { "일시정지".into() }, p.amber),
        Phase::Running if multi => (v.step_name.clone(), color),
        Phase::Running => ("진행 중".into(), color),
        Phase::Done => ("완료".into(), p.green),
    };
    let sub = if let Some((s, n)) = pomo_session(def, v.idx) {
        let lp = if v.loops > 0 { format!("{}회차 · ", v.loops + 1) } else { String::new() };
        format!("{lp}세션 {} / {}", s.min(n), n)
    } else if multi && v.phase.active() {
        format!("단계 {} / {}", v.idx + 1, v.n)
    } else if v.phase == Phase::Running {
        let end = chrono::Local::now() + chrono::Duration::milliseconds(v.total_rem.as_millis() as i64);
        format!("{} 에 끝", end.format("%H:%M"))
    } else if v.phase == Phase::Idle && !def.hotkey_toggle.is_empty() {
        def.hotkey_toggle.clone()
    } else {
        String::new()
    };
    Disp { text, dcol, rem_frac, done_total, status, status_col, sub, color }
}

/// 원형 고리 — 위(12시)에서 시계 방향으로 `frac` 만큼
pub fn ring(p: &Painter, c: Pos2, r: f32, w: f32, frac: f32, color: Color32, track: Color32) {
    p.circle_stroke(c, r, Stroke::new(w, track));
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.001 {
        return;
    }
    let n = ((frac * 160.0).ceil() as usize).max(2);
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = (-90.0f32 + 360.0 * frac * i as f32 / n as f32).to_radians();
            Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    let (a, b) = (pts[0], *pts.last().unwrap());
    p.add(Shape::line(pts, Stroke::new(w, color)));
    p.circle_filled(a, w / 2.0, color);
    p.circle_filled(b, w / 2.0, color);
}

/// 7세그먼트 LCD 숫자 — `center` = 숫자 묶음의 정가운데, `h` = 숫자 높이. 꺼진 막대는 `off` 로 은은하게
pub fn lcd(p: &Painter, center: Pos2, h: f32, text: &str, on: Color32, off: Color32) {
    const SEGS: [u8; 10] = [
        0b0111111, // 0: a b c d e f
        0b0000110, // 1: b c
        0b1011011, // 2: a b d e g
        0b1001111, // 3: a b c d g
        0b1100110, // 4: b c f g
        0b1101101, // 5: a c d f g
        0b1111101, // 6: a c d e f g
        0b0000111, // 7: a b c
        0b1111111, // 8
        0b1101111, // 9: a b c d f g
    ];
    let w = h * 0.5;
    let t = h * 0.12;
    let gap = h * 0.16;
    let colon_w = h * 0.24;
    let skew = h * 0.07;
    let total: f32 = text
        .chars()
        .map(|c| if c.is_ascii_digit() { w + gap } else if c == ':' || c == '.' { colon_w + gap * 0.4 } else { gap })
        .sum::<f32>()
        - gap;
    let top = center.y - h / 2.0;
    let sk = |q: Pos2| Pos2::new(q.x + skew * (center.y + h / 2.0 - q.y) / h - skew / 2.0, q.y);
    let glow = on.gamma_multiply(0.18);
    // 가로/세로 육각형 막대
    let hseg = |x0: f32, x1: f32, y: f32| -> Vec<Pos2> {
        vec![
            Pos2::new(x0, y),
            Pos2::new(x0 + t / 2.0, y - t / 2.0),
            Pos2::new(x1 - t / 2.0, y - t / 2.0),
            Pos2::new(x1, y),
            Pos2::new(x1 - t / 2.0, y + t / 2.0),
            Pos2::new(x0 + t / 2.0, y + t / 2.0),
        ]
        .into_iter()
        .map(sk)
        .collect()
    };
    let vseg = |x: f32, y0: f32, y1: f32| -> Vec<Pos2> {
        vec![
            Pos2::new(x, y0),
            Pos2::new(x + t / 2.0, y0 + t / 2.0),
            Pos2::new(x + t / 2.0, y1 - t / 2.0),
            Pos2::new(x, y1),
            Pos2::new(x - t / 2.0, y1 - t / 2.0),
            Pos2::new(x - t / 2.0, y0 + t / 2.0),
        ]
        .into_iter()
        .map(sk)
        .collect()
    };
    let gs = t * 0.18;
    let mut x = center.x - total / 2.0;
    for ch in text.chars() {
        if let Some(dg) = ch.to_digit(10) {
            let bits = SEGS[dg as usize];
            let (l, r) = (x + t / 2.0, x + w - t / 2.0);
            let (yt, ym, yb) = (top + t / 2.0, top + h / 2.0, top + h - t / 2.0);
            let segs = [
                hseg(l + gs, r - gs, yt),  // a
                vseg(r, yt + gs, ym - gs), // b
                vseg(r, ym + gs, yb - gs), // c
                hseg(l + gs, r - gs, yb),  // d
                vseg(l, ym + gs, yb - gs), // e
                vseg(l, yt + gs, ym - gs), // f
                hseg(l + gs, r - gs, ym),  // g
            ];
            for (i, s) in segs.into_iter().enumerate() {
                let lit = bits & (1 << i) != 0;
                if lit {
                    p.add(Shape::closed_line(s.clone(), Stroke::new(t * 0.55, glow)));
                }
                p.add(Shape::convex_polygon(s, if lit { on } else { off }, Stroke::NONE));
            }
            x += w + gap;
        } else if ch == ':' || ch == '.' {
            let cx = x + colon_w / 2.0;
            let d = t * 0.95;
            let dots: &[f32] = if ch == ':' { &[0.32, 0.68] } else { &[0.95] };
            for fy in dots {
                let c = sk(Pos2::new(cx, top + h * fy));
                p.rect_filled(Rect::from_center_size(c, egui::vec2(d, d)), CornerRadius::same(1), on);
            }
            x += colon_w + gap * 0.4;
        } else {
            x += gap;
        }
    }
}

/// 꺼진 LCD 막대 색
/// (egui 의 from_white_alpha 는 선형→감마 변환 때문에 11 이 밝기 ~58 회색이 됨 → 패널색에 섞은 불투명 색을 씀)
pub fn lcd_off(p: &Pal) -> Color32 {
    lerp(lcd_panel(p), Color32::WHITE, 0.05)
}

/// 꺼진 진행 눈금 색
pub fn lcd_tick_off(p: &Pal) -> Color32 {
    lerp(lcd_panel(p), Color32::WHITE, 0.10)
}

/// 바탕 채우기 — 왼쪽부터 `frac`
pub fn fill(p: &Painter, rect: Rect, radius: f32, frac: f32, color: Color32) {
    let frac = frac.clamp(0.0, 1.0);
    if frac <= 0.001 {
        return;
    }
    let w = rect.width() * frac;
    let r = radius.round().clamp(0.0, 255.0) as u8;
    let right_r = if w > rect.width() - radius { r } else { 0 };
    p.rect_filled(Rect::from_min_size(rect.min, egui::vec2(w, rect.height())), CornerRadius { nw: r, sw: r, ne: right_r, se: right_r }, color);
}

/// LCD 패널 바탕색 (테마 밝기에 맞춤)
pub fn lcd_panel(p: &Pal) -> Color32 {
    if p.dark {
        Color32::from_rgb(0x0A, 0x0C, 0x0F)
    } else {
        Color32::from_rgb(0x1E, 0x22, 0x28)
    }
}

/// 디지털 스타일 글자색 — 타이머 색을 밝게
pub fn lcd_color(c: Color32) -> Color32 {
    lerp(c, Color32::WHITE, 0.25)
}

/// 미리보기 타일용 작은 그림
pub fn preview(p: &Painter, ctx: &egui::Context, rect: Rect, style: Style, pal: &Pal) {
    let color = pal.accent;
    p.rect_filled(rect, cr(10.0), pal.card);
    p.rect_stroke(rect, cr(10.0), Stroke::new(1.0_f32, pal.border), egui::StrokeKind::Inside);
    let c = rect.center();
    match style {
        Style::Classic => {
            draw_digits(p, ctx, Pos2::new(rect.left() + 12.0, c.y + 8.0), false, "05:00", digits(24.0), pal.text);
            paint_segments(p, Rect::from_min_size(Pos2::new(rect.left() + 12.0, c.y + 16.0), egui::vec2(rect.width() - 24.0, 4.0)), &[1, 1, 1, 1], &[color], 1, 0.5, SegState::Active, pal.track, pal.green);
        }
        Style::Ring => {
            ring(p, c, rect.height() * 0.36, 4.0, 0.7, color, pal.track);
            p.text(c, Align2::CENTER_CENTER, "05:00", digits(15.0), pal.text);
        }
        Style::Minimal => {
            let w = digits_width(ctx, "05:00", &digits(32.0));
            draw_digits(p, ctx, Pos2::new(c.x - w / 2.0, c.y + 12.0), false, "05:00", digits(32.0), pal.text);
            p.line_segment([Pos2::new(rect.left() + 14.0, c.y + 20.0), Pos2::new(c.x, c.y + 20.0)], Stroke::new(1.5_f32, color));
        }
        Style::Digital => {
            let panel = rect.shrink2(egui::vec2(10.0, 12.0));
            p.rect_filled(panel, cr(6.0), lcd_panel(pal));
            lcd(p, panel.center(), 26.0, "05:00", lcd_color(color), lcd_off(pal));
        }
        Style::Hourglass => {
            let gc = glass_colors(pal, color);
            hourglass(p, Pos2::new(rect.left() + 34.0, c.y), rect.height() * 0.84, 0.6, true, 0.0, 0.0, &gc);
            draw_digits(p, ctx, Pos2::new(rect.left() + 62.0, c.y + 9.0), false, "05:00", digits(20.0), pal.text);
        }
        Style::Fill => {
            fill(p, rect.shrink(1.0), 9.0, 0.6, color.gamma_multiply(0.35));
            let w = digits_width(ctx, "05:00", &digits(26.0));
            draw_digits(p, ctx, Pos2::new(c.x - w / 2.0, c.y + 10.0), false, "05:00", digits(26.0), pal.text);
        }
    }
}

pub fn font(s: f32) -> FontId {
    FontId::proportional(s)
}

// ── 모래시계 ────────────────────────────────────────────────

/// 유리 한쪽(목 → 뚜껑) 반폭. s: 0 = 목, 1 = 뚜껑
fn bulb_hw(s: f32, wide: f32, neck: f32) -> f32 {
    neck + (wide - neck) * (s * std::f32::consts::FRAC_PI_2).sin().powf(0.75)
}

/// 목에서부터 s 까지의 면적 비율 (0..1) — 위 칸 모래 높이를 면적으로 맞추려고
fn bulb_area_to(s: f32, wide: f32, neck: f32) -> f32 {
    let n = 64;
    let (mut a, mut tot) = (0.0, 0.0);
    for i in 0..n {
        let x = (i as f32 + 0.5) / n as f32;
        let w = bulb_hw(x, wide, neck);
        tot += w;
        if x <= s {
            a += w;
        }
    }
    a / tot
}

/// 면적 비율 → 목에서부터의 높이 s
fn bulb_level(frac: f32, wide: f32, neck: f32) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let m = (lo + hi) / 2.0;
        if bulb_area_to(m, wide, neck) < frac {
            lo = m;
        } else {
            hi = m;
        }
    }
    (lo + hi) / 2.0
}

pub struct GlassColors {
    pub frame: Color32,
    pub frame_hi: Color32,
    pub glass: Color32,
    pub outline: Color32,
    pub shine: Color32,
    pub sand: Color32,
    pub sand_hi: Color32,
}

/// 모래시계. `top` = 위 칸에 남은 모래 비율, `angle` = 회전(라디안, 뒤집는 중), `t` = 시간(초, 알갱이 애니메이션)
pub fn hourglass(p: &Painter, c: Pos2, h: f32, top: f32, falling: bool, angle: f32, t: f64, col: &GlassColors) {
    let top = top.clamp(0.0, 1.0);
    let w = h * 0.60;
    let cap_h = h * 0.062;
    let gap = h * 0.010; // 받침과 유리 사이
    let hb = h / 2.0 - cap_h - gap; // 유리 한 칸 높이
    let wide = w * 0.41;
    let neck = h * 0.017;
    let sw = (h * 0.008).max(1.0); // 유리 선 굵기
    let (sa, ca) = angle.sin_cos();
    let tr = |u: f32, v: f32| Pos2::new(c.x + u * ca - v * sa, c.y + u * sa + v * ca);
    let poly = |pts: &[(f32, f32)], fill: Color32| {
        p.add(Shape::convex_polygon(pts.iter().map(|&(u, v)| tr(u, v)).collect(), fill, Stroke::NONE));
    };
    let steps = 32;
    let hw = |s: f32| bulb_hw(s, wide, neck);
    let hw_in = |s: f32| (bulb_hw(s, wide, neck) - sw * 1.1).max(0.0);
    let bulb = |dir: f32| -> Vec<(f32, f32)> {
        let mut v = Vec::with_capacity(steps * 2 + 2);
        for i in 0..=steps {
            let s = i as f32 / steps as f32;
            v.push((-hw(s), dir * s * hb));
        }
        for i in (0..=steps).rev() {
            let s = i as f32 / steps as f32;
            v.push((hw(s), dir * s * hb));
        }
        v
    };
    let (up, down) = (bulb(-1.0), bulb(1.0));
    // 유리 안쪽
    poly(&up, col.glass);
    poly(&down, col.glass);

    // 위 칸 모래: 목에서 level 까지 (유리 안쪽에 꼭 맞게)
    if top > 0.002 {
        let lv = bulb_level(top, wide, neck);
        let n = (lv * steps as f32).ceil().max(2.0) as usize;
        let mut v = Vec::new();
        for i in 0..=n {
            let s = lv * i as f32 / n as f32;
            v.push((-hw_in(s), -s * hb));
        }
        for i in (0..=n).rev() {
            let s = lv * i as f32 / n as f32;
            v.push((hw_in(s), -s * hb));
        }
        poly(&v, col.sand);
        // 모래 윗면 밝은 선
        let y = -lv * hb;
        p.line_segment([tr(-hw_in(lv) * 0.96, y), tr(hw_in(lv) * 0.96, y)], Stroke::new(sw * 1.2, col.sand_hi));
    }

    // 아래 칸 모래: 바닥에서 쌓이고 위가 둥근 언덕
    let bottom = 1.0 - top;
    let mut surface_v = hb;
    if bottom > 0.002 {
        let sb = bulb_level(1.0 - bottom, wide, neck).min(0.995);
        // 언덕 꼭짓점이 목 위로 넘으면 다각형이 볼록하지 않아 채우기가 깨짐 → 모래 높이의 절반까지만
        let peak = (hb * 0.12 * bottom.sqrt()).min(((sb - 0.04) * hb).max(0.0) * 0.5);
        let ws = hw_in(sb);
        let mut v = Vec::new();
        let m = 16;
        for i in 0..=m {
            let x = -ws + 2.0 * ws * i as f32 / m as f32;
            let r = x / ws.max(0.001);
            v.push((x, sb * hb - peak * (1.0 - r * r)));
        }
        let n = ((1.0 - sb) * steps as f32).ceil().max(2.0) as usize;
        for i in 1..=n {
            let s = sb + (1.0 - sb) * i as f32 / n as f32;
            v.push((hw_in(s), s * hb));
        }
        for i in (1..=n).rev() {
            let s = sb + (1.0 - sb) * i as f32 / n as f32;
            v.push((-hw_in(s), s * hb));
        }
        poly(&v, col.sand);
        surface_v = sb * hb - peak;
    }

    // 떨어지는 모래 줄기 + 알갱이
    if falling && top > 0.002 {
        p.line_segment([tr(0.0, 0.0), tr(0.0, surface_v)], Stroke::new(h * 0.008, col.sand));
        for k in 0..6 {
            let ph = ((t * 1.7 + k as f64 / 6.0).fract()) as f32;
            let jitter = ((k * 37 % 5) as f32 - 2.0) * h * 0.0018;
            p.circle_filled(tr(jitter, ph * surface_v), h * 0.0065, col.sand_hi);
        }
    }

    // 유리 선 + 반사광
    p.add(Shape::closed_line(up.iter().map(|&(u, v)| tr(u, v)).collect(), Stroke::new(sw, col.outline)));
    p.add(Shape::closed_line(down.iter().map(|&(u, v)| tr(u, v)).collect(), Stroke::new(sw, col.outline)));
    for dir in [-1.0f32, 1.0] {
        let pts: Vec<Pos2> = (0..=14)
            .map(|i| {
                let s = 0.22 + 0.58 * i as f32 / 14.0;
                tr(-hw(s) * 0.66, dir * s * hb)
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(sw * 1.6, col.shine)));
    }

    // 받침(둥근 모서리) · 기둥
    let rrect = |u0: f32, v0: f32, u1: f32, v1: f32, r: f32| -> Vec<(f32, f32)> {
        let mut v = Vec::new();
        let corners = [(u1 - r, v0 + r, -90.0f32), (u1 - r, v1 - r, 0.0), (u0 + r, v1 - r, 90.0), (u0 + r, v0 + r, 180.0)];
        for (cx, cy, a0) in corners {
            for i in 0..=5 {
                let a = (a0 + 90.0 * i as f32 / 5.0).to_radians();
                v.push((cx + r * a.cos(), cy + r * a.sin()));
            }
        }
        v
    };
    let cw = w * 0.54;
    let pw = h * 0.014;
    for side in [-1.0f32, 1.0] {
        let x0 = side * cw * 0.84;
        poly(&rrect(x0 - pw, -hb - gap, x0 + pw, hb + gap, pw * 0.9), col.frame);
    }
    for (v0, v1) in [(-h / 2.0, -h / 2.0 + cap_h), (h / 2.0 - cap_h, h / 2.0)] {
        poly(&rrect(-cw, v0, cw, v1, cap_h * 0.38), col.frame);
        // 받침 윗면 밝은 띠
        poly(&rrect(-cw * 0.94, v0 + cap_h * 0.14, cw * 0.94, v0 + cap_h * 0.36, cap_h * 0.11), col.frame_hi);
    }
}

pub fn glass_colors(p: &Pal, color: Color32) -> GlassColors {
    let frame = if p.dark { Color32::from_rgb(0x8C, 0x6A, 0x48) } else { Color32::from_rgb(0xA0, 0x78, 0x50) };
    let sand = lerp(color, Color32::from_rgb(0xE8, 0xC5, 0x8A), 0.35);
    GlassColors {
        frame,
        frame_hi: lerp(frame, Color32::WHITE, 0.22),
        glass: lerp(p.card, p.text, if p.dark { 0.05 } else { 0.035 }),
        outline: lerp(p.card, p.text, if p.dark { 0.32 } else { 0.38 }),
        shine: lerp(p.card, Color32::WHITE, if p.dark { 0.22 } else { 0.85 }),
        sand,
        sand_hi: lerp(sand, Color32::WHITE, 0.35),
    }
}

/// 뒤집기 애니메이션: 단계가 바뀐 직후 0.7초 동안 0→π. (타이머별 마지막 epoch 기억)
pub fn flip_angle(id: crate::model::Id, epoch: u64, active: bool) -> Option<f32> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;
    static SEEN: Mutex<Option<HashMap<crate::model::Id, (u64, Option<Instant>)>>> = Mutex::new(None);
    let mut g = SEEN.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    let e = m.entry(id).or_insert((epoch, None));
    if e.0 != epoch {
        e.0 = epoch;
        e.1 = if active { Some(Instant::now()) } else { None };
    }
    let start = e.1?;
    let x = start.elapsed().as_secs_f32() / 0.7;
    if x >= 1.0 {
        e.1 = None;
        return None;
    }
    let ease = 1.0 - (1.0 - x).powi(3);
    Some(ease * std::f32::consts::PI)
}

/// 모래시계에 보일 위 칸 비율 · 떨어지는 중인지 (뒤집는 중이면 빈 위 칸을 돌림)
pub fn glass_state(v: &TimerView, d: &Disp) -> (f32, bool, f32) {
    let flip = flip_angle(v.id, v.epoch, v.phase.active());
    match flip {
        Some(a) => (0.0, false, a),
        None => match v.phase {
            Phase::Idle | Phase::Done => (0.0, false, 0.0),
            Phase::Paused if v.waiting => (1.0, false, 0.0),
            Phase::Paused => (d.rem_frac, false, 0.0),
            Phase::Running => (d.rem_frac, true, 0.0),
        },
    }
}
