import React from "react";
import { BarChart3 } from "lucide-react";
import { chartPointFromDay, fmtTokensShort, recentUsageDays } from "../format";
import type { LoadState, UsageResult } from "../types";
import { StackedBarChart } from "./StackedBarChart";

function UsageChart({
  usage,
  state,
  error,
}: {
  usage: UsageResult | null;
  state: LoadState;
  error: string;
}) {
  const days = recentUsageDays(usage?.days ?? []);
  // all：合计含未识别模型（差额并入 other），避免按日图静默丢量
  const points = days.map((day) => chartPointFromDay(day, "all"));
  const hasOther = points.some((point) => point.other > 0);
  const sumHit = points.reduce((sum, point) => sum + point.hit, 0);
  const sumMiss = points.reduce((sum, point) => sum + point.miss, 0);
  const sumTotal = points.reduce((sum, point) => sum + point.total, 0);
  const hitRate =
    sumHit + sumMiss > 0
      ? ((sumHit / (sumHit + sumMiss)) * 100).toFixed(0)
      : "0";
  const placeholder =
    state === "loading"
      ? "查询中…"
      : state === "nokey"
        ? "未配置用量 Token，可在设置页同步或粘贴"
        : state === "error"
          ? error || "用量查询失败，余额不受影响"
          : "暂无数据";

  return (
    <article className="card chart-card">
      <div className="card-title-row">
        <div className="caption-with-icon">
          <BarChart3 size={16} className="brand-blue" />
          <span>缓存命中明细</span>
        </div>
        <span className="chart-total">
          {state === "ok" ? (
            <>
              命中率 {hitRate}% · 合计 {fmtTokensShort(sumTotal)}
              {error ? " · 上次刷新失败" : ""}
            </>
          ) : (
            "—"
          )}
        </span>
      </div>
      {state === "ok" && points.length > 0 ? (
        <StackedBarChart
          points={points}
          variant="summary"
          hasOther={hasOther}
        />
      ) : (
        <div className="chart-placeholder">{placeholder}</div>
      )}
    </article>
  );
}
export { UsageChart };
