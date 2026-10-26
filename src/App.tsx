import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toast } from "sonner";
import { Archive, Copy, Gamepad2, Minus, Puzzle, RotateCcw, Square, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { BackupTab } from "@/backupTab";
import { RestoreTab } from "@/restoreTab";
import { TrainersTab } from "@/trainersTab";
import { SettingsDialog } from "@/settingsDialog";
import { useScanStore, useUpdateStore } from "@/store";

function App() {
  const autoScanOnStartup = useScanStore((s) => s.autoScanOnStartup);
  const autoScanIntervalMin = useScanStore((s) => s.autoScanIntervalMin);
  const runScan = useScanStore((s) => s.runScan);
  const updateCheckMode = useUpdateStore((s) => s.updateCheckMode);
  const [settingsOpen, setSettingsOpen] = useState(false);
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
      const t = setTimeout(() => runScan(), 600);
      return () => clearTimeout(t);
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
    const t = setTimeout(() => {
      const { lastUpdateCheckAt, markUpdateChecked } = useUpdateStore.getState();
      const intervalMs = updateCheckMode === "startup" ? 0 : Number(updateCheckMode) * 86_400_000;
      if (lastUpdateCheckAt && Date.now() - lastUpdateCheckAt < intervalMs) return;
      invoke<{ hasUpdate: boolean; latestVersion: string | null; releaseUrl: string }>(
        "check_app_update",
      )
        .then((r) => {
          markUpdateChecked();
          if (r.hasUpdate) {
            toast.info(`发现新版本 v${r.latestVersion}`, {
              description: "点击「前往」下载，或稍后在 设置 → 关于 处理",
              duration: 12_000,
              action: {
                label: "前往",
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
    return () => clearTimeout(t);
  }, [updateCheckMode]);

  return (
    <main className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <Tabs defaultValue="saves" className="flex min-h-0 flex-1 flex-col">
        {/* Apple §12：半透明材质层。decorations:false 后本层即自绘标题栏 */}
        <div className="gm-material z-40 border-b border-border/40">
          {/* 第一行：品牌名居左（macOS 版红绿灯按钮的预留位），窗口控制居右 */}
          <div className="flex h-9 items-start justify-between px-2" data-tauri-drag-region>
            <span className="pl-3 pt-1 text-xs font-medium text-muted-foreground" data-tauri-drag-region>
              Game Manage Center
            </span>
            <div className="flex items-center">
              <button
                onClick={() => getCurrentWindow().minimize()}
                title="最小化"
                className="flex h-8 w-11 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground"
              >
                <Minus className="size-4" />
              </button>
              <button
                onClick={() => getCurrentWindow().toggleMaximize()}
                title={isMaximized ? "向下还原" : "最大化"}
                className="flex h-8 w-11 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground"
              >
                {isMaximized ? (
                  <Copy className="size-3.5 -scale-x-100" />
                ) : (
                  <Square className="size-3.5" />
                )}
              </button>
              <button
                onClick={() => getCurrentWindow().close()}
                title="关闭"
                className="flex h-8 w-11 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-red-500 hover:text-white"
              >
                <X className="size-4" />
              </button>
            </div>
          </div>
          {/* 第二行：顶层模块导航（与 gm-core module 注册表对应）+ 设置 */}
          <div className="flex items-center gap-2 px-4 pb-2" data-tauri-drag-region>
            <TabsList className="h-9 rounded-full bg-muted/70 p-1">
              <TabsTrigger
                value="saves"
                className="gap-1.5 rounded-full px-5 data-[state=active]:bg-background data-[state=active]:shadow-sm"
              >
                <Archive className="size-4" />
                存档
              </TabsTrigger>
              <TabsTrigger
                value="trainers"
                className="gap-1.5 rounded-full px-5 data-[state=active]:bg-background data-[state=active]:shadow-sm"
              >
                <Gamepad2 className="size-4" />
                修改器
              </TabsTrigger>
              <TabsTrigger
                value="mods"
                disabled
                title="规划中：MOD 的安装、版本管理与冲突排查"
                className="gap-1.5 rounded-full px-5"
              >
                <Puzzle className="size-4" />
                MOD
              </TabsTrigger>
            </TabsList>
            <div className="h-6 flex-1" data-tauri-drag-region />
            <Button variant="outline" size="sm" onClick={() => setSettingsOpen(true)}>
              设置
            </Button>
          </div>
        </div>

        <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />

        {/* 存档模块：二级标签（备份 / 恢复） */}
        <TabsContent value="saves" className="min-h-0 flex-1 overflow-hidden">
          <Tabs defaultValue="backup" className="flex h-full min-h-0 flex-col">
            <div className="flex items-center justify-between px-6 pb-1 pt-4">
              <div>
                <h1 className="text-2xl font-semibold tracking-tight">存档</h1>
                <p className="text-xs text-muted-foreground">
                  自动备份与恢复你的游戏进度 —— 支持多平台、多账号
                </p>
              </div>
              {/* Apple 分段式二级导航 */}
              <TabsList className="h-9 rounded-full bg-muted/70 p-1">
                <TabsTrigger
                  value="backup"
                  className="gap-1.5 rounded-full px-5 data-[state=active]:bg-background data-[state=active]:shadow-sm"
                >
                  <Archive className="size-4" />
                  备份
                </TabsTrigger>
                <TabsTrigger
                  value="restore"
                  className="gap-1.5 rounded-full px-5 data-[state=active]:bg-background data-[state=active]:shadow-sm"
                >
                  <RotateCcw className="size-4" />
                  恢复
                </TabsTrigger>
              </TabsList>
            </div>

            <TabsContent value="backup" className="min-h-0 flex-1 overflow-y-auto p-6 pt-3">
              <BackupTab />
            </TabsContent>
            <TabsContent value="restore" className="min-h-0 flex-1 overflow-y-auto p-6 pt-3">
              <RestoreTab />
            </TabsContent>
          </Tabs>
        </TabsContent>

        {/* 修改器模块：目录搜索 + 一键下载安装（标题区/内容区间距与存档页同构，含二级 Tabs 的 gap-2） */}
        <TabsContent value="trainers" className="flex min-h-0 flex-1 flex-col gap-2 overflow-hidden">
          <div className="px-6 pb-1 pt-4">
            <h1 className="text-2xl font-semibold tracking-tight">修改器</h1>
            <p className="text-xs text-muted-foreground">
              一键下载安装单机游戏修改器 —— 与本机游戏库联动，多来源持续接入
            </p>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-6 pt-3">
            <TrainersTab />
          </div>
        </TabsContent>
        {/* MOD 模块占位（规划中） */}
        <TabsContent value="mods" className="min-h-0 flex-1" />
      </Tabs>
    </main>
  );
}

export default App;
