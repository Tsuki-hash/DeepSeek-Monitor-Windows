import React from "react";

function BrandIcon({ size = 32 }: { size?: number }) {
  // 静态资源缺失时（打包遗漏、被杀软清理）不显示裂图，退化为一个同尺寸的占位块。
  const [failed, setFailed] = React.useState(false);
  return (
    <div className="brand-icon" style={{ width: size, height: size }}>
      {failed ? (
        <span className="brand-icon-fallback" aria-hidden="true">
          DS
        </span>
      ) : (
        <img
          src="/assets/deepseek-color.png"
          alt="DeepSeek"
          onError={() => setFailed(true)}
        />
      )}
    </div>
  );
}
export { BrandIcon };
