import React from "react";
import { fmtInt, fmtTokensShort, mmdd } from "../format";

export type StackedPoint = {
  date: string;
  hit: number;
  miss: number;
  response: number;
  other: number;
  total: number;
};

// 主面板（Flash + Pro 合并）与详情页（单模型）的柱状图此前是两份近乎逐行相同的 JSX
// （约 70 行 × 2），本次评审就需要人工比对两处确认逻辑一致。抽成同一个组件后，
// 分段顺序、tooltip 结构、可访问性只有一处实现，改样式或加分段不必再同步两处。
// 两处仅类名与日期标签元素不同，用 variant 区分，保持各自的渲染结果不变。
function StackedBarChart({
  points,
  variant,
  hasOther,
}: {
  points: StackedPoint[];
  variant: "summary" | "detail";
  hasOther: boolean;
}) {
  const [hoveredIdx, setHoveredIdx] = React.useState<number | null>(null);
  const MIN_BAR = 3; // 整根柱子的最小可见高度百分比（含空数据占位）
  const maxVal = Math.max(...points.map((point) => point.total), 1);
  const isSummary = variant === "summary";

  return (
    <>
      <div
        className={isSummary ? "bars" : "detail-bars"}
        onMouseLeave={() => setHoveredIdx(null)}
      >
        {points.map((point, idx) => {
          // 键盘用户与读屏用户拿到的是同一条信息：日期、合计与各分段明细
          const label =
            `${point.date}：合计 ${fmtInt(point.total)} tokens，` +
            `命中 ${fmtInt(point.hit)}，未命中 ${fmtInt(point.miss)}，输出 ${fmtInt(point.response)}` +
            (point.other > 0 ? `，其他 ${fmtInt(point.other)}` : "");
          return (
            <div className={isSummary ? "bar-column" : "detail-bar-column"} key={point.date}>
              {hoveredIdx === idx && point.total > 0 && (
                <div
                  className={`bar-tooltip${
                    idx <= 1 ? " align-left" : idx >= points.length - 2 ? " align-right" : ""
                  }`}
                >
                  <div className="bar-tooltip-head">
                    <span className="bar-tooltip-date">{point.date}</span>
                    <strong>{fmtInt(point.total)} tokens</strong>
                  </div>
                  <span className="bar-tooltip-row">
                    <i className="dot hit" />输入（命中缓存）
                    <strong>{fmtInt(point.hit)} tokens</strong>
                  </span>
                  <span className="bar-tooltip-row">
                    <i className="dot miss" />输入（未命中缓存）
                    <strong>{fmtInt(point.miss)} tokens</strong>
                  </span>
                  <span className="bar-tooltip-row">
                    <i className="dot response" />输出
                    <strong>{fmtInt(point.response)} tokens</strong>
                  </span>
                  {point.other > 0 && (
                    <span className="bar-tooltip-row">
                      <i className="dot other" />其他（含未识别模型）
                      <strong>{fmtInt(point.other)} tokens</strong>
                    </span>
                  )}
                </div>
              )}
              {isSummary ? (
                <span className="bar-value">
                  {point.total > 0 ? fmtTokensShort(point.total) : "0"}
                </span>
              ) : (
                <span>{point.total > 0 ? fmtTokensShort(point.total) : ""}</span>
              )}
              <div className={isSummary ? "bar-slot" : "detail-bar-slot"}>
                <div
                  className={isSummary ? "cache-bar" : "detail-bar-stacked"}
                  role="img"
                  tabIndex={0}
                  aria-label={label}
                  style={{
                    height: `${point.total > 0 ? Math.max(MIN_BAR, (point.total / maxVal) * 100) : MIN_BAR}%`,
                  }}
                  onMouseEnter={() => setHoveredIdx(idx)}
                  onMouseLeave={() => setHoveredIdx(null)}
                  onFocus={() => setHoveredIdx(idx)}
                  onBlur={() => setHoveredIdx(null)}
                >
                  {point.total > 0 ? (
                    <>
                      {point.hit > 0 && <i className="seg hit" style={{ flexGrow: point.hit }} />}
                      {point.miss > 0 && <i className="seg miss" style={{ flexGrow: point.miss }} />}
                      {point.response > 0 && (
                        <i className="seg response" style={{ flexGrow: point.response }} />
                      )}
                      {point.other > 0 && <i className="seg other" style={{ flexGrow: point.other }} />}
                    </>
                  ) : (
                    <i className="seg empty" />
                  )}
                </div>
              </div>
              {isSummary ? (
                <span className="bar-day">{mmdd(point.date)}</span>
              ) : (
                <em>{mmdd(point.date)}</em>
              )}
            </div>
          );
        })}
      </div>
      <div className="chart-legend-bottom">
        <span className="chart-legend-item">
          <i className="dot hit" />命中
        </span>
        <span className="chart-legend-item">
          <i className="dot miss" />未命中
        </span>
        <span className="chart-legend-item">
          <i className="dot response" />输出
        </span>
        {hasOther && (
          <span className="chart-legend-item">
            <i className="dot other" />其他
          </span>
        )}
      </div>
    </>
  );
}
export { StackedBarChart };
