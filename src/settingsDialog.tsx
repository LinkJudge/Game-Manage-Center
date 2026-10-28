import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  Archive,
  Blocks,
  Cloud,
  DatabaseZap,
  Download,
  ExternalLink,
  FolderOpen,
  Gamepad2,
  Info,
  Loader2,
  Puzzle,
  RefreshCw,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
  UploadCloud,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { ActionTooltip } from "@/components/ui/tooltip";
import { setLocaleSetting, useI18n } from "@/i18n";
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
import { clearCoverMemoryCache } from "@/components/gameCover";

// ---------------------------------------------------------------- 设置分区导航

type SettingsTab = "general" | "saves" | "trainers" | "extensions" | "mods" | "about";

const SETTINGS_TABS: {
  id: SettingsTab;
  label: string;
  icon: typeof SlidersHorizontal;
  disabled?: boolean;
  title?: string;
}[] = [
  { id: "general", label: "settings.tab.general", icon: SlidersHorizontal },
  { id: "saves", label: "settings.tab.saves", icon: Archive },
  { id: "trainers", label: "settings.tab.trainers", icon: Gamepad2 },
  { id: "extensions", label: "settings.tab.extensions", icon: Blocks },
  { id: "mods", label: "MOD", icon: Puzzle, disabled: true, title: "app.mod.planned" },
  { id: "about", label: "settings.tab.about", icon: Info },
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
  return <div className="divide-y rounded-2xl border bg-card px-3.5">{children}</div>;
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
  const { t, setting: localeSetting } = useI18n();
  const [backupPath, setBackupPath] = useState<string>("");
  const [trainerPath, setTrainerPath] = useState<string>("");
  const [steamgriddbApiKey, setSteamgriddbApiKey] = useState<string>("");
  const { theme, setTheme } = useTheme();
  const {
    autoScanOnStartup, autoScanIntervalMin, setPrefs: setScanPrefs,
  } = useScanStore();
  const [maxBackups, setMaxBackups] = useState<string>("5");
  const [compress, setCompress] = useState(false);
  const [autoBackupEnabled, setAutoBackupEnabled] = useState(false);
  const [closeToTray, setCloseToTray] = useState(true);
  // 云存档（WebDAV）
  const [webdavUrl, setWebdavUrl] = useState<string>("");
  const [webdavUsername, setWebdavUsername] = useState<string>("");
  const [webdavPassword, setWebdavPassword] = useState<string>("");
  const [webdavEnabled, setWebdavEnabled] = useState(false);
  const [testingDav, setTestingDav] = useState(false);
  const [davResult, setDavResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [uploadingDav, setUploadingDav] = useState(false);
  const [cloudOpen, setCloudOpen] = useState(false);
  const [cloudList, setCloudList] = useState<
    { path: string; size: number; modified: string | null; isDir: boolean }[]
  >([]);
  const [loadingCloud, setLoadingCloud] = useState(false);
  const [downloadingName, setDownloadingName] = useState<string | null>(null);

  // 只在每次打开时用后端值初始化一次。设置项改为即时保存后，保存会 invalidate
  // settings 查询，若每次都回灌 state，正在输入的内容会被打断、光标会跳。
  const hydratedRef = useRef(false);
  useEffect(() => {
    if (!open) {
      hydratedRef.current = false;
      return;
    }
    if (hydratedRef.current || !settings.data) return;
    hydratedRef.current = true;
    setBackupPath(settings.data.backupPath ?? "");
    setTrainerPath(settings.data.trainerPath ?? "");
    setSteamgriddbApiKey(settings.data.steamgriddbApiKey ?? "");
    setMaxBackups(String(settings.data.maxBackups));
    setCompress(settings.data.compressBackups);
    setAutoBackupEnabled(settings.data.autoBackupEnabled);
    setCloseToTray(settings.data.closeToTray);
    setWebdavUrl(settings.data.webdavUrl ?? "");
    setWebdavUsername(settings.data.webdavUsername ?? "");
    setWebdavPassword(settings.data.webdavPassword ?? "");
    setWebdavEnabled(settings.data.webdavEnabled);
    // 后端有明确语言设置时以其为准；为空表示跟随系统，保留本地状态
    if (settings.data.locale === "zh" || settings.data.locale === "en") {
      setLocaleSetting(settings.data.locale);
    }
  }, [open, settings.data, setLocaleSetting]);

  // 测试 WebDAV 连接：用当前输入框的值（不必先保存）
  const testDav = () => {
    const url = webdavUrl.trim();
    if (!url) {
      setDavResult({ ok: false, text: t("settings.extensions.webdav.needUrl") });
      return;
    }
    setTestingDav(true);
    setDavResult(null);
    invoke<{ status: number; server: string | null; entries: number }>("test_webdav", {
      url,
      username: webdavUsername.trim() || null,
      password: webdavPassword || null,
    })
      .then((info) =>
        setDavResult({
          ok: true,
          text: t("settings.extensions.webdav.ok", { entries: info.entries }),
        }),
      )
      .catch((e) =>
        setDavResult({ ok: false, text: `${t("settings.extensions.webdav.fail")}：${String(e)}` }),
      )
      .finally(() => setTestingDav(false));
  };

  // 上传全部备份到云端
  const uploadAllDav = () => {
    setUploadingDav(true);
    setDavResult(null);
    invoke<{ uploaded: number; skipped: number; failed: number; bytes: number; errors: string[] }>(
      "upload_backups_to_webdav",
    )
      .then((r) => {
        if (r.uploaded === 0 && r.skipped === 0 && r.failed === 0) {
          setDavResult({ ok: false, text: t("settings.extensions.webdav.uploadNone") });
          return;
        }
        const text =
          t("settings.extensions.webdav.uploadDone", {
            uploaded: r.uploaded,
            skipped: r.skipped,
            failed: r.failed,
          }) + (r.errors.length ? `：${r.errors[0]}` : "");
        setDavResult({ ok: r.failed === 0, text });
      })
      .catch((e) => setDavResult({ ok: false, text: String(e) }))
      .finally(() => setUploadingDav(false));
  };

  // 打开云端备份列表
  const openCloudList = () => {
    setCloudOpen(true);
    setLoadingCloud(true);
    setCloudList([]);
    invoke<{ path: string; size: number; modified: string | null; isDir: boolean }[]>(
      "list_cloud_backups",
    )
      .then((list) => setCloudList(list.filter((e) => !e.isDir && e.path.endsWith(".zip"))))
      .catch((e) => toast.error(String(e)))
      .finally(() => setLoadingCloud(false));
  };

  // 下载单份云端备份并还原
  const downloadOne = (path: string) => {
    const name = path.split("/").pop() ?? path;
    setDownloadingName(name);
    invoke<{ title: string }>("download_cloud_backup", { fileName: name })
      .then((info) => {
        toast.success(t("settings.extensions.webdav.downloadDone", { title: info.title }));
        queryClient.invalidateQueries({ queryKey: ["all-backups"] });
      })
      .catch((e) => toast.error(String(e)))
      .finally(() => setDownloadingName(null));
  };

  const [migrateAsk, setMigrateAsk] = useState<string | null>(null);
  const [updatingCatalog, setUpdatingCatalog] = useState(false);
  const [updatingApp, setUpdatingApp] = useState(false);
  const [clearingCovers, setClearingCovers] = useState(false);
  const [updatingDb, setUpdatingDb] = useState(false);

  // 存档数据库在线更新：下载 → 校验（条数 > 0）→ 原子替换 → 热切换
  const updateDatabase = () => {
    setUpdatingDb(true);
    invoke<{ count: number; updatedAt: string | null }>("update_save_database")
      .then((r) => {
        toast.success(t("settings.toast.dbUpdated", { count: r.count.toLocaleString() }));
        queryClient.invalidateQueries({ queryKey: ["save-database-info"] });
      })
      .catch((e) => toast.error(t("settings.toast.dbUpdateFailed"), { description: String(e) }))
      .finally(() => setUpdatingDb(false));
  };
  const [addingDefender, setAddingDefender] = useState(false);

  // 即时生效：只提交变更字段。后端 save_settings 的参数都是 Option，有值才改，
  // 因此这里传什么就改什么，不会覆盖未传的项。
  const save = useMutation({
    mutationFn: (patch: Record<string, unknown>) =>
      invoke<AppSettings>("save_settings", patch),
    onSuccess: (saved) => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      queryClient.invalidateQueries({ queryKey: ["sgdb-status"] });
      // 备份位置变了 → 询问是否把现有备份搬过去（只在真正换目录时问一次）
      const previous = settings.data?.backupPath ?? "";
      const next = saved.backupPath ?? "";
      if (next && previous && previous !== next) {
        setMigrateAsk(next);
      }
    },
    onError: (e) => toast.error(t("settings.toast.saveFailed"), { description: String(e) }),
  });

  const browse = async () => {
    const picked = await invoke<string | null>("pick_folder");
    if (picked) {
      setBackupPath(picked);
      save.mutate({ backupPath: picked });
    }
  };

  const browseTrainer = async () => {
    const picked = await invoke<string | null>("pick_folder");
    if (picked) {
      setTrainerPath(picked);
      save.mutate({ trainerPath: picked });
    }
  };


  const migrate = useMutation({
    mutationFn: (target: string) => invoke<number>("migrate_backups", { newPath: target }),
    onSuccess: (count) => {
      toast.success(t("settings.toast.migrated", { count }));
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      queryClient.invalidateQueries({ queryKey: ["all-backups"] });
    },
    onError: (e) => toast.error(t("settings.toast.migrateFailed"), { description: String(e) }),
  });

  const toggleAutostart = useMutation({
    mutationFn: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
    onSuccess: (enabled) => {
      queryClient.invalidateQueries({ queryKey: ["autostart"] });
      toast.success(enabled ? t("settings.toast.autostartOn") : t("settings.toast.autostartOff"));
    },
    onError: (e) => toast.error(t("saves.toast.opFailed"), { description: String(e) }),
  });

  // 修改器目录在线更新：下载 → 校验通过才落盘 → 重载
  const updateCatalog = () => {
    setUpdatingCatalog(true);
    invoke<{ count: number; generatedAt: string | null }>("update_trainer_catalog")
      .then((r) =>
        toast.success(t("settings.toast.catalogUpdated", { count: r.count }), {
          description: r.generatedAt ? t("settings.toast.catalogGeneratedAt", { date: r.generatedAt.slice(0, 10) }) : undefined,
        }),
      )
      .catch((e) => toast.error(t("settings.toast.catalogUpdateFailed"), { description: String(e) }))
      .finally(() => setUpdatingCatalog(false));
  };

  // 清空已下载的游戏封面缓存；下次打开列表会按当前规则重新获取
  const clearCoverCache = () => {
    setClearingCovers(true);
    invoke<number>("clear_game_cover_cache")
      .then((count) => {
        clearCoverMemoryCache();
        toast.success(count > 0 ? t("settings.toast.coverCleared", { count }) : t("settings.toast.coverNone"));
      })
      .catch((e) => toast.error(t("settings.toast.clearFailed"), { description: String(e) }))
      .finally(() => setClearingCovers(false));
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
      .catch((e) => toast.error(t("settings.toast.checkUpdateFailed"), { description: String(e) }))
      .finally(() => setUpdatingApp(false));
  };

  const openProjectPage = () => {
    import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
      openUrl("https://github.com/LinkJudge/Game-Manage-Center").catch((e) =>
        toast.error(t("settings.toast.openFailed"), { description: String(e) }),
      ),
    );
  };

  const openReleasePage = (url: string) => {
    import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
      openUrl(url).catch((e) => toast.error(t("settings.toast.openFailed"), { description: String(e) })),
    );
  };

  const openSgdbKeys = () => {
    import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
      openUrl("https://www.steamgriddb.com/profile/preferences/api").catch((e) =>
        toast.error(t("settings.toast.openFailed"), { description: String(e) }),
      ),
    );
  };

  // Defender 白名单：把修改器目录加入排除项，防止修改器文件被杀软误删（UAC 确认）
  const addDefender = () => {
    setAddingDefender(true);
    invoke("add_defender_exclusion")
      .then(() => toast.success(t("settings.toast.defenderAdded")))
      .catch((e) => {
        const msg = String(e);
        if (msg.includes("取消"))
          toast.info(t("settings.toast.uacCancelled"), { description: t("settings.toast.uacCancelledDesc") });
        else toast.error(t("settings.toast.defenderFailed"), { description: msg });
      })
      .finally(() => setAddingDefender(false));
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[600px] max-h-[86vh] w-full flex-col gap-0 overflow-hidden p-0 sm:max-w-[960px]">
        <DialogHeader className="sr-only">
          <DialogTitle>{t("settings.dialog.title")}</DialogTitle>
          <DialogDescription>{t("settings.dialog.desc")}</DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1">
          {/* 左侧分区导航（Discord 式） */}
          <nav className="w-44 shrink-0 space-y-0.5 border-r bg-muted/40 p-3">
            <p className="px-2 pb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
              {t("settings.dialog.title")}
            </p>
            {SETTINGS_TABS.map((item) => {
              const Icon = item.icon;
              const active = tab === item.id;
              const buttonNode = (
                <button
                  key={item.id}
                  onClick={() => !item.disabled && setTab(item.id)}
                  disabled={item.disabled}
                  aria-label={item.title ? t(item.title) : undefined}
                  className={`flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-sm transition-colors ${
                    active
                      ? "bg-muted font-medium text-foreground"
                      : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
                  } ${item.disabled ? "cursor-not-allowed opacity-50" : ""}`}
                >
                  <Icon className="size-4 shrink-0" />
                  {t(item.label)}
                </button>
              );
              return item.title ? (
                <ActionTooltip key={item.id} content={t(item.title)} disabled={item.disabled}>
                  {buttonNode}
                </ActionTooltip>
              ) : (
                buttonNode
              );
            })}
          </nav>

          {/* 右侧内容区（独立滚动） */}
          <div className="min-w-0 flex-1 overflow-y-auto p-6 pt-9">
            {tab === "general" && (
              <div>
                <PaneHeader title={t("settings.general.title")} description={t("settings.general.desc")} />
                <RowCard>
                  <Row
                    label={t("settings.language.label")}
                    description={t("settings.language.desc")}
                    control={
                      <Select
                        value={localeSetting}
                        onValueChange={(v) => {
                          setLocaleSetting(v as "system" | "zh" | "en");
                          save.mutate({ locale: v === "system" ? "" : v });
                        }}
                      >
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="system">{t("settings.language.system")}</SelectItem>
                          <SelectItem value="zh">简体中文</SelectItem>
                          <SelectItem value="en">English</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label={t("settings.theme.label")}
                    description={t("settings.theme.desc")}
                    control={
                      <Select value={theme} onValueChange={(v) => setTheme(v as ThemeMode)}>
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="system">{t("settings.theme.system")}</SelectItem>
                          <SelectItem value="dark">{t("settings.theme.dark")}</SelectItem>
                          <SelectItem value="light">{t("settings.theme.light")}</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label={t("settings.autoBackup.label")}
                    description={t("settings.autoBackup.desc")}
                    control={
                      <Switch checked={autoBackupEnabled} onCheckedChange={(v) => {
                          setAutoBackupEnabled(v);
                          save.mutate({ autoBackupEnabled: v });
                        }} />
                    }
                  />
                  <Row
                    label={t("settings.autoScan.label")}
                    description={t("settings.autoScan.desc")}
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
                          <SelectItem value="off">{t("settings.autoScan.off")}</SelectItem>
                          <SelectItem value="startup">{t("settings.autoScan.startup")}</SelectItem>
                          <SelectItem value="60">{t("settings.general.m60")}</SelectItem>
                          <SelectItem value="180">{t("settings.general.m180")}</SelectItem>
                          <SelectItem value="360">{t("settings.general.m360")}</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label={t("settings.closeTray.label")}
                    description={t("settings.closeTray.desc")}
                    control={
                      <Select value={closeToTray ? "tray" : "exit"} onValueChange={(v) => {
                          const toTray = v === "tray";
                          setCloseToTray(toTray);
                          save.mutate({ closeToTray: toTray });
                        }}>
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="tray">{t("settings.closeTray.tray")}</SelectItem>
                          <SelectItem value="exit">{t("settings.closeTray.exit")}</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label={t("settings.updateCheck.label")}
                    description={t("settings.updateCheck.desc")}
                    control={
                      <Select
                        value={updateCheckMode}
                        onValueChange={(v) => setUpdateCheckMode(v as UpdateCheckMode)}
                      >
                        <SelectTrigger className="w-[120px] shrink-0">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="startup">{t("settings.updateCheck.startup")}</SelectItem>
                          <SelectItem value="7">{t("settings.updateCheck.d7")}</SelectItem>
                          <SelectItem value="30">{t("settings.updateCheck.d30")}</SelectItem>
                          <SelectItem value="never">{t("settings.updateCheck.never")}</SelectItem>
                        </SelectContent>
                      </Select>
                    }
                  />
                  <Row
                    label={t("settings.autostart.label")}
                    description={t("settings.autostart.desc")}
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

            {tab === "extensions" && (
              <div>
                <PaneHeader title={t("settings.extensions.title")} description={t("settings.extensions.desc")} />
                <div className="space-y-4">
                  {/* 云存档（WebDAV） */}
                  <div className="space-y-3 rounded-2xl border bg-card p-4">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <Label className="flex items-center gap-1.5">
                          <Cloud className="size-3.5" />
                          {t("settings.extensions.webdav.title")}
                        </Label>
                        <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                          {t("settings.extensions.webdav.desc")}
                        </p>
                      </div>
                      <Switch
                        checked={webdavEnabled}
                        onCheckedChange={(v) => {
                          setWebdavEnabled(v);
                          save.mutate({ webdavEnabled: v });
                        }}
                      />
                    </div>

                    <div className="space-y-2">
                      <Input
                        value={webdavUrl}
                        onChange={(e) => setWebdavUrl(e.target.value)}
                        onBlur={() => save.mutate({ webdavUrl: webdavUrl.trim() })}
                        placeholder={t("settings.extensions.webdav.urlPlaceholder")}
                        className="font-mono text-xs"
                        autoComplete="off"
                        spellCheck={false}
                      />
                      <div className="grid grid-cols-2 gap-2">
                        <Input
                          value={webdavUsername}
                          onChange={(e) => setWebdavUsername(e.target.value)}
                          onBlur={() => save.mutate({ webdavUsername: webdavUsername.trim() })}
                          placeholder={t("settings.extensions.webdav.user")}
                          className="text-xs"
                          autoComplete="off"
                        />
                        <Input
                          type="password"
                          value={webdavPassword}
                          onChange={(e) => setWebdavPassword(e.target.value)}
                          onBlur={() => save.mutate({ webdavPassword })}
                          placeholder={t("settings.extensions.webdav.pass")}
                          className="text-xs"
                          autoComplete="off"
                        />
                      </div>
                    </div>

                    <div className="flex flex-wrap items-center gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={testDav}
                        disabled={testingDav}
                        className="shrink-0"
                      >
                        {testingDav ? (
                          <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                        ) : (
                          <Cloud className="mr-1.5 size-3.5" />
                        )}
                        {testingDav
                          ? t("settings.extensions.webdav.testing")
                          : t("settings.extensions.webdav.test")}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={uploadAllDav}
                        disabled={uploadingDav || !webdavEnabled}
                        className="shrink-0"
                      >
                        {uploadingDav ? (
                          <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                        ) : (
                          <UploadCloud className="mr-1.5 size-3.5" />
                        )}
                        {uploadingDav
                          ? t("settings.extensions.webdav.uploading")
                          : t("settings.extensions.webdav.upload")}
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={openCloudList}
                        disabled={!webdavEnabled}
                        className="shrink-0"
                      >
                        <Cloud className="mr-1.5 size-3.5" />
                        {t("settings.extensions.webdav.browse")}
                      </Button>
                      {davResult && (
                        <span
                          className={`text-xs ${davResult.ok ? "text-emerald-500" : "text-destructive"}`}
                        >
                          {davResult.text}
                        </span>
                      )}
                    </div>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("settings.extensions.webdav.uploadHint")}
                    </p>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      {t("settings.extensions.webdav.passHint")}
                    </p>
                  </div>

                  <div className="space-y-3 rounded-2xl border bg-card p-4">
                    <div>
                      <Label>{t("settings.extensions.sgdbKey.label")}</Label>
                      <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                        {t("settings.extensions.sgdbKey.desc")}
                      </p>
                    </div>
                    <div className="flex gap-2">
                      <Input
                        type="password"
                        value={steamgriddbApiKey}
                        onChange={(e) => setSteamgriddbApiKey(e.target.value)}
                        onBlur={() => save.mutate({ steamgriddbApiKey: steamgriddbApiKey.trim() })}
                        placeholder={t("settings.extensions.sgdbKey.placeholder")}
                        className="font-mono text-xs"
                        autoComplete="off"
                      />
                      <Button variant="outline" size="sm" onClick={openSgdbKeys} className="shrink-0">
                        <ExternalLink className="mr-1.5 size-3.5" />
                        {t("settings.extensions.sgdbKey.get")}
                      </Button>
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      {t("settings.extensions.sgdbKey.hint")}
                    </p>
                  </div>

                  <div className="space-y-3 rounded-2xl border bg-card p-4">
                    <div>
                      <Label>{t("settings.extensions.cache.label")}</Label>
                      <p className="mt-0.5 text-xs text-muted-foreground">
                        {t("settings.extensions.cache.desc")}
                      </p>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      className="text-destructive hover:text-destructive"
                      onClick={clearCoverCache}
                      disabled={clearingCovers}
                    >
                      {clearingCovers ? (
                        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                      ) : (
                        <Trash2 className="mr-1.5 size-3.5" />
                      )}
                      {t("settings.extensions.cache.clear")}
                    </Button>
                  </div>

                  <div className="rounded-2xl border bg-card p-4 text-xs leading-relaxed text-muted-foreground">
                    <p>
                      {t("settings.extensions.tip")}
                    </p>
                  </div>
                </div>
              </div>
            )}

            {tab === "saves" && (
              <div>
                <PaneHeader title={t("settings.saves.title")} description={t("settings.saves.desc")} />
                <div className="space-y-4 rounded-2xl border bg-card p-4">
                  <div className="space-y-1.5">
                    <Label>{t("settings.saves.pathLabel")}</Label>
                    <p className="text-xs text-muted-foreground">{t("settings.saves.pathHint")}</p>
                    <div className="flex gap-2">
                      <Input value={backupPath} onChange={(e) => setBackupPath(e.target.value)}
                        onBlur={() => save.mutate({ backupPath: backupPath.trim() })} className="text-xs" placeholder="默认位置" />
                      <Button variant="outline" size="sm" onClick={browse} className="shrink-0">
                        <FolderOpen className="mr-1 size-4" />
                        {t("settings.saves.browse")}
                      </Button>
                    </div>
                  </div>
                  <div className="flex items-center justify-between border-t pt-3">
                    <div>
                      <Label>{t("settings.saves.maxLabel")}</Label>
                      <p className="text-xs text-muted-foreground">{t("settings.saves.maxHint")}</p>
                    </div>
                    {/* 主题化步进器：隐藏原生 spinner（深色主题下与 UI 不协调） */}
                    <div className="flex shrink-0 items-center rounded-lg border border-input dark:bg-input/30">
                      <ActionTooltip
                        content={t("settings.saves.decrease")}
                        disabled={(Number(maxBackups) || 1) <= 1}
                      >
                        <button
                          type="button"
                          onClick={() => {
                            const next = Math.max(1, (Number(maxBackups) || 5) - 1);
                            setMaxBackups(String(next));
                            save.mutate({ maxBackups: next });
                          }}
                          disabled={(Number(maxBackups) || 1) <= 1}
                          aria-label={t("settings.saves.decrease")}
                          className="flex h-8 w-8 items-center justify-center rounded-l-lg text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground disabled:opacity-40"
                        >
                          −
                        </button>
                      </ActionTooltip>
                      <Input
                        type="number"
                        min={1}
                        max={100}
                        value={maxBackups}
                        onChange={(e) => {
                          const v = e.target.value.replace(/[^\d]/g, "");
                          setMaxBackups(v === "" ? "" : String(Math.min(100, Math.max(1, Number(v)))));
                        }}
                        onBlur={(e) => {
                          const v = e.target.value.replace(/[^\d]/g, "");
                          const num = v === "" ? 5 : Math.min(100, Math.max(1, Number(v)));
                          setMaxBackups(String(num));
                          save.mutate({ maxBackups: num });
                        }}
                        className="w-12 border-0 bg-transparent text-center [appearance:textfield] focus-visible:ring-0 [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none dark:bg-transparent"
                      />
                      <ActionTooltip
                        content={t("settings.saves.increase")}
                        disabled={(Number(maxBackups) || 0) >= 100}
                      >
                        <button
                          type="button"
                          onClick={() => {
                            const next = Math.min(100, (Number(maxBackups) || 5) + 1);
                            setMaxBackups(String(next));
                            save.mutate({ maxBackups: next });
                          }}
                          disabled={(Number(maxBackups) || 0) >= 100}
                          aria-label={t("settings.saves.increase")}
                          className="flex h-8 w-8 items-center justify-center rounded-r-lg text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground disabled:opacity-40"
                        >
                          +
                        </button>
                      </ActionTooltip>
                    </div>
                  </div>
                  <div className="flex items-center justify-between">
                    <div>
                      <Label>{t("settings.saves.zipLabel")}</Label>
                      <p className="text-xs text-muted-foreground">{t("settings.saves.zipHint")}</p>
                    </div>
                    <Switch
                      checked={compress}
                      onCheckedChange={(v) => {
                        setCompress(v);
                        save.mutate({ compressBackups: v });
                      }}
                    />
                  </div>
                  <div className="flex items-center justify-between gap-3 border-t pt-3">
                    <div className="min-w-0">
                      <Label>{t("settings.saves.dbLabel")}</Label>
                      <p className="text-xs text-muted-foreground">
                        {dbInfo.data
                          ? t("settings.saves.dbInfo", {
                              count: dbInfo.data.count.toLocaleString(),
                              upped:
                                dbInfo.data.source === "user"
                                  ? t("settings.saves.dbUpped")
                                  : "",
                            })
                          : t("settings.saves.dbLoading")}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={updateDatabase} disabled={updatingDb} className="shrink-0">
                      {updatingDb ? (
                        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                      ) : (
                        <DatabaseZap className="mr-1.5 size-3.5" />
                      )}
                      {t("settings.saves.update")}
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {tab === "trainers" && (
              <div>
                <PaneHeader title={t("settings.trainers.title")} description={t("settings.trainers.desc")} />
                <div className="space-y-4 rounded-2xl border bg-card p-4">
                  <div className="space-y-1.5">
                    <Label>{t("settings.trainers.pathLabel")}</Label>
                    <p className="text-xs text-muted-foreground">
                      {t("settings.trainers.pathHint")}
                    </p>
                    <div className="flex gap-2">
                      <Input value={trainerPath} onChange={(e) => setTrainerPath(e.target.value)}
                        onBlur={() => save.mutate({ trainerPath: trainerPath.trim() })} className="text-xs" placeholder="默认位置" />
                      <Button variant="outline" size="sm" onClick={browseTrainer} className="shrink-0">
                        <FolderOpen className="mr-1 size-4" />
                        {t("settings.saves.browse")}
                      </Button>
                    </div>
                  </div>
                  <div className="flex items-center justify-between gap-3 border-t pt-3">
                    <div className="min-w-0">
                      <Label>{t("settings.trainers.catalogLabel")}</Label>
                      <p className="text-xs text-muted-foreground">{t("settings.trainers.catalogHint")}</p>
                    </div>
                    <Button variant="outline" size="sm" onClick={updateCatalog} disabled={updatingCatalog} className="shrink-0">
                      {updatingCatalog ? (
                        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                      ) : (
                        <DatabaseZap className="mr-1.5 size-3.5" />
                      )}
                      {t("settings.trainers.update")}
                    </Button>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <Label>{t("settings.trainers.defenderLabel")}</Label>
                      <p className="text-xs text-muted-foreground">
                        {t("settings.trainers.defenderHint")}
                      </p>
                    </div>
                    <Button variant="outline" size="sm" onClick={addDefender} disabled={addingDefender} className="shrink-0">
                      {addingDefender ? (
                        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                      ) : (
                        <ShieldCheck className="mr-1.5 size-3.5" />
                      )}
                      {t("settings.trainers.run")}
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {tab === "about" && (
              <div>
                <PaneHeader title={t("settings.about.title")} description={t("settings.about.desc")} />
                <div className="space-y-4">
                  {/* 品牌行 */}
                  <div className="flex items-center gap-4 rounded-2xl border bg-card p-4">
                    <img src="/app-icon.png" alt={t("settings.about.brandAlt")} className="size-14 rounded-xl" />
                    <div className="min-w-0">
                      <div className="text-lg font-semibold">{t("settings.about.brandAlt")}</div>
                      <p className="text-xs text-muted-foreground">
                        Game Manage Center
                        {appInfo.data
                          ? t("settings.about.versionLine", {
                              app: appInfo.data.version,
                              core: appInfo.data.coreVersion,
                            })
                          : t("settings.about.loading")}
                      </p>
                    </div>
                  </div>

                  {/* 更新与链接 */}
                  <div className="divide-y rounded-2xl border bg-card px-3.5">
                    <div className="flex items-center justify-between gap-3 py-3">
                      <div className="min-w-0">
                        <Label>{t("settings.about.checkLabel")}</Label>
                        <p className="text-xs text-muted-foreground">
                          {updateResult
                            ? updateResult.hasUpdate
                              ? t("settings.about.hasUpdate", {
                                  version: updateResult.latestVersion ?? "",
                                })
                              : t("settings.about.upToDate", {
                                  version: appInfo.data?.version ?? "",
                                })
                            : t("settings.about.hint")}
                        </p>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        {updateResult?.hasUpdate && (
                          <Button
                            variant="default"
                            size="sm"
                            onClick={() => openReleasePage(updateResult.releaseUrl)}
                          >
                            {t("settings.about.goDownload")}
                          </Button>
                        )}
                        <Button variant="outline" size="sm" onClick={checkUpdate} disabled={updatingApp}>
                          {updatingApp ? (
                            <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                          ) : (
                            <RefreshCw className="mr-1.5 size-3.5" />
                          )}
                          {updateResult ? t("settings.about.recheck") : t("settings.about.check")}
                        </Button>
                      </div>
                    </div>
                    <div className="flex items-center justify-between gap-3 py-3">
                      <div className="min-w-0">
                        <Label>{t("settings.about.projectLabel")}</Label>
                        <p className="truncate text-xs text-muted-foreground">
                          github.com/LinkJudge/Game-Manage-Center
                        </p>
                      </div>
                      <Button variant="outline" size="sm" onClick={openProjectPage} className="shrink-0">
                        <ExternalLink className="mr-1.5 size-3.5" />
                        {t("settings.about.open")}
                      </Button>
                    </div>
                  </div>

                  {/* 许可与致谢 */}
                  <div className="rounded-2xl border bg-card p-4 text-xs leading-relaxed text-muted-foreground">
                    <p>{t("settings.about.license")}</p>
                    <p className="mt-1.5">{t("settings.about.dataFrom")}</p>
                    <p className="mt-1.5">{t("settings.about.coverNote")}</p>
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* 设置项即时生效，无需底部保存/取消 */}
      </DialogContent>

      {/* 云端备份列表 */}
      <Dialog open={cloudOpen} onOpenChange={setCloudOpen}>
        <DialogContent className="max-w-xl">
          <DialogHeader>
            <DialogTitle>{t("settings.extensions.webdav.browseTitle")}</DialogTitle>
            <DialogDescription>
              {loadingCloud
                ? t("settings.extensions.webdav.browseLoading")
                : t("settings.extensions.webdav.browseDesc", { count: cloudList.length })}
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[420px] divide-y overflow-y-auto">
            {!loadingCloud && cloudList.length === 0 && (
              <p className="py-8 text-center text-sm text-muted-foreground">
                {t("settings.extensions.webdav.browseEmpty")}
              </p>
            )}
            {cloudList.map((entry) => {
              const name = entry.path.split("/").pop() ?? entry.path;
              const busy = downloadingName === name;
              return (
                <div key={entry.path} className="flex items-center justify-between gap-3 py-2.5">
                  <div className="min-w-0">
                    <p className="truncate text-sm">{name}</p>
                    <p className="text-xs text-muted-foreground">
                      {(entry.size / 1024).toFixed(1)} KB
                      {entry.modified ? ` · ${entry.modified}` : ""}
                    </p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => downloadOne(entry.path)}
                    disabled={busy}
                    className="shrink-0"
                  >
                    {busy ? (
                      <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                    ) : (
                      <Download className="mr-1.5 size-3.5" />
                    )}
                    {busy
                      ? t("settings.extensions.webdav.downloading")
                      : t("settings.extensions.webdav.download")}
                  </Button>
                </div>
              );
            })}
          </div>
        </DialogContent>
      </Dialog>

      {/* 备份位置变更 → 询问是否迁移现有备份 */}
      <AlertDialog open={migrateAsk != null} onOpenChange={(open) => !open && setMigrateAsk(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("settings.migrate.title")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("settings.migrate.desc")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("settings.migrate.skip")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (migrateAsk) migrate.mutate(migrateAsk);
                setMigrateAsk(null);
              }}
            >
              {t("settings.migrate.do")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Dialog>
  );
}
