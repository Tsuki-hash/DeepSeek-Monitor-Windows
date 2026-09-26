pub mod autostart;
pub mod cache_watch;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod deepseek_api;
pub mod http;
pub mod sync_script;
pub mod token_sync;
pub mod tray;
pub mod usage;
pub mod usage_watcher;
pub mod window_pos;

#[cfg(test)]
mod test_support;

// 命令层在 commands（含权限校验），托盘与显隐在 tray，登录同步在 usage_watcher，
// HTTP 客户端在 http，开机自启在 autostart，面板几何在 window_pos。
// 本文件只做进程装配：插件、共享状态、命令注册、托盘初始化。

use std::sync::{
    atomic::{AtomicBool, AtomicU64},
    Arc,
};

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例守卫：必须作为第一个注册的插件。
        // 程序已运行时再次启动 exe，第二个进程不会新开窗口，
        // 而是触发此回调把已有主窗口显示并聚焦，随后第二个进程自行退出。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                tray::show_main_window(&window);
            }
        }))
        // 本次用量同步是否已成功捕获（watcher 退出判定用）。
        .manage(Arc::new(AtomicBool::new(false)))
        // watcher 代际号。每次 start_usage_sync 建窗时 +1，旧 watcher 发现
        // 代际不匹配则退出，避免关窗后 1.5s 内再点同步拉起双 watcher。
        .manage(Arc::new(AtomicU64::new(0)))
        .invoke_handler(tauri::generate_handler![
            commands::hide_main_window,
            commands::is_main_window_visible,
            commands::get_app_config,
            commands::save_api_key,
            commands::clear_api_key,
            commands::save_refresh_interval,
            commands::save_auto_refresh_enabled,
            commands::save_autostart,
            commands::fetch_balance,
            commands::save_usage_token,
            commands::clear_usage_token,
            commands::fetch_usage,
            commands::start_usage_sync,
            commands::usage_token_captured
        ])
        .setup(|app| {
            // 日志在 debug 与 release 都要注册。代码里的 log::warn!（未知模型名、
            // 未归类 token 类型、配置损坏、同步失败原因）在用户机器上必须真正落盘，
            // 否则排障只能靠复现。默认 target 含 LogDir，落盘位置为 app_log_dir()，
            // Windows 下即 %LOCALAPPDATA%\com.deepseek.monitor.windows\logs。
            // release 只记 Warn 及以上，避免常规路径刷日志。
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(if cfg!(debug_assertions) {
                        log::LevelFilter::Info
                    } else {
                        log::LevelFilter::Warn
                    })
                    .build(),
            )?;

            tray::build_tray(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
