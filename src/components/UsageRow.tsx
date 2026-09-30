import React from "react";
import { Brain, Shapes, Zap } from "lucide-react";
import { fmtInt, fmtMoney, fmtMoneyDisplay, fmtTokensShort } from "../format";
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
  const isOther = modelKey === "other";
  const tint = isFlash ? "flash" : isOther ? "other" : "pro";
  // 显示名优先取后端 model_slot 的 name，避免前后端双源漂移
  const name =
    data?.name ?? (isFlash ? "V4.1 Flash" : isOther ? "其他" : "V4 Pro");
  // 这一行只有约 115px 给 token 数，千万级以上再写精确千分位会溢出成省略号，
  // 因此大数用紧凑写法，精确值仍可通过图表按钮的无障碍标签与明细标题查看。
  const tokensText = data
    ? `${
        data.totalTokens < 1_000_000
          ? fmtInt(data.totalTokens)
          : fmtTokensShort(data.totalTokens)
      } Tokens`
    : state === "loading"
      ? "查询中…"
      : state === "nokey"
        ? "未配置 Token"
        : state === "error"
          ? "用量不可用"
          : "—";
  const cost = data ? fmtMoneyDisplay(data.cost) : "—";
  const ratio =
    data && data.cost > 0
      ? `${fmtTokensShort(data.totalTokens / data.cost)} T/¥`
      : "—";
  const width = data
    ? `${Math.max(2, (data.totalTokens / maxTokens) * 100)}%`
    : "0%";

  return (
    <button className="card usage-row" onClick={onClick}>
      <div className={`model-badge ${tint}`}>
        {isFlash ? (
          <Zap size={27} fill="currentColor" />
        ) : isOther ? (
          <Shapes size={25} />
        ) : (
          <Brain size={25} />
        )}
      </div>
      <div className="usage-main">
        <h2>
          <span className="model-name" title={name}>
            {name}
          </span>
          <small className="model-period">本月</small>
        </h2>
        <div className="token-line">
          <span title={data ? `${fmtInt(data.totalTokens)} Tokens` : undefined}>
            {tokensText}
          </span>
          <div className="progress-track">
            <i
              className={
                isFlash ? "flash-fill" : isOther ? "other-fill" : "pro-fill"
              }
              style={{ width }}
            />
          </div>
        </div>
        {data && data.cacheHitTokens + data.cacheMissTokens > 0 && (
          <span
            className={`cache-hit-rate ${tint}`}
            title="缓存命中输入 /（缓存命中输入 + 未命中输入），不含输出与未知输入"
          >
            缓存命中{" "}
            {(
              (data.cacheHitTokens /
                (data.cacheHitTokens + data.cacheMissTokens)) *
              100
            ).toFixed(0)}
            %
          </span>
        )}
      </div>
      <div className="usage-price">
        <strong title={data ? fmtMoney(data.cost) : undefined}>{cost}</strong>
        <span title="本月 Token 总数 / 本月人民币费用">{ratio}</span>
      </div>
    </button>
  );
}
export { UsageRow };
