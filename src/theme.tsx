import { createContext, useContext, useEffect, useState } from "react";
import type { ReactNode } from "react";

// 三档主题：系统默认 / 深色 / 浅色。偏好存 localStorage（纯 UI 偏好，无需进 settings.json）
export type ThemeMode = "system" | "dark" | "light";
const STORAGE_KEY = "gm-theme";
const mql = window.matchMedia("(prefers-color-scheme: dark)");

function resolvedTheme(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") return mql.matches ? "dark" : "light";
  return mode;
}

/** 读取持久化的主题偏好；localStorage 不可用（隐私模式/策略限制）时回退 system */
export function storedThemeMode(): ThemeMode {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored === "dark" || stored === "light" ? stored : "system";
  } catch {
    return "system";
  }
}

/** 把当前主题应用到 <html> 的 dark 类上；切换时加短暂过渡，避免亮度跳变（Apple §14） */
export function applyThemeClass(mode: ThemeMode) {
  const root = document.documentElement;
  root.classList.toggle("dark", resolvedTheme(mode) === "dark");
  root.classList.add("theme-transition");
  window.setTimeout(() => root.classList.remove("theme-transition"), 260);
}

function readStoredMode(): ThemeMode {
  return storedThemeMode();
}

const ThemeContext = createContext<{
  theme: ThemeMode;
  setTheme: (mode: ThemeMode) => void;
}>({ theme: "system", setTheme: () => {} });

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeMode>(readStoredMode);

  useEffect(() => {
    applyThemeClass(theme);
    if (theme !== "system") return;
    const onSystemChange = () => applyThemeClass("system");
    mql.addEventListener("change", onSystemChange);
    return () => mql.removeEventListener("change", onSystemChange);
  }, [theme]);

  const setTheme = (mode: ThemeMode) => {
    try {
      localStorage.setItem(STORAGE_KEY, mode);
    } catch {
      // 存储不可用时本次会话内生效即可
    }
    setThemeState(mode);
  };

  return <ThemeContext.Provider value={{ theme, setTheme }}>{children}</ThemeContext.Provider>;
}

export const useTheme = () => useContext(ThemeContext);

/** 解析后的实际主题（system 时跟随系统，且响应系统切换） */
export function useResolvedTheme(): "light" | "dark" {
  const { theme } = useTheme();
  const [sysDark, setSysDark] = useState(mql.matches);
  useEffect(() => {
    if (theme !== "system") return;
    const onChange = () => setSysDark(mql.matches);
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, [theme]);
  return theme === "system" ? (sysDark ? "dark" : "light") : theme;
}
