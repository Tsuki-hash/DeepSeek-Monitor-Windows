pub mod autostart;
pub mod config;
pub mod credentials;
pub mod http;
pub mod token_sync;
pub mod usage;
pub mod window_pos;

#[cfg(test)]
mod test_support;

use autostart::apply_autostart;
use config::{
    edit_config, normalize_refresh_interval_seconds, read_stored_config, to_app_config, AppConfig,
};
use http::http_client;
use token_sync::{find_webview_cached_usage_token, CacheScanState};
use usage::{
    cost_sum, merge_model_slot, model_slot, token_breakdown, Entry, UsageModelSummary, FLASH_SLOT,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use serde::{Deserialize, Serialize};
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
        // 唤出）时退回光标位置。按锚点最近的工作区边贴靠（P2-12，几何在 window_pos）。
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

    // 面板显隐事件。窗口隐藏不会卸载 WebView，前端的 setInterval 不会自己停；
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

    /// 敏感命令仅允许主面板窗口调用（P0-02）。
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
        apply_autostart(autostart)?;
        to_app_config(edit_config(|config| {
            config.autostart = autostart;
            Ok(())
        })?)
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct BalanceResult {
        is_available: bool,
        currency: String,
        total_balance: String,
        granted_balance: String,
        topped_up_balance: String,
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

        let client = http_client();
        let response = client
            .get("https://api.deepseek.com/user/balance")
            .bearer_auth(&api_key)
            .send()
            .await
            .map_err(|error| format!("网络请求失败：{error}"))?;

        match response.status().as_u16() {
            200 => {}
            401 => return Err("API Key 无效或已过期".to_string()),
            429 => return Err("请求过于频繁，请稍后再试".to_string()),
            code if code >= 500 => return Err(format!("DeepSeek 服务器错误：{code}")),
            code => return Err(format!("请求失败：HTTP {code}")),
        }

        #[derive(Deserialize)]
        struct BalanceInfo {
            currency: String,
            total_balance: String,
            granted_balance: String,
            topped_up_balance: String,
        }
        #[derive(Deserialize)]
        struct BalanceResponse {
            is_available: bool,
            balance_infos: Vec<BalanceInfo>,
        }

        let data: BalanceResponse = response
            .json()
            .await
            .map_err(|error| format!("解析余额数据失败：{error}"))?;

        let info = data
            .balance_infos
            .into_iter()
            .next()
            .ok_or_else(|| "余额信息为空".to_string())?;

        Ok(BalanceResult {
            is_available: data.is_available,
            currency: info.currency,
            total_balance: info.total_balance,
            granted_balance: info.granted_balance,
            topped_up_balance: info.topped_up_balance,
        })
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

    // 用 token 试调平台用量接口，验证它确实是有效的用量 token。
    async fn verify_usage_token(token: &str, month: u32, year: u32) -> Result<(), String> {
        let url =
            format!("https://platform.deepseek.com/api/v0/usage/amount?month={month}&year={year}");
        let resp = http_client()
            .get(&url)
            .bearer_auth(token)
            .header("x-app-version", "1.0.0")
            .header("Accept", "*/*")
            .send()
            .await
            .map_err(|error| format!("验证 token 失败：{error}"))?;
        if resp.status().as_u16() == 200 {
            Ok(())
        } else {
            Err(format!("token 无效：HTTP {}", resp.status().as_u16()))
        }
    }

    /// 本地当前 (year, month)。verify 只要求「能调通用量接口」，用当月即可。
    fn current_ym() -> (u32, u32) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut y = 1970u32;
        let mut d = secs / 86_400;
        loop {
            let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            let year_len = if leap { 366 } else { 365 };
            if d < year_len {
                break;
            }
            d -= year_len;
            y += 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let md = [
            31u64,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut m = 1u32;
        for len in md {
            if d < len {
                break;
            }
            d -= len;
            m += 1;
        }
        (y, m)
    }

    /// 缓存扫描的 accept 回调：verify 通过才接受该 token（P0-06）。
    fn accept_verified_cached_token(token: &str) -> bool {
        let (year, month) = current_ym();
        tauri::async_runtime::block_on(verify_usage_token(token, month, year)).is_ok()
    }

    fn start_usage_title_watcher(app: tauri::AppHandle, generation: u64) {
        thread::spawn(move || {
            // 登录页加载并触发平台 API 请求需要时间，等待后再开始扫缓存
            thread::sleep(Duration::from_secs(3));
            let mut scan = CacheScanState::default();
            for _ in 0..1200 {
                // 新一轮同步已开始：本 watcher 作废（P1-01）
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
                        // 先抹掉 title 再解析：完整凭据在标题里停留的时间越短越好（P0-01）
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
      // 本脚本作为 initialization_script 在 login-sync 的每次导航都会注入，
      // 而该窗口允许用户自由跳转。这里先收窄作用域：非平台域名直接不装 hook，
      // 否则用户在这个窗口里访问任何第三方站点时，其 Authorization 头都会被读到。
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
        // 主通道：IPC 直传。完整 Bearer 不进 document.title，避免任意进程
        // 用 EnumWindows/GetWindowText 读到（P0-01）。
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
            // 只允许 DeepSeek 站内导航，降低钓鱼/任意站点套壳风险（P0-02）
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
        // 仅登录窗口可回传 token（P0-02），防止主窗口以外的上下文滥用
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

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct UsageDaySummary {
        date: String,
        flash_tokens: u64,
        flash_cache_hit: u64,
        flash_cache_miss: u64,
        flash_response: u64,
        pro_tokens: u64,
        pro_cache_hit: u64,
        pro_cache_miss: u64,
        pro_response: u64,
        // 各模型未归类 token（如多模态图片输入）的当日合计
        flash_other_tokens: u64,
        pro_other_tokens: u64,
        total_tokens: u64,
        total_cost: f64,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct UsageResult {
        models: Vec<UsageModelSummary>,
        days: Vec<UsageDaySummary>,
        month_cost: f64,
    }

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

        #[derive(Deserialize)]
        struct ModelUsage {
            model: String,
            usage: Vec<Entry>,
        }
        #[derive(Deserialize)]
        struct DayUsage {
            date: String,
            data: Vec<ModelUsage>,
        }
        #[derive(Deserialize)]
        struct AmountBiz {
            total: Vec<ModelUsage>,
            days: Vec<DayUsage>,
        }
        #[derive(Deserialize)]
        struct AmountData {
            biz_data: AmountBiz,
        }
        #[derive(Deserialize)]
        struct AmountResp {
            data: AmountData,
        }
        #[derive(Deserialize)]
        struct CostBiz {
            total: Vec<ModelUsage>,
            days: Vec<DayUsage>,
        }
        #[derive(Deserialize)]
        struct CostData {
            biz_data: Vec<CostBiz>,
        }
        #[derive(Deserialize)]
        struct CostResp {
            data: CostData,
        }

        async fn get_json<T: serde::de::DeserializeOwned>(
            client: &reqwest::Client,
            url: &str,
            token: &str,
        ) -> Result<T, String> {
            let resp = client
                .get(url)
                .bearer_auth(token)
                .header("x-app-version", "1.0.0")
                .header("Accept", "*/*")
                .send()
                .await
                .map_err(|error| format!("用量请求失败：{error}"))?;
            match resp.status().as_u16() {
                200 => {}
                401 => return Err("用量 Token 无效或已过期，请重新获取".to_string()),
                429 => return Err("请求过于频繁，请稍后再试".to_string()),
                code => return Err(format!("用量接口错误：HTTP {code}")),
            }
            resp.json::<T>()
                .await
                .map_err(|error| format!("解析用量数据失败：{error}"))
        }

        let client = http_client();
        let amount_url =
            format!("https://platform.deepseek.com/api/v0/usage/amount?month={month}&year={year}");
        let cost_url =
            format!("https://platform.deepseek.com/api/v0/usage/cost?month={month}&year={year}");

        // 两个端点互相独立、同一 token，串行等待会让总延迟等于两者之和（各自含 15s 超时上限）。
        // 并发发起后总延迟约等于较慢的那个；配合复用的 Client 还能共享连接握手。
        // 注意 http_client() 返回的已是 &Client，这里直接传 client，不要再取一次引用。
        let (amount, cost): (AmountResp, CostResp) = tokio::try_join!(
            get_json(client, &amount_url, &token),
            get_json(client, &cost_url, &token),
        )?;

        let cost_total = cost.data.biz_data.first();
        let cost_for_model = |model: &str| -> f64 {
            cost_total
                .and_then(|item| item.total.iter().find(|m| m.model == model))
                .map(|m| cost_sum(&m.usage))
                .unwrap_or(0.0)
        };

        // 按槽位归并：迁移期内 deepseek-flash 与旧名可能同时出现在同一份账单里，
        // 它们其实是同一个模型，必须累加，否则前端只取第一个会漏掉另一部分用量。
        let mut flash_sum: Option<UsageModelSummary> = None;
        let mut pro_sum: Option<UsageModelSummary> = None;
        for model_usage in &amount.data.biz_data.total {
            let Some((slot, display)) = model_slot(&model_usage.model) else {
                log::warn!("未知模型 {}，未计入模型列表", model_usage.model);
                continue;
            };
            let breakdown = token_breakdown(&model_usage.usage);
            let cost = cost_for_model(&model_usage.model);
            if slot == FLASH_SLOT {
                flash_sum = Some(merge_model_slot(
                    flash_sum.take(),
                    slot,
                    display,
                    &breakdown,
                    cost,
                ));
            } else {
                pro_sum = Some(merge_model_slot(
                    pro_sum.take(),
                    slot,
                    display,
                    &breakdown,
                    cost,
                ));
            }
        }

        let mut models = Vec::new();
        models.extend(flash_sum);
        models.extend(pro_sum);

        let mut cost_by_date: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        if let Some(item) = cost_total {
            for day in &item.days {
                let day_cost: f64 = day.data.iter().map(|m| cost_sum(&m.usage)).sum();
                cost_by_date.insert(day.date.clone(), day_cost);
            }
        }

        let mut days = Vec::new();
        for day in &amount.data.biz_data.days {
            let mut flash = 0u64;
            let mut flash_hit = 0u64;
            let mut flash_miss = 0u64;
            let mut flash_resp = 0u64;
            let mut pro = 0u64;
            let mut pro_hit = 0u64;
            let mut pro_miss = 0u64;
            let mut pro_resp = 0u64;
            let mut total = 0u64;
            let mut flash_other = 0u64;
            let mut pro_other = 0u64;
            for model_usage in &day.data {
                let breakdown = token_breakdown(&model_usage.usage);
                total += breakdown.total;
                // total 覆盖当天全部模型（含未知模型），槽位分摊只作用于已识别的模型
                if let Some((slot, _)) = model_slot(&model_usage.model) {
                    if slot == FLASH_SLOT {
                        flash += breakdown.total;
                        flash_hit += breakdown.cache_hit;
                        flash_miss += breakdown.cache_miss;
                        flash_resp += breakdown.response;
                        flash_other += breakdown.other;
                    } else {
                        pro += breakdown.total;
                        pro_hit += breakdown.cache_hit;
                        pro_miss += breakdown.cache_miss;
                        pro_resp += breakdown.response;
                        pro_other += breakdown.other;
                    }
                }
            }
            days.push(UsageDaySummary {
                date: day.date.clone(),
                flash_tokens: flash,
                flash_cache_hit: flash_hit,
                flash_cache_miss: flash_miss,
                flash_response: flash_resp,
                pro_tokens: pro,
                pro_cache_hit: pro_hit,
                pro_cache_miss: pro_miss,
                pro_response: pro_resp,
                flash_other_tokens: flash_other,
                pro_other_tokens: pro_other,
                total_tokens: total,
                total_cost: cost_by_date.get(&day.date).copied().unwrap_or(0.0),
            });
        }

        let month_cost: f64 = cost_total
            .map(|item| item.total.iter().map(|m| cost_sum(&m.usage)).sum())
            .unwrap_or(0.0);

        Ok(UsageResult {
            models,
            days,
            month_cost,
        })
    }

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
