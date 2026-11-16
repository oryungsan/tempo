//! 트레이 아이콘 — 왼쪽 클릭 = 열기, 오른쪽 클릭 메뉴 = 열기 · 종료
use crate::core::Core;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub fn build(core: Arc<Core>) -> Option<TrayIcon> {
    let menu = Menu::new();
    let open = MenuItem::new("Tempo 열기", true, None);
    let quit = MenuItem::new("종료", true, None);
    menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let (open_id, quit_id) = (open.id().clone(), quit.id().clone());
    let c1 = core.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == open_id {
            c1.open_main.store(true, Ordering::Relaxed);
        } else if e.id == quit_id {
            c1.quit.store(true, Ordering::Relaxed);
        }
        c1.repaint();
    }));
    let c2 = core.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            c2.open_main.store(true, Ordering::Relaxed);
            c2.repaint();
        }
    }));
    let icon = Icon::from_rgba(crate::ui::theme::icon_rgba(32), 32, 32).ok()?;
    match TrayIconBuilder::new().with_menu(Box::new(menu)).with_menu_on_left_click(false).with_tooltip("Tempo — 알람 · 타이머").with_icon(icon).build() {
        Ok(t) => Some(t),
        Err(e) => {
            crate::store::log(format!("tray failed: {e}"));
            None
        }
    }
}
