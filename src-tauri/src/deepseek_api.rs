//! DeepSeek 平台 HTTP 业务：余额、用量拉取与聚合。
//!
//! 网络 I/O 与「响应 → 界面模型」的纯聚合拆开，后者可直接单测（含反序列化样本），
//! 不必启动 Tauri 或打真实接口。

use crate::http::http_client;
use crate::usage::{
    cost_sum, merge_model_slot, model_slot, token_breakdown, Entry, UsageModelSummary, FLASH_SLOT,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceResult {
    pub is_available: bool,
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

#[derive(Debug, Deserialize)]
struct BalanceInfo {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    is_available: bool,
    balance_infos: Vec<BalanceInfo>,
}

/// 纯解析：余额 JSON → 结构。供单测直接喂样本。
fn parse_balance(data: BalanceResponse) -> Result<BalanceResult, String> {
    let info = data
        .balance_infos
        .into_iter()
        .next()
        .ok_or_else(|| "余额信息为空".to_string())?;
    Ok(BalanceResult {
        is_available: data.is_available,
        currency: info.currency,
        total_balance: info.total_balance,
        granted_balance: info.granted_balance,
        topped_up_balance: info.topped_up_balance,
    })
}

// BalanceResponse 字段私有，测试在本模块内反序列化后调用 parse_balance。
impl BalanceResponse {
    fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|error| format!("解析余额数据失败：{error}"))
    }
}

/// 实时查询 DeepSeek 账户余额。
pub async fn fetch_balance_with_key(api_key: &str) -> Result<BalanceResult, String> {
    let client = http_client();
    let response = client
        .get("https://api.deepseek.com/user/balance")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|error| format!("网络请求失败：{error}"))?;

    match response.status().as_u16() {
        200 => {}
        401 => return Err("API Key 无效或已过期".to_string()),
        429 => return Err("请求过于频繁，请稍后再试".to_string()),
        code if code >= 500 => return Err(format!("DeepSeek 服务器错误：{code}")),
        code => return Err(format!("请求失败：HTTP {code}")),
    }

    let text = response
        .text()
        .await
        .map_err(|error| format!("读取余额响应失败：{error}"))?;
    parse_balance(BalanceResponse::from_json(&text)?)
}

/// 用 token 试调平台用量接口，验证它确实是有效的用量 token。
pub async fn verify_usage_token(token: &str, month: u32, year: u32) -> Result<(), String> {
    let url =
        format!("https://platform.deepseek.com/api/v0/usage/amount?month={month}&year={year}");
    let resp = http_client()
        .get(&url)
        .bearer_auth(token)
        .header("x-app-version", "1.0.0")
        .header("Accept", "*/*")
        .send()
        .await
        .map_err(|error| format!("验证 token 失败：{error}"))?;
    if resp.status().as_u16() == 200 {
        Ok(())
    } else {
        Err(format!(
            "token 校验未通过（HTTP {}），请重新同步或手动粘贴用量 Token",
            resp.status().as_u16()
        ))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub usage: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DayUsage {
    pub date: String,
    pub data: Vec<ModelUsage>,
}

#[derive(Debug, Deserialize)]
pub struct AmountBiz {
    pub total: Vec<ModelUsage>,
    pub days: Vec<DayUsage>,
}

#[derive(Debug, Deserialize)]
pub struct AmountData {
    pub biz_data: AmountBiz,
}

#[derive(Debug, Deserialize)]
pub struct AmountResp {
    pub data: AmountData,
}

#[derive(Debug, Deserialize)]
pub struct CostBiz {
    pub total: Vec<ModelUsage>,
    pub days: Vec<DayUsage>,
}

#[derive(Debug, Deserialize)]
pub struct CostData {
    pub biz_data: Vec<CostBiz>,
}

#[derive(Debug, Deserialize)]
pub struct CostResp {
    pub data: CostData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDaySummary {
    pub date: String,
    pub flash_tokens: u64,
    pub flash_cache_hit: u64,
    pub flash_cache_miss: u64,
    pub flash_response: u64,
    pub pro_tokens: u64,
    pub pro_cache_hit: u64,
    pub pro_cache_miss: u64,
    pub pro_response: u64,
    pub flash_other_tokens: u64,
    pub pro_other_tokens: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageResult {
    pub models: Vec<UsageModelSummary>,
    pub days: Vec<UsageDaySummary>,
    pub month_cost: f64,
}

/// 纯聚合：amount/cost 响应 → 界面用量结构。新旧模型名归并、费用口径均在此。
pub fn build_usage_result(amount: &AmountResp, cost: &CostResp) -> UsageResult {
    let cost_total = cost.data.biz_data.first();
    let cost_for_model = |model: &str| -> f64 {
        cost_total
            .and_then(|item| item.total.iter().find(|m| m.model == model))
            .map(|m| cost_sum(&m.usage))
            .unwrap_or(0.0)
    };

    // 按槽位归并：迁移期内 deepseek-flash 与旧名可能同时出现在同一份账单里，
    // 它们其实是同一个模型，必须累加，否则前端只取第一个会漏掉另一部分用量。
    let mut flash_sum: Option<UsageModelSummary> = None;
    let mut pro_sum: Option<UsageModelSummary> = None;
    // 未知模型（如后续上线的 V4.1 Pro）聚合为「其他」兜底行，避免新模型名
    // 接入前其用量「日合计有、模型行无」地静默消失。正式接入仍以 model_slot 为准。
    let mut other_sum: Option<UsageModelSummary> = None;
    for model_usage in &amount.data.biz_data.total {
        let breakdown = token_breakdown(&model_usage.usage);
        let cost = cost_for_model(&model_usage.model);
        match model_slot(&model_usage.model) {
            Some((slot, display)) if slot == FLASH_SLOT => {
                flash_sum = Some(merge_model_slot(
                    flash_sum.take(),
                    slot,
                    display,
                    &breakdown,
                    cost,
                ));
            }
            Some((slot, display)) => {
                pro_sum = Some(merge_model_slot(
                    pro_sum.take(),
                    slot,
                    display,
                    &breakdown,
                    cost,
                ));
            }
            None => {
                log::warn!("未知模型 {}，已聚合进「其他」行", model_usage.model);
                other_sum = Some(merge_model_slot(
                    other_sum.take(),
                    "other",
                    "其他",
                    &breakdown,
                    cost,
                ));
            }
        }
    }

    let mut models = Vec::new();
    models.extend(flash_sum);
    models.extend(pro_sum);
    // 本月没有未知模型时不出「其他」行，避免常态下多一行空白
    if let Some(other) = other_sum.filter(|sum| sum.total_tokens > 0 || sum.cost != 0.0) {
        models.push(other);
    }

    let mut cost_by_date: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    if let Some(item) = cost_total {
        for day in &item.days {
            let day_cost: f64 = day.data.iter().map(|m| cost_sum(&m.usage)).sum();
            cost_by_date.insert(day.date.clone(), day_cost);
        }
    }

    let mut days = Vec::new();
    for day in &amount.data.biz_data.days {
        let mut flash = 0u64;
        let mut flash_hit = 0u64;
        let mut flash_miss = 0u64;
        let mut flash_resp = 0u64;
        let mut pro = 0u64;
        let mut pro_hit = 0u64;
        let mut pro_miss = 0u64;
        let mut pro_resp = 0u64;
        let mut total = 0u64;
        let mut flash_other = 0u64;
        let mut pro_other = 0u64;
        for model_usage in &day.data {
            let breakdown = token_breakdown(&model_usage.usage);
            total += breakdown.total;
            // total 覆盖当天全部模型（含未知模型），槽位分摊只作用于已识别的模型
            if let Some((slot, _)) = model_slot(&model_usage.model) {
                if slot == FLASH_SLOT {
                    flash += breakdown.total;
                    flash_hit += breakdown.cache_hit;
                    flash_miss += breakdown.cache_miss;
                    flash_resp += breakdown.response;
                    flash_other += breakdown.other;
                } else {
                    pro += breakdown.total;
                    pro_hit += breakdown.cache_hit;
                    pro_miss += breakdown.cache_miss;
                    pro_resp += breakdown.response;
                    pro_other += breakdown.other;
                }
            }
        }
        days.push(UsageDaySummary {
            date: day.date.clone(),
            flash_tokens: flash,
            flash_cache_hit: flash_hit,
            flash_cache_miss: flash_miss,
            flash_response: flash_resp,
            pro_tokens: pro,
            pro_cache_hit: pro_hit,
            pro_cache_miss: pro_miss,
            pro_response: pro_resp,
            flash_other_tokens: flash_other,
            pro_other_tokens: pro_other,
            total_tokens: total,
            total_cost: cost_by_date.get(&day.date).copied().unwrap_or(0.0),
        });
    }

    let month_cost: f64 = cost_total
        .map(|item| item.total.iter().map(|m| cost_sum(&m.usage)).sum())
        .unwrap_or(0.0);

    UsageResult {
        models,
        days,
        month_cost,
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    token: &str,
) -> Result<T, String> {
    let resp = client
        .get(url)
        .bearer_auth(token)
        .header("x-app-version", "1.0.0")
        .header("Accept", "*/*")
        .send()
        .await
        .map_err(|error| format!("用量请求失败：{error}"))?;
    match resp.status().as_u16() {
        200 => {}
        401 => return Err("用量 Token 无效或已过期，请重新同步用量 Token（设置页）".to_string()),
        403 => return Err("用量接口拒绝访问，请重新同步用量 Token（设置页）".to_string()),
        404 => return Err("用量接口路径可能已变更，可稍后重试或使用手动粘贴 Token".to_string()),
        429 => return Err("请求过于频繁，请稍后再试".to_string()),
        code if code >= 500 => {
            return Err(format!(
                "用量服务暂时不可用（HTTP {code}），余额查询不受影响"
            ))
        }
        code => return Err(format!("用量接口错误：HTTP {code}，余额查询不受影响")),
    }
    resp.json::<T>()
        .await
        .map_err(|error| format!("解析用量数据失败：{error}"))
}

/// 并发拉取 amount/cost 并聚合成界面结构。
pub async fn fetch_usage_with_token(
    token: &str,
    month: u32,
    year: u32,
) -> Result<UsageResult, String> {
    let client = http_client();
    let amount_url =
        format!("https://platform.deepseek.com/api/v0/usage/amount?month={month}&year={year}");
    let cost_url =
        format!("https://platform.deepseek.com/api/v0/usage/cost?month={month}&year={year}");

    // 两个端点互相独立、同一 token，串行等待会让总延迟等于两者之和（各自含 15s 超时上限）。
    let (amount, cost): (AmountResp, CostResp) = tokio::try_join!(
        get_json(client, &amount_url, token),
        get_json(client, &cost_url, token),
    )?;

    Ok(build_usage_result(&amount, &cost))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amount_sample() -> AmountResp {
        let json = r#"{
            "data": {
                "biz_data": {
                    "total": [
                        {
                            "model": "deepseek-flash",
                            "usage": [
                                {"type":"PROMPT_CACHE_HIT_TOKEN","amount":"100.0"},
                                {"type":"PROMPT_CACHE_MISS_TOKEN","amount":"50.0"},
                                {"type":"RESPONSE_TOKEN","amount":"25.0"},
                                {"type":"REQUEST","amount":"3.0"}
                            ]
                        },
                        {
                            "model": "deepseek-v4-flash",
                            "usage": [
                                {"type":"PROMPT_CACHE_HIT_TOKEN","amount":"10.0"},
                                {"type":"RESPONSE_TOKEN","amount":"5.0"}
                            ]
                        }
                    ],
                    "days": [
                        {
                            "date": "2026-09-01",
                            "data": [
                                {
                                    "model": "deepseek-flash",
                                    "usage": [
                                        {"type":"PROMPT_CACHE_HIT_TOKEN","amount":"40.0"},
                                        {"type":"PROMPT_CACHE_MISS_TOKEN","amount":"20.0"},
                                        {"type":"RESPONSE_TOKEN","amount":"10.0"}
                                    ]
                                }
                            ]
                        }
                    ]
                }
            }
        }"#;
        serde_json::from_str(json).unwrap()
    }

    fn cost_sample() -> CostResp {
        let json = r#"{
            "data": {
                "biz_data": [
                    {
                        "total": [
                            {
                                "model": "deepseek-flash",
                                "usage": [
                                    {"type":"PROMPT_CACHE_HIT_TOKEN","amount":"0.10"},
                                    {"type":"RESPONSE_TOKEN","amount":"0.20"},
                                    {"type":"REQUEST","amount":"9"}
                                ]
                            }
                        ],
                        "days": [
                            {
                                "date": "2026-09-01",
                                "data": [
                                    {
                                        "model": "deepseek-flash",
                                        "usage": [
                                            {"type":"RESPONSE_TOKEN","amount":"0.05"}
                                        ]
                                    }
                                ]
                            }
                        ]
                    }
                ]
            }
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn 余额样本_解析出第一条() {
        let raw = r#"{
            "is_available": true,
            "balance_infos": [
                {
                    "currency": "CNY",
                    "total_balance": "12.34",
                    "granted_balance": "1.00",
                    "topped_up_balance": "11.34"
                }
            ]
        }"#;
        let parsed = BalanceResponse::from_json(raw).unwrap();
        let balance = parse_balance(parsed).unwrap();
        assert!(balance.is_available);
        assert_eq!(balance.currency, "CNY");
        assert_eq!(balance.total_balance, "12.34");
    }

    #[test]
    fn 余额样本_空列表报错() {
        let raw = r#"{"is_available": true, "balance_infos": []}"#;
        let parsed = BalanceResponse::from_json(raw).unwrap();
        assert!(parse_balance(parsed).is_err());
    }

    #[test]
    fn 用量聚合_新旧模型名归并到同一行() {
        let result = build_usage_result(&amount_sample(), &cost_sample());
        assert_eq!(result.models.len(), 1);
        let flash = &result.models[0];
        assert_eq!(flash.key, "flash");
        assert_eq!(flash.name, "V4.1 Flash");
        assert_eq!(flash.cache_hit_tokens, 110);
        assert_eq!(flash.response_tokens, 30);
        assert_eq!(flash.request_count, 3);
        // 0.10 + 0.20（REQUEST 不计费用）
        assert!((flash.cost - 0.30).abs() < 1e-9);
        assert!((result.month_cost - 0.30).abs() < 1e-9);
    }

    #[test]
    fn 用量聚合_按日费用对齐日期() {
        let result = build_usage_result(&amount_sample(), &cost_sample());
        assert_eq!(result.days.len(), 1);
        assert_eq!(result.days[0].date, "2026-09-01");
        assert!((result.days[0].total_cost - 0.05).abs() < 1e-9);
        assert_eq!(result.days[0].total_tokens, 70);
    }

    #[test]
    fn 用量聚合_未知模型聚合为其他行() {
        let amount_json = r#"{
            "data": {
                "biz_data": {
                    "total": [
                        {
                            "model": "deepseek-mystery",
                            "usage": [
                                {"type":"PROMPT_CACHE_HIT_TOKEN","amount":"5.0"},
                                {"type":"RESPONSE_TOKEN","amount":"5.0"}
                            ]
                        }
                    ],
                    "days": [
                        {
                            "date": "2026-09-02",
                            "data": [
                                {
                                    "model": "deepseek-mystery",
                                    "usage": [
                                        {"type":"PROMPT_CACHE_HIT_TOKEN","amount":"5.0"},
                                        {"type":"RESPONSE_TOKEN","amount":"5.0"}
                                    ]
                                }
                            ]
                        }
                    ]
                }
            }
        }"#;
        let cost_json = r#"{"data":{"biz_data":[]}}"#;
        let amount: AmountResp = serde_json::from_str(amount_json).unwrap();
        let cost: CostResp = serde_json::from_str(cost_json).unwrap();
        let result = build_usage_result(&amount, &cost);
        // 未知模型聚合为「其他」兜底行，而不是静默丢失
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].key, "other");
        assert_eq!(result.models[0].name, "其他");
        assert_eq!(result.models[0].total_tokens, 10);
        assert_eq!(result.days[0].total_tokens, 10);
    }

    #[test]
    fn 用量聚合_仅在days中出现的未知模型_计入日total但不出行() {
        // 边界口径：模型行一律来自月度 total；只在 days 出现的模型不生成
        // 「其他」行（其 token 仍计入当日合计）。记录该边界，改动须是刻意的。
        let amount_json = r#"{
            "data": {
                "biz_data": {
                    "total": [],
                    "days": [
                        {
                            "date": "2026-09-02",
                            "data": [
                                {
                                    "model": "deepseek-mystery",
                                    "usage": [
                                        {"type":"PROMPT_CACHE_HIT_TOKEN","amount":"5.0"},
                                        {"type":"RESPONSE_TOKEN","amount":"5.0"}
                                    ]
                                }
                            ]
                        }
                    ]
                }
            }
        }"#;
        let cost_json = r#"{"data":{"biz_data":[]}}"#;
        let amount: AmountResp = serde_json::from_str(amount_json).unwrap();
        let cost: CostResp = serde_json::from_str(cost_json).unwrap();
        let result = build_usage_result(&amount, &cost);
        assert!(result.models.is_empty());
        assert_eq!(result.days[0].total_tokens, 10);
    }

    #[test]
    fn 用量聚合_多个未知模型累加进其他行() {
        let amount_json = r#"{
            "data": {
                "biz_data": {
                    "total": [
                        {"model": "deepseek-mystery-a", "usage": [{"type":"RESPONSE_TOKEN","amount":"10.0"}]},
                        {"model": "deepseek-mystery-b", "usage": [{"type":"RESPONSE_TOKEN","amount":"6.0"}]}
                    ],
                    "days": []
                }
            }
        }"#;
        let cost_json = r#"{"data":{"biz_data":[]}}"#;
        let amount: AmountResp = serde_json::from_str(amount_json).unwrap();
        let cost: CostResp = serde_json::from_str(cost_json).unwrap();
        let result = build_usage_result(&amount, &cost);
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].key, "other");
        assert_eq!(result.models[0].total_tokens, 16);
    }
}
