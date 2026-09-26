//! 托盘图标、菜单与主面板显隐定位（从 lib.rs 拆出，评审 F-22）。
//!
//! 面板显隐事件：窗口隐藏不会卸载 WebView，前端的刷新定时器不会自己停；
//! 反过来，从托盘唤出窗口也不会触发 React 重渲染。两端都要靠事件对齐，
//! 否则会出现「打开面板看到旧数据、收进托盘还在后台轮询」。

use std::sync::{Mutex, OnceLock};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, PhysicalPosition, Position, WebviewWindow,
};

use crate::window_pos;

const EVENT_MAIN_WINDOW_SHOWN: &str = "main-window-shown";
const EVENT_MAIN_WINDOW_HIDDEN: &str = "main-window-hidden";

/// 最近一次托盘图标所在矩形（物理坐标）。TrayIconEvent::Click 会带上图标在屏幕上的
/// 真实位置，它比「光标所在显示器的右下角」更可靠：多显示器时光标可能停在另一块屏上，
/// 旧逻辑会把面板放到错误的屏幕角落。
#[derive(Clone, Copy)]
struct TrayRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn tray_rect_store() -> &'static Mutex<Option<TrayRect>> {
    static STORE: OnceLock<Mutex<Option<TrayRect>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(None))
}

fn remember_tray_rect(rect: TrayRect) {
    if let Ok(mut slot) = tray_rect_store().lock() {
        *slot = Some(rect);
    }
}

fn last_tray_rect() -> Option<TrayRect> {
    tray_rect_store().lock().ok().and_then(|slot| *slot)
}

fn position_near_tray(window: &WebviewWindow) -> tauri::Result<()> {
    // 定位锚点优先取托盘图标中心；托盘事件还没发生过（例如从菜单项「显示主面板」
    // 唤出）时退回光标位置。按锚点最近的工作区边贴靠（几何在 window_pos）。
    let anchor = last_tray_rect()
        .map(|rect| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0))
        .or_else(|| {
            window
                .cursor_position()
                .ok()
                .map(|point| (point.x, point.y))
        })
        .ok_or(tauri::Error::WindowNotFound)?;

    let monitor = window
        .monitor_from_point(anchor.0, anchor.1)?
        .or(window.current_monitor()?)
        .or(window.primary_monitor()?)
        .ok_or(tauri::Error::WindowNotFound)?;

    let work_area = monitor.work_area();
    let scale_factor = monitor.scale_factor();
    let size = window.outer_size()?;
    let margin = (12.0 * scale_factor).round() as i32;
    let area = window_pos::WorkArea {
        x: work_area.position.x,
        y: work_area.position.y,
        width: work_area.size.width as i32,
        height: work_area.size.height as i32,
    };
    let (x, y) = window_pos::panel_origin(
        area,
        size.width as i32,
        size.height as i32,
        margin,
        anchor.0,
        anchor.1,
    );

    window.set_position(Position::Physical(PhysicalPosition::new(x, y)))
}

pub(crate) fn show_main_window(window: &WebviewWindow) {
    let _ = position_near_tray(window);
    let _ = window.show();
    let _ = window.set_focus();
    // 通知前端：面板被唤出，立刻拉一次最新数据
    let _ = window.emit(EVENT_MAIN_WINDOW_SHOWN, ());
}

pub(crate) fn hide_main_window_inner(window: &WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())?;
    // 通知前端：面板已隐藏，停掉自动刷新定时器
    let _ = window.emit(EVENT_MAIN_WINDOW_HIDDEN, ());
    Ok(())
}

/// 装配托盘图标与菜单（lib.rs 的 setup 调用）。
pub(crate) fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "显示主面板", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    show_main_window(&window);
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 任何一次托盘点击都带着图标的实际矩形（左右键都有），先无条件记下来。
            // 右键走的是菜单路径「显示主面板」，也需要这个位置。
            let TrayIconEvent::Click {
                rect,
                button,
                button_state,
                ..
            } = event
            else {
                return;
            };

            let origin = rect.position.to_physical::<f64>(1.0);
            let size = rect.size.to_physical::<f64>(1.0);
            remember_tray_rect(TrayRect {
                x: origin.x,
                y: origin.y,
                width: size.width,
                height: size.height,
            });

            // 仅在左键“抬起”时切换；否则按下+抬起各触发一次，窗口会闪现后立即隐藏
            if button != MouseButton::Left || button_state != MouseButtonState::Up {
                return;
            }

            let app = tray.app_handle();
            if let Some(window) = app.get_webview_window("main") {
                let is_visible = window.is_visible().unwrap_or(false);
                if is_visible {
                    let _ = hide_main_window_inner(&window);
                } else {
                    show_main_window(&window);
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
    }

    tray_builder.build(app)?;
    Ok(())
}
