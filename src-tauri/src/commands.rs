//! Tauri 命令薄封装（从 lib.rs 拆出，评审 F-22）。
//!
//! 权限模型：除 `usage_token_captured` 仅限登录窗调用外，其余命令一律
//! `require_main`——命令层是远程登录窗能触达的最后防线（登录窗能力刻意
//! 只授予空权限集，见 `capabilities/login-sync.json`），**新增命令必须先过这一关**。

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use tauri::{Manager, WebviewWindow};

use crate::config::{
    edit_config, normalize_refresh_interval_seconds, read_stored_config, to_app_config, AppConfig,
};
use crate::deepseek_api::{
    fetch_balance_with_key, fetch_usage_with_token, verify_usage_token, BalanceResult, UsageResult,
};
use crate::tray::hide_main_window_inner;
use crate::usage_watcher::{
    capture_usage_token, open_login_window, prescan_cached_token, spawn_title_watcher,
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
pub(crate) fn save_api_key(window: WebviewWindow, api_key: String) -> Result<AppConfig, String> {
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
    // 先改注册表，再落配置；配置失败时回滚注册表，保证两者一致。
    crate::autostart::apply_autostart(autostart)?;
    let persisted = edit_config(|config| {
        config.autostart = autostart;
        Ok(())
    });
    let stored = match persisted {
        Ok(stored) => stored,
        Err(error) => {
            let _ = crate::autostart::rollback_autostart(autostart);
            return Err(error);
        }
    };
    to_app_config(stored)
}

// 实时查询 DeepSeek 账户余额。DeepSeek 官方仅提供余额接口，无用量接口。
#[tauri::command]
pub(crate) async fn fetch_balance(window: WebviewWindow) -> Result<BalanceResult, String> {
    require_main(&window)?;
    let config = read_stored_config()?;
    let api_key = config
        .api_key
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "未配置 API Key".to_string())?;
    fetch_balance_with_key(&api_key).await
}

#[tauri::command]
pub(crate) fn save_usage_token(
    window: WebviewWindow,
    usage_token: String,
) -> Result<AppConfig, String> {
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
pub(crate) fn clear_usage_token(window: WebviewWindow) -> Result<AppConfig, String> {
    require_main(&window)?;
    to_app_config(edit_config(|config| {
        config.usage_token = None;
        Ok(())
    })?)
}

// 通过 DeepSeek 平台内部接口拉取用量与费用（需网页登录 token，非官方 API Key）。
#[tauri::command]
pub(crate) async fn fetch_usage(
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

#[tauri::command]
pub(crate) async fn start_usage_sync(
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
    // 收集在 spawn_blocking（同步 IO 不占 async 工作线程），验证是异步请求，
    // 且候选先收集后验证——单个候选失败不会卡住其余候选（评审 F-25）。
    if let Some(token) = prescan_cached_token().await {
        capture_usage_token(&app, token)?;
        return Ok(true);
    }

    // 登录窗口已存在：刷新它，促使用量页重新请求接口、把响应写入缓存，
    // 用户随后再点一次本按钮即可命中。不重复弹新窗口、不死等。
    if let Some(login_window) = app.get_webview_window("login-sync") {
        let _ = login_window.eval("location.reload();");
        return Ok(false);
    }

    open_login_window(&app)?;
    spawn_title_watcher(app, generation);
    Ok(false)
}

#[tauri::command]
pub(crate) async fn usage_token_captured(
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
