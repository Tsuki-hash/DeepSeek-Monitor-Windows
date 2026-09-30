//! 使用注册表 API，保留 Unicode 路径并区分不存在与权限错误。
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
    RegKey,
};
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "DeepSeekMonitorWindows";

pub fn read_value() -> Result<Option<String>, String> {
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let key = match user.open_subkey_with_flags(RUN_KEY, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取开机自启失败：{error}")),
    };
    match key.get_value(VALUE_NAME) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("读取开机自启失败：{error}")),
    }
}

pub fn set_value(value: Option<&str>) -> Result<(), String> {
    let user = RegKey::predef(HKEY_CURRENT_USER);
    if let Some(value) = value {
        let (key, _) = user
            .create_subkey(RUN_KEY)
            .map_err(|e| format!("写入开机自启失败：{e}"))?;
        return key
            .set_value(VALUE_NAME, &value)
            .map_err(|e| format!("写入开机自启失败：{e}"));
    }
    let key = match user.open_subkey_with_flags(RUN_KEY, KEY_WRITE) {
        Ok(key) => key,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("关闭开机自启失败：{error}")),
    };
    match key.delete_value(VALUE_NAME) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("关闭开机自启失败：{error}")),
    }
}

pub fn apply_autostart(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        set_value(Some(&format!("\"{}\"", exe.to_string_lossy())))
    } else {
        set_value(None)
    }
}

pub fn save_setting<T>(
    enabled: bool,
    persist: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let previous = read_value()?;
    let desired = if enabled {
        Some(format!(
            "\"{}\"",
            std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
        ))
    } else {
        None
    };
    transact(previous, desired.as_deref(), set_value, persist)
}

fn transact<T>(
    previous: Option<String>,
    desired: Option<&str>,
    write: impl Fn(Option<&str>) -> Result<(), String>,
    persist: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    write(desired)?;
    match persist() {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Err(rollback) = write(previous.as_deref()) {
                return Err(format!("{error}；恢复开机自启失败：{rollback}"));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 配置保存失败完整恢复旧路径和参数() {
        let old = "\"C:\\旧目录\\app.exe\" --background";
        let writes = std::cell::RefCell::new(Vec::new());
        let result = transact(
            Some(old.into()),
            Some("\"C:\\new\\app.exe\""),
            |value| {
                writes.borrow_mut().push(value.map(str::to_string));
                Ok(())
            },
            || Err::<(), _>("disk denied".into()),
        );
        assert!(result.is_err());
        assert_eq!(writes.borrow().last().unwrap().as_deref(), Some(old));
    }
}
