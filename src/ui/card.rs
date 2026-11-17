//! 떠 있는 타이머 카드 — 항상 위, 끌어서 이동(위치 기억), 오른쪽 클릭 메뉴, 두 번 누르면 집중 화면.
//! 디자인 스타일마다 모양이 다름: 클래식(단계 목록 펼침) · 원형(동그란 위젯) · 미니멀 · 디지털 · 채우기
use super::styles::{self, Disp};
use super::theme::*;
use crate::core::{Core, TimerView};
use crate::engine::{Cmd, Phase};
use crate::model::{CardMode, Id, Style, TimerDef};
use eframe::egui::{self, text::LayoutJob, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextFormat, ViewportCommand};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const W: f32 = 300.0;
pub const H: f32 = 186.0;
const ROW_H: f32 = 27.0;
/// 원형 본체와 펼친 목록 판 사이 간격
const LIST_GAP: f32 = 6.0;

/// 스타일별 카드 크기 (논리 px, 배율 전)
pub fn card_size(s: Style) -> (f32, f32) {
    match s {
        Style::Classic => (W, H),
        Style::Ring => (232.0, 232.0),
        Style::Minimal => (272.0, 108.0),
        Style::Digital => (292.0, 134.0),
        Style::Fill => (300.0, 142.0),
        Style::Hourglass => (300.0, 196.0),
    }
}

pub struct CardState {
    pub expanded: bool,
    size: Option<(f32, f32)>,
    init_pos: Pos2,
    last_pos: Option<Pos2>,
    dirty_at: Option<Instant>,
    /// 직접 끌기 중 실제로 움직였나 (움직였으면 클릭 무시)
    dragged: Arc<std::sync::atomic::AtomicBool>,
    /// 마지막으로 적용한 창 모양
    shape: Option<super::drag::Shape>,
    /// 가장자리 끌어 크기 조절 중: (시작 커서, 시작 배율, 방향)
    resize: Option<((i32, i32), f32, Zone)>,
    /// 조절 중 배율 (놓으면 저장)
    live_k: Option<f32>,
    /// 메뉴(별도 스레드)에서 「단계 목록 펼치기」를 고름
    expand_req: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Zone {
    Right,
    Bottom,
    Corner,
}

/// 크기 조절 손잡이 위치 (창 안 좌표, points)
fn resize_zone(p: Pos2, rect: Rect, style: Style, k: f32) -> Option<Zone> {
    if style == Style::Ring {
        let d = (p - rect.center()).length();
        let r = rect.width() / 2.0;
        return (d > r - 9.0 * k.min(1.5) && d <= r + 1.0).then_some(Zone::Corner);
    }
    let m = 9.0;
    let right = p.x > rect.right() - m && p.x <= rect.right() + 1.0;
    let bottom = p.y > rect.bottom() - m && p.y <= rect.bottom() + 1.0;
    match (right, bottom) {
        (true, true) => Some(Zone::Corner),
        (true, false) => Some(Zone::Right),
        (false, true) => Some(Zone::Bottom),
        _ => None,
    }
}

impl CardState {
    pub fn new(init_pos: Pos2) -> Self {
        CardState { expanded: false, size: None, init_pos, last_pos: None, dirty_at: None, dragged: Default::default(), shape: None, resize: None, live_k: None, expand_req: Default::default() }
    }
}

/// 이 카드가 항상 위인지 (카드별 설정 → 없으면 전체 설정)
pub fn card_on_top(core: &Core, def: &TimerDef) -> bool {
    def.card_on_top.unwrap_or_else(|| core.data().settings.cards_on_top)
}

/// 지금 떠 있어야 하나 — 카드 설정(베드 = 치료실 설정) 또는 직접 연 카드, 닫은 카드는 빼고. (「카드 띄우기」 효과는 따로)
pub fn shown(core: &Core, def: &TimerDef, v: &TimerView) -> bool {
    if core.card_hidden(def.id) {
        return false;
    }
    let by_setting = if def.bed.is_some() { core.data().settings.bed_cards && v.phase != Phase::Idle } else { card_visible(def, v) };
    by_setting || core.card_opened(def.id)
}

pub fn card_visible(def: &TimerDef, v: &TimerView) -> bool {
    match def.card {
        CardMode::Always => true,
        CardMode::WhenActive => v.phase != Phase::Idle,
        CardMode::Off => false,
    }
}

/// 단계 목록을 펼친 높이 (카드 본체 h0 아래에 목록)
fn h_expanded(h0: f32, n: usize) -> f32 {
    h0 + 10.0 + n as f32 * ROW_H + 8.0
}

#[allow(clippy::too_many_arguments)]
pub fn show(ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, st: &mut CardState, k: f32, pc: &Pal, style: Style) {
    let k = st.live_k.unwrap_or(k).clamp(0.5, 4.0);
    let multi = def.steps.len() > 1;
    let (w, h0) = card_size(style);
    let card_h = if st.expanded && multi { h_expanded(h0, def.steps.len()) } else { h0 };
    let builder = egui::ViewportBuilder::default()
        .with_title(format!("{} — Tempo", def.name))
        .with_inner_size([w * k, h0 * k])
        .with_position(st.init_pos)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false)
        .with_window_level(if card_on_top(core, def) { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal })
        .with_icon(app_icon());
    let vid = egui::ViewportId::from_hash_of(("card", def.id));
    ctx.show_viewport_immediate(vid, builder, |ctx, _| {
        // 창 위치 기억 (움직임 멈추고 0.8초 뒤 저장)
        if let Some(p) = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min) {
            if st.last_pos.map(|l| (l - p).length() > 0.5).unwrap_or(true) {
                if st.last_pos.is_some() {
                    st.dirty_at = Some(Instant::now());
                }
                st.last_pos = Some(p);
            }
        }
        if st.dirty_at.map(|t| t.elapsed() > Duration::from_millis(800)).unwrap_or(false) {
            st.dirty_at = None;
            if let Some(p) = st.last_pos {
                let id = def.id;
                core.update_quiet(|d| {
                    if let Some(t) = d.timers.iter_mut().find(|t| t.id == id) {
                        t.card_pos = Some([p.x, p.y]);
                    }
                });
            }
        }

        // 가장자리(원형은 바깥 테두리)를 끌어 크기 조절 — 비율 유지, 0.5~4배, 카드마다 저장
        let ppp = ctx.pixels_per_point();
        // 원형은 동그란 본체에서만 (펼친 목록 판 제외)
        let card_rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(w * k, if style == Style::Ring { h0 } else { card_h } * k));
        let zone = ctx.input(|i| i.pointer.hover_pos()).and_then(|p| resize_zone(p, card_rect, style, k));
        if st.resize.is_none() {
            if let Some(z) = zone {
                ctx.set_cursor_icon(match z {
                    Zone::Right => egui::CursorIcon::ResizeHorizontal,
                    Zone::Bottom => egui::CursorIcon::ResizeVertical,
                    Zone::Corner => egui::CursorIcon::ResizeNwSe,
                });
                if ctx.input(|i| i.pointer.primary_pressed()) {
                    st.resize = Some((super::drag::cursor(), k, z));
                }
            }
        }
        if let Some((start, k0, z)) = st.resize {
            if super::drag::lbutton_down() {
                let (cx, cy) = super::drag::cursor();
                let (sw, sh) = (w * k0 * ppp, card_h * k0 * ppp);
                let fx = (sw + (cx - start.0) as f32) / sw;
                let fy = (sh + (cy - start.1) as f32) / sh;
                let f = match z {
                    Zone::Right => fx,
                    Zone::Bottom => fy,
                    Zone::Corner => fx.max(fy),
                };
                st.live_k = Some((k0 * f).clamp(0.5, 4.0));
                ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                ctx.request_repaint();
            } else {
                st.resize = None;
                if let Some(nk) = st.live_k.take() {
                    let id = def.id;
                    let nk = (nk * 100.0).round() / 100.0;
                    core.update_quiet(|d| {
                        if let Some(t) = d.timers.iter_mut().find(|t| t.id == id) {
                            t.card_scale = Some(nk);
                        }
                    });
                    st.live_k = Some(nk); // 다음 프레임에 저장값이 들어올 때까지 유지
                }
            }
        } else if st.live_k.is_some() && def.card_scale.map(|s| (s - k).abs() < 0.01).unwrap_or(false) {
            st.live_k = None;
        }
        let allow_drag = st.resize.is_none() && zone.is_none();
        if std::env::var_os("TEMPO_DEBUG").is_some() {
            let evs: Vec<String> = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::PointerButton { button, pressed, pos, .. } => Some(format!("btn {button:?} {pressed} at {pos:?}")),
                        egui::Event::PointerGone => Some("gone".into()),
                        egui::Event::WindowFocused(f) => Some(format!("focus {f}")),
                        _ => None,
                    })
                    .collect()
            });
            if !evs.is_empty() {
                crate::store::log(format!("ev {}: {}", def.name, evs.join(", ")));
            }
        }

        let mut menu_req = false;
        egui::CentralPanel::default().frame(egui::Frame::default()).show(ctx, |ui| {
            menu_req = draw(ui, ctx, core, def, v, st, k, (w, card_h), pc, style, allow_drag);
        });
        let want = (w, card_h);
        if st.size != Some(want) {
            if st.size.is_some() || want != (w, h0) {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(want.0 * k, want.1 * k)));
            }
            st.size = Some(want);
        }
        // 창 모양 = 카드 모양 (투명 창이 안 되는 그래픽에서도 둥글게)
        let ppp = ctx.pixels_per_point();
        let px = |v: f32| (v * ppp).round() as i32;
        let shape = if style == Style::Ring && card_h > h0 {
            super::drag::Shape::RingList { w: px(w * k), d: px(h0 * k), h: px(card_h * k), r: px(16.0 * k), gap: px(LIST_GAP * k) }
        } else if style == Style::Ring {
            super::drag::Shape::Ellipse { w: px(w * k), h: px(card_h * k) }
        } else {
            super::drag::Shape::RoundRect { w: px(w * k), h: px(card_h * k), r: px(16.0 * k) }
        };
        let title = format!("{} — Tempo", def.name);
        if st.shape != Some(shape) && std::env::var_os("TEMPO_DEBUG").is_some() {
            crate::store::log(format!("shape {} {:?} inner={:?}", def.name, shape, ctx.input(|i| i.viewport().inner_rect)));
        }
        if st.shape != Some(shape) && super::drag::set_shape(&title, shape) {
            st.shape = Some(shape);
        }
        // 오른쪽 클릭 = Windows 기본 메뉴 (프레임을 다 그린 뒤 띄움)
        if menu_req {
            card_menu(core, def, v, st, style, k, pc.dark);
        }
        if st.expand_req.swap(false, Ordering::Relaxed) {
            st.expanded = !st.expanded;
        }
    });
}

fn truncated(ctx: &egui::Context, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ctx.fonts(|f| f.layout_job(job))
}

/// 오른쪽 클릭 메뉴 — Windows 기본 메뉴(전용 스레드). 고른 명령은 그 스레드에서 실행
fn card_menu(core: &Arc<Core>, def: &TimerDef, v: &TimerView, st: &CardState, style: Style, k: f32, dark: bool) {
    use super::native_menu::{cmd, popup, Item};
    let id = def.id;
    let multi = def.steps.len() > 1;
    let label = match v.phase {
        Phase::Running => "일시정지",
        Phase::Paused => "이어서",
        _ => "시작",
    };
    let mut items = vec![cmd(1, label)];
    if multi {
        items.push(Item::Cmd(2, "다음 단계로".into(), false, v.phase.active()));
    }
    items.push(Item::Cmd(3, "처음으로".into(), false, v.phase != Phase::Idle));
    items.push(Item::Sep);
    if multi {
        items.push(cmd(4, if st.expanded { "단계 목록 접기" } else { "단계 목록 펼치기" }));
    }
    if style == Style::Hourglass {
        items.push(cmd(5, "모래시계 뒤집기 (이 단계 처음부터)"));
    }
    items.push(Item::Sub(
        "디자인".into(),
        Style::ALL.iter().enumerate().map(|(i, s)| Item::Cmd(10 + i as u32, s.label().into(), *s == style, true)).collect(),
    ));
    items.push(Item::Sub(
        format!("카드 크기 ({}%)", (k * 100.0).round()),
        vec![cmd(20, "크게 (+25%)"), cmd(21, "작게 (−20%)"), Item::Cmd(22, "원래대로".into(), false, def.card_scale.is_some())],
    ));
    let on_top = card_on_top(core, def);
    items.push(Item::Cmd(16, "항상 위에 표시".into(), on_top, true));
    items.push(Item::Sep);
    items.push(cmd(30, "집중 화면 열기"));
    items.push(cmd(31, "편집…"));
    items.push(cmd(33, "카드 닫기 (메인 창 · 트레이에서 다시 열기)"));
    items.push(cmd(32, if def.card == CardMode::Always { "앞으로 실행 중에만 카드 표시" } else { "앞으로 카드 표시 안 함" }));
    let core = core.clone();
    let card_mode = def.card;
    let expand = st.expand_req.clone();
    popup(items, dark, move |r| {
        let Some(c) = r else { return };
        let set = |f: &dyn Fn(&mut TimerDef)| {
            core.update(|d| {
                if let Some(t) = d.timers.iter_mut().find(|t| t.id == id) {
                    f(t);
                }
            })
        };
        match c {
            1 => core.cmd(id, Cmd::Toggle, "card"),
            2 => core.cmd(id, Cmd::Skip, "card"),
            3 => core.cmd(id, Cmd::Reset, "card"),
            4 => {
                expand.store(true, Ordering::Relaxed);
                core.repaint();
            }
            5 => core.flip(id),
            10..=15 => {
                let s = Style::ALL[(c - 10) as usize];
                set(&|t| t.style = Some(s));
            }
            16 => set(&|t| t.card_on_top = Some(!on_top)),
            20 => set(&|t| t.card_scale = Some(((k * 1.25).min(4.0) * 100.0).round() / 100.0)),
            21 => set(&|t| t.card_scale = Some(((k * 0.8).max(0.5) * 100.0).round() / 100.0)),
            22 => set(&|t| t.card_scale = None),
            30 => {
                *core.focus_request.lock().unwrap() = Some(id);
                core.open_main.store(true, Ordering::Relaxed);
                core.repaint();
            }
            31 => {
                *core.edit_request.lock().unwrap() = Some(id);
                core.open_main.store(true, Ordering::Relaxed);
                core.repaint();
            }
            32 => {
                let mode = if card_mode == CardMode::Always { CardMode::WhenActive } else { CardMode::Off };
                set(&|t| t.card = mode);
            }
            33 => core.hide_card(id),
            _ => {}
        }
    });
}

/// 재생/일시정지 동그란 버튼 (스타일 공용)
fn play_btn(ui: &mut egui::Ui, core: &Arc<Core>, def: &TimerDef, v: &TimerView, c: Pos2, r: f32, pc: &Pal, accent: Color32) {
    let running = v.phase == Phase::Running;
    let rp = round_btn(ui, c, r, ui.id().with(("play", def.id)), if running { pc.input } else { accent }, if running { pc.card_hi } else { accent.gamma_multiply(1.15) });
    let p = ui.painter();
    if running {
        paint_pause(p, c, r * 0.37, pc.text);
    } else {
        paint_play(p, Pos2::new(c.x + r * 0.05, c.y), r * 0.37, Color32::WHITE);
    }
    if rp.clicked() {
        core.cmd(def.id, Cmd::Toggle, "card");
    }
    let hk = if def.hotkey_toggle.is_empty() { String::new() } else { format!("  ({})", def.hotkey_toggle) };
    rp.on_hover_text(format!(
        "{}{hk}",
        match v.phase {
            Phase::Running => "일시정지",
            Phase::Paused => "이어서",
            _ => "시작",
        }
    ));
}

#[allow(clippy::too_many_arguments)]
fn draw(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, st: &mut CardState, k: f32, size: (f32, f32), pc: &Pal, style: Style, allow_drag: bool) -> bool {
    let full = ui.max_rect();
    let h0 = card_size(style).1;
    let multi = def.steps.len() > 1;
    let card = Rect::from_min_size(full.min, egui::vec2(size.0 * k, size.1 * k)).shrink(1.0);
    // 본체 = 디자인별 카드 (펼친 목록은 그 아래)
    let face = Rect::from_min_size(card.min, egui::vec2(card.width(), h0 * k - 2.0));
    let expanded = st.expanded && multi && size.1 > h0;
    let p = ui.painter().clone();
    // 바탕
    // 창은 둥근 영역으로 잘림(투명 창이 안 되는 그래픽 대비) → 바탕은 그 경계를 넘치게 칠하고(잘린 가장자리가 같은 색),
    // 테두리 선은 경계보다 안쪽에 (선의 부드러운 가장자리가 계단 모양으로 잘리지 않게)
    let outer = Rect::from_min_size(full.min, egui::vec2(size.0 * k, size.1 * k));
    let list_area = if style == Style::Ring {
        p.circle_filled(face.center(), face.width() / 2.0 + 1.5, pc.card);
        p.circle_stroke(face.center(), face.width() / 2.0 - 1.5 * k.max(1.0), Stroke::new(1.0_f32, pc.border));
        let panel = Rect::from_min_max(Pos2::new(card.left(), face.bottom() + LIST_GAP * k), card.max);
        if expanded {
            p.rect_filled(panel.expand(1.0), cr(16.0 * k - 1.0), pc.card);
            p.rect_stroke(panel.shrink(1.5 * k.max(1.0)), cr(16.0 * k - 2.0), Stroke::new(1.0_f32, pc.border), egui::StrokeKind::Inside);
        }
        panel
    } else {
        p.rect_filled(outer, cr(16.0 * k - 1.0), pc.card);
        p.rect_stroke(outer.shrink(1.5 * k.max(1.0)), cr(16.0 * k - 2.0), Stroke::new(1.0_f32, pc.border), egui::StrokeKind::Inside);
        if expanded {
            let y = face.bottom() + 2.0 * k;
            p.line_segment([Pos2::new(card.left() + 18.0 * k, y), Pos2::new(card.right() - 18.0 * k, y)], Stroke::new(1.0_f32, pc.border));
        }
        Rect::from_min_max(Pos2::new(card.left(), face.bottom()), card.max)
    };
    let bg = ui.interact(card, ui.id().with(("card", def.id)), Sense::click());
    if allow_drag {
        super::drag::on_press(ctx, &bg, &format!("{} — Tempo", def.name), &st.dragged);
    }
    let moved = st.dragged.load(Ordering::Relaxed);
    if bg.double_clicked() && !moved {
        *core.focus_request.lock().unwrap() = Some(def.id);
        core.open_main.store(true, Ordering::Relaxed);
    } else if bg.clicked() && !moved && multi {
        // 한 번 누르면 단계 목록 펼치기/접기 (모든 디자인)
        st.expanded = !st.expanded;
        if std::env::var_os("TEMPO_DEBUG").is_some() {
            crate::store::log(format!("click {} -> expanded={}", def.name, st.expanded));
        }
    }
    let menu_req = bg.secondary_clicked();
    if menu_req && std::env::var_os("TEMPO_DEBUG").is_some() {
        crate::store::log(format!("secondary click on {}", def.name));
    }
    let d = styles::disp(def, v, pc);
    match style {
        Style::Classic => paint_classic(ui, ctx, core, def, v, st, k, face, pc),
        Style::Ring => paint_ring(ui, ctx, core, def, v, k, face, pc, &d),
        Style::Minimal => paint_minimal(ui, ctx, core, def, v, k, face, pc, &d),
        Style::Digital => paint_digital(ui, ctx, core, def, v, k, face, pc, &d),
        Style::Fill => paint_fill(ui, ctx, core, def, v, k, face, pc, &d),
        Style::Hourglass => paint_hourglass(ui, ctx, core, def, v, k, face, pc, &d),
    }
    if expanded {
        paint_steps(&p, ctx, def, v, k, pc, list_area);
    }
    // 끝나기 전 화면 효과 (깜박임 · 테두리 빛 · 점점 빨갛게 덧그림, 흔들림 · 튀어오름 = 그림 전체 이동)
    match super::fx::active(core, def, v, ctx.input(|i| i.time)) {
        Some(fx) => {
            let area = if style == Style::Ring { super::fx::Area::Circle(face.center(), face.width() / 2.0) } else { super::fx::Area::Round(card, 16.0 * k) };
            super::fx::overlay(&p, area, &fx, pc, k);
            ctx.set_transform_layer(ui.layer_id(), super::fx::transform(&fx, card, k));
        }
        None => ctx.set_transform_layer(ui.layer_id(), egui::emath::TSTransform::IDENTITY),
    }
    // 닫기 × — 카드 위에 마우스가 있을 때만 (그 타이머를 다시 시작하면 다시 뜸)
    let hovering = ctx.input(|i| i.pointer.hover_pos()).map(|q| card.contains(q)).unwrap_or(false);
    if hovering {
        let xc = if style == Style::Ring {
            let r = face.width() / 2.0;
            Pos2::new(face.center().x + r * 0.70, face.center().y - r * 0.70)
        } else {
            Pos2::new(card.right() - 11.0 * k, card.top() + 11.0 * k)
        };
        let xr = 8.0 * k;
        let rx = round_btn(ui, xc, xr, ui.id().with(("close", def.id)), pc.input, pc.red.gamma_multiply(0.85));
        let s = xr * 0.38;
        let col = if rx.hovered() { Color32::WHITE } else { pc.muted };
        p.line_segment([xc + egui::vec2(-s, -s), xc + egui::vec2(s, s)], Stroke::new(1.6_f32 * k, col));
        p.line_segment([xc + egui::vec2(-s, s), xc + egui::vec2(s, -s)], Stroke::new(1.6_f32 * k, col));
        if rx.on_hover_text("카드 닫기 — 메인 창 타이머 메뉴 · 트레이에서 다시 열 수 있습니다").clicked() {
            core.hide_card(def.id);
        }
    }
    menu_req
}

/// 펼친 단계 목록 (모든 디자인 공용)
fn paint_steps(p: &egui::Painter, ctx: &egui::Context, def: &TimerDef, v: &TimerView, k: f32, pc: &Pal, area: Rect) {
    let color = color_of(&def.color);
    let left = area.left() + 18.0 * k;
    let right = area.right() - 18.0 * k;
    let font = |s: f32| FontId::proportional(s * k);
    for (i, s) in def.steps.iter().enumerate() {
        let ry = area.top() + 8.0 * k + i as f32 * ROW_H * k;
        let row = Rect::from_min_max(Pos2::new(left - 8.0 * k, ry), Pos2::new(right + 8.0 * k, ry + (ROW_H - 3.0) * k));
        let current = v.phase.active() && i == v.idx;
        let done = v.phase == Phase::Done || (v.phase.active() && i < v.idx);
        if current {
            p.rect_filled(row, cr(8.0 * k), color.gamma_multiply(0.16));
        }
        let cy = row.center().y;
        let dot = Pos2::new(left + 6.0 * k, cy);
        if done {
            paint_check(p, dot, k, color.gamma_multiply(0.8));
        } else {
            p.circle_filled(dot, 3.5 * k, if current { color } else { color.gamma_multiply(0.4) });
        }
        let col = if current { pc.text } else if done { pc.faint } else { pc.muted };
        let dur = ctx.fonts(|f| f.layout_no_wrap(short_dur(s.secs), font(12.5), col));
        let name_w = right - (left + 20.0 * k) - dur.size().x - 10.0 * k;
        let g = truncated(ctx, &s.name, if current { bold(13.0 * k) } else { font(13.0) }, col, name_w);
        p.galley(Pos2::new(left + 20.0 * k, cy - g.size().y / 2.0), g, col);
        p.galley(Pos2::new(right - dur.size().x, cy - dur.size().y / 2.0), dur, col);
    }
}

// ── 모래시계 ───────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn paint_hourglass(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, k: f32, card: Rect, pc: &Pal, d: &Disp) {
    let p = ui.painter().clone();
    let (top, falling, angle) = styles::glass_state(v, d);
    if angle > 0.0 || falling {
        ctx.request_repaint_after(std::time::Duration::from_millis(if angle > 0.0 { 16 } else { 60 }));
    }
    let gc = styles::glass_colors(pc, d.color);
    let gh = card.height() - 26.0 * k;
    styles::hourglass(&p, Pos2::new(card.left() + 62.0 * k, card.center().y), gh, top, falling, angle, ctx.input(|i| i.time), &gc);
    let x = card.left() + 124.0 * k;
    let tw = card.right() - x - 52.0 * k;
    let name = truncated(ctx, &def.name, bold(13.0 * k), pc.text, tw);
    p.galley(Pos2::new(x, card.top() + 16.0 * k), name, pc.text);
    let st = truncated(ctx, &d.status, FontId::proportional(12.0 * k), d.status_col, tw + 40.0 * k);
    p.galley(Pos2::new(x, card.top() + 38.0 * k), st, d.status_col);
    let font = digits(if d.text.len() > 5 { 34.0 * k } else { 46.0 * k });
    draw_digits(&p, ctx, Pos2::new(x - 2.0 * k, card.bottom() - 30.0 * k), false, &d.text, font, d.dcol);
    let sg = truncated(ctx, &d.sub, FontId::proportional(11.5 * k), pc.faint, card.right() - x - 16.0 * k);
    p.galley(Pos2::new(x, card.bottom() - 28.0 * k), sg, pc.faint);
    let pb = Pos2::new(card.right() - 32.0 * k, card.top() + 32.0 * k);
    play_btn(ui, core, def, v, pb, 15.0 * k, pc, d.color);
    // 뒤집기
    let fb = Pos2::new(pb.x, pb.y + 38.0 * k);
    let rf = round_btn(ui, fb, 15.0 * k, ui.id().with(("flip", def.id)), pc.input, pc.card_hi);
    paint_reset(&p, fb, 5.6 * k, pc.text);
    if rf.clicked() {
        core.flip(def.id);
    }
    rf.on_hover_text("뒤집기 — 이 단계를 처음부터");
}

// ── 원형: 동그란 위젯 ───────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn paint_ring(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, k: f32, card: Rect, pc: &Pal, d: &Disp) {
    let p = ui.painter().clone();
    let c = card.center();
    let r = card.width() / 2.0 - 14.0 * k;
    styles::ring(&p, c, r, 7.0 * k, d.rem_frac, if v.phase == Phase::Done { pc.green } else { d.color }, pc.track);
    let name = truncated(ctx, &def.name, bold(12.5 * k), pc.muted, r * 1.3);
    p.galley(Pos2::new(c.x - name.size().x / 2.0, c.y - 62.0 * k), name, pc.muted);
    let font = digits(if d.text.len() > 5 { 34.0 * k } else { 44.0 * k });
    let w = digits_width(ctx, &d.text, &font);
    draw_digits(&p, ctx, Pos2::new(c.x - w / 2.0, c.y + 20.0 * k), false, &d.text, font, d.dcol);
    let sub = if v.phase.active() && def.steps.len() > 1 { v.step_name.clone() } else { d.sub.clone() };
    let sg = truncated(ctx, &sub, FontId::proportional(11.5 * k), pc.faint, r * 1.4);
    p.galley(Pos2::new(c.x - sg.size().x / 2.0, c.y + 24.0 * k), sg, pc.faint);
    play_btn(ui, core, def, v, Pos2::new(c.x, c.y + 62.0 * k), 13.0 * k, pc, d.color);
}

// ── 미니멀: 숫자만 크게 ─────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn paint_minimal(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, k: f32, card: Rect, pc: &Pal, d: &Disp) {
    let p = ui.painter().clone();
    let l = card.left() + 18.0 * k;
    let font = digits(if d.text.len() > 5 { 42.0 * k } else { 54.0 * k });
    draw_digits(&p, ctx, Pos2::new(l - 2.0 * k, card.bottom() + 1.0 * k), false, &d.text, font, d.dcol);
    let name = truncated(ctx, &def.name, FontId::proportional(12.0 * k), pc.faint, card.width() - 90.0 * k);
    p.galley(Pos2::new(l, card.top() + 10.0 * k), name, pc.faint);
    // 얇은 진행선
    let y = card.bottom() - 8.0 * k;
    p.line_segment([Pos2::new(l, y), Pos2::new(card.right() - 18.0 * k, y)], Stroke::new(1.0_f32, pc.track));
    let x2 = l + (card.right() - 18.0 * k - l) * d.done_total;
    if d.done_total > 0.0 {
        p.line_segment([Pos2::new(l, y), Pos2::new(x2, y)], Stroke::new(2.0_f32 * k, d.color));
    }
    play_btn(ui, core, def, v, Pos2::new(card.right() - 32.0 * k, card.center().y + 2.0 * k), 15.0 * k, pc, d.color);
}

// ── 디지털: LCD 패널 ───────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn paint_digital(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, k: f32, card: Rect, pc: &Pal, d: &Disp) {
    let p = ui.painter().clone();
    let l = card.left() + 14.0 * k;
    let r = card.right() - 14.0 * k;
    // 이름 · 지금 단계(집중 2 / 휴식 1 / 일시정지 …)
    let head = if v.phase.active() { format!("{}  ·  {}", def.name, d.status) } else { def.name.clone() };
    let hcol = if v.phase.active() { d.status_col } else { pc.muted };
    let name = truncated(ctx, &head, bold(12.0 * k), hcol, card.width() - 120.0 * k);
    p.galley(Pos2::new(l + 2.0 * k, card.top() + 9.0 * k), name, hcol);
    let sg = truncated(ctx, &d.sub, FontId::proportional(11.0 * k), pc.faint, 120.0 * k);
    p.galley(Pos2::new(r - sg.size().x, card.top() + 11.0 * k), sg, pc.faint);
    let panel = Rect::from_min_max(Pos2::new(l, card.top() + 30.0 * k), Pos2::new(r - 44.0 * k, card.bottom() - 14.0 * k));
    p.rect_filled(panel, cr(8.0 * k), styles::lcd_panel(pc));
    let col = match v.phase {
        Phase::Paused if !v.waiting => pc.amber,
        Phase::Done => pc.green,
        _ => styles::lcd_color(d.color),
    };
    let size = if d.text.len() > 5 { 32.0 * k } else { 44.0 * k };
    let _ = size;
    let dh = (panel.height() - 28.0 * k).min(if d.text.len() > 5 { 34.0 * k } else { 46.0 * k });
    styles::lcd(&p, Pos2::new(panel.center().x, panel.top() + 8.0 * k + (panel.height() - 18.0 * k) / 2.0), dh, &d.text, col, styles::lcd_off(pc));
    // 진행 눈금 (10칸)
    let n = 10;
    for i in 0..n {
        let x = panel.left() + 10.0 * k + i as f32 * (panel.width() - 20.0 * k) / n as f32;
        let on = (i as f32 + 0.5) / n as f32 <= d.done_total;
        p.rect_filled(Rect::from_min_size(Pos2::new(x, panel.bottom() - 7.0 * k), egui::vec2((panel.width() - 20.0 * k) / n as f32 - 3.0 * k, 3.0 * k)), cr(1.0), if on { col } else { styles::lcd_tick_off(pc) });
    }
    play_btn(ui, core, def, v, Pos2::new(r - 16.0 * k, panel.center().y), 15.0 * k, pc, d.color);
}

// ── 채우기: 진행만큼 바탕이 차오름 ─────────────────────────────

#[allow(clippy::too_many_arguments)]
fn paint_fill(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, k: f32, card: Rect, pc: &Pal, d: &Disp) {
    let p = ui.painter().clone();
    let tint = if v.phase == Phase::Done { pc.green } else { d.color };
    styles::fill(&p, card.shrink(1.0), 15.0 * k, d.done_total, tint.gamma_multiply(if pc.dark { 0.38 } else { 0.28 }));
    let l = card.left() + 18.0 * k;
    let name = truncated(ctx, &def.name, bold(13.0 * k), pc.text, card.width() - 100.0 * k);
    p.galley(Pos2::new(l, card.top() + 14.0 * k), name, pc.text);
    let st = truncated(ctx, &d.status, FontId::proportional(12.0 * k), pc.muted, card.width() - 100.0 * k);
    p.galley(Pos2::new(l, card.top() + 34.0 * k), st, pc.muted);
    let font = digits(if d.text.len() > 5 { 40.0 * k } else { 52.0 * k });
    draw_digits(&p, ctx, Pos2::new(l - 2.0 * k, card.bottom() - 12.0 * k), false, &d.text, font, d.dcol);
    let pct = format!("{}%", (d.done_total * 100.0).round());
    p.text(Pos2::new(card.right() - 18.0 * k, card.bottom() - 16.0 * k), Align2::RIGHT_BOTTOM, pct, digits(15.0 * k), pc.muted);
    play_btn(ui, core, def, v, Pos2::new(card.right() - 34.0 * k, card.top() + 34.0 * k), 15.0 * k, pc, d.color);
}

// ── 클래식 ────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn paint_classic(ui: &mut egui::Ui, ctx: &egui::Context, core: &Arc<Core>, def: &TimerDef, v: &TimerView, st: &mut CardState, k: f32, card: Rect, pc: &Pal) {
    let id: Id = def.id;
    let multi = def.steps.len() > 1;
    let color = color_of(&def.color);
    let accent = if v.phase == Phase::Done { pc.green } else { color };
    let p = ui.painter().clone();
    let pad = 18.0 * k;
    let left = card.left() + pad;
    let right = card.right() - pad;
    let top = card.top();
    let font = |s: f32| FontId::proportional(s * k);
    let bf = |s: f32| bold(s * k);

    // 머리줄
    let hy = top + 30.0 * k;
    p.circle_filled(Pos2::new(left + 4.0 * k, hy), 4.0 * k, accent);
    let title = if v.phase == Phase::Done { format!("{} 완료", def.name) } else { def.name.clone() };
    let tg = truncated(ctx, &title, bf(13.0), pc.text, 150.0 * k);
    let tx = left + 14.0 * k;
    p.galley(Pos2::new(tx, hy - tg.size().y / 2.0), tg.clone(), pc.text);
    let mut x = tx + tg.size().x + 8.0 * k;
    if multi && v.phase.active() {
        let sg = ctx.fonts(|f| f.layout_no_wrap(format!("{} / {}", v.idx + 1, v.n), font(12.5), pc.muted));
        p.galley(Pos2::new(x, hy - sg.size().y / 2.0), sg.clone(), pc.muted);
        x += sg.size().x + 8.0 * k;
    }
    if v.loops > 0 && v.phase.active() {
        let lg = ctx.fonts(|f| f.layout_no_wrap(format!("{}회차", v.loops + 1), font(12.0), pc.faint));
        if x + lg.size().x < right - 110.0 * k {
            p.galley(Pos2::new(x, hy - lg.size().y / 2.0), lg.clone(), pc.faint);
            x += lg.size().x + 8.0 * k;
        }
    }
    if v.phase == Phase::Paused {
        let pg = ctx.fonts(|f| f.layout_no_wrap(if v.waiting { "대기".into() } else { "일시정지".into() }, bf(11.0), pc.amber));
        let pr = Rect::from_min_size(Pos2::new(x, hy - pg.size().y / 2.0 - 2.0 * k), pg.size() + egui::vec2(14.0 * k, 4.0 * k));
        if pr.right() < right - 70.0 * k {
            p.rect_filled(pr, cr(pr.height() / 2.0), pc.amber.gamma_multiply(0.16));
            p.galley(pr.min + egui::vec2(7.0 * k, 2.0 * k), pg, pc.amber);
        }
    }

    // 버튼
    let r = 15.0 * k;
    let b2 = Pos2::new(right - r, hy);
    play_btn(ui, core, def, v, b2, r, pc, accent);
    if v.phase != Phase::Idle {
        let b1 = Pos2::new(b2.x - 38.0 * k, hy);
        let rr = round_btn(ui, b1, r, ui.id().with(("reset", def.id)), pc.input, pc.card_hi);
        paint_reset(&p, b1, 5.6 * k, pc.text);
        if rr.clicked() {
            core.cmd(id, Cmd::Reset, "card");
        }
        rr.on_hover_text("처음으로");
    }

    // 단계 이름 / 상태 (베드 = 환자 · 프로그램)
    let name = if let Some(l) = super::styles::bed_label(def, v) { l } else { match v.phase {
        Phase::Idle if multi => format!("{}단계 · {}", v.n, human_dur(def.total_secs())),
        Phase::Idle => format!("{} 타이머", human_dur(def.total_secs())),
        Phase::Done => "끝났습니다".to_string(),
        _ if multi => v.step_name.clone(),
        _ => "남은 시간".to_string(),
    } };
    let name_color = if v.phase.active() && (multi || def.bed.is_some()) { pc.text } else { pc.muted };
    let ng = truncated(ctx, &name, bf(15.5), name_color, right - left);
    p.galley(Pos2::new(left, top + 54.0 * k), ng, name_color);

    // 큰 숫자
    let digits_bottom = top + 128.0 * k;
    let (dtext, dcolor) = match v.phase {
        Phase::Idle => (mmss(v.total), pc.muted),
        Phase::Done => ("00:00".to_string(), pc.green),
        Phase::Paused if v.waiting => (mmss(v.step_rem), pc.text),
        Phase::Paused => (mmss(if multi { v.step_rem } else { v.total_rem }), pc.muted),
        Phase::Running => {
            let rem = if multi { v.step_rem } else { v.total_rem };
            (mmss(rem), if rem <= Duration::from_secs(10) { accent } else { pc.text })
        }
    };
    let dsize = if dtext.len() > 5 { 40.0 } else { 50.0 };
    draw_digits(&p, ctx, Pos2::new(left - 2.0 * k, digits_bottom + 6.0 * k), false, &dtext, digits(dsize * k), dcolor);

    // 오른쪽 보조 정보
    if v.phase.active() {
        let (lbl, val) = if multi {
            ("전체 남음".to_string(), mmss(v.total_rem))
        } else {
            let end = chrono::Local::now() + chrono::Duration::milliseconds(v.total_rem.as_millis() as i64);
            ("끝나는 시각".to_string(), end.format("%H:%M").to_string())
        };
        let lg = ctx.fonts(|f| f.layout_no_wrap(lbl, font(11.5), pc.faint));
        let vg = ctx.fonts(|f| f.layout_no_wrap(val, digits(17.0 * k), pc.muted));
        let vy = digits_bottom - vg.size().y - 2.0 * k;
        if v.phase == Phase::Running || multi {
            p.galley(Pos2::new(right - vg.size().x, vy), vg, pc.muted);
            p.galley(Pos2::new(right - lg.size().x, vy - lg.size().y), lg, pc.faint);
        }
    } else if v.phase == Phase::Idle && !def.hotkey_toggle.is_empty() {
        let hint = ctx.fonts(|f| f.layout_no_wrap(def.hotkey_toggle.clone(), font(11.5), pc.faint));
        p.galley(Pos2::new(right - hint.size().x, digits_bottom - hint.size().y - 4.0 * k), hint, pc.faint);
    }

    // 단계 막대
    let bar_y = top + 146.0 * k;
    let bar = Rect::from_min_size(Pos2::new(left, bar_y), egui::vec2(right - left, 6.0 * k));
    let seg = match v.phase {
        Phase::Idle => SegState::Idle,
        Phase::Done => SegState::Done,
        _ => SegState::Active,
    };
    let frac = if multi { v.frac } else { 1.0 - v.total_rem.as_secs_f32() / v.total.as_secs_f32().max(0.001) };
    paint_segments(&p, bar, &v.step_secs, &[color], v.idx, frac, seg, pc.track, pc.green.gamma_multiply(0.85));

    // 아래 줄
    let foot = match v.phase {
        Phase::Paused if v.waiting => "▶ 를 누르면 이 단계를 시작합니다".to_string(),
        Phase::Running | Phase::Paused if multi => match &v.next_name {
            Some(s) => format!("다음  ·  {s}"),
            None => "마지막 단계".to_string(),
        },
        Phase::Running | Phase::Paused => String::new(),
        Phase::Idle if multi => "카드를 누르면 단계 목록".to_string(),
        Phase::Idle => String::new(),
        Phase::Done => "잠시 후 처음으로 돌아갑니다".to_string(),
    };
    if !foot.is_empty() {
        let fg = truncated(ctx, &foot, font(12.0), pc.faint, right - left);
        p.galley(Pos2::new(left, bar_y + 15.0 * k), fg, pc.faint);
    }

}
