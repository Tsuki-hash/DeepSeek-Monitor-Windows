//! 用量 Token 的抓取与解析。
//!
//! 从 WebView2 磁盘缓存里把网页登录态解析出来，是整条同步链路里最容易悄悄坏掉的一环：
//! 缓存文件是二进制混杂文本，标记串一旦变化（平台前端改动）就会静默抓不到 token，
//! 表现为「点了同步但一直没反应」。这类解析逻辑必须能被测试直接喂样本。

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// 单文件读取上限。WebView2 的缓存文件绝大多数远小于此，设上限是为了避免
/// 偶发的巨型文件把整个 watcher 循环卡住。
const MAX_CACHE_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// 以共享方式读取一个可能正被 WebView2 占用的文件。
///
/// Windows 上 WebView2 以独占写句柄持有缓存文件，普通 `fs::read` 会拿到
/// 「另一个程序正在使用此文件」。这里用 `share_mode` 显式允许读写删除共享，
/// 拿到快照即可；读到的内容不完整也无妨，解析函数会自行判断。
#[cfg(windows)]
pub fn read_shared_text(path: &Path) -> Option<String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2 | 0x4)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() == 0 || metadata.len() > MAX_CACHE_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes).ok()?;
    // 缓存文件里夹着 NUL 填充，先剔掉再交给字符串匹配
    Some(String::from_utf8_lossy(&bytes).replace('\0', ""))
}

/// 非 Windows：无 WebView2 共享句柄语义，直接整读（P1-04）。
#[cfg(not(windows))]
pub fn read_shared_text(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() == 0 || metadata.len() > MAX_CACHE_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).replace('\0', ""))
}

/// 从缓存文本里提取网页登录 token。
///
/// 匹配策略：找 `"token":"..."`，并检查其后 1800 字符内同时出现 `id_profile` 与
/// `feature_gates` 两个上下文特征。这两个字段是登录态用户对象的组成部分，
/// 用来把真正的用户 token 和平台前端里其它同名短字符串区分开。
pub fn extract_user_api_token(text: &str) -> Option<String> {
    let mut search_from = 0;
    let marker = "\"token\":\"";
    while let Some(relative_index) = text[search_from..].find(marker) {
        let token_start = search_from + relative_index + marker.len();
        // 未闭合的候选（缓存截断）要跳过继续找，不能整函数放弃——后面可能还有完整 token
        let Some(close_quote) = text[token_start..].find('"') else {
            break;
        };
        let token_end = token_start + close_quote;
        let token = &text[token_start..token_end];
        let context_end = (token_end + 1800).min(text.len());
        let context = &text[token_end..context_end];
        if token.len() > 20
            && context.contains("\"id_profile\"")
            && context.contains("\"feature_gates\"")
        {
            return Some(token.to_string());
        }
        search_from = token_end + 1;
    }
    None
}

/// 轮询缓存目录的去重状态：path -> (文件大小, 修改时间)。
/// watcher 每 1.5s 扫一次，而缓存目录里绝大多数文件（动辄上万个、单个上限 20MB）在两次
/// 轮询之间并没有变化。只比对元数据即可判断「读过且没变」，避免反复整读。
/// 注意不能用「最近 N 分钟」这类时间过滤：用户隔天再点一次同步时，缓存文件可能已经
/// 是一天前的，按时间过滤会让本来能命中的旧缓存扫不到，那是功能回退。
#[derive(Default)]
pub struct CacheScanState {
    seen: HashMap<PathBuf, (u64, Option<std::time::SystemTime>)>,
}

/// `seen` 表项上限。缓存目录文件极多时无限增长会拖慢每次扫描（P2-08）；
/// 超限后整表清空，代价是短暂重读一遍，远好于无界膨胀。
const MAX_SEEN_ENTRIES: usize = 50_000;

/// 在 WebView2 缓存目录里找用量 token。
///
/// `accept` 用于在外层做网络校验：返回 `false` 表示该 token 不可用，继续扫下一个文件；
/// 命中且接受的文件不写入 `seen`，便于调用方重试。未解析出 token 的文件才记入 `seen` 以跳过整读。
pub fn find_webview_cached_usage_token(
    scan: &mut CacheScanState,
    accept: &mut dyn FnMut(&str) -> bool,
) -> Option<String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    let cache_dir = PathBuf::from(local_app_data)
        .join("com.deepseek.monitor.windows")
        .join("EBWebView")
        .join("Default")
        .join("Cache")
        .join("Cache_Data");
    let entries = fs::read_dir(cache_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        // 取一次元数据同时完成「是否普通文件」与「是否变动」两项判断，比 is_file()
        // 后再取一次少一次系统调用。
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let stamp = (metadata.len(), metadata.modified().ok());
        if scan.seen.len() >= MAX_SEEN_ENTRIES {
            scan.seen.clear();
        }
        if scan.seen.get(&path) == Some(&stamp) {
            // 上次已经读过且文件未变动，跳过整读
            continue;
        }
        let Some(text) = read_shared_text(&path) else {
            scan.seen.insert(path, stamp);
            continue;
        };
        match extract_user_api_token(&text) {
            Some(token) => {
                if accept(&token) {
                    return Some(token);
                }
                // 调用方拒收（校验失败）：记入 seen，避免同一坏 token 每次轮询重复弹出
                scan.seen.insert(path, stamp);
            }
            None => {
                scan.seen.insert(path, stamp);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一段带完整登录态上下文的缓存文本
    fn cache_text_with(token: &str) -> String {
        format!(
            "{{\"id\":123,\"token\":\"{token}\",\"user\":{{\"id_profile\":{{\"name\":\"x\"}},\
             \"feature_gates\":[\"a\",\"b\"]}}}}"
        )
    }

    #[test]
    fn 提取_标准登录态() {
        let token = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6";
        let text = cache_text_with(token);
        assert_eq!(extract_user_api_token(&text).as_deref(), Some(token));
    }

    #[test]
    fn 提取_前面有其它同名短字符串时会跳过它() {
        // 平台前端可能先出现一个短的 "token"（如 CSRF 片段），必须跳过继续找
        let real = "real-token-value-that-is-long-enough-123456";
        let text = format!("{{\"token\":\"short\",\"x\":1}}{}", cache_text_with(real));
        assert_eq!(extract_user_api_token(&text).as_deref(), Some(real));
    }

    #[test]
    fn 缺少_id_profile_则不识别() {
        let token = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6";
        let text = format!("{{\"token\":\"{token}\",\"feature_gates\":[]}}");
        assert_eq!(extract_user_api_token(&text), None);
    }

    #[test]
    fn 缺少_feature_gates_则不识别() {
        let token = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6";
        let text = format!("{{\"token\":\"{token}\",\"id_profile\":{{}}}}");
        assert_eq!(extract_user_api_token(&text), None);
    }

    #[test]
    fn 过短的_token_不识别() {
        // 打断在 20 字符边界内的短串不可能是真 token
        let text = "{\"token\":\"12345678901234567890\",\"id_profile\":{},\"feature_gates\":[]}";
        assert_eq!(extract_user_api_token(text), None, "恰好 20 字符应被拒绝");
        let text21 = "{\"token\":\"123456789012345678901\",\"id_profile\":{},\"feature_gates\":[]}";
        assert_eq!(
            extract_user_api_token(text21).as_deref(),
            Some("123456789012345678901"),
            "21 字符应被接受"
        );
    }

    #[test]
    fn 上下文超出_1800_字符窗口_则不识别() {
        let token = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6";
        // 把 id_profile / feature_gates 推到 1800 字符之外
        let filler = "x".repeat(1900);
        let text = format!("{{\"token\":\"{token}\"}}{filler}\"id_profile\"\"feature_gates\"");
        assert_eq!(extract_user_api_token(&text), None);
    }

    #[test]
    fn 无标记_返回_none() {
        assert_eq!(extract_user_api_token(""), None);
        assert_eq!(extract_user_api_token("{\"foo\":\"bar\"}"), None);
    }

    #[test]
    fn 未闭合的_token_不_panic() {
        // 缓存被截断时可能只剩半个标记，不能因此崩溃 watcher 线程
        assert_eq!(extract_user_api_token("{\"token\":\""), None);
        assert_eq!(extract_user_api_token("{\"token\":"), None);
        assert_eq!(extract_user_api_token("\"token\":\"abc"), None);
    }

    #[test]
    fn 前面有未闭合候选_仍能提取后面完整_token() {
        // 回归：find('\"') 失败曾用 ? 直接返回，导致后面合法 token 扫不到
        let real = "real-token-value-that-is-long-enough-123456";
        let text = format!("{{\"token\":\"unclosed-no-end{}", cache_text_with(real));
        assert_eq!(extract_user_api_token(&text).as_deref(), Some(real));
    }

    #[test]
    fn 多字节内容_不_panic() {
        // 缓存里混有中文时按字节查找仍须落在字符边界上
        let token = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6";
        let text = format!("中文前缀✅{}中文后缀✅", cache_text_with(token));
        assert_eq!(extract_user_api_token(&text).as_deref(), Some(token));
    }

    #[test]
    fn find_在缓存目录缺失时_返回_none() {
        // 未登录过（缓存目录不存在）不应 panic，只返回未命中
        let mut scan = CacheScanState::default();
        let previous = std::env::var_os("LOCALAPPDATA");
        std::env::set_var(
            "LOCALAPPDATA",
            std::env::temp_dir().join("dsm-test-no-such-localappdata"),
        );
        let found = find_webview_cached_usage_token(&mut scan, &mut |_| true);
        match previous {
            Some(value) => std::env::set_var("LOCALAPPDATA", value),
            None => std::env::remove_var("LOCALAPPDATA"),
        }
        assert_eq!(found, None);
    }
}
