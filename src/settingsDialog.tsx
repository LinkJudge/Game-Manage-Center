import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { FolderOpen, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
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
import { useScanStore } from "@/store";

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

  const [backupPath, setBackupPath] = useState<string>("");
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

  const [migrateAsk, setMigrateAsk] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: () =>
      invoke<AppSettings>("save_settings", {
        backupPath: backupPath || null,
        maxBackups: Number(maxBackups) || 5,
        compressBackups: compress,
        autoBackupEnabled: autoBackupEnabled,
        closeToTray: closeToTray,
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[880px]">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>备份位置与轮换上限会立即影响后续备份操作。</DialogDescription>
        </DialogHeader>

        {settings.isLoading ? (
          <div className="flex h-24 items-center justify-center text-muted-foreground">
            <Loader2 className="mr-2 size-4 animate-spin" /> 加载中……
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-5 py-2">
            <section className="space-y-2">
              <h3 className="px-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">备份</h3>
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
                <div className="flex items-center justify-between pt-1">
                  <div>
                    <Label>滚动备份上限</Label>
                    <p className="text-xs text-muted-foreground">超出上限自动删除最旧备份；固定（Pin）的不受影响</p>
                  </div>
                  <Input
                    type="number"
                    min={1}
                    max={100}
                    value={maxBackups}
                    onChange={(e) => setMaxBackups(e.target.value)}
                    className="w-20 shrink-0 text-center"
                  />
                </div>
                <div className="flex items-center justify-between pt-1">
                  <Label>压缩备份为 .zip</Label>
                  <Switch checked={compress} onCheckedChange={setCompress} />
                </div>
              </div>
            </section>

            <section className="space-y-2">
              <h3 className="px-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">通用</h3>
              <div className="divide-y rounded-xl border bg-card px-3.5">
                <div className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <Label>主题</Label>
                    <p className="text-xs text-muted-foreground">跟随系统、深色或浅色</p>
                  </div>
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
                </div>
                <div className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <Label>自动备份</Label>
                    <p className="text-xs text-muted-foreground">存档变化或游戏退出时自动备份所有已安装游戏</p>
                  </div>
                  <Switch
                    checked={autoBackupEnabled}
                    onCheckedChange={(v) => setAutoBackupEnabled(v)}
                  />
                </div>
                <div className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <Label>自动扫描</Label>
                    <p className="text-xs text-muted-foreground">启动时或按间隔扫描本机游戏</p>
                  </div>
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
                </div>
                <div className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <Label>关闭窗口时</Label>
                    <p className="text-xs text-muted-foreground">进入托盘可继续自动备份；直接退出将结束所有任务</p>
                  </div>
                  <Select
                    value={closeToTray ? "tray" : "exit"}
                    onValueChange={(v) => setCloseToTray(v === "tray")}
                  >
                    <SelectTrigger className="w-[120px] shrink-0">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="tray">进入托盘</SelectItem>
                      <SelectItem value="exit">直接退出</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className="flex items-center justify-between gap-3 py-3">
                  <div className="min-w-0">
                    <Label>开机自动启动</Label>
                    <p className="text-xs text-muted-foreground">随 Windows 登录自动运行</p>
                  </div>
                  <Switch
                    checked={autostart.data ?? false}
                    onCheckedChange={(v) => toggleAutostart.mutate(v)}
                    disabled={toggleAutostart.isPending}
                  />
                </div>
              </div>
            </section>
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            {save.isPending && <Loader2 className="mr-2 size-4 animate-spin" />}
            保存
          </Button>
        </DialogFooter>
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
