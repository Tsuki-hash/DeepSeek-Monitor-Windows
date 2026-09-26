import React from "react";
import {
  BarChart3,
  CheckCircle2,
  Info,
  KeyRound,
  Power,
  RefreshCw,
  X,
  XCircle,
} from "lucide-react";
import { refreshOptions } from "../usage-api";
import { useTheme } from "../theme";
import { useSettingsState } from "../settings-state";
import type { UsageResult } from "../types";
import { BrandIcon } from "./BrandIcon";
import { SettingsSection, Toggle } from "./ui";

// 状态与动作在 useSettingsState（评审 F-09 拆分），本组件只负责视图与文案。
function SettingsPanel({
  onBack,
  onUsageLoaded,
  onUsageCleared,
  onRefreshIntervalChanged,
  onAutoRefreshChanged,
}: {
  onBack: () => void;
  onUsageLoaded: (usage: UsageResult) => void;
  onUsageCleared: () => void;
  onRefreshIntervalChanged: (seconds: number) => void;
  onAutoRefreshChanged: (enabled: boolean) => void;
}) {
  const {
    config,
    configPath,
    status,
    busy,
    appVersion,
    apiKey,
    setApiKey,
    usageToken,
    setUsageToken,
    refresh,
    autoRefresh,
    autostart,
    usageStatus,
    usageSyncing,
    showManualPaste,
    setShowManualPaste,
    saveApiKey,
    clearApiKey,
    pasteApiKey,
    saveUsageToken,
    clearUsageToken,
    pasteUsageToken,
    startUsageSync,
    saveRefreshInterval,
    saveAutoRefreshEnabled,
    saveAutostart,
  } = useSettingsState({
    onUsageLoaded,
    onUsageCleared,
    onRefreshIntervalChanged,
    onAutoRefreshChanged,
  });
  const { theme, toggleTheme } = useTheme();

  return (
    <section className="settings-panel" data-testid="settings-panel">
      <button
        className="floating-close settings-close"
        onClick={onBack}
        aria-label="返回主面板"
      >
        <X size={20} />
      </button>
      <div className="settings-inner">
        <header className="settings-header" data-tauri-drag-region>
          <BrandIcon size={42} />
          <div>
            <h1>DeepSeek Monitor</h1>
            <p>设置</p>
          </div>
        </header>

        <SettingsSection icon={<KeyRound size={15} />} title="API Key">
          <p>
            仅用于查询账户余额（用量需网页登录
            Token，见下一节）。会保存在应用本地设置中。
          </p>
          <p className="muted">API Key 只在当前这台 Windows 电脑本地保留。</p>
          <p className="muted config-path">
            <span>本地位置：</span>
            <span>{configPath}</span>
          </p>
          <div className="key-row">
            <input
              aria-label="API Key"
              type="password"
              value={apiKey}
              placeholder={
                config?.apiKeyConfigured
                  ? "•••••••••••••••••••••••••••••••••••••••••••••••••••"
                  : "sk-..."
              }
              onChange={(event) => setApiKey(event.target.value)}
            />
          </div>
          <div className="settings-actions">
            <button
              className="primary"
              onClick={saveApiKey}
              disabled={busy || !apiKey.trim()}
            >
              验证并保存
            </button>
            <button className="secondary" onClick={pasteApiKey} disabled={busy}>
              粘贴
            </button>
            <span
              className={
                config?.apiKeyConfigured ? "configured" : "configured missing"
              }
            >
              {config?.apiKeyConfigured ? (
                <CheckCircle2 size={17} />
              ) : (
                <XCircle size={17} />
              )}
              {config?.apiKeyConfigured ? "已配置" : "未配置"}
            </span>
            <button
              className="secondary"
              onClick={clearApiKey}
              disabled={busy || !config?.apiKeyConfigured}
            >
              清除 Key
            </button>
          </div>
          <p className="muted">{status}</p>
        </SettingsSection>

        <SettingsSection icon={<BarChart3 size={15} />} title="用量同步 Token">
          <p>
            用于同步 Token 用量、消费和趋势图。DeepSeek 无官方用量
            API，需网页登录 token（与上面的 API Key 不同）。
          </p>
          <p className="muted">方式一网页登录自动同步</p>
          <div className="settings-actions usage-sync-actions">
            <button
              className="primary"
              onClick={startUsageSync}
              disabled={usageSyncing}
            >
              {usageSyncing ? "等待登录" : "网页登录自动同步"}
            </button>
            <span
              className={
                config?.usageTokenConfigured
                  ? "configured"
                  : "configured missing"
              }
            >
              {config?.usageTokenConfigured ? (
                <CheckCircle2 size={17} />
              ) : (
                <XCircle size={17} />
              )}
              {config?.usageTokenConfigured ? "已配置" : "未配置"}
            </span>
            <button
              className="secondary"
              onClick={clearUsageToken}
              disabled={busy || !config?.usageTokenConfigured}
            >
              清除 Token
            </button>
          </div>
          <p className="muted">{usageStatus}</p>
          <button
            className="link-button"
            onClick={() => setShowManualPaste((value) => !value)}
          >
            {showManualPaste ? "收起手动粘贴" : "方式二：手动粘贴 token"}
          </button>
          {showManualPaste && (
            <>
              <p className="muted">
                获取：浏览器登录 platform.deepseek.com，按 F12 打开开发者工具，
                切到「控制台 / Console」标签，在 ❯ 提示符后输入
                JSON.parse(localStorage.userToken).value
                回车，右键复制返回的字符串。
              </p>
              <p className="muted">
                若返回 undefined（平台可能调整了存储键名），改输入{" "}
                {
                  "Object.entries(localStorage).filter(([k]) => /token/i.test(k))"
                }
                ，从结果里找形如 token 的长字符串复制。
              </p>
              <p className="muted">
                token 会过期，用量查询失败时重新获取一次即可。
              </p>
              <div className="key-row">
                <input
                  aria-label="用量 Token"
                  type="password"
                  value={usageToken}
                  placeholder={
                    config?.usageTokenConfigured
                      ? "•••••••••••••••••••••••••••••••••••••••••••••••••••"
                      : ""
                  }
                  onChange={(event) => setUsageToken(event.target.value)}
                />
              </div>
              <div className="settings-actions">
                <button
                  className="primary"
                  onClick={saveUsageToken}
                  disabled={busy || !usageToken.trim()}
                >
                  保存 Token
                </button>
                <button
                  className="secondary"
                  onClick={pasteUsageToken}
                  disabled={busy}
                >
                  粘贴
                </button>
              </div>
            </>
          )}
        </SettingsSection>

        <SettingsSection icon={<Power size={15} />} title="开机自启">
          <p>开启后，每次登录 Windows 时自动启动 DeepSeek Monitor。</p>
          <Toggle
            label="登录时自动启动"
            checked={autostart}
            onChange={saveAutostart}
          />
        </SettingsSection>

        <SettingsSection icon={<RefreshCw size={15} />} title="自动刷新">
          <p>开启后，按设定周期自动从 DeepSeek API 拉取最新数据。</p>
          <Toggle
            label="启用自动刷新"
            checked={autoRefresh}
            onChange={saveAutoRefreshEnabled}
          />
          {autoRefresh && (
            <div className="segmented">
              {refreshOptions.map((option) => (
                <button
                  key={option.value}
                  className={refresh === option.value ? "selected" : ""}
                  onClick={() => saveRefreshInterval(option.value)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          )}
        </SettingsSection>

        <SettingsSection icon={<Info size={15} />} title="外观">
          <Toggle
            label={
              theme === "dark"
                ? "深色皮肤（点击切换浅色）"
                : "浅色皮肤（点击切换深色）"
            }
            checked={theme === "light"}
            onChange={toggleTheme}
          />
        </SettingsSection>

        <SettingsSection icon={<Info size={15} />} title="关于">
          <div className="version-row">
            <span>当前版本</span>
            <strong>{appVersion ? `v${appVersion}` : "—"}</strong>
          </div>
        </SettingsSection>
      </div>
    </section>
  );
}
export { SettingsPanel };
