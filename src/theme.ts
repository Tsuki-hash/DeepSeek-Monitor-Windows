// 主题的唯一来源：存储键、DOM 属性名、默认值只在这里定义一次。
// 首屏渲染前的引导代码与 React 内的 useTheme() 都调用同一组函数。
import React from "react";

export const THEME_STORAGE_KEY = "ui-theme";
export const THEME_ATTR = "data-theme";
export type Theme = "dark" | "light";

export const readStoredTheme = (): Theme =>
  localStorage.getItem(THEME_STORAGE_KEY) === "light" ? "light" : "dark";

export const applyTheme = (theme: Theme) =>
  document.documentElement.setAttribute(THEME_ATTR, theme);

export function useTheme() {
  const [theme, setTheme] = React.useState<Theme>(readStoredTheme);
  // 状态与 DOM 属性、持久化三者在这里同步；调用方只关心 theme 与 toggleTheme
  React.useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  }, [theme]);
  const toggleTheme = React.useCallback(() => {
    setTheme((prev) => (prev === "dark" ? "light" : "dark"));
  }, []);
  return { theme, toggleTheme };
}
