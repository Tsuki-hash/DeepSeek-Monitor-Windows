//! 复用的 HTTP 客户端与超时配置。
//!
//! `reqwest::Client` 内含连接池与 TLS 会话，官方建议复用；每次请求都新建会让
//! 每轮自动刷新重做 TCP + TLS 握手。UA 与超时集中在此定义，避免调用点各写一份。

use std::{sync::OnceLock, time::Duration};

pub const HTTP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
                                   AppleWebKit/537.36 (KHTML, like Gecko) \
                                   Chrome/148.0.0.0 Safari/537.36";
pub const HTTP_TIMEOUT_SECONDS: u64 = 15;

pub fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(HTTP_USER_AGENT)
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECONDS))
            .build()
            // 极少失败；失败时退回默认 Client，避免 panic 导致托盘进程直接退出
            .unwrap_or_default()
    })
}
