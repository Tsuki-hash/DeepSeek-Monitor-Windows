//! 开机自启：写 HKCU Run 键。
//!
//! 路径必须带引号写入，否则含空格的安装目录会被错误解析（Unquoted Path）。

use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "DeepSeekMonitorWindows";

fn reg_delete_value() -> Result<(), String> {
    let status = Command::new("reg")
        .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
        .status()
        .map_err(|error| format!("关闭开机自启失败：{error}"))?;
    // 值本就不存在时 delete 失败属正常
    let _ = status;
    Ok(())
}

pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        // Run 键必须写带引号的路径：安装到含空格目录（如 Program Files）时，
        // 裸路径会被解析成 C:\Program.exe + 参数，导致自启失败或被劫持。
        let exe_arg = format!("\"{}\"", exe.to_string_lossy());
        let status = Command::new("reg")
            .args(["add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d"])
            .arg(exe_arg)
            .args(["/f"])
            .status()
            .map_err(|error| format!("写入开机自启失败：{error}"))?;
        if !status.success() {
            return Err("写入开机自启失败".to_string());
        }
        return Ok(());
    }

    reg_delete_value()
}

/// 注册表与配置需保持一致：配置写入失败时回滚注册表，避免「已自启但设置显示关」。
pub fn apply_autostart_with_rollback(enabled: bool, persist_ok: bool) -> Result<(), String> {
    if enabled && !persist_ok {
        reg_delete_value()?;
        return Err("开机自启配置保存失败，已回滚注册表".to_string());
    }
    if !enabled && !persist_ok {
        // 关闭方向：配置失败则恢复注册表值为开
        return apply_autostart(true).map_err(|error| format!("回滚开机自启失败：{error}"));
    }
    Ok(())
}
