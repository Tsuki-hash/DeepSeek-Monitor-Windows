//! Tauri 命令薄封装（从 lib.rs 拆出，评审 F-22）。
//!
//! 权限模型：除 `usage_token_captured` 仅限登录窗调用外，其余命令一律
//! `require_main`——命令层是远程登录窗能触达的最后防线（登录窗能力刻意
//! 只授予空权限集，见 `capabilities/login-sync.json`），**新增命令必须先过这一关**。

use tauri::{Emitter, Manager, WebviewWindow};

use crate::config::{
    edit_config, normalize_refresh_interval_seconds, read_stored_config, to_app_config, AppConfig,
};
use crate::deepseek_api::{
    fetch_balance_with_key, fetch_usage_with_token, BalanceResult, UsageResult,
};
use crate::error::CommandError;
use crate::sync_session::SyncSession;
use crate::tray::hide_main_window_inner;
use crate::usage_watcher::{
    capture_usage_token, open_login_window, prescan_cached_token, spawn_title_watcher,
    verify_token_with_probes,
};

/// 敏感命令仅允许主面板窗口调用，防止远程登录页等非主窗口上下文滥用。
fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("非法调用方".to_string());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn hide_main_window(window: WebviewWindow) -> Result<(), String> {
    require_main(&window)?;
    hide_main_window_inner(&window)
}

#[tauri::command]
pub(crate) fn is_main_window_visible(window: WebviewWindow) -> Result<bool, String> {
    require_main(&window)?;
    Ok(window.is_visible().unwrap_or(true))
}

#[tauri::command]
pub(crate) fn get_app_config(window: WebviewWindow) -> Result<AppConfig, String> {
    require_main(&window)?;
    to_app_config(read_stored_config()?)
}

#[tauri::command]
pub(crate) fn get_diagnostics(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    require_main(&window)?;
    let config = to_app_config(read_stored_config()?)?;
    Ok(serde_json::json!({
        "version": app.package_info().version.to_string(),
        "platform": std::env::consts::OS,
        "apiKeyConfigured": config.api_key_configured,
        "usageTokenConfigured": config.usage_token_configured,
        "autoRefreshEnabled": config.auto_refresh_enabled,
        "refreshIntervalSeconds": config.refresh_interval_seconds
    }))
}

#[tauri::command]
pub(crate) fn open_support_page(window: WebviewWindow, page: String) -> Result<(), String> {
    require_main(&window)?;
    let url = match page.as_str() {
        "releases" => "https://github.com/Tsuki-hash/DeepSeekMonitorWin/releases",
        "issues" => "https://github.com/Tsuki-hash/DeepSeekMonitorWin/issues",
        _ => return Err("未知支持页面".into()),
    };
    let explorer =
        std::path::PathBuf::from(std::env::var_os("WINDIR").ok_or("Windows 目录不可用")?)
            .join("explorer.exe");
    std::process::Command::new(explorer)
        .arg(url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn save_api_key(
    window: WebviewWindow,
    api_key: String,
) -> Result<SavedApiKey, CommandError> {
    require_main(&window)?;
    let value = api_key.trim().to_string();
    if value.is_empty() {
        return Err("API Key 不能为空".into());
    }

    let balance = fetch_balance_with_key(&value).await.map_err(|e| {
        CommandError::new(format!("验证未通过，原 Key 未更改：{e}"), Some("api_key"))
    })?;
    let config = to_app_config(edit_config(|config| {
        config.api_key = Some(value);
        Ok(())
    })?)?;
    Ok(SavedApiKey { config, balance })
}

#[derive(serde::Serialize)]
pub(crate) struct SavedApiKey {
    config: AppConfig,
    balance: BalanceResult,
}

#[tauri::command]
pub(crate) fn clear_api_key(window: WebviewWindow) -> Result<AppConfig, String> {
    require_main(&window)?;
    to_app_config(edit_config(|config| {
        config.api_key = None;
        Ok(())
    })?)
}

#[tauri::command]
pub(crate) fn save_refresh_interval(
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
pub(crate) fn save_auto_refresh_enabled(
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
pub(crate) fn save_autostart(window: WebviewWindow, autostart: bool) -> Result<AppConfig, String> {
    require_main(&window)?;
    let stored = crate::autostart::save_setting(autostart, || {
        edit_config(|config| {
            config.autostart = autostart;
            Ok(())
        })
    })?;
    to_app_config(stored)
}

// 实时查询 DeepSeek 账户余额。DeepSeek 官方仅提供余额接口，无用量接口。
#[tauri::command]
pub(crate) async fn fetch_balance(window: WebviewWindow) -> Result<BalanceResult, CommandError> {
    require_main(&window)?;
    let config = read_stored_config()?;
    let api_key = config
        .api_key
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CommandError::new("未配置 API Key".into(), Some("api_key")))?;
    fetch_balance_with_key(&api_key)
        .await
        .map_err(|e| CommandError::new(e, Some("api_key")))
}

#[tauri::command]
pub(crate) async fn save_usage_token(
    window: WebviewWindow,
    app: tauri::AppHandle,
    usage_token: String,
) -> Result<AppConfig, CommandError> {
    require_main(&window)?;
    let value = usage_token.trim().to_string();
    if value.is_empty() {
        return Err("用量 Token 不能为空".into());
    }
    let generation = app.state::<SyncSession>().begin()?;
    if let Some(login) = app.get_webview_window("login-sync") {
        let _ = login.close();
    }
    verify_token_with_probes(&value).await.map_err(|e| {
        CommandError::new(
            format!("验证未通过，原 Token 未更改：{e}"),
            Some("usage_token"),
        )
    })?;
    capture_usage_token(&app, value, generation).map_err(Into::into)
}

#[tauri::command]
pub(crate) fn clear_usage_token(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<AppConfig, String> {
    require_main(&window)?;
    app.state::<SyncSession>().cancel(|| {
        if let Some(login) = app.get_webview_window("login-sync") {
            let _ = login.close();
        }
        to_app_config(edit_config(|config| {
            config.usage_token = None;
            Ok(())
        })?)
    })
}

#[tauri::command]
pub(crate) fn cancel_usage_sync(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<(), String> {
    require_main(&window)?;
    app.state::<SyncSession>().cancel(|| {
        if let Some(login) = app.get_webview_window("login-sync") {
            let _ = login.close();
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn forget_usage_session(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<AppConfig, String> {
    require_main(&window)?;
    let config = app.state::<SyncSession>().start_clear(|| {
        if let Some(login) = app.get_webview_window("login-sync") {
            login.close().map_err(|e| e.to_string())?;
        }
        to_app_config(edit_config(|stored| {
            stored.usage_token = None;
            Ok(())
        })?)
    })?;
    let _ = app.emit("usage-token-cleared", &config);
    crate::browsing_data::clear(&window).await?;
    Ok(config)
}

// 通过 DeepSeek 平台内部接口拉取用量与费用（需网页登录 token，非官方 API Key）。
#[tauri::command]
pub(crate) async fn fetch_usage(
    window: WebviewWindow,
    month: u32,
    year: u32,
) -> Result<UsageResult, CommandError> {
    require_main(&window)?;
    // 拒绝非法月份，避免把垃圾参数打进平台接口
    if !(1..=12).contains(&month) || !(2020..=2100).contains(&year) {
        return Err("非法的月份或年份".into());
    }
    let config = read_stored_config()?;
    let token = config
        .usage_token
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CommandError::new("未配置用量 Token".into(), Some("usage_token")))?;
    fetch_usage_with_token(&token, month, year)
        .await
        .map_err(|e| CommandError::new(e, Some("usage_token")))
}

#[tauri::command]
pub(crate) async fn start_usage_sync(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    if window.label() != "main" {
        return Err("非法调用方".to_string());
    }
    let session = app.state::<SyncSession>();
    if session.active_generation().is_some() && app.get_webview_window("login-sync").is_some() {
        return Ok(false);
    }
    let generation = session.begin()?;

    // 先扫一次缓存：登录完成后重复点击本命令，缓存落盘后即可命中。
    // 收集在 spawn_blocking（同步 IO 不占 async 工作线程），验证是异步请求，
    // 且候选先收集后验证——单个候选失败不会卡住其余候选（评审 F-25）。
    if let Some(token) = prescan_cached_token(&app, generation).await {
        capture_usage_token(&app, token, generation)?;
        return Ok(true);
    }

    // 建窗和取消也共用会话锁，防止慢预扫描在取消后重新弹窗。
    session.with_current(generation, || open_login_window(&app, generation))?;
    spawn_title_watcher(app.clone(), generation);
    Ok(false)
}

#[tauri::command]
pub(crate) async fn usage_token_captured(
    window: WebviewWindow,
    app: tauri::AppHandle,
    token: String,
    generation: u64,
) -> Result<AppConfig, String> {
    // 仅登录窗口可回传 token，防止主窗口以外的上下文滥用
    if window.label() != "login-sync" {
        return Err("非法调用方".to_string());
    }
    let url = window.url().map_err(|e| e.to_string())?;
    if url.scheme() != "https" || url.host_str() != Some("platform.deepseek.com") {
        return Err("非法登录来源".into());
    }
    if !app.state::<SyncSession>().is_current(generation) {
        return Err("同步已取消".into());
    }
    let value = token.trim().to_string();
    if value.is_empty() {
        return Err("用量 Token 为空".to_string());
    }
    // 先验证再保存：拦截到的 token 可能是登录中途的临时 token。
    // 校验统一走东八区探针月（与预扫描/后台 watcher 一致），不再依赖
    // 登录窗口本地时区给出的月份
    verify_token_with_probes(&value).await?;
    capture_usage_token(&app, value, generation)
}
