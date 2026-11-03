import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import { ErrorBoundary } from "@/components/errorBoundary";
import { Toaster } from "@/components/ui/sonner";
import { ThemeProvider, applyThemeClass, storedThemeMode } from "@/theme";
import "./index.css";

// 首帧前同步应用主题，避免深色用户看到白屏闪烁
applyThemeClass(storedThemeMode());

// 屏蔽 WebView2 的浏览器默认右键菜单（返回/刷新/另存为/检查），
// 文本输入框保留（复制粘贴要用）；自带右键菜单的行（存档/恢复）不受影响
document.addEventListener("contextmenu", (e) => {
  const target = e.target as HTMLElement | null;
  if (target?.closest("input, textarea, [contenteditable='true']")) return;
  e.preventDefault();
});

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // IPC 数据不像网络请求那样频繁变化，给足缓存时间
      staleTime: 30_000,
      retry: 1,
    },
  },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <ErrorBoundary><App /></ErrorBoundary>
        <Toaster
          richColors
          position="bottom-right"
          duration={1500}
        />
      </QueryClientProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
