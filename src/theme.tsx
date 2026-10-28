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

/** View Transitions（Chromium 111+ / WebView2 新版支持）——整帧快照交叉淡入 */
type ViewTransitionDoc = Document & {
  startViewTransition?: (cb: () => void) => { finished: Promise<void> };
};

let switching = false;

/**
 * 把主题应用到 <html> 的 dark 类上。
 *
 * 为什么不用「给所有元素加 transition」：
 * 那样 background-color 与 color 会各自独立渐变，交叉点上背景已经变亮、文字还没变深，
 * 整屏会出现一瞬间的低对比度（发白、字看不清）。
 *
 * 改用 View Transitions：对旧画面做整帧快照，新主题生效后两帧交叉淡入，
 * 视觉上是「整体溶解」，不会出现逐元素错位。不支持时直接瞬时切换（同样没有中间态）。
 */
export function applyThemeClass(mode: ThemeMode) {
  const root = document.documentElement;
  const nextDark = resolvedTheme(mode) === "dark";
  if (root.classList.contains("dark") === nextDark) return; // 无变化不折腾
  if (switching) {
    root.classList.toggle("dark", nextDark);
    return;
  }

  const doc = document as ViewTransitionDoc;
  if (typeof doc.startViewTransition === "function") {
    switching = true;
    const vt = doc.startViewTransition(() => {
      root.classList.toggle("dark", nextDark);
    });
    // 等动画结束再解锁，避免连点叠加多个 transition
    Promise.resolve(vt.finished)
      .catch(() => {})
      .finally(() => {
        switching = false;
      });
    return;
  }

  // 降级：瞬时切换。宁可没有动画，也不要那段发白的中间态。
  root.classList.toggle("dark", nextDark);
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
