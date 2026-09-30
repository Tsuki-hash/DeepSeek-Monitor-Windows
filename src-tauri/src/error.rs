//! IPC 错误契约。前端按稳定 code 判定状态，message 只负责展示。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
    pub credential: Option<&'static str>,
    pub retryable: bool,
}
impl CommandError {
    pub fn new(message: String, credential: Option<&'static str>) -> Self {
        let code = if message.contains("未配置") {
            "not_configured"
        } else if message.contains("取消") || message.contains("替代") {
            "cancelled"
        } else if message.contains("无效")
            || message.contains("过期")
            || message.contains("拒绝访问")
        {
            "credentials_invalid"
        } else if message.contains("过于频繁") {
            "rate_limited"
        } else if message.contains("加密") {
            "encryption_unavailable"
        } else if ["解析", "非法", "统计口径", "超出范围"]
            .iter()
            .any(|s| message.contains(s))
        {
            "invalid_data"
        } else if ["网络", "请求失败", "服务器", "暂时"]
            .iter()
            .any(|s| message.contains(s))
        {
            "unavailable"
        } else {
            "unknown"
        };
        Self {
            code,
            message,
            credential,
            retryable: matches!(code, "rate_limited" | "unavailable"),
        }
    }
}
impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::new(message, None)
    }
}
impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        message.to_string().into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn 凭据缺失与网络故障区分_序列化保留稳定状态码() {
        let error = CommandError::new("未配置用量 Token".into(), Some("usage_token"));
        assert_eq!(error.code, "not_configured");
        assert!(!error.retryable);
        let json = serde_json::to_value(error).unwrap();
        assert_eq!(json["credential"], "usage_token");
        assert!(CommandError::new("网络请求失败".into(), None).retryable);
    }
}
