//! 用量 Token 的抓取与解析。
//!
//! 从 WebView2 磁盘缓存里把网页登录态解析出来，是整条同步链路里最容易悄悄坏掉的一环：
//! 缓存文件是二进制混杂文本，标记串一旦变化（平台前端改动）就会静默抓不到 token，
//! 表现为「点了同步但一直没反应」。这类解析逻辑必须能被测试直接喂样本。

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::{
    collections::{HashMap, VecDeque},
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

/// 非 Windows：无 WebView2 共享句柄语义，直接整读。
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
/// watcher 周期扫一次，而缓存目录里绝大多数文件在两次轮询之间并没有变化。
/// 只比对元数据即可判断「读过且没变」，避免反复整读。
///
/// 淘汰采用惰性 LRU：`seen` 每个表项带一个递增的代际号，`order` 只是触碰历史
/// 的排队记录，出队时代际对不上说明同一路径后来又被触碰过，这条记录已过期、
/// 直接跳过。好处有二：出队均摊 O(1)（没有 `Vec::remove(0)` 的整段搬移）；
/// 内容频繁变动的热文件每次重读都会挪到队尾——FIFO 恰好相反，最先入表的
/// 热文件会最先被淘汰，逼着 watcher 反复重读最活跃的那批文件。
#[derive(Default)]
pub struct CacheScanState {
    seen: HashMap<PathBuf, (Stamp, u64)>,
    order: VecDeque<(PathBuf, u64)>,
    next_generation: u64,
}

/// 表项内容指纹：文件大小 + 修改时间。
type Stamp = (u64, Option<std::time::SystemTime>);

/// `seen` 表项上限。超过后按 LRU 淘汰到 3/4，保留近期文件的去重信息。
const MAX_SEEN_ENTRIES: usize = 50_000;

/// `order` 相对 `seen` 的膨胀上限。超出说明积累了大量过期排队记录（同一路径
/// 被反复触碰），重建一次队列清掉，防止极端热点文件把队列撑得比表还大。
const ORDER_BLOAT_FACTOR: usize = 2;
const ORDER_BLOAT_SLACK: usize = 64;

/// 记录「该路径本轮已处理过」。已有表项且内容指纹变化（刚被重读）视为一次
/// LRU 触碰，换新代际号挪到队尾。
fn mark_seen(scan: &mut CacheScanState, path: PathBuf, stamp: Stamp) {
    match scan.seen.get_mut(&path) {
        Some(entry) => {
            if entry.0 != stamp {
                scan.next_generation += 1;
                let generation = scan.next_generation;
                entry.1 = generation;
                entry.0 = stamp;
                scan.order.push_back((path, generation));
            }
        }
        None => {
            scan.next_generation += 1;
            let generation = scan.next_generation;
            scan.seen.insert(path.clone(), (stamp, generation));
            scan.order.push_back((path, generation));
        }
    }
    let order_cap = scan.seen.len() * ORDER_BLOAT_FACTOR + ORDER_BLOAT_SLACK;
    if scan.order.len() > order_cap {
        compact_order(scan);
    }
}

/// 重建触碰队列：只保留每个路径当前代际的记录，过期排队记录全部丢弃。
fn compact_order(scan: &mut CacheScanState) {
    scan.order = scan
        .seen
        .iter()
        .map(|(path, (_, generation))| (path.clone(), *generation))
        .collect();
}

/// 把 `seen` 淘汰到 `keep` 项以内：从触碰历史最旧的一端出队，代际对不上的
/// 过期记录直接跳过，对得上的才真正移除。
fn evict_lru(scan: &mut CacheScanState, keep: usize) {
    while scan.seen.len() > keep {
        let Some((path, generation)) = scan.order.pop_front() else {
            // 触碰历史意外耗尽（不应发生）：宁可整体放弃去重重读一轮，
            // 也不能让 watcher 卡死在这里
            scan.seen.clear();
            return;
        };
        if scan
            .seen
            .get(&path)
            .is_some_and(|(_, generation_now)| *generation_now == generation)
        {
            scan.seen.remove(&path);
        }
    }
}

/// WebView2 缓存目录（登录同步窗口使用本应用标识符下的 WebView 数据目录）。
/// 缓存目录变更监听（见 `cache_watch`）与扫描共用这一个定位，避免两处路径
/// 漂移后出现「监听了 A 目录、扫描 B 目录」的静默失配。
pub fn webview_cache_dir() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local_app_data)
            .join("com.deepseek.monitor.windows")
            .join("EBWebView")
            .join("Default")
            .join("Cache")
            .join("Cache_Data"),
    )
}

/// 在 WebView2 缓存目录里找用量 token。
///
/// `accept` 用于在外层做网络校验：返回 `false` 表示该 token 不可用，继续扫下一个文件；
/// 命中且接受的文件不写入 `seen`，便于调用方重试。未解析出 token 的文件才记入 `seen` 以跳过整读。
pub fn find_webview_cached_usage_token(
    scan: &mut CacheScanState,
    accept: &mut dyn FnMut(&str) -> bool,
) -> Option<String> {
    let cache_dir = webview_cache_dir()?;
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
            evict_lru(scan, MAX_SEEN_ENTRIES * 3 / 4);
        }
        if scan
            .seen
            .get(&path)
            .is_some_and(|(seen_stamp, _)| *seen_stamp == stamp)
        {
            // 上次已经读过且文件未变动，跳过整读
            continue;
        }
        let Some(text) = read_shared_text(&path) else {
            mark_seen(scan, path, stamp);
            continue;
        };
        match extract_user_api_token(&text) {
            Some(token) => {
                if accept(&token) {
                    return Some(token);
                }
                // 调用方拒收（校验失败）：记入 seen，避免同一坏 token 每次轮询重复弹出
                mark_seen(scan, path, stamp);
            }
            None => {
                mark_seen(scan, path, stamp);
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

    // —— CacheScanState 惰性 LRU（白盒：直接操作内部状态） ——

    /// 用编号路径构造已填充的扫描状态，插入顺序即编号顺序
    fn filled_state(count: u64) -> CacheScanState {
        let mut scan = CacheScanState::default();
        for i in 0..count {
            mark_seen(&mut scan, PathBuf::from(format!("f_{i:05}")), (i, None));
        }
        scan
    }

    #[test]
    fn 淘汰_最旧先出() {
        let mut scan = filled_state(10);
        evict_lru(&mut scan, 5);
        assert_eq!(scan.seen.len(), 5);
        for i in 0..5u64 {
            assert!(
                !scan.seen.contains_key(&PathBuf::from(format!("f_{i:05}"))),
                "f_{i:05} 是最旧的一批，应被淘汰"
            );
        }
        for i in 5..10u64 {
            assert!(scan.seen.contains_key(&PathBuf::from(format!("f_{i:05}"))));
        }
    }

    #[test]
    fn 淘汰_被触碰的热文件优先保留() {
        // 区分 LRU 与 FIFO 的关键用例：f_00000 最早插入，FIFO 会先淘汰它；
        // 但它刚被重读（内容变化）是热文件，LRU 应改为淘汰 f_00001
        let mut scan = filled_state(10);
        mark_seen(&mut scan, PathBuf::from("f_00000"), (100, None));
        evict_lru(&mut scan, 9);
        assert!(scan.seen.contains_key(&PathBuf::from("f_00000")));
        assert!(!scan.seen.contains_key(&PathBuf::from("f_00001")));
    }

    #[test]
    fn 淘汰_过期排队记录不会误删活表项() {
        let mut scan = filled_state(4);
        // 同一路径连续触碰多次，队列里留下大量过期代际记录
        for version in 10..30 {
            mark_seen(&mut scan, PathBuf::from("f_00000"), (version, None));
        }
        evict_lru(&mut scan, 0);
        assert!(scan.seen.is_empty());
        assert!(scan.order.is_empty());
        // 状态仍可复用：重新填充后淘汰照常工作
        let mut scan = filled_state(3);
        evict_lru(&mut scan, 1);
        assert_eq!(scan.seen.len(), 1);
    }

    #[test]
    fn 触碰_热点文件不会把触碰队列撑大() {
        let mut scan = CacheScanState::default();
        for version in 0..500u64 {
            mark_seen(&mut scan, PathBuf::from("hot"), (version, None));
        }
        assert_eq!(scan.seen.len(), 1);
        assert!(
            scan.order.len() <= ORDER_BLOAT_SLACK + ORDER_BLOAT_FACTOR,
            "反复触碰应触发队列压缩，长度 {} 超过上限",
            scan.order.len()
        );
    }

    #[test]
    fn 同一指纹重复记录_不会新增触碰() {
        // 内容未变化的重读（例如 accept 拒收后的重扫）不应挤占触碰历史
        let mut scan = CacheScanState::default();
        for _ in 0..100 {
            mark_seen(&mut scan, PathBuf::from("stable"), (7, None));
        }
        assert_eq!(scan.seen.len(), 1);
        assert_eq!(scan.order.len(), 1);
    }
}
