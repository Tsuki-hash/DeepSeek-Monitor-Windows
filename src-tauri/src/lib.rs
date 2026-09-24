pub mod autostart;
pub mod config;
pub mod credentials;
pub mod deepseek_api;
pub mod http;
pub mod token_sync;
pub mod usage;
pub mod window_pos;

#[cfg(test)]
mod test_support;

use autostart::{apply_autostart, apply_autostart_with_rollback};
use config::{
    edit_config, normalize_refresh_interval_seconds, read_stored_config, to_app_config, AppConfig,
};
use deepseek_api::{
    fetch_balance_with_key, fetch_usage_with_token, verify_usage_token, BalanceResult, UsageResult,
};
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
                let _ = apply_autostart_with_rollback(autostart, false);
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
            // title 是本机全局可读的侧信道（任意进程可用 EnumWindows + GetWindowText 读到），
            // 凭据不该在里面停留。主通道写入的 token 必须先抹掉，再关窗口。
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

                if let Ok(title) = window.title() {
                    if let Some(rest) = title.strip_prefix(USAGE_TOKEN_TITLE_PREFIX) {
                        // 先抹掉 title 再解析：完整凭据在标题里停留的时间越短越好
                        let _ = window
                            .eval("try { document.title = 'DeepSeek 账号登录'; } catch (e) {}");
                        // 注入脚本写入的格式：{year}:{month}:{token}
                        let mut parts = rest.splitn(3, ':');
                        if let (Some(y), Some(m), Some(tok)) =
                            (parts.next(), parts.next(), parts.next())
                        {
                            if let (Ok(year), Ok(month)) = (y.parse::<u32>(), m.parse::<u32>()) {
                                let token = tok.to_string();
                                // 验证 token 真能调用用量接口，过滤登录中途的临时 token
                                let verified = tauri::async_runtime::block_on(verify_usage_token(
                                    &token, month, year,
                                ));
                                if verified.is_ok() {
                                    let _ = capture_usage_token(&app, token);
                                    return;
                                }
                            }
                        }
                    }
                }

                thread::sleep(Duration::from_millis(1500));
            }
            // 30 分钟超时，若仍未成功则通知前端结束等待
            let captured = app
                .try_state::<Arc<AtomicBool>>()
                .map(|flag| flag.load(Ordering::SeqCst))
                .unwrap_or(false);
            if !captured {
                let _ = app.emit("usage-sync-ended", ());
            }
        });
    }

    // 在登录窗口注入，hook fetch / XMLHttpRequest，主动从平台 API 请求的
    // Authorization 头里抓 Bearer token。登录后页面自动调 API 即可即时捕获，
    // 不再依赖 WebView2 磁盘缓存的延迟落盘。
    const USAGE_SYNC_POLL_JS: &str = r#"
    (function() {
      // 本脚本作为 initialization_script 在 login-sync 的每次导航都会注入。
      // 导航已限制在 DeepSeek 域内；这里再收窄：非平台域名不装 hook，
      // 避免读到无关页面的 Authorization 头。
      if (location.host !== 'platform.deepseek.com') return;
      if (window.__dsm_token_hook__) return;
      window.__dsm_token_hook__ = true;
      var done = false;
      var pending = false;

      function deliver(token) {
        if (done) return;
        if (!token || typeof token !== 'string') return;
        token = token.trim();
        if (token.length < 20) return;
        var now = new Date();
        var y = now.getFullYear();
        var m = now.getMonth() + 1;
        // 主通道：IPC 直传。完整 Bearer 尽量不进 document.title，
        // 避免任意进程用 EnumWindows/GetWindowText 读到。
        try {
          if (!pending && window.__TAURI__ && window.__TAURI__.core) {
            pending = true;
            window.__TAURI__.core.invoke('usage_token_captured', {
              token: token, month: m, year: y
            }).then(function() { done = true; }).catch(function() { pending = false; });
            return;
          }
        } catch (e) {}
        // 兜底：无 IPC 时仍写 title（本机侧信道残余风险），原生侧读到后会立刻抹掉
        try { document.title = 'DSM_USAGE_TOKEN:' + y + ':' + m + ':' + token; } catch (e) {}
      }

      function fromAuth(value) {
        if (!value) return;
        var m = /Bearer\s+(\S+)/i.exec(String(value));
        if (m && m[1]) deliver(m[1]);
      }

      var origFetch = window.fetch;
      if (typeof origFetch === 'function') {
        window.fetch = function(input, init) {
          try {
            var headers = (init && init.headers) || (input && input.headers);
            if (headers) {
              if (typeof Headers !== 'undefined' && headers instanceof Headers) {
                fromAuth(headers.get('authorization'));
              } else if (Array.isArray(headers)) {
                for (var i = 0; i < headers.length; i++) {
                  if (headers[i] && String(headers[i][0]).toLowerCase() === 'authorization') {
                    fromAuth(headers[i][1]);
                  }
                }
              } else if (typeof headers === 'object') {
                for (var k in headers) {
                  if (k.toLowerCase() === 'authorization') fromAuth(headers[k]);
                }
              }
            }
          } catch (e) {}
          return origFetch.apply(this, arguments);
        };
      }

      var origSet = XMLHttpRequest.prototype.setRequestHeader;
      XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
        try {
          if (name && String(name).toLowerCase() === 'authorization') fromAuth(value);
        } catch (e) {}
        return origSet.apply(this, arguments);
      };
    })();
    "#;

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
        // P2-13：拒绝非法月份，避免把垃圾参数打进平台接口
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
        // P1-01：watcher 代际号。每次 start_usage_sync 建窗时 +1，旧 watcher 发现
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
