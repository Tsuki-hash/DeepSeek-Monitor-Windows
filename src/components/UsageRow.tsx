import React from "react";
import { Brain, Zap } from "lucide-react";
import { fmtInt, fmtMoney, fmtTokensShort } from "../format";
import type { LoadState, ModelName, UsageModel } from "../types";

function UsageRow({
  modelKey,
  data,
  maxTokens,
  state,
  onClick,
}: {
  modelKey: ModelName;
  data: UsageModel | null;
  maxTokens: number;
  state: LoadState;
  onClick: () => void;
}) {
  const isFlash = modelKey === "flash";
  // 显示名优先取后端 model_slot 的 name，避免前后端双源漂移
  const name = data?.name ?? (isFlash ? "V4.1 Flash" : "V4 Pro");
  const tokensText = data
    ? `${fmtInt(data.totalTokens)} Tokens`
    : state === "loading"
      ? "查询中…"
      : state === "nokey"
        ? "未配置 Token"
        : state === "error"
          ? "用量不可用"
          : "—";
  const cost = data ? fmtMoney(data.cost) : "—";
  const ratio = data && data.cost > 0 ? `${fmtTokensShort(data.totalTokens / data.cost)} T/¥` : "—";
  const width = data ? `${Math.max(2, (data.totalTokens / maxTokens) * 100)}%` : "0%";

  return (
    <button className="card usage-row" onClick={onClick}>
      <div className={`model-badge ${isFlash ? "flash" : "pro"}`}>
        {isFlash ? <Zap size={27} fill="currentColor" /> : <Brain size={25} />}
      </div>
      <div className="usage-main">
        <h2>
          {name}
          {!isFlash && (
            <span className="deprecation-tag" title="V4 Pro 正在逐步下线，2026-09-14 起请求路由至 V4.1 Flash">
              逐步下线
            </span>
          )}
        </h2>
        <div className="token-line">
          <span>{tokensText}</span>
          <div className="progress-track">
            <i className={isFlash ? "flash-fill" : "pro-fill"} style={{ width }} />
          </div>
        </div>
        {data && data.cacheHitTokens + data.cacheMissTokens > 0 && (
          <span className={`cache-hit-rate ${isFlash ? "flash" : "pro"}`}>
            缓存命中{" "}
            {((data.cacheHitTokens / (data.cacheHitTokens + data.cacheMissTokens)) * 100).toFixed(0)}%
          </span>
        )}
      </div>
      <div className="usage-price">
        <strong>{cost}</strong>
        <span>{ratio}</span>
      </div>
    </button>
  );
}
export { UsageRow };
