pub mod autostart;
pub mod cache_watch;
pub mod config;
pub mod credentials;
pub mod deepseek_api;
pub mod http;
pub mod sync_script;
pub mod token_sync;
pub mod usage;
pub mod window_pos;

#[cfg(test)]
mod test_support;

use autostart::{apply_autostart, rollback_autostart};
use config::{
    edit_config, normalize_refresh_interval_seconds, read_stored_config, to_app_config, AppConfig,
};
use deepseek_api::{
    fetch_balance_with_key, fetch_usage_with_token, verify_usage_token, BalanceResult, UsageResult,
};
use sync_script::USAGE_SYNC_POLL_JS;
use token_sync::{find_webview_cached_usage_token, CacheScanState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex, OnceLock,
        },
        thread,
        time::Duration,
    };
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
        webview::PageLoadEvent,
        Emitter, Manager, PhysicalPosition, Position, WebviewWindow,
    };

    // HTTP 客户端见 http 模块；开机自启见 autostart 模块。

    // 最近一次托盘图标所在矩形（物理坐标）。TrayIconEvent::Click 会带上图标在屏幕上的
    // 真实位置，它比「光标所在显示器的右下角」更可靠：多显示器时光标可能停在另一块屏上，
    // 旧逻辑会把面板放到错误的屏幕角落。
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
            .ok_or_else(|| tauri::Error::WindowNotFound)?;

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

    // 面板显隐事件。窗口隐藏不会卸载 WebView，前端的刷新定时器不会自己停；
    // 反过来，从托盘唤出窗口也不会触发 React 重渲染。两端都要靠事件对齐，
    // 否则会出现"打开面板看到旧数据、收进托盘还在后台轮询"。
    const EVENT_MAIN_WINDOW_SHOWN: &str = "main-window-shown";
    const EVENT_MAIN_WINDOW_HIDDEN: &str = "main-window-hidden";

    fn show_main_window(window: &WebviewWindow) {
        let _ = position_near_tray(window);
        let _ = window.show();
        let _ = window.set_focus();
        // 通知前端：面板被唤出，立刻拉一次最新数据
        let _ = window.emit(EVENT_MAIN_WINDOW_SHOWN, ());
    }

    fn hide_main_window_inner(window: &WebviewWindow) -> Result<(), String> {
        window.hide().map_err(|error| error.to_string())?;
        // 通知前端：面板已隐藏，停掉自动刷新定时器
        let _ = window.emit(EVENT_MAIN_WINDOW_HIDDEN, ());
        Ok(())
    }

    /// 敏感命令仅允许主面板窗口调用，防止远程登录页等非主窗口上下文滥用。
    fn require_main(window: &WebviewWindow) -> Result<(), String> {
        if window.label() != "main" {
            return Err("非法调用方".to_string());
        }
        Ok(())
    }

    #[tauri::command]
    fn hide_main_window(window: WebviewWindow) -> Result<(), String> {
        require_main(&window)?;
        hide_main_window_inner(&window)
    }

    #[tauri::command]
    fn is_main_window_visible(window: WebviewWindow) -> Result<bool, String> {
        require_main(&window)?;
        Ok(window.is_visible().unwrap_or(true))
    }

    #[tauri::command]
    fn get_app_config(window: WebviewWindow) -> Result<AppConfig, String> {
        require_main(&window)?;
        to_app_config(read_stored_config()?)
    }

    #[tauri::command]
    fn save_api_key(window: WebviewWindow, api_key: String) -> Result<AppConfig, String> {
        require_main(&window)?;
        let value = api_key.trim().to_string();
        if value.is_empty() {
            return Err("API Key 不能为空".to_string());
        }

        to_app_config(edit_config(|config| {
            config.api_key = Some(value);
            Ok(())
        })?)
    }

    #[tauri::command]
    fn clear_api_key(window: WebviewWindow) -> Result<AppConfig, String> {
        require_main(&window)?;
        to_app_config(edit_config(|config| {
            config.api_key = None;
            Ok(())
        })?)
    }

    #[tauri::command]
    fn save_refresh_interval(
        window: WebviewWindow,
        refresh_interval_seconds: u64,
    ) -> Result<AppConfig, String> {
        require_main(&window)?;
        to_app_config(edit_config(|config| {
            config.refresh_interval_seconds =
                normalize_refresh_interval_seconds(refresh_interval_seconds);
            Ok(())
        })?)
    }

    #[tauri::command]
    fn save_auto_refresh_enabled(
        window: WebviewWindow,
        auto_refresh_enabled: bool,
    ) -> Result<AppConfig, String> {
        require_main(&window)?;
        to_app_config(edit_config(|config| {
            config.auto_refresh_enabled = auto_refresh_enabled;
            Ok(())
        })?)
    }

    #[tauri::command]
    fn save_autostart(window: WebviewWindow, autostart: bool) -> Result<AppConfig, String> {
        require_main(&window)?;
        // 先改注册表，再落配置；配置失败时回滚注册表，保证两者一致。
        apply_autostart(autostart)?;
        let persisted = edit_config(|config| {
            config.autostart = autostart;
            Ok(())
        });
        let stored = match persisted {
            Ok(stored) => stored,
            Err(error) => {
                let _ = rollback_autostart(autostart);
                return Err(error);
            }
        };
        to_app_config(stored)
    }

    // 实时查询 DeepSeek 账户余额。DeepSeek 官方仅提供余额接口，无用量接口。
    #[tauri::command]
    async fn fetch_balance(window: WebviewWindow) -> Result<BalanceResult, String> {
        require_main(&window)?;
        let config = read_stored_config()?;
        let api_key = config
            .api_key
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "未配置 API Key".to_string())?;
        fetch_balance_with_key(&api_key).await
    }

    #[tauri::command]
    fn save_usage_token(window: WebviewWindow, usage_token: String) -> Result<AppConfig, String> {
        require_main(&window)?;
        let value = usage_token.trim().to_string();
        if value.is_empty() {
            return Err("用量 Token 不能为空".to_string());
        }
        to_app_config(edit_config(|config| {
            config.usage_token = Some(value);
            Ok(())
        })?)
    }

    #[tauri::command]
    fn clear_usage_token(window: WebviewWindow) -> Result<AppConfig, String> {
        require_main(&window)?;
        to_app_config(edit_config(|config| {
            config.usage_token = None;
            Ok(())
        })?)
    }

    /// 仅用于识别并清除历史 title 通道残留，不再解析其中的 token。
    const USAGE_TOKEN_TITLE_PREFIX: &str = "DSM_USAGE_TOKEN:";

    fn capture_usage_token(app: &tauri::AppHandle, token: String) -> Result<AppConfig, String> {
        let value = token.trim().to_string();
        if value.is_empty() {
            return Err("用量 Token 为空".to_string());
        }
        let app_config = to_app_config(edit_config(|config| {
            config.usage_token = Some(value);
            Ok(())
        })?)?;

        // 标记本次同步已成功，避免 watcher 在窗口关闭后误发"结束等待"事件
        if let Some(flag) = app.try_state::<Arc<AtomicBool>>() {
            flag.store(true, Ordering::SeqCst);
        }

        if let Some(window) = app.get_webview_window("login-sync") {
            // 防御：确保标题不含历史 title 通道残留后再关窗
            let _ = window.eval("try { document.title = 'DeepSeek 账号登录'; } catch (e) {}");
            let _ = window.close();
        }

        let _ = app.emit("usage-token-captured", &app_config);

        Ok(app_config)
    }

    // verify_usage_token 见 deepseek_api 模块。

    /// verify 只要求「能调通用量接口」，固定用一个肯定存在的查询月即可，
    /// 避免用本机时钟推年月时受时区/时钟偏差误伤（真实用量拉取仍用前端传入的月份）。
    const VERIFY_QUERY_YEAR: u32 = 2026;
    const VERIFY_QUERY_MONTH: u32 = 1;

    /// 缓存扫描的 accept 回调：verify 通过才接受该 token。
    fn accept_verified_cached_token(token: &str) -> bool {
        tauri::async_runtime::block_on(verify_usage_token(
            token,
            VERIFY_QUERY_MONTH,
            VERIFY_QUERY_YEAR,
        ))
        .is_ok()
    }

    fn start_usage_title_watcher(app: tauri::AppHandle, generation: u64) {
        thread::spawn(move || {
            // 登录页加载并触发平台 API 请求需要时间，等待后再开始扫缓存
            thread::sleep(Duration::from_secs(3));
            let mut scan = CacheScanState::default();
            // 缓存目录可能尚未创建（首次登录时 WebView2 还没落盘），每轮空闲时
            // 检查监听存活、失效就重新拉起；拉起失败自然退回定时轮询
            let cache_dir = token_sync::webview_cache_dir();
            let mut change_signal = cache_dir.as_deref().and_then(cache_watch::spawn);
            let mut idle_rounds = 0u32;
            for _ in 0..1200 {
                // 新一轮同步已开始：本 watcher 作废
                let current = app
                    .try_state::<Arc<AtomicU64>>()
                    .map(|g| g.load(Ordering::SeqCst))
                    .unwrap_or(0);
                if current != generation {
                    return;
                }
                if let Some(token) =
                    find_webview_cached_usage_token(&mut scan, &mut accept_verified_cached_token)
                {
                    let _ = capture_usage_token(&app, token);
                    return;
                }
                idle_rounds = idle_rounds.saturating_add(1);

                let Some(window) = app.get_webview_window("login-sync") else {
                    // 窗口已关闭：若不是因成功捕获而关闭，才通知前端结束等待
                    let captured = app
                        .try_state::<Arc<AtomicBool>>()
                        .map(|flag| flag.load(Ordering::SeqCst))
                        .unwrap_or(false);
                    if !captured {
                        let _ = app.emit("usage-sync-ended", ());
                    }
                    return;
                };

                // 旧版 title 通道已移除：完整 Bearer 不再写入窗口标题。
                // 兼容：若外部工具仍写过 title，读到后立刻抹掉，避免凭据留在标题栏。
                if let Ok(title) = window.title() {
                    if title.starts_with(USAGE_TOKEN_TITLE_PREFIX) {
                        let _ = window
                            .eval("try { document.title = 'DeepSeek 账号登录'; } catch (e) {}");
                    }
                }

                // 有目录变更 → 去抖后立刻扫（WebView2 落盘后数百毫秒内即可命中）；
                // 无变更 → 维持 1.5s→4s 的退避节奏兜底轮询，防通知丢失或目录重建
                let wait_ms = if idle_rounds < 20 { 1500 } else { 4000 };
                let changed = match change_signal.as_mut() {
                    Some(signal) if signal.is_alive() => {
                        signal.wait(Duration::from_millis(wait_ms))
                    }
                    _ => {
                        thread::sleep(Duration::from_millis(wait_ms));
                        false
                    }
                };
                if !change_signal
                    .as_ref()
                    .is_some_and(|signal| signal.is_alive())
                {
                    change_signal = cache_dir.as_deref().and_then(cache_watch::spawn);
                }
                if changed {
                    // WebView2 一次页面加载会写一批缓存文件，稍等写入平息再扫
                    thread::sleep(Duration::from_millis(250));
                }
            }
            // 达到轮次上限（实际多由登录窗口关闭提前退出），若仍未成功则通知前端结束等待
            let captured = app
                .try_state::<Arc<AtomicBool>>()
                .map(|flag| flag.load(Ordering::SeqCst))
                .unwrap_or(false);
            if !captured {
                let _ = app.emit("usage-sync-ended", ());
            }
        });
    }

    #[tauri::command]
    async fn start_usage_sync(
        window: WebviewWindow,
        app: tauri::AppHandle,
    ) -> Result<bool, String> {
        if window.label() != "main" {
            return Err("非法调用方".to_string());
        }
        // 重置本次同步的成功标志，并递增 watcher 代际（作废旧 watcher）
        if let Some(flag) = app.try_state::<Arc<AtomicBool>>() {
            flag.store(false, Ordering::SeqCst);
        }
        let generation = app
            .try_state::<Arc<AtomicU64>>()
            .map(|g| g.fetch_add(1, Ordering::SeqCst) + 1)
            .unwrap_or(1);

        // 先扫一次缓存：登录完成后重复点击本命令，缓存落盘后即可命中。
        // 扫描是同步阻塞 IO（逐文件整读，单个上限 20MB），放进 spawn_blocking 执行，
        // 不占用 async runtime 的工作线程，避免拖住同期的余额/用量请求。
        // 与标题通道一致：先 verify 再落盘，避免缓存里的残缺/无关 "token" 覆盖可用凭据。
        let cached_token = tauri::async_runtime::spawn_blocking(|| {
            find_webview_cached_usage_token(&mut CacheScanState::default(), &mut |token| {
                accept_verified_cached_token(token)
            })
        })
        .await
        .ok()
        .flatten();
        if let Some(token) = cached_token {
            capture_usage_token(&app, token)?;
            return Ok(true);
        }

        // 登录窗口已存在：刷新它，促使用量页重新请求接口、把响应写入缓存，
        // 用户随后再点一次本按钮即可命中。不重复弹新窗口、不死等。
        if app.get_webview_window("login-sync").is_some() {
            if let Some(window) = app.get_webview_window("login-sync") {
                let _ = window.eval("location.reload();");
            }
            return Ok(false);
        }

        let url = tauri::WebviewUrl::External("https://platform.deepseek.com".parse().unwrap());
        tauri::WebviewWindowBuilder::new(&app, "login-sync", url)
            .title("DeepSeek 账号登录")
            .inner_size(480.0, 720.0)
            .min_inner_size(360.0, 480.0)
            .resizable(true)
            .center()
            .visible(true)
            .initialization_script(USAGE_SYNC_POLL_JS)
            // 只允许 DeepSeek 站内导航，降低钓鱼/任意站点套壳风险
            .on_navigation(|nav_url| {
                nav_url
                    .host_str()
                    .is_some_and(|host| host == "deepseek.com" || host.ends_with(".deepseek.com"))
            })
            .on_page_load(|window, payload| {
                if matches!(payload.event(), PageLoadEvent::Finished)
                    && payload
                        .url()
                        .host_str()
                        .is_some_and(|host| host == "platform.deepseek.com")
                {
                    // 双保险：万一 initialization_script 未注入，页面加载完再装一次 hook
                    let _ = window.eval(USAGE_SYNC_POLL_JS);
                }
            })
            .build()
            .map_err(|error| format!("打开登录窗口失败：{error}"))?;
        start_usage_title_watcher(app, generation);
        Ok(false)
    }

    #[tauri::command]
    async fn usage_token_captured(
        window: WebviewWindow,
        app: tauri::AppHandle,
        token: String,
        month: u32,
        year: u32,
    ) -> Result<AppConfig, String> {
        // 仅登录窗口可回传 token，防止主窗口以外的上下文滥用
        if window.label() != "login-sync" {
            return Err("非法调用方".to_string());
        }
        let value = token.trim().to_string();
        if value.is_empty() {
            return Err("用量 Token 为空".to_string());
        }
        // 先验证再保存：拦截到的 token 可能是登录中途的临时 token，
        // 只有能真正调用用量接口的才接受
        verify_usage_token(&value, month, year).await?;
        capture_usage_token(&app, value)
    }

    // UsageDaySummary / UsageResult 已迁至 deepseek_api。

    // 通过 DeepSeek 平台内部接口拉取用量与费用（需网页登录 token，非官方 API Key）。
    #[tauri::command]
    async fn fetch_usage(
        window: WebviewWindow,
        month: u32,
        year: u32,
    ) -> Result<UsageResult, String> {
        require_main(&window)?;
        // 拒绝非法月份，避免把垃圾参数打进平台接口
        if !(1..=12).contains(&month) || !(2020..=2100).contains(&year) {
            return Err("非法的月份或年份".to_string());
        }
        let config = read_stored_config()?;
        let token = config
            .usage_token
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "未配置用量 Token".to_string())?;
        fetch_usage_with_token(&token, month, year).await
    }

    // 旧聚合实现已迁至 deepseek_api::build_usage_result。
    tauri::Builder::default()
        // 单实例守卫：必须作为第一个注册的插件。
        // 程序已运行时再次启动 exe，第二个进程不会新开窗口，
        // 而是触发此回调把已有主窗口显示并聚焦，随后第二个进程自行退出。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                show_main_window(&window);
            }
        }))
        .manage(Arc::new(AtomicBool::new(false)))
        // watcher 代际号。每次 start_usage_sync 建窗时 +1，旧 watcher 发现
        // 代际不匹配则退出，避免关窗后 1.5s 内再点同步拉起双 watcher。
        .manage(Arc::new(AtomicU64::new(0)))
        .invoke_handler(tauri::generate_handler![
            hide_main_window,
            is_main_window_visible,
            get_app_config,
            save_api_key,
            clear_api_key,
            save_refresh_interval,
            save_auto_refresh_enabled,
            save_autostart,
            fetch_balance,
            save_usage_token,
            clear_usage_token,
            fetch_usage,
            start_usage_sync,
            usage_token_captured
        ])
        .setup(|app| {
            // 日志在 debug 与 release 都要注册。代码里的 log::warn!（未知模型名、
            // 未归类 token 类型、配置损坏）在用户机器上必须真正落盘，否则排障只能靠复现。
            // 默认 target 含 LogDir，落盘位置为 app_log_dir()，
            // Windows 下即 %LOCALAPPDATA%\com.deepseek.monitor.windows\logs。
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(if cfg!(debug_assertions) {
                        log::LevelFilter::Info
                    } else {
                        log::LevelFilter::Warn
                    })
                    .build(),
            )?;

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
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
