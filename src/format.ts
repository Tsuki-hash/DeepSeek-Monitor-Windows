// 纯格式化 / 日期工具。
//
// 这些函数原先散在 main.tsx 里，与 React 组件混在一起，导致无法单独测。
// 抽出来之后可以用 Node 内置的 `node --test`（配合 --experimental-strip-types 直接读 .ts）
// 跑最小单测，不需要引入 vitest / jest 这类会拉进几十个 devDependency 的测试框架。
//
// 注意本文件**不得** import 任何浏览器或 Tauri API，否则 Node 侧测不了。

export type UsageDay = {
  date: string;
  flashTokens: number;
  flashCacheHit: number;
  flashCacheMiss: number;
  flashResponse: number;
  proTokens: number;
  proCacheHit: number;
  proCacheMiss: number;
  proResponse: number;
  flashOtherTokens: number;
  proOtherTokens: number;
  totalTokens: number;
  totalCost: number;
};

/** 千分位整数。用于 tooltip 里的精确 token 数。 */
export const fmtInt = (n: number) => Math.round(n).toLocaleString("en-US");

/**
 * 紧凑 token 数：柱顶标签用，必须在 ~34px 宽度内放得下。
 *
 * 三个阈值都按「换算后不再进位」来切，否则会出现 `999999 → "1000.0K"` 这种
 * 比 M 档还长的字符串（7 字符），正是柱顶标签被裁掉的直接原因。
 * 因此 K 档上界取到 999_499（四舍五入后仍为 999.5K），超过就升到 M 档。
 */
export const fmtTokensShort = (n: number) => {
  if (n >= 1e8) return (n / 1e6).toFixed(0) + "M";
  if (n >= 999_500) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "K";
  return String(Math.round(n));
};

export const fmtMoney = (n: number, symbol = "¥") => symbol + n.toFixed(2);

/**
 * 币种符号的唯一来源。余额来自官方接口、带 currency 字段（正常为 CNY，也存在 USD 账户）；
 * 而用量与消费来自平台内部接口，恒为人民币计价。两者口径不同，所以共享同一面板时必须
 * 用余额的币种符号，否则会出现「余额 $xx 而当日消耗 ¥xx」的矛盾显示。
 */
export const currencySymbol = (currency?: string) =>
  currency === "USD" ? "$" : "¥";

/** `2026-09-11` → `9/11`。 */
export const mmdd = (date: string) => {
  const parts = date.split("-");
  return parts.length === 3 ? `${Number(parts[1])}/${Number(parts[2])}` : date;
};

/** 本地时区的今天，格式与接口返回的 date 字段一致（`YYYY-MM-DD`）。 */
export const todayStr = () => dateKey(new Date());

/** Date → `YYYY-MM-DD`（本地时区，不走 toISOString 以免被 UTC 偏移带偏一天）。 */
export const dateKey = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(
    date.getDate(),
  ).padStart(2, "0")}`;

export const addDays = (date: Date, offset: number) => {
  const next = new Date(date);
  next.setDate(next.getDate() + offset);
  return next;
};

/** 空的一天。用于把接口没返回的日期补成零值，保证柱状图始终 7 根。 */
export const emptyUsageDay = (date: string): UsageDay => ({
  date,
  flashTokens: 0,
  flashCacheHit: 0,
  flashCacheMiss: 0,
  flashResponse: 0,
  proTokens: 0,
  proCacheHit: 0,
  proCacheMiss: 0,
  proResponse: 0,
  flashOtherTokens: 0,
  proOtherTokens: 0,
  totalTokens: 0,
  totalCost: 0,
});

/**
 * 取最近 `count` 天（含今天）的用量，接口没给的日期补零。
 *
 * 两个刻意的取舍：
 * 1. 过滤掉 `date > today` 的条目——接口在月初可能返回整月（含未来日期）的占位行，
 *    不过滤会让活跃度柱状图出现「未来几天有数据」的怪象；
 * 2. 按**本地日期**逐天递减生成，而不是拿接口返回的最后一天往前数——跨月时
 *    上个月的日期必须能被正确补进窗口，否则月初打开面板会看到几根空柱。
 */
export const recentUsageDays = (days: UsageDay[], count = 7): UsageDay[] => {
  const today = todayStr();
  const source = new Map(
    days.filter((day) => day.date <= today).map((day) => [day.date, day]),
  );
  const now = new Date();
  return Array.from({ length: count }, (_, index) => {
    const date = dateKey(addDays(now, index - count + 1));
    return source.get(date) ?? emptyUsageDay(date);
  });
};

/** 上一个月，给「查看上月用量」用。跨年时 year 一并回退。 */
export const previousMonth = (date: Date) => {
  const previous = new Date(date.getFullYear(), date.getMonth() - 1, 1);
  return { month: previous.getMonth() + 1, year: previous.getFullYear() };
};

/** 刷新失败后的展示态：silent 且已有数据时保留「ok」，便于界面保留快照。 */
export const nextLoadStateAfterError = (
  prev: "loading" | "ok" | "error" | "nokey",
  silent: boolean,
  message: string,
): "loading" | "ok" | "error" | "nokey" => {
  if (silent && prev === "ok") {
    return "ok";
  }
  return message.includes("未配置") ? "nokey" : "error";
};

export type ChartScope = "all" | "flash" | "pro" | "other";

export type ChartPoint = {
  date: string;
  hit: number;
  miss: number;
  response: number;
  other: number;
  total: number;
};

/**
 * 把单日用量映射成柱状图数据点。
 *
 * `scope === "all"` 时合计优先取 `day.totalTokens`（后端覆盖含未识别模型），
 * 与已知分段之和的差额并入 other，避免新模型名出现时按日图静默丢量。
 * 单模型（flash/pro）没有独立的 total 字段，仍按分段求和。
 * `scope === "other"` 是未接入模型的兜底档：平台不给分段明细，
 * 按日量 = 当日合计 − flash − pro，柱子只有一段。
 */
export const chartPointFromDay = (
  day: UsageDay,
  scope: ChartScope = "all",
): ChartPoint => {
  const hit =
    scope === "flash"
      ? day.flashCacheHit
      : scope === "pro"
        ? day.proCacheHit
        : scope === "all"
          ? day.flashCacheHit + day.proCacheHit
          : 0;
  const miss =
    scope === "flash"
      ? day.flashCacheMiss
      : scope === "pro"
        ? day.proCacheMiss
        : scope === "all"
          ? day.flashCacheMiss + day.proCacheMiss
          : 0;
  const response =
    scope === "flash"
      ? day.flashResponse
      : scope === "pro"
        ? day.proResponse
        : scope === "all"
          ? day.flashResponse + day.proResponse
          : 0;
  const knownOther =
    scope === "flash"
      ? day.flashOtherTokens
      : scope === "pro"
        ? day.proOtherTokens
        : scope === "all"
          ? day.flashOtherTokens + day.proOtherTokens
          : Math.max(0, day.totalTokens - day.flashTokens - day.proTokens);
  const segmented = hit + miss + response + knownOther;
  if (scope !== "all") {
    return {
      date: day.date,
      hit,
      miss,
      response,
      other: knownOther,
      total: segmented,
    };
  }
  const total = Math.max(day.totalTokens, segmented);
  const other = knownOther + (total - segmented);
  return { date: day.date, hit, miss, response, other, total };
};
