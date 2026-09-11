//! 用量口径。
//!
//! 这是全项目最该被测试保护的模块：README 明确写着「改模型或加模型从 `model_slot()` 入手」，
//! 而历史 bug（v1.2.1 修的归并漏统计）恰恰落在这里。这些函数原先嵌在 `lib.rs` 的 `run()`
//! 内部，测试无从下手，只能靠人工打开应用对数。
//!
//! 本模块只做纯计算，不碰网络与文件——拉到响应后的原始条目由调用方反序列化后传进来。

use serde::{Deserialize, Serialize};

pub const FLASH_SLOT: &str = "flash";
pub const PRO_SLOT: &str = "pro";

/// 平台响应里的单条用量记录：`{"type":"PROMPT_CACHE_HIT_TOKEN","amount":"1234.0"}`
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    #[serde(rename = "type")]
    pub kind: String,
    pub amount: String,
}

/// 模型名映射。2026-09-10 DeepSeek 上线 V4.1 Flash，模型名改为 `deepseek-flash`。
/// 旧名 `deepseek-v4-flash` / `deepseek-v4-flash-vision-exp` 对应的模型已下线，但出于
/// 兼容仍被路由到 V4.1 Flash，因此迁移期内平台可能同时返回新旧名字，必须归并到同一槽位。
/// `deepseek-v4-pro` 自 2026-09-14 12:00（北京时间）起同样路由到 V4.1 Flash 并按 Flash
/// 计价，直到 V4.1 Pro 上线，故暂不删除 pro 槽位，仅作为历史数据承接。
///
/// 返回 `(槽位 key, 界面显示名)`；未知模型返回 `None`（调用方打 warn 并跳过）。
pub fn model_slot(model: &str) -> Option<(&'static str, &'static str)> {
    match model {
        "deepseek-flash" | "deepseek-v4-flash" | "deepseek-v4-flash-vision-exp" => {
            Some((FLASH_SLOT, "V4.1 Flash"))
        }
        "deepseek-v4-pro" => Some((PRO_SLOT, "V4 Pro")),
        _ => None,
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TokenBreakdown {
    /// 总 token = 缓存命中 + 缓存未命中 + 输出 + 未归类
    pub total: u64,
    /// 请求数是计次而非计 token，不参与 total 累加
    pub request: u64,
    pub cache_hit: u64,
    pub cache_miss: u64,
    pub response: u64,
    /// V4.1 Flash 原生多模态后，平台可能返回当前未归类的 token 类型（如图片输入）。
    /// 保守计入 total，宁可多算也不静默丢数据；单独记录便于前端提示。
    pub other: u64,
}

/// 把一组原始条目归并成结构化的 token 明细。
///
/// `PROMPT_TOKEN` 的处理是这里唯一的微妙点：它表示输入总量，而缓存命中 + 未命中通常
/// 就等于输入总量。若两者并存还累加，输入量会被重复计算。目前的策略是「有明细就不用总量
/// 兜底」，即偏保守地不重复。该互斥假设尚未用真实响应验证过——见本模块单测
/// `prompt_token_与明细并存_不重复计入`，抓真实样本后只需改那一处断言即可反向确认。
pub fn token_breakdown(usage: &[Entry]) -> TokenBreakdown {
    let mut result = TokenBreakdown::default();
    let mut prompt_total = 0u64;
    for entry in usage {
        let value = entry.amount.parse::<f64>().unwrap_or(0.0).round() as u64;
        match entry.kind.as_str() {
            "REQUEST" => result.request += value,
            "PROMPT_CACHE_HIT_TOKEN" => {
                result.cache_hit += value;
                result.total += value;
            }
            "PROMPT_CACHE_MISS_TOKEN" => {
                result.cache_miss += value;
                result.total += value;
            }
            "RESPONSE_TOKEN" => {
                result.response += value;
                result.total += value;
            }
            "PROMPT_TOKEN" => prompt_total += value,
            kind => {
                result.other += value;
                result.total += value;
                log::warn!("未归类的用量类型 {kind}，已计入 other token：{value}");
            }
        }
    }
    // 只有当平台没有给出缓存明细时，才用 PROMPT_TOKEN 兜底计入 total。
    if result.cache_hit == 0 && result.cache_miss == 0 {
        result.total += prompt_total;
    }
    result
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageModelSummary {
    pub key: String,
    pub name: String,
    pub total_tokens: u64,
    pub request_count: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub response_tokens: u64,
    /// 平台返回的、当前未归类的 token 类型（如多模态图片输入），已计入 total_tokens
    pub other_tokens: u64,
    pub cost: f64,
}

impl UsageModelSummary {
    pub fn new(key: &str, name: &str, breakdown: &TokenBreakdown, cost: f64) -> Self {
        Self {
            key: key.to_string(),
            name: name.to_string(),
            total_tokens: breakdown.total,
            request_count: breakdown.request,
            cache_hit_tokens: breakdown.cache_hit,
            cache_miss_tokens: breakdown.cache_miss,
            response_tokens: breakdown.response,
            other_tokens: breakdown.other,
            cost,
        }
    }
}

/// 迁移期内同一模型可能有多个名字（`deepseek-flash` 与旧名并存），
/// 必须**累加**而非覆盖：前端只按槽位取一个汇总行，覆盖会让另一部分用量凭空消失。
/// 这正是 v1.2.1 修的 bug，本函数是它的回归防线。
pub fn merge_model_slot(
    slot: Option<UsageModelSummary>,
    key: &str,
    name: &str,
    breakdown: &TokenBreakdown,
    cost: f64,
) -> UsageModelSummary {
    match slot {
        Some(mut existing) => {
            existing.total_tokens += breakdown.total;
            existing.request_count += breakdown.request;
            existing.cache_hit_tokens += breakdown.cache_hit;
            existing.cache_miss_tokens += breakdown.cache_miss;
            existing.response_tokens += breakdown.response;
            existing.other_tokens += breakdown.other;
            existing.cost += cost;
            existing
        }
        None => UsageModelSummary::new(key, name, breakdown, cost),
    }
}

/// 费用求和。`REQUEST` 条目的 amount 是**次数**而不是金额，必须排除，
/// 否则每月费用会被请求数污染成一个天文数字。
pub fn cost_sum(usage: &[Entry]) -> f64 {
    usage
        .iter()
        .filter(|entry| entry.kind != "REQUEST")
        .map(|entry| entry.amount.parse::<f64>().unwrap_or(0.0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, amount: &str) -> Entry {
        Entry {
            kind: kind.to_string(),
            amount: amount.to_string(),
        }
    }

    // ---------- model_slot ----------

    #[test]
    fn 模型名映射表_与_readme_一致() {
        // README 的模型口径表格直接转成断言：改口径必须先动这里。
        assert_eq!(
            model_slot("deepseek-flash"),
            Some((FLASH_SLOT, "V4.1 Flash"))
        );
        assert_eq!(
            model_slot("deepseek-v4-flash"),
            Some((FLASH_SLOT, "V4.1 Flash"))
        );
        assert_eq!(
            model_slot("deepseek-v4-flash-vision-exp"),
            Some((FLASH_SLOT, "V4.1 Flash"))
        );
        assert_eq!(model_slot("deepseek-v4-pro"), Some((PRO_SLOT, "V4 Pro")));
    }

    #[test]
    fn 未知模型名_返回_none() {
        assert_eq!(model_slot(""), None);
        assert_eq!(model_slot("deepseek-chat"), None);
        assert_eq!(model_slot("deepseek-reasoner"), None);
        // 大小写敏感：平台返回的是小写规范名，拼错不该被静默归并
        assert_eq!(model_slot("DeepSeek-Flash"), None);
        // 前后空格不应被容错吞掉（原始值直接比对）
        assert_eq!(model_slot(" deepseek-flash"), None);
    }

    // ---------- token_breakdown ----------

    #[test]
    fn 六种_kind_的归并与累加() {
        let usage = vec![
            entry("PROMPT_CACHE_HIT_TOKEN", "100.0"),
            entry("PROMPT_CACHE_MISS_TOKEN", "200.0"),
            entry("RESPONSE_TOKEN", "50.0"),
            entry("REQUEST", "3.0"),
            entry("MULTIMODAL_IMAGE_TOKEN", "7.0"),
        ];
        let b = token_breakdown(&usage);
        assert_eq!(b.cache_hit, 100);
        assert_eq!(b.cache_miss, 200);
        assert_eq!(b.response, 50);
        assert_eq!(b.request, 3);
        assert_eq!(b.other, 7);
        // total 不含 request（计次而非计 token）
        assert_eq!(b.total, 100 + 200 + 50 + 7);
    }

    #[test]
    fn 同种_kind_多条_累加() {
        let usage = vec![
            entry("PROMPT_CACHE_HIT_TOKEN", "10.0"),
            entry("PROMPT_CACHE_HIT_TOKEN", "20.5"),
            entry("RESPONSE_TOKEN", "1.0"),
        ];
        let b = token_breakdown(&usage);
        assert_eq!(b.cache_hit, 31); // 10 + 20.5 → round 后 10 + 21
        assert_eq!(b.total, 31 + 1);
    }

    #[test]
    fn 小数_按四舍五入归整() {
        let b = token_breakdown(&[
            entry("RESPONSE_TOKEN", "1.4"),
            entry("RESPONSE_TOKEN", "1.6"),
        ]);
        assert_eq!(b.response, 1 + 2);
    }

    #[test]
    fn 无法解析的_amount_按零处理且不灾难退出() {
        let b = token_breakdown(&[
            entry("RESPONSE_TOKEN", "not-a-number"),
            entry("RESPONSE_TOKEN", ""),
            entry("PROMPT_CACHE_HIT_TOKEN", "5"),
        ]);
        assert_eq!(b.response, 0);
        assert_eq!(b.cache_hit, 5);
        assert_eq!(b.total, 5);
    }

    #[test]
    fn 只有_prompt_token_时_兜底计入总量() {
        let b = token_breakdown(&[entry("PROMPT_TOKEN", "800.0")]);
        assert_eq!(b.total, 800);
        assert_eq!(b.cache_hit, 0);
        assert_eq!(b.cache_miss, 0);
    }

    #[test]
    fn prompt_token_与明细并存_不重复计入() {
        // L-16 的双计边界，也是当前实现里唯一「未经真实样本验证」的假设。
        // 若平台实际同时返回三者且 PROMPT_TOKEN ≠ HIT + MISS，输入量会被**少计**；
        // 拿到真实响应样本后，把这里的 500 改成实际期望值即可反向确认真伪。
        let b = token_breakdown(&[
            entry("PROMPT_TOKEN", "300.0"),
            entry("PROMPT_CACHE_HIT_TOKEN", "100.0"),
            entry("PROMPT_CACHE_MISS_TOKEN", "200.0"),
            entry("RESPONSE_TOKEN", "50.0"),
        ]);
        assert_eq!(b.total, 350, "有缓存明细时不得把 PROMPT_TOKEN 再算一遍");
        assert_eq!(b.cache_hit + b.cache_miss, 300);
    }

    #[test]
    fn 只有命中明细_无未命中时_也算有明细() {
        // 边界：cache_miss == 0 但 cache_hit > 0，仍应视为「平台给了明细」
        let b = token_breakdown(&[
            entry("PROMPT_TOKEN", "999.0"),
            entry("PROMPT_CACHE_HIT_TOKEN", "10.0"),
        ]);
        assert_eq!(b.total, 10);
    }

    #[test]
    fn 只有未命中明细_无命中时_也算有明细() {
        let b = token_breakdown(&[
            entry("PROMPT_TOKEN", "999.0"),
            entry("PROMPT_CACHE_MISS_TOKEN", "10.0"),
        ]);
        assert_eq!(b.total, 10);
    }

    #[test]
    fn 空输入_全零() {
        assert_eq!(token_breakdown(&[]), TokenBreakdown::default());
    }

    // ---------- merge_model_slot ----------

    #[test]
    fn 首次归并_直接采用明细() {
        let b = token_breakdown(&[
            entry("PROMPT_CACHE_HIT_TOKEN", "10.0"),
            entry("RESPONSE_TOKEN", "5.0"),
            entry("REQUEST", "2.0"),
        ]);
        let merged = merge_model_slot(None, FLASH_SLOT, "V4.1 Flash", &b, 1.25);
        assert_eq!(merged.key, FLASH_SLOT);
        assert_eq!(merged.name, "V4.1 Flash");
        assert_eq!(merged.total_tokens, 15);
        assert_eq!(merged.request_count, 2);
        assert_eq!(merged.cache_hit_tokens, 10);
        assert_eq!(merged.response_tokens, 5);
        assert_eq!(merged.cost, 1.25);
    }

    #[test]
    fn 新旧模型名并存_求和而非覆盖() {
        // v1.2.1 修的 bug 的回归测试：迁移期 deepseek-flash 与 deepseek-v4-flash
        // 会同时出现在账单里，它们其实是同一个模型。
        let old_name = token_breakdown(&[
            entry("PROMPT_CACHE_HIT_TOKEN", "100.0"),
            entry("PROMPT_CACHE_MISS_TOKEN", "200.0"),
            entry("RESPONSE_TOKEN", "50.0"),
            entry("REQUEST", "4.0"),
        ]);
        let new_name = token_breakdown(&[
            entry("PROMPT_CACHE_HIT_TOKEN", "1.0"),
            entry("PROMPT_CACHE_MISS_TOKEN", "2.0"),
            entry("RESPONSE_TOKEN", "3.0"),
            entry("REQUEST", "1.0"),
        ]);

        let first = merge_model_slot(None, FLASH_SLOT, "V4.1 Flash", &old_name, 10.0);
        let merged = merge_model_slot(Some(first), FLASH_SLOT, "V4.1 Flash", &new_name, 0.5);

        assert_eq!(merged.total_tokens, 350 + 6);
        assert_eq!(merged.cache_hit_tokens, 101);
        assert_eq!(merged.cache_miss_tokens, 202);
        assert_eq!(merged.response_tokens, 53);
        assert_eq!(merged.request_count, 5);
        assert_eq!(merged.cost, 10.5);
    }

    #[test]
    fn 三次归并_累计不丢() {
        let b = token_breakdown(&[entry("RESPONSE_TOKEN", "10.0")]);
        let once = merge_model_slot(None, FLASH_SLOT, "V4.1 Flash", &b, 1.0);
        let twice = merge_model_slot(Some(once), FLASH_SLOT, "V4.1 Flash", &b, 1.0);
        let thrice = merge_model_slot(Some(twice), FLASH_SLOT, "V4.1 Flash", &b, 1.0);
        assert_eq!(
            thrice.total_tokens, 30,
            "三个模型名应累加成 30，而非停留在 10"
        );
        assert_eq!(thrice.cost, 3.0);
    }

    #[test]
    fn 归并_保留未归类_token() {
        let b = token_breakdown(&[entry("MULTIMODAL_IMAGE_TOKEN", "42.0")]);
        let merged = merge_model_slot(None, FLASH_SLOT, "V4.1 Flash", &b, 0.0);
        assert_eq!(merged.other_tokens, 42);
        assert_eq!(merged.total_tokens, 42);
    }

    // ---------- cost_sum ----------

    #[test]
    fn 费用求和_排除_request_条目() {
        let usage = vec![
            entry("REQUEST", "120.0"), // 次数，不是钱
            entry("PROMPT_CACHE_HIT_TOKEN", "0.5"),
            entry("RESPONSE_TOKEN", "1.5"),
        ];
        assert!((cost_sum(&usage) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn 费用求和_无法解析按零() {
        let usage = vec![
            entry("RESPONSE_TOKEN", "abc"),
            entry("RESPONSE_TOKEN", "2.5"),
        ];
        assert!((cost_sum(&usage) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn 费用求和_空输入为零() {
        assert_eq!(cost_sum(&[]), 0.0);
    }
}
