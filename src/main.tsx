import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import { Toaster } from "@/components/ui/sonner";
import { ThemeProvider, applyThemeClass } from "@/theme";
import "./index.css";

// 首帧前同步应用主题，避免深色用户看到白屏闪烁
applyThemeClass(
  (localStorage.getItem("gm-theme") as "dark" | "light" | null) ?? "system",
);

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
        <App />
        <Toaster
          richColors
          position="bottom-right"
          duration={1500}
        />
      </QueryClientProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
