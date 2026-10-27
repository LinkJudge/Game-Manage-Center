import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toast } from "sonner";
import { Copy, Minus, Square, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ActionTooltip, TooltipProvider } from "@/components/ui/tooltip";
import { BackupTab } from "@/backupTab";
import { TrainersTab } from "@/trainersTab";
import { SettingsDialog } from "@/settingsDialog";
import { useScanStore, useUpdateStore } from "@/store";
import { useI18n } from "@/i18n";

function App() {
  const { t } = useI18n();
  const autoScanOnStartup = useScanStore((s) => s.autoScanOnStartup);
  const autoScanIntervalMin = useScanStore((s) => s.autoScanIntervalMin);
  const runScan = useScanStore((s) => s.runScan);
  const updateCheckMode = useUpdateStore((s) => s.updateCheckMode);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [activeTab, setActiveTab] = useState("saves");
  // 最大化状态下显示"还原"图标（Windows 标准行为）
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    const update = () => win.isMaximized().then(setIsMaximized).catch(() => {});
    update();
    const unlisten = win.onResized(update);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // 启动时自动扫描（persist 重hydration 后触发一次）
  useEffect(() => {
    if (autoScanOnStartup) {
      const timer = setTimeout(() => runScan(), 600);
      return () => clearTimeout(timer);
    }
  }, []);

  // 定时自动重扫
  useEffect(() => {
    if (autoScanIntervalMin <= 0) return;
    const id = setInterval(() => runScan(), autoScanIntervalMin * 60_000);
    return () => clearInterval(id);
  }, [autoScanIntervalMin]);

  // 按设置频率检查软件更新（每次启动 / 每隔 N 天 / 从不）；发现新版弹可点击通知
  useEffect(() => {
    if (updateCheckMode === "never") return;
    const timer = setTimeout(() => {
      const { lastUpdateCheckAt, markUpdateChecked } = useUpdateStore.getState();
      const intervalMs = updateCheckMode === "startup" ? 0 : Number(updateCheckMode) * 86_400_000;
      if (lastUpdateCheckAt && Date.now() - lastUpdateCheckAt < intervalMs) return;
      invoke<{ hasUpdate: boolean; latestVersion: string | null; releaseUrl: string }>(
        "check_app_update",
      )
        .then((r) => {
          markUpdateChecked();
          if (r.hasUpdate) {
            toast.info(t("app.update.found", { version: r.latestVersion ?? "" }), {
              description: t("app.update.desc"),
              duration: 12_000,
              action: {
                label: t("app.update.action"),
                onClick: () => {
                  import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
                    openUrl(r.releaseUrl).catch(() => {}),
                  );
                },
              },
            });
          }
        })
        .catch(() => {}); // 静默：网络不通不打扰用户
    }, 2_500);
    return () => clearTimeout(timer);
  }, [updateCheckMode]);

  return (
    <TooltipProvider>
      <main className={`gm-app-shell gm-tab-${activeTab} flex h-screen flex-col overflow-hidden text-foreground`}>
        <Tabs value={activeTab} onValueChange={setActiveTab} className="flex min-h-0 flex-1 flex-col">
        {/* Apple §12：半透明材质层。decorations:false 后本层即自绘标题栏 */}
        <div className="gm-titlebar gm-material z-40">
          {/* 第一行：品牌名居左（macOS 版红绿灯按钮的预留位），窗口控制居右 */}
          <div className="flex h-10 items-center justify-between pl-3 pr-2" data-tauri-drag-region>
            <span className="flex items-center gap-2 text-[0.72rem] font-semibold tracking-[0.01em] text-muted-foreground" data-tauri-drag-region>
              <span className="gm-brand-mark">
                <img src="/icons/app.svg" alt="" />
              </span>
              <span className="flex items-baseline gap-2">
                {t("app.name")}
                <span className="text-[0.62rem] font-medium tracking-[0.08em] text-muted-foreground/60">GAME MANAGE CENTER</span>
              </span>
            </span>
            <div className="flex items-center gap-1.5">
              <ActionTooltip content={t("app.win.minimize")}>
                <button
                  onClick={() => getCurrentWindow().minimize()}
                  aria-label={t("app.win.minimize")}
                  className="gm-window-button gm-window-minimize"
                >
                  <Minus className="size-3.5" />
                </button>
              </ActionTooltip>
              <ActionTooltip
                content={isMaximized ? t("app.win.restore") : t("app.win.maximize")}
              >
                <button
                  onClick={() => getCurrentWindow().toggleMaximize()}
                  aria-label={isMaximized ? t("app.win.restore") : t("app.win.maximize")}
                  className="gm-window-button gm-window-maximize"
                >
                  {isMaximized ? (
                    <Copy className="size-3 -scale-x-100" />
                  ) : (
                    <Square className="size-3" />
                  )}
                </button>
              </ActionTooltip>
              <ActionTooltip content={t("app.win.close")}>
                <button
                  onClick={() => getCurrentWindow().close()}
                  aria-label={t("app.win.close")}
                  className="gm-window-button gm-window-close"
                >
                  <X className="size-3.5" />
                </button>
              </ActionTooltip>
            </div>
          </div>
          {/* 第二行：顶层模块导航（与 gm-core module 注册表对应）+ 设置 */}
          <div className="flex items-center gap-2 px-4 pb-2.5" data-tauri-drag-region>
            <TabsList className="gm-main-tabs h-10 rounded-full p-1">
              <TabsTrigger value="saves" className="gm-main-tab">
                <span className="gm-nav-icon"><img src="/icons/save.svg" alt="" /></span>
                {t("nav.saves")}
              </TabsTrigger>
              <TabsTrigger value="trainers" className="gm-main-tab">
                <span className="gm-nav-icon"><img src="/icons/trainer.svg" alt="" /></span>
                {t("nav.trainers")}
              </TabsTrigger>
              <ActionTooltip content={t("app.mod.planned")} disabled>
                <TabsTrigger value="mods" disabled className="gm-main-tab">
                  <span className="gm-nav-icon"><img src="/icons/mod.svg" alt="" /></span>
                  MOD
                </TabsTrigger>
              </ActionTooltip>
            </TabsList>
            <div className="h-6 flex-1" data-tauri-drag-region />
            <Button variant="outline" size="sm" className="gm-settings-button rounded-full" onClick={() => setSettingsOpen(true)}>
              {t("nav.settings")}
            </Button>
          </div>
        </div>

        <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />

        {/* 存档模块：备份、恢复与记录合并为同一游戏列表 */}
        <TabsContent
          value="saves"
          forceMount
          className="flex min-h-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden"
        >
          <div className="gm-page-hero flex items-center gap-3 px-7 pb-2 pt-5">
            <span className="gm-page-icon">
              <img src="/icons/save.svg" alt="" />
            </span>
            <div>
              <h1 className="text-[1.65rem] font-semibold tracking-[-0.03em]">{t("saves.title")}</h1>
              <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                {t("saves.subtitle")}
              </p>
            </div>
          </div>
          <div className="gm-scroll-area min-h-0 flex-1 overflow-y-auto px-7 pb-7 pt-3">
            <BackupTab />
          </div>
        </TabsContent>

        {/* 修改器模块：目录搜索 + 一键下载安装（标题区/内容区间距与存档页同构，含二级 Tabs 的 gap-2） */}
        <TabsContent
          value="trainers"
          forceMount
          className="flex min-h-0 flex-1 flex-col overflow-hidden data-[state=inactive]:hidden"
        >
          <div className="gm-page-hero flex items-center gap-3 px-7 pb-2 pt-5">
            <span className="gm-page-icon">
              <img src="/icons/trainer.svg" alt="" />
            </span>
            <div>
              <h1 className="text-[1.65rem] font-semibold tracking-[-0.03em]">{t("trainers.title")}</h1>
              <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                {t("trainers.subtitle")}
              </p>
            </div>
          </div>
          <div className="gm-scroll-area min-h-0 flex-1 overflow-y-auto px-7 pb-7 pt-3">
            <TrainersTab />
          </div>
        </TabsContent>
        {/* MOD 模块占位（规划中） */}
        <TabsContent value="mods" className="min-h-0 flex-1" />
      </Tabs>
      </main>
    </TooltipProvider>
  );
}

export default App;
