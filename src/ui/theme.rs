//! 색 · 글꼴 · 공용 위젯
use crate::model::Theme;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui, Vec2,
};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub bg: Color32,
    pub side: Color32,
    pub card: Color32,
    pub card_hi: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    /// 강조색 위 글자색
    pub on_accent: Color32,
    pub track: Color32,
    pub input: Color32,
    pub green: Color32,
    pub amber: Color32,
    pub red: Color32,
}

const fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// (bg, side, card, card_hi, border, text, muted, faint, accent, track, input)
const fn dark(v: [u32; 11]) -> Pal {
    Pal {
        dark: true,
        bg: c(v[0]),
        side: c(v[1]),
        card: c(v[2]),
        card_hi: c(v[3]),
        border: c(v[4]),
        text: c(v[5]),
        muted: c(v[6]),
        faint: c(v[7]),
        accent: c(v[8]),
        on_accent: Color32::WHITE,
        track: c(v[9]),
        input: c(v[10]),
        green: c(0x3DD68C),
        amber: c(0xF2B13D),
        red: c(0xF06B6B),
    }
}

const fn light(v: [u32; 11]) -> Pal {
    Pal {
        dark: false,
        bg: c(v[0]),
        side: c(v[1]),
        card: c(v[2]),
        card_hi: c(v[3]),
        border: c(v[4]),
        text: c(v[5]),
        muted: c(v[6]),
        faint: c(v[7]),
        accent: c(v[8]),
        on_accent: Color32::WHITE,
        track: c(v[9]),
        input: c(v[10]),
        green: c(0x1FA866),
        amber: c(0xC98A10),
        red: c(0xD94848),
    }
}

pub fn pal(t: Theme) -> Pal {
    match t {
        Theme::Dark => dark([0x0F1115, 0x13161B, 0x181B21, 0x1E2229, 0x262A32, 0xECEEF1, 0x9097A2, 0x5F6670, 0x5B8DEF, 0x2A2E37, 0x1F232A]),
        Theme::Midnight => dark([0x0B1020, 0x0F1629, 0x141C33, 0x1A2440, 0x243052, 0xE6EAF5, 0x8C97B5, 0x56607D, 0x7AA2FF, 0x233056, 0x18213B]),
        Theme::Forest => dark([0x0E1411, 0x121A16, 0x16201B, 0x1C2822, 0x26352D, 0xE6EFE9, 0x8FA398, 0x5A6B61, 0x4CC38A, 0x23332B, 0x1A251F]),
        Theme::Sunset => dark([0x17110F, 0x1C1513, 0x221A17, 0x2A201C, 0x3A2B25, 0xF3E9E4, 0xB09A90, 0x6F5E57, 0xFF8A5B, 0x382A24, 0x261D1A]),
        Theme::Ocean => dark([0x0C1517, 0x0F1A1D, 0x132125, 0x182A2F, 0x22383E, 0xE3F1F3, 0x8AA6AB, 0x557075, 0x3FC1C9, 0x1F3439, 0x162529]),
        Theme::Nordic => dark([0x2E3440, 0x2B303B, 0x3B4252, 0x434C5E, 0x4C566A, 0xECEFF4, 0xAEB7C7, 0x7B8496, 0x88C0D0, 0x4C566A, 0x353C4A]),
        Theme::Mono => Pal {
            on_accent: c(0x111111),
            ..dark([0x111111, 0x151515, 0x1B1B1B, 0x232323, 0x2C2C2C, 0xF0F0F0, 0x9A9A9A, 0x5E5E5E, 0xE8E8E8, 0x2E2E2E, 0x202020])
        },
        Theme::Light => light([0xF5F6F8, 0xECEEF2, 0xFFFFFF, 0xF7F8FA, 0xE1E4E9, 0x1A1D23, 0x5E6570, 0x9AA0A9, 0x3D72E0, 0xE3E6EB, 0xF0F2F5]),
        Theme::Rose => light([0xFBF5F6, 0xF5EAED, 0xFFFFFF, 0xFCF4F6, 0xEED9DE, 0x2B1D22, 0x7A6268, 0xB39BA2, 0xE0607E, 0xF1DEE3, 0xF7ECEF]),
        Theme::Sand => light([0xF6F1E7, 0xEFE8DA, 0xFFFDF8, 0xFAF5EA, 0xE5DCC9, 0x2A251C, 0x6E6553, 0xA79E8A, 0xC2853D, 0xE9E0CC, 0xF1EADB]),
    }
}

pub fn cr(v: f32) -> CornerRadius {
    CornerRadius::same(v.round().clamp(0.0, 255.0) as u8)
}

pub fn rgb(c3: [u8; 3]) -> Color32 {
    Color32::from_rgb(c3[0], c3[1], c3[2])
}

pub fn color_of(hex: &str) -> Color32 {
    rgb(crate::model::parse_color(hex))
}

pub fn apply_visuals(ctx: &egui::Context, p: &Pal) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_corner_radius = cr(14.0);
    v.menu_corner_radius = cr(10.0);
    v.extreme_bg_color = p.input;
    v.faint_bg_color = p.card_hi;
    v.selection.bg_fill = p.accent.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.hyperlink_color = p.accent;
    v.override_text_color = Some(p.text);
    for (w, fill) in [
        (&mut v.widgets.inactive, p.input),
        (&mut v.widgets.hovered, p.card_hi),
        (&mut v.widgets.active, p.card_hi),
        (&mut v.widgets.open, p.card_hi),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = cr(8.0);
        w.bg_stroke = Stroke::new(1.0_f32, p.border);
    }
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.corner_radius = cr(8.0);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }) };
    v.window_shadow = v.popup_shadow;
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, FontId::new(22.0, FontFamily::Name("bold".into())));
    });
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let mut add = |key: &str, file: &str| -> bool {
        match std::fs::read(format!("{win}\\Fonts\\{file}")) {
            Ok(b) => {
                fonts.font_data.insert(key.into(), Arc::new(egui::FontData::from_owned(b)));
                true
            }
            Err(_) => false,
        }
    };
    let malgun = add("malgun", "malgun.ttf");
    let malgun_bd = add("malgunbd", "malgunbd.ttf");
    let digits = add("digits", "bahnschrift.ttf") || add("digits", "segoeuib.ttf");
    if malgun {
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "malgun".into());
        fonts.families.entry(FontFamily::Monospace).or_default().push("malgun".into());
    }
    let base: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    let mut bold = Vec::new();
    if malgun_bd {
        bold.push("malgunbd".to_string());
    }
    bold.extend(base.iter().cloned());
    fonts.families.insert(FontFamily::Name("bold".into()), bold.clone());
    let mut dg = Vec::new();
    if digits {
        dg.push("digits".to_string());
    }
    dg.extend(bold);
    fonts.families.insert(FontFamily::Name("digits".into()), dg);
    ctx.set_fonts(fonts);
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}
pub fn digits(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("digits".into()))
}

/// 시간 표시: 1시간 넘으면 h:mm:ss
pub fn mmss(d: Duration) -> String {
    let s = d.as_secs_f32().ceil() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

pub fn short_dur(secs: u32) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

pub fn human_dur(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let mut v = Vec::new();
    if h > 0 {
        v.push(format!("{h}시간"));
    }
    if m > 0 {
        v.push(format!("{m}분"));
    }
    if s > 0 || v.is_empty() {
        v.push(format!("{s}초"));
    }
    v.join(" ")
}

/// 숫자는 칸 너비 고정 — 1초마다 글자가 흔들리지 않게. 반환 = 그린 너비
pub fn draw_digits(painter: &egui::Painter, ctx: &egui::Context, anchor: Pos2, align_right: bool, text: &str, font: FontId, color: Color32) -> f32 {
    let (cell, colon) = ctx.fonts(|f| {
        (('0'..='9').map(|ch| f.glyph_width(&font, ch)).fold(0.0, f32::max), f.glyph_width(&font, ':'))
    });
    let widths: Vec<f32> = text.chars().map(|ch| if ch == ':' || ch == '.' { colon } else { cell }).collect();
    let total: f32 = widths.iter().sum();
    let mut x = if align_right { anchor.x - total } else { anchor.x };
    for (ch, w) in text.chars().zip(widths) {
        painter.text(Pos2::new(x + w / 2.0, anchor.y), Align2::CENTER_BOTTOM, ch, font.clone(), color);
        x += w;
    }
    total
}

pub fn digits_width(ctx: &egui::Context, text: &str, font: &FontId) -> f32 {
    ctx.fonts(|f| {
        let cell = ('0'..='9').map(|ch| f.glyph_width(font, ch)).fold(0.0, f32::max);
        let colon = f.glyph_width(font, ':');
        text.chars().map(|ch| if ch == ':' || ch == '.' { colon } else { cell }).sum()
    })
}

// ── 위젯 ───────────────────────────────────────────────────

pub fn toggle(ui: &mut Ui, p: &Pal, on: &mut bool) -> Response {
    let size = egui::vec2(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let bg = lerp(p.track, p.accent, t);
    ui.painter().rect_filled(rect, cr(11.0), bg);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    ui.painter().circle_filled(Pos2::new(x, rect.center().y), 8.0, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn primary(ui: &mut Ui, p: &Pal, text: &str) -> Response {
    let b = egui::Button::new(RichText::new(text).color(p.on_accent).font(bold(14.0)))
        .fill(p.accent)
        .stroke(Stroke::NONE)
        .corner_radius(cr(9.0))
        .min_size(egui::vec2(0.0, 34.0));
    let r = ui.add(b);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

pub fn ghost(ui: &mut Ui, p: &Pal, text: &str) -> Response {
    let b = egui::Button::new(RichText::new(text).color(p.text)).fill(p.input).stroke(Stroke::NONE).corner_radius(cr(9.0)).min_size(egui::vec2(0.0, 34.0));
    ui.add(b)
}

pub fn danger(ui: &mut Ui, p: &Pal, text: &str) -> Response {
    let b = egui::Button::new(RichText::new(text).color(p.red)).fill(p.red.gamma_multiply(0.12)).stroke(Stroke::NONE).corner_radius(cr(9.0)).min_size(egui::vec2(0.0, 34.0));
    ui.add(b)
}

/// 작은 네모 아이콘 버튼 (글리프)
pub fn icon_btn(ui: &mut Ui, p: &Pal, glyph: &str, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, cr(8.0), p.card_hi);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(15.0), if resp.hovered() { p.text } else { p.muted });
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// 세그먼트 선택 — 바뀌면 true
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, p: &Pal, value: &mut T, opts: &[(T, &str)]) -> bool {
    let mut changed = false;
    let font = FontId::proportional(13.0);
    let widths: Vec<f32> = opts.iter().map(|(_, l)| ui.fonts(|f| f.layout_no_wrap(l.to_string(), font.clone(), p.text).size().x) + 24.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, 32.0), Sense::hover());
    ui.painter().rect_filled(rect, cr(9.0), p.input);
    let mut x = rect.left() + 3.0;
    for ((v, label), w) in opts.iter().zip(widths) {
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + 3.0), egui::vec2(w, rect.height() - 6.0));
        let resp = ui.interact(r, ui.id().with(("seg", label)), Sense::click());
        let sel = *v == *value;
        if sel {
            ui.painter().rect_filled(r, cr(7.0), if p.dark { p.card_hi } else { p.card });
            ui.painter().rect_stroke(r, cr(7.0), Stroke::new(1.0_f32, p.border), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *label, font.clone(), if sel { p.text } else { p.muted });
        if resp.clicked() && !sel {
            *value = *v;
            changed = true;
        }
        x += w;
    }
    changed
}

pub fn chip(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> Response {
    let font = FontId::proportional(12.0);
    let g = ui.fonts(|f| f.layout_no_wrap(text.to_string(), font, fg));
    let (rect, resp) = ui.allocate_exact_size(g.size() + egui::vec2(16.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, cr(rect.height() / 2.0), bg);
    ui.painter().galley(rect.min + egui::vec2(8.0, 3.0), g, fg);
    resp
}

pub fn card_frame(p: &Pal) -> egui::Frame {
    egui::Frame::default().fill(p.card).stroke(Stroke::new(1.0_f32, p.border)).corner_radius(cr(14.0)).inner_margin(egui::Margin::same(18))
}

pub fn label_muted(ui: &mut Ui, p: &Pal, text: &str) {
    ui.label(RichText::new(text).color(p.muted).size(13.0));
}

// ── 그림 아이콘 (재생·정지·처음으로·다음) ─────────────────────

pub fn paint_play(pt: &egui::Painter, c: Pos2, s: f32, fg: Color32) {
    pt.add(Shape::convex_polygon(
        vec![Pos2::new(c.x - s * 0.62, c.y - s), Pos2::new(c.x + s * 1.0, c.y), Pos2::new(c.x - s * 0.62, c.y + s)],
        fg,
        Stroke::NONE,
    ));
}

pub fn paint_pause(pt: &egui::Painter, c: Pos2, s: f32, fg: Color32) {
    let (w, h, g) = (s * 0.58, s * 2.0, s * 0.47);
    for dx in [-g - w / 2.0, g + w / 2.0] {
        pt.rect_filled(Rect::from_center_size(Pos2::new(c.x + dx, c.y), egui::vec2(w, h)), cr(s * 0.22), fg);
    }
}

pub fn paint_reset(pt: &egui::Painter, c: Pos2, s: f32, fg: Color32) {
    let r = s;
    let stroke = Stroke::new(s * 0.34, fg);
    let pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = (-50.0f32 + i as f32 * (270.0 / 20.0)).to_radians();
            Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    let end = pts[0];
    pt.add(Shape::line(pts, stroke));
    let a = (-50.0f32).to_radians();
    let tangent = egui::vec2(a.sin(), -a.cos());
    let normal = egui::vec2(a.cos(), a.sin());
    let k = s * 0.6;
    pt.add(Shape::convex_polygon(
        vec![end + tangent * k, end + normal * k * 0.9 - tangent * k * 0.2, end - normal * k * 0.9 - tangent * k * 0.2],
        fg,
        Stroke::NONE,
    ));
}

pub fn paint_next(pt: &egui::Painter, c: Pos2, s: f32, fg: Color32) {
    let o = Pos2::new(c.x - s * 0.25, c.y);
    paint_play(pt, o, s * 0.85, fg);
    pt.rect_filled(Rect::from_center_size(Pos2::new(c.x + s * 0.95, c.y), egui::vec2(s * 0.34, s * 1.7)), cr(1.0), fg);
}

pub fn paint_check(pt: &egui::Painter, c: Pos2, s: f32, color: Color32) {
    pt.add(Shape::line(
        vec![Pos2::new(c.x - 4.0 * s, c.y), Pos2::new(c.x - 1.2 * s, c.y + 3.0 * s), Pos2::new(c.x + 4.2 * s, c.y - 3.4 * s)],
        Stroke::new(1.8 * s, color),
    ));
}

/// 동그란 아이콘 버튼 — 반환 (응답, 전경색)
pub fn round_btn(ui: &mut Ui, center: Pos2, r: f32, id: egui::Id, fill: Color32, hover: Color32) -> Response {
    let rect = Rect::from_center_size(center, Vec2::splat(r * 2.0));
    let resp = ui.interact(rect, id, Sense::click());
    ui.painter().circle_filled(center, r, if resp.hovered() { hover } else { fill });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// 단계 막대 (길이 = 단계 시간 비례)
pub fn paint_segments(pt: &egui::Painter, rect: Rect, secs: &[u32], colors: &[Color32], idx: usize, frac: f32, state: SegState, track: Color32, done_color: Color32) {
    let n = secs.len().max(1);
    let gap = if n > 1 { 3.0 } else { 0.0 };
    let usable = rect.width() - gap * (n as f32 - 1.0);
    let total: f32 = secs.iter().map(|s| *s as f32).sum::<f32>().max(1.0);
    let h = rect.height();
    let mut x = rect.left();
    for (i, s) in secs.iter().enumerate() {
        let w = usable * *s as f32 / total;
        let r = Rect::from_min_size(Pos2::new(x, rect.top()), egui::vec2(w, h));
        pt.rect_filled(r, cr(h / 2.0), track);
        let fill = match state {
            SegState::Done => 1.0,
            SegState::Idle => 0.0,
            SegState::Active if i < idx => 1.0,
            SegState::Active if i == idx => frac,
            _ => 0.0,
        };
        let col = colors.get(i).copied().unwrap_or(colors[0]);
        if fill > 0.0 {
            let c2 = match state {
                SegState::Done => done_color,
                _ if i < idx => col.gamma_multiply(0.55),
                _ => col,
            };
            pt.rect_filled(Rect::from_min_size(r.min, egui::vec2((w * fill).max(h.min(w)), h)), cr(h / 2.0), c2);
        }
        x += w + gap;
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SegState {
    Idle,
    Active,
    Done,
}

/// 창 아이콘 — 프로세스에 하나. 매 프레임 같은 Arc 를 넘겨야 egui 가 「안 바뀜」으로 본다
pub fn app_icon() -> Arc<egui::IconData> {
    static ICON: std::sync::OnceLock<Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| Arc::new(egui::IconData { rgba: icon_rgba(64), width: 64, height: 64 })).clone()
}

/// 앱 아이콘 (창·트레이)
pub fn icon_rgba(n: usize) -> Vec<u8> {
    let mut rgba = vec![0u8; n * n * 4];
    let c0 = n as f32 / 2.0;
    let s = n as f32 / 64.0;
    for y in 0..n {
        for x in 0..n {
            let dx = x as f32 + 0.5 - c0;
            let dy = y as f32 + 0.5 - c0;
            let d = (dx * dx + dy * dy).sqrt();
            let i = (y * n + x) * 4;
            let ang = dy.atan2(dx).to_degrees(); // -180..180, 0 = 오른쪽
            let (col, a) = if d < 30.0 * s {
                let ring = (19.0 * s..25.0 * s).contains(&d);
                // 고리: 위쪽(-90°)부터 시계 방향 3/4 만
                let sweep = (ang + 90.0 + 360.0) % 360.0;
                if ring && sweep < 270.0 {
                    ([0x5B, 0x8D, 0xEF], 255)
                } else if d < 3.2 * s || (dx.abs() < 1.6 * s && dy < 0.0 && dy > -13.0 * s) {
                    ([0xEC, 0xEE, 0xF1], 255)
                } else {
                    ([0x15, 0x17, 0x1C], 255)
                }
            } else {
                ([0, 0, 0], 0)
            };
            rgba[i..i + 4].copy_from_slice(&[col[0], col[1], col[2], a]);
        }
    }
    rgba
}
