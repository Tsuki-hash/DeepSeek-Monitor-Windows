// 前端纯函数的单测。
//
// 运行方式：`npm test` → `node --test --experimental-strip-types src/`。
// Node 22 内置类型剥离可直接 import .ts（前提：只用可擦除的 TS 语法，不用 enum / namespace），
// 因此整个前端测试链不需要任何 devDependency。
//
// 覆盖范围刻意只挑「算错了不容易被发现」的函数：跨月补零、单位换算阈值。

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  addDays,
  chartPointFromDay,
  currencySymbol,
  dateKey,
  emptyUsageDay,
  fmtInt,
  fmtMoney,
  fmtTokensShort,
  mmdd,
  nextLoadStateAfterError,
  previousMonth,
  recentUsageDays,
  todayStr,
  type UsageDay,
} from "./format.ts";

const day = (date: string, totalTokens: number, totalCost = 0): UsageDay => ({
  ...emptyUsageDay(date),
  totalTokens,
  totalCost,
});

// ---------- fmtTokensShort ----------

test("fmtTokensShort：各档阈值", () => {
  assert.equal(fmtTokensShort(0), "0");
  assert.equal(fmtTokensShort(999), "999");
  assert.equal(fmtTokensShort(1000), "1.0K");
  assert.equal(fmtTokensShort(1234), "1.2K");
  assert.equal(fmtTokensShort(268400), "268.4K");
  assert.equal(fmtTokensShort(999499), "999.5K");
  // 关键边界：999500 不能输出 "1000.0K"（7 字符，比下一档还长）
  assert.equal(fmtTokensShort(999500), "1.0M");
  assert.equal(fmtTokensShort(1e6), "1.0M");
  assert.equal(fmtTokensShort(1234567), "1.2M");
  assert.equal(fmtTokensShort(99999999), "100.0M");
  // 1e8 起改用整百万，避免出现「100.0M」这种带小数的大数
  assert.equal(fmtTokensShort(1e8), "100M");
  assert.equal(fmtTokensShort(2.5e8), "250M");
});

test("fmtTokensShort：输出长度可控（柱顶标签的硬约束）", () => {
  // 356px 宽、7 列布局下每列约 34px，10px 字号最多容得下 6 个字符。
  // 这条断言是柱顶数值不再被 ellipsis 裁掉的护栏。
  const samples = [
    0, 999, 1000, 9999, 99999, 999499, 999500, 1e6, 9.9e6, 99999999, 1e8, 9.9e8,
  ];
  for (const value of samples) {
    assert.ok(
      fmtTokensShort(value).length <= 6,
      `${value} 格式化后长度 ${fmtTokensShort(value).length} 超过 6：${fmtTokensShort(value)}`,
    );
  }
});

test("fmtTokensShort：负数与小数不产生异常输出", () => {
  assert.equal(fmtTokensShort(-1), "-1");
  assert.equal(fmtTokensShort(0.4), "0");
});

// ---------- fmtInt / fmtMoney / currencySymbol ----------

test("fmtInt：千分位与四舍五入", () => {
  assert.equal(fmtInt(0), "0");
  assert.equal(fmtInt(1234), "1,234");
  assert.equal(fmtInt(1234567.6), "1,234,568");
  assert.equal(fmtInt(268400), "268,400");
});

test("fmtMoney：默认人民币，固定两位小数", () => {
  assert.equal(fmtMoney(0), "¥0.00");
  assert.equal(fmtMoney(12.345), "¥12.35");
  assert.equal(fmtMoney(12.3, "$"), "$12.30");
});

test("currencySymbol：USD 用美元符号，其余一律人民币", () => {
  assert.equal(currencySymbol("USD"), "$");
  assert.equal(currencySymbol("CNY"), "¥");
  assert.equal(currencySymbol(undefined), "¥");
  assert.equal(currencySymbol(""), "¥");
});

// ---------- 日期工具 ----------

test("dateKey：补零到两位，且不受时区偏移影响", () => {
  assert.equal(dateKey(new Date(2026, 0, 1)), "2026-01-01");
  assert.equal(dateKey(new Date(2026, 8, 11)), "2026-09-11");
  // 本地午夜不能因 toISOString 的 UTC 转换回退成前一天
  assert.equal(dateKey(new Date(2026, 8, 11, 0, 0, 0)), "2026-09-11");
  assert.equal(dateKey(new Date(2026, 8, 11, 23, 59, 59)), "2026-09-11");
});

test("todayStr：与 dateKey 口径一致", () => {
  assert.equal(todayStr(), dateKey(new Date()));
});

test("addDays：跨月与跨年", () => {
  assert.equal(dateKey(addDays(new Date(2026, 8, 11), -1)), "2026-09-10");
  assert.equal(dateKey(addDays(new Date(2026, 8, 1), -1)), "2026-08-31");
  assert.equal(dateKey(addDays(new Date(2026, 0, 1), -1)), "2025-12-31");
  assert.equal(dateKey(addDays(new Date(2025, 11, 31), 1)), "2026-01-01");
  // 不修改入参
  const base = new Date(2026, 8, 11);
  addDays(base, 5);
  assert.equal(dateKey(base), "2026-09-11");
});

test("mmdd：三段式日期转月/日，异常输入原样返回", () => {
  assert.equal(mmdd("2026-09-11"), "9/11");
  assert.equal(mmdd("2026-01-01"), "1/1");
  // 不补零到两位：柱状图下方标签空间紧张
  assert.equal(mmdd("bad"), "bad");
  assert.equal(mmdd("2026-09"), "2026-09");
});

test("previousMonth：跨年回退", () => {
  assert.deepEqual(previousMonth(new Date(2026, 8, 11)), {
    month: 8,
    year: 2026,
  });
  assert.deepEqual(previousMonth(new Date(2026, 0, 15)), {
    month: 12,
    year: 2025,
  });
  assert.deepEqual(previousMonth(new Date(2026, 11, 31)), {
    month: 11,
    year: 2026,
  });
});

// ---------- recentUsageDays ----------

test("recentUsageDays：固定返回 7 天且按时间升序", () => {
  const result = recentUsageDays([]);
  assert.equal(result.length, 7);
  const dates = result.map((item) => item.date);
  assert.deepEqual(dates, [...dates].sort(), "应按日期升序排列");
  assert.equal(dates[6], todayStr(), "最后一天必须是今天");
});

test("recentUsageDays：接口没返回的日期补零", () => {
  const today = new Date();
  const yesterday = dateKey(addDays(today, -1));
  const result = recentUsageDays([day(yesterday, 1000)]);
  const filled = result.find((item) => item.date === yesterday);
  assert.equal(filled?.totalTokens, 1000);
  // 其余 6 天必须是零值，而不是 undefined
  for (const item of result) {
    if (item.date === yesterday) continue;
    assert.equal(item.totalTokens, 0);
    assert.equal(item.flashTokens, 0);
    assert.equal(item.totalCost, 0);
  }
});

test("recentUsageDays：跨月窗口能正确回溯到上月", () => {
  // 无论今天是几号，窗口起点都应是「今天减 6 天」——月初时必然落在上个月。
  // 这条是跨月补零逻辑的护栏：若改成「拿接口最后一天往前数」，这里会失败。
  const result = recentUsageDays([]);
  assert.equal(result[0].date, dateKey(addDays(new Date(), -6)));
  assert.equal(result[0].date < result[6].date, true, "窗口起点必须早于终点");
});

test("recentUsageDays：过滤掉未来日期的占位行", () => {
  const tomorrow = dateKey(addDays(new Date(), 1));
  const farFuture = dateKey(addDays(new Date(), 30));
  const result = recentUsageDays([day(tomorrow, 999), day(farFuture, 888)]);
  assert.equal(result.length, 7);
  assert.equal(
    result.some((item) => item.totalTokens === 999 || item.totalTokens === 888),
    false,
    "未来日期的数据不应出现在窗口里（接口月初会返回整月占位行）",
  );
});

test("recentUsageDays：自定义窗口长度", () => {
  assert.equal(recentUsageDays([], 1).length, 1);
  assert.equal(recentUsageDays([], 30).length, 30);
  assert.equal(recentUsageDays([], 30)[29].date, todayStr());
});

test("recentUsageDays：同一天出现多条时取最后一条", () => {
  // Map 构造保证后者覆盖前者；这里锁定该行为，避免将来改成 reduce 时语义漂移
  const today = todayStr();
  const result = recentUsageDays([day(today, 100), day(today, 200)]);
  assert.equal(result[6].totalTokens, 200);
});

test("recentUsageDays：不修改入参数组", () => {
  const input = [day("2026-09-01", 1)];
  const snapshot = JSON.stringify(input);
  recentUsageDays(input);
  assert.equal(JSON.stringify(input), snapshot);
});

// ---------- chartPointFromDay ----------

test("chartPointFromDay：all 用 totalTokens 吞掉未识别模型差额", () => {
  const sample: UsageDay = {
    ...emptyUsageDay("2026-09-11"),
    flashCacheHit: 100,
    flashCacheMiss: 200,
    flashResponse: 50,
    proCacheHit: 10,
    // 分段和 = 360，totalTokens = 500 → 差额 140 进 other
    totalTokens: 500,
  };
  const point = chartPointFromDay(sample, "all");
  assert.equal(point.hit, 110);
  assert.equal(point.miss, 200);
  assert.equal(point.response, 50);
  assert.equal(point.other, 140);
  assert.equal(point.total, 500);
  assert.equal(
    point.hit + point.miss + point.response + point.other,
    point.total,
  );
});

test("chartPointFromDay：all 在 totalTokens 缺失时退回分段和", () => {
  const sample: UsageDay = {
    ...emptyUsageDay("2026-09-11"),
    flashCacheHit: 10,
    flashOtherTokens: 5,
    totalTokens: 0,
  };
  const point = chartPointFromDay(sample, "all");
  assert.equal(point.total, 15);
  assert.equal(point.other, 5);
});

test("nextLoadStateAfterError：silent 保留 ok，否则标 error/nokey", () => {
  assert.equal(nextLoadStateAfterError("ok", true, "网络失败"), "ok");
  assert.equal(nextLoadStateAfterError("ok", false, "网络失败"), "error");
  assert.equal(
    nextLoadStateAfterError("loading", true, "未配置用量 Token"),
    "nokey",
  );
  assert.equal(
    nextLoadStateAfterError("error", true, "未配置 API Key"),
    "nokey",
  );
});

test("chartPointFromDay：单模型不含未识别模型差额", () => {
  const sample: UsageDay = {
    ...emptyUsageDay("2026-09-11"),
    flashCacheHit: 100,
    flashResponse: 20,
    proCacheMiss: 999,
    totalTokens: 5000,
  };
  const flash = chartPointFromDay(sample, "flash");
  assert.equal(flash.total, 120);
  assert.equal(flash.other, 0);
  const pro = chartPointFromDay(sample, "pro");
  assert.equal(pro.total, 999);
});

test("chartPointFromDay：other 档只含未接入模型量（合计 − flash − pro）", () => {
  const sample: UsageDay = {
    ...emptyUsageDay("2026-09-11"),
    flashTokens: 100,
    flashCacheHit: 60,
    flashCacheMiss: 40,
    proTokens: 20,
    proResponse: 20,
    // 合计 150：flash 100 + pro 20 → 未接入模型 30，无分段明细
    totalTokens: 150,
  };
  const point = chartPointFromDay(sample, "other");
  assert.equal(point.hit, 0);
  assert.equal(point.miss, 0);
  assert.equal(point.response, 0);
  assert.equal(point.other, 30);
  assert.equal(point.total, 30);
});

test("chartPointFromDay：other 档在合计小于已知模型时夹到 0", () => {
  // 防御：接口口径异常时不应出现负数柱高
  const sample: UsageDay = {
    ...emptyUsageDay("2026-09-11"),
    flashTokens: 100,
    totalTokens: 50,
  };
  const point = chartPointFromDay(sample, "other");
  assert.equal(point.other, 0);
  assert.equal(point.total, 0);
});
