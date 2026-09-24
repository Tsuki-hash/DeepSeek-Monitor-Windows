//! 开机自启：写 HKCU Run 键。
//!
//! 路径必须带引号写入，否则含空格的安装目录会被错误解析（Unquoted Path）。

use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "DeepSeekMonitorWindows";

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

    let status = Command::new("reg")
        .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
        .status()
        .map_err(|error| format!("关闭开机自启失败：{error}"))?;
    // 值本就不存在时 delete 失败属正常，视为已关闭
    if !status.success() {
        return Ok(());
    }
    Ok(())
}
