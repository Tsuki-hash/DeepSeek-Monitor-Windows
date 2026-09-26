import React from "react";
import { Brain, Zap, X } from "lucide-react";
import {
  chartPointFromDay,
  fmtInt,
  fmtMoney,
  fmtTokensShort,
  mmdd,
  recentUsageDays,
} from "../format";
import type { LoadState, ModelName, UsageResult } from "../types";
import { StackedBarChart } from "./StackedBarChart";

function ModelDetailPanel({
  model,
  usage,
  usageState,
  usageError,
  onBack,
}: {
  model: ModelName;
  usage: UsageResult | null;
  usageState: LoadState;
  usageError: string;
  onBack: () => void;
}) {
  const isFlash = model === "flash";
  const data = usage?.models.find((item) => item.key === model) ?? null;
  // 显示名以后端 model_slot 为准，避免前后端双源漂移
  const title = data?.name ?? (isFlash ? "V4.1 Flash" : "V4 Pro");
  const tintClass = isFlash ? "flash" : "pro";
  const cost = data ? fmtMoney(data.cost) : "—";
  const totalText = data ? fmtTokensShort(data.totalTokens) : "—";

  const days = recentUsageDays(usage?.days ?? []);
  const points = days.map((day) =>
    chartPointFromDay(day, isFlash ? "flash" : "pro"),
  );
  const hasOther = points.some((point) => point.other > 0);
  const rangeText =
    points.length > 0
      ? `${mmdd(points[0].date)} - ${mmdd(points[points.length - 1].date)}`
      : "";

  return (
    <section className="panel detail-panel" data-testid="detail-panel">
      <button
        className="floating-close"
        onClick={onBack}
        aria-label="返回主面板"
      >
        <X size={20} />
      </button>
      <article className="card detail-hero" data-tauri-drag-region>
        <div className={`model-badge large ${tintClass}`}>
          {isFlash ? (
            <Zap size={34} fill="currentColor" />
          ) : (
            <Brain size={33} />
          )}
        </div>
        <div>
          <h1>{title}</h1>
          <p>{cost}</p>
        </div>
      </article>

      <div className="detail-metrics">
        <article className="card metric-card">
          <span>API 请求次数</span>
          <strong className={tintClass}>
            {data ? fmtInt(data.requestCount) : "—"}
          </strong>
        </article>
        <article className="card metric-card">
          <span>Tokens</span>
          <strong className={tintClass}>{totalText}</strong>
        </article>
      </div>

      <article className="card detail-chart">
        <div className="detail-chart-head">
          <div>
            <h2>按日 Token 消耗</h2>
            <span>{rangeText}</span>
          </div>
        </div>
        {usageState === "ok" && points.length > 0 ? (
          <StackedBarChart
            points={points}
            variant="detail"
            hasOther={hasOther}
          />
        ) : (
          <div className="chart-placeholder">
            {usageState === "nokey"
              ? "未配置用量 Token"
              : usageState === "loading"
                ? "查询中…"
                : usageState === "error"
                  ? usageError || "用量不可用"
                  : "暂无数据"}
          </div>
        )}
      </article>
    </section>
  );
}
export { ModelDetailPanel };
