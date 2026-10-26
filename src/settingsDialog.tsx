import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  Archive,
  DatabaseZap,
  ExternalLink,
  FolderOpen,
  Gamepad2,
  Info,
  Loader2,
  Puzzle,
  RefreshCw,
  ShieldCheck,
  SlidersHorizontal,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import type { AppSettings } from "@/types";
import { useTheme, type ThemeMode } from "@/theme";
import { useScanStore, useUpdateStore, type UpdateCheckMode } from "@/store";

// ---------------------------------------------------------------- 设置分区导航

type SettingsTab = "general" | "saves" | "trainers" | "mods" | "about";

const SETTINGS_TABS: {
  id: SettingsTab;
  label: string;
  icon: typeof SlidersHorizontal;
  disabled?: boolean;
  title?: string;
}[] = [
  { id: "general", label: "通用", icon: SlidersHorizontal },
  { id: "saves", label: "存档", icon: Archive },
  { id: "trainers", label: "修改器", icon: Gamepad2 },
  { id: "mods", label: "MOD", icon: Puzzle, disabled: true, title: "规划中：MOD 的安装、版本管理与冲突排查" },
  { id: "about", label: "关于", icon: Info },
];

/** 分区页头 */
function PaneHeader({ title, description }: { title: string; description: string }) {
  return (
    <div className="mb-4">
      <h2 className="text-lg font-semibold">{title}</h2>
      <p className="text-xs text-muted-foreground">{description}</p>
    </div>
  );
}

/** 通用区块的行容器 */
function RowCard({ children }: { children: React.ReactNode }) {
  return <div className="divide-y rounded-xl border bg-card px-3.5">{children}</div>;
}

function Row({ label, description, control }: { label: string; description: string; control: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 py-3">
      <div className="min-w-0">
        <Label>{label}</Label>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      {control}
    </div>
  );
}

// ---------------------------------------------------------------- 设置对话框

export function SettingsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (v: boolean) => void }) {
  const queryClient = useQueryClient();
  const settings = useQuery({
    queryKey: ["settings"],
    queryFn: () => invoke<AppSettings>("get_settings"),
    enabled: open,
  });
  const autostart = useQuery({
    queryKey: ["autostart"],
    queryFn: () => invoke<boolean>("get_autostart"),
    enabled: open,
  });
  const appInfo = useQuery({
    queryKey: ["app-info"],
    queryFn: () =>
      invoke<{ name: string; version: string; coreVersion: string }>("app_info"),
    enabled: open,
  });
  const dbInfo = useQuery({
    queryKey: ["save-database-info"],
    queryFn: () =>
      invoke<{ count: number; source: string; updatedAt: string | null }>(
        "save_database_info",
      ),
    enabled: open,
  });

  const [tab, setTab] = useState<SettingsTab>("general");
  const [backupPath, setBackupPath] = useState<string>("");
  const [trainerPath, setTrainerPath] = useState<string>("");
  const { theme, setTheme } = useTheme();
  const {
    autoScanOnStartup, autoScanIntervalMin, setPrefs: setScanPrefs,
  } = useScanStore();
  const [maxBackups, setMaxBackups] = useState<string>("5");
  const [compress, setCompress] = useState(false);
  const [autoBackupEnabled, setAutoBackupEnabled] = useState(false);
  const [closeToTray, setCloseToTray] = useState(true);

  useEffect(() => {
    if (settings.data) {
      setBackupPath(settings.data.backupPath ?? "");
      setTrainerPath(settings.data.trainerPath ?? "");
      setMaxBackups(String(settings.data.maxBackups));
      setCompress(settings.data.compressBackups);
      setAutoBackupEnabled(settings.data.autoBackupEnabled);
      setCloseToTray(settings.data.closeToTray);
    }
  }, [settings.data]);

  const browse = async () => {
    const picked = await invoke<string | null>("pick_folder");
    if (picked) setBackupPath(picked);
  };

  const browseTrainer = async () => {
    const picked = await invoke<string | null>("pick_folder");
    if (picked) setTrainerPath(picked);
  };

  const [migrateAsk, setMigrateAsk] = useState<string | null>(null);
  const [updatingCatalog, setUpdatingCatalog] = useState(false);
  const [updatingApp, setUpdatingApp] = useState(false);
  const [updatingDb, setUpdatingDb] = useState(false);

  // 存档数据库在线更新：下载 → 校验（条数 > 0）→ 原子替换 → 热切换
  const updateDatabase = () => {
    setUpdatingDb(true);
    invoke<{ count: number; updatedAt: string | null }>("update_save_database")
      .then((r) => {
        toast.success(`存档数据库已更新：${r.count.toLocaleString()} 条`);
        queryClient.invalidateQueries({ queryKey: ["save-database-info"] });
      })
      .catch((e) => toast.error("数据库更新失败", { description: String(e) }))
      .finally(() => setUpdatingDb(false));
  };
  const [addingDefender, setAddingDefender] = useState(false);

  const save = useMutation({
    mutationFn: () =>
      invoke<AppSettings>("save_settings", {
        backupPath: backupPath || null,
        maxBackups: Number(maxBackups) || 5,
        compressBackups: compress,
        autoBackupEnabled: autoBackupEnabled,
        closeToTray: closeToTray,
        trainerPath: trainerPath || null,
      }),
    onSuccess: (saved) => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      toast.success("设置已保存");
      onOpenChange(false);
      // 备份位置变了 → 询问是否把现有备份搬过去
      const previous = settings.data?.backupPath ?? "";
      const next = saved.backupPath ?? "";
      if (next && previous && previous !== next) {
        setMigrateAsk(next);
      }
    },
    onError: (e) => toast.error("保存失败", { description: String(e) }),
  });

  const migrate = useMutation({
    mutationFn: (target: string) => invoke<number>("migrate_backups", { newPath: target }),
    onSuccess: (count) => {
      toast.success(`已迁移 ${count} 个游戏的备份到新位置`);
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      queryClient.invalidateQueries({ queryKey: ["all-backups"] });
    },
    onError: (e) => toast.error("迁移失败", { description: String(e) }),
  });

  const toggleAutostart = useMutation({
    mutationFn: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
    onSuccess: (enabled) => {
      queryClient.invalidateQueries({ queryKey: ["autostart"] });
      toast.success(enabled ? "已开启开机自启" : "已关闭开机自启");
    },
    onError: (e) => toast.error("操作失败", { description: String(e) }),
  });

  // 修改器目录在线更新：下载 → 校验通过才落盘 → 重载
  const updateCatalog = () => {
    setUpdatingCatalog(true);
    invoke<{ count: number; generatedAt: string | null }>("update_trainer_catalog")
      .then((r) =>
        toast.success(`目录已更新：${r.count} 个修改器`, {
          description: r.generatedAt ? `目录生成于 ${r.generatedAt.slice(0, 10)}` : undefined,
        }),
      )
      .catch((e) => toast.error("目录更新失败", { description: String(e) }))
      .finally(() => setUpdatingCatalog(false));
  };

  // 应用更新检查（匿名查 GitHub Release）
  const [updateResult, setUpdateResult] = useState<{
    hasUpdate: boolean;
    latestVersion: string | null;
    releaseUrl: string;
  } | null>(null);
  const updateCheckMode = useUpdateStore((s) => s.updateCheckMode);
  const markUpdateChecked = useUpdateStore((s) => s.markUpdateChecked);
  const setUpdateCheckMode = useUpdateStore((s) => s.setUpdateCheckMode);
  const checkUpdate = () => {
    setUpdateResult(null);
    setUpdatingApp(true);
    invoke<{ hasUpdate: boolean; latestVersion: string | null; releaseUrl: string }>(
      "check_app_update",
    )
      .then((r) => {
        markUpdateChecked();
        setUpdateResult(r);
      })
      .catch((e) => toast.error("检查更新失败", { description: String(e) }))
      .finally(() => setUpdatingApp(false));
  };

  const openProjectPage = () => {
    import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
      openUrl("https://github.com/LinkJudge/Game-Manage-Center").catch((e) =>
        toast.error("打开失败", { description: String(e) }),
      ),
    );
  };

  const openReleasePage = (url: string) => {
    import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
      openUrl(url).catch((e) => toast.error("打开失败", { description: String(e) })),
    );
  };

  // Defender 白名单：把修改器目录加入排除项，防止修改器文件被杀软误删（UAC 确认）
  const addDefender = () => {
    setAddingDefender(true);
    invoke("add_defender_exclusion")
      .then(() => toast.success("已将修改器目录加入 Defender 排除项"))
      .catch((e) => {
        const msg = String(e);
        if (msg.includes("取消"))
          toast.info("已取消", { description: "未确认 UAC 弹窗，未添加排除项" });
        else toast.error("添加失败", { description: msg });
      })
      .finally(() => setAddingDefender(false));
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[600px] max-h-[86vh] w-full flex-col gap-0 overflow-hidden p-0 sm:max-w-[960px]">
        <DialogHeader className="sr-only">
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>管理 Game Manage Center 的通用、存档与修改器设置</DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1">
          {/* 左侧分区导航（Discord 式） */}
          <nav className="w-44 shrink-0 space-y-0.5 border-r bg-muted/40 p-3">
            <p className="px-2 pb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
              设置
            </p>
            {SETTINGS_TABS.map((t) => {
              const Icon = t.icon;
              const active = tab === t.id;
              return (
                <button
                  key={t.id}
                  onClick={() => !t.disabled && setTab(t.id)}
                  disabled={t.disabled}
                  title={t.title}
                  className={`flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors ${
                    active
                      ? "bg-muted font-medium text-foreground"
                      : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
                  } ${t.disabled ? "cursor-not-allowed opacity-50" : ""}`}
                >
                  <Icon className="size-4 shrink-0" />
                  {t.label}
                </button>
              );
            })}
          </nav>

          {/* 右侧内容区（独立滚动） */}
          <div className="min-w-0 flex-1 overflow-y-auto p-6 pt-9">
            {tab === "general" && (
              <div>
                <PaneHeader title="通用" description="外观与全局行为" />
                <RowCard>
                  <Row
                    label="主题"
                    description="跟随系统、深色或浅色"
                    control={
                      <Select value={theme} onValueChange={(v) => setTheme(v as ThemeMode)}>
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="system">系统默认</SelectItem>
                          <SelectItem value="dark">深色</SelectItem>
                          <SelectItem value="light">浅色</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label="自动备份"
                    description="存档变化或游戏退出时自动备份所有已安装游戏"
                    control={
                      <Switch checked={autoBackupEnabled} onCheckedChange={(v) => setAutoBackupEnabled(v)} />
                    }
                  />
                  <Row
                    label="自动扫描"
                    description="启动时或按间隔扫描本机游戏"
                    control={
                      <Select
                        value={autoScanOnStartup ? (autoScanIntervalMin > 0 ? String(autoScanIntervalMin) : "startup") : "off"}
                        onValueChange={(v) => {
                          if (v === "off") setScanPrefs({ autoScanOnStartup: false });
                          else if (v === "startup") setScanPrefs({ autoScanOnStartup: true, autoScanIntervalMin: 0 });
                          else setScanPrefs({ autoScanOnStartup: true, autoScanIntervalMin: Number(v) });
                        }}
                      >
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="off">关闭</SelectItem>
                          <SelectItem value="startup">每次启动</SelectItem>
                          <SelectItem value="60">每 60 分钟</SelectItem>
                          <SelectItem value="180">每 3 小时</SelectItem>
                          <SelectItem value="360">每 6 小时</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label="关闭窗口时"
                    description="进入托盘可继续自动备份；直接退出将结束所有任务"
                    control={
                      <Select value={closeToTray ? "tray" : "exit"} onValueChange={(v) => setCloseToTray(v === "tray")}>
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="tray">进入托盘</SelectItem>
                          <SelectItem value="exit">直接退出</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label="软件更新检查"
                    description="按频率连接 GitHub 检查新版本；发现新版会通知"
                    control={
                      <Select
                        value={updateCheckMode}
                        onValueChange={(v) => setUpdateCheckMode(v as UpdateCheckMode)}
                      >
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="startup">每次启动</SelectItem>
                          <SelectItem value="7">每 7 天</SelectItem>
                          <SelectItem value="30">每 30 天</SelectItem>
                          <SelectItem value="never">从不</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label="开机自动启动"
                    description="随 Windows 登录自动运行"
                    control={
                      <Switch
                        checked={autostart.data ?? false}
                        onCheckedChange={(v) => toggleAutostart.mutate(v)}
                        disabled={toggleAutostart.isPending}
                      />
                    }
                  />
                </RowCard>
              </div>
            )}

            {tab === "saves" && (
              <div>
                <PaneHeader title="存档" description="备份位置与轮换策略会立即影响后续备份操作" />
                <div className="space-y-4 rounded-xl border bg-card p-4">
                  <div className="space-y-1.5">
                    <Label>备份保存位置</Label>
                    <p className="text-xs text-muted-foreground">留空使用应用数据目录下的 Backups</p>
                    <div className="flex gap-2">
                      <Input value={backupPath} onChange={(e) => setBackupPath(e.target.value)} className="font-mono text-xs" placeholder="默认位置" />
                      <Button variant="outline" size="sm" onClick={browse} className="shrink-0">
                        <FolderOpen className="mr-1 size-4" />
                        浏览
                      </Button>
                    </div>
                  </div>
                  <div className="flex items-center justify-between border-t pt-3">
                    <div>
                      <Label>滚动备份上限</Label>
                      <p className="text-xs text-muted-foreground">超出上限自动删除最旧备份；固定（Pin）的不受影响</p>
                    </div>
                    {/* 主题化步进器：隐藏原生 spinner（深色主题下与 UI 不协调） */}
                    <div className="flex shrink-0 items-center rounded-lg border border-input dark:bg-input/30">
                      <button
                        type="button"
                        onClick={() => setMaxBackups(String(Math.max(1, (Number(maxBackups) || 5) - 1)))}
                        disabled={(Number(maxBackups) || 1) <= 1}
                        title="减少"
                        className="flex h-8 w-8 items-center justify-center rounded-l-lg text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground disabled:opacity-40"
                      >
                        −
                      </button>
                      <Input
                        type="number"
                        min={1}
                        max={100}
                        value={maxBackups}
                        onChange={(e) => {
                          const v = e.target.value.replace(/[^\d]/g, "");
                          setMaxBackups(v === "" ? "" : String(Math.min(100, Math.max(1, Number(v)))));
                        }}
                        className="w-12 border-0 bg-transparent text-center [appearance:textfield] focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none dark:bg-transparent"
                      />
                      <button
                        type="button"
                        onClick={() => setMaxBackups(String(Math.min(100, (Number(maxBackups) || 5) + 1)))}
                        disabled={(Number(maxBackups) || 0) >= 100}
                        title="增加"
                        className="flex h-8 w-8 items-center justify-center rounded-r-lg text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground disabled:opacity-40"
                      >
                        +
                      </button>
                    </div>
                  </div>
                  <div className="flex items-center justify-between">
                    <div>
                      <Label>压缩备份为 .zip</Label>
                      <p className="text-xs text-muted-foreground">体积更小，但备份与恢复略慢</p>
                    </div>
                    <Switch checked={compress} onCheckedChange={setCompress} />
                  </div>
                  <div className="flex items-center justify-between gap-3 border-t pt-3">
                    <div className="min-w-0">
                      <Label>存档数据库</Label>
                      <p className="text-xs text-muted-foreground">
                        {dbInfo.data
                          ? `当前 ${dbInfo.data.count.toLocaleString()} 条${
                              dbInfo.data.source === "user" ? "（已更新）" : ""
                            } · 从发布页拉取最新数据，校验通过才替换`
                          : "读取中……"}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={updateDatabase} disabled={updatingDb} className="shrink-0">
                      {updatingDb ? (
                        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                      ) : (
                        <DatabaseZap className="mr-1.5 size-3.5" />
                      )}
                      更新
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {tab === "trainers" && (
              <div>
                <PaneHeader title="修改器" description="安装位置与目录维护" />
                <div className="space-y-4 rounded-xl border bg-card p-4">
                  <div className="space-y-1.5">
                    <Label>修改器安装位置</Label>
                    <p className="text-xs text-muted-foreground">
                      留空使用应用数据目录下的 Trainers；更改后已安装的修改器不会自动移动
                    </p>
                    <div className="flex gap-2">
                      <Input value={trainerPath} onChange={(e) => setTrainerPath(e.target.value)} className="font-mono text-xs" placeholder="默认位置" />
                      <Button variant="outline" size="sm" onClick={browseTrainer} className="shrink-0">
                        <FolderOpen className="mr-1 size-4" />
                        浏览
                      </Button>
                    </div>
                  </div>
                  <div className="flex items-center justify-between gap-3 border-t pt-3">
                    <div className="min-w-0">
                      <Label>修改器目录</Label>
                      <p className="text-xs text-muted-foreground">从发布页拉取最新目录，校验通过才替换本地</p>
                    </div>
                    <Button variant="outline" size="sm" onClick={updateCatalog} disabled={updatingCatalog} className="shrink-0">
                      {updatingCatalog ? (
                        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                      ) : (
                        <DatabaseZap className="mr-1.5 size-3.5" />
                      )}
                      更新
                    </Button>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <Label>Defender 白名单</Label>
                      <p className="text-xs text-muted-foreground">
                        把修改器目录加入排除项，防止修改器文件被杀软误删（需 UAC 确认）
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={addDefender} disabled={addingDefender} className="shrink-0">
                      {addingDefender ? (
                        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                      ) : (
                        <ShieldCheck className="mr-1.5 size-3.5" />
                      )}
                      执行
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {tab === "about" && (
              <div>
                <PaneHeader title="关于" description="版本信息、项目链接与更新检查" />
                <div className="space-y-4">
                  {/* 品牌行 */}
                  <div className="flex items-center gap-4 rounded-xl border bg-card p-4">
                    <img src="/app-icon.png" alt="游戏管理中心" className="size-14 rounded-xl" />
                    <div className="min-w-0">
                      <div className="text-lg font-semibold">游戏管理中心</div>
                      <p className="text-xs text-muted-foreground">
                        Game Manage Center
                        {appInfo.data
                          ? ` · 应用 v${appInfo.data.version} · 引擎 v${appInfo.data.coreVersion}`
                          : " · 加载中……"}
                      </p>
                    </div>
                  </div>

                  {/* 更新与链接 */}
                  <div className="divide-y rounded-xl border bg-card px-3.5">
                    <div className="flex items-center justify-between gap-3 py-3">
                      <div className="min-w-0">
                        <Label>检查更新</Label>
                        <p className="text-xs text-muted-foreground">
                          {updateResult
                            ? updateResult.hasUpdate
                              ? `发现新版本 v${updateResult.latestVersion}`
                              : `已是最新版本（v${appInfo.data?.version ?? ""}）`
                            : "连接 GitHub 查询最新发布版本"}
                        </p>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        {updateResult?.hasUpdate && (
                          <Button
                            variant="default"
                            size="sm"
                            onClick={() => openReleasePage(updateResult.releaseUrl)}
                          >
                            前往下载
                          </Button>
                        )}
                        <Button variant="outline" size="sm" onClick={checkUpdate} disabled={updatingApp}>
                          {updatingApp ? (
                            <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                          ) : (
                            <RefreshCw className="mr-1.5 size-3.5" />
                          )}
                          {updateResult ? "重新检查" : "检查"}
                        </Button>
                      </div>
                    </div>
                    <div className="flex items-center justify-between gap-3 py-3">
                      <div className="min-w-0">
                        <Label>项目地址</Label>
                        <p className="truncate text-xs text-muted-foreground">
                          github.com/LinkJudge/Game-Manage-Center
                        </p>
                      </div>
                      <Button variant="outline" size="sm" onClick={openProjectPage} className="shrink-0">
                        <ExternalLink className="mr-1.5 size-3.5" />
                        打开
                      </Button>
                    </div>
                  </div>

                  {/* 许可与致谢 */}
                  <div className="rounded-xl border bg-card p-4 text-xs leading-relaxed text-muted-foreground">
                    <p>
                      本应用以 <span className="font-medium text-foreground">GPL-3.0</span> 协议开源。
                    </p>
                    <p className="mt-1.5">
                      存档路径数据来自{" "}
                      <span className="font-medium text-foreground">PCGamingWiki</span>
                      社区（CC BY-NC-SA）；修改器目录数据采集自{" "}
                      <span className="font-medium text-foreground">风灵月影</span>{" "}
                      官网公开发布的信息，修改器软件版权归原作者所有。
                    </p>
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* 底部操作条 */}
        <div className="flex items-center justify-end gap-2 border-t bg-muted/30 px-5 py-3">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            {save.isPending && <Loader2 className="mr-2 size-4 animate-spin" />}
            保存
          </Button>
        </div>
      </DialogContent>

      {/* 备份位置变更 → 询问是否迁移现有备份 */}
      <AlertDialog open={migrateAsk != null} onOpenChange={(open) => !open && setMigrateAsk(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>备份位置已更改</AlertDialogTitle>
            <AlertDialogDescription>
              是否把旧位置里现有的全部备份迁移到新位置？
              选择"不迁移"的话，旧备份会留在原处，之后的备份写入新位置。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>不迁移</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (migrateAsk) migrate.mutate(migrateAsk);
                setMigrateAsk(null);
              }}
            >
              迁移
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Dialog>
  );
}
