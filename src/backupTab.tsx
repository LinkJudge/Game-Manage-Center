import { Fragment, useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  Archive,
  Eraser,
  FolderOpen,
  FolderPlus,
  History,
  ImagePlus,
  Loader2,
  RotateCcw,
  ScanSearch,
  Search,
  Telescope,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
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
import { RowMenu, type RowMenuItem } from "@/components/rowMenu";
import { ActionTooltip } from "@/components/ui/tooltip";
import { ACTION_COPY } from "@/lib/actionCopy";
import { GameCover, refreshAllCovers } from "@/components/gameCover";
import { BackupHistoryDialog } from "@/backupHistoryDialog";
import { CoverPickerDialog } from "@/components/coverPickerDialog";
import { useScanStore } from "@/store";
import type { ConflictInfo, BatchResult, CoverFillSummary, CoverLookup, GameBackups, ScanOutput, ScannedGame } from "@/types";
import { CustomGamesDialog } from "@/customGamesDialog";
import { gameNamePair, useI18n } from "@/i18n";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

// 各启动器平台的代表色
const PLATFORM_COLORS: Record<string, string> = {
  Steam: "#66c0f4",
  Epic: "#8f96a3",
  GOG: "#b366e0",
  Xbox: "#4cb748",
  EA: "#e0684b",
  Ubisoft: "#3d8bff",
  "Battle.net": "#38b0ff",
  Blizzard: "#38b0ff",
  Rockstar: "#fcaf17",
  自定义: "#9b8cf0",
};

/** 安装平台徽章：该平台代表色的浅底 + 彩字 */
function PlatformBadges({ platforms }: { platforms: string[] }) {
  const { t } = useI18n();
  if (platforms.length === 0) return <span className="text-xs text-muted-foreground">{t("saves.row.notInstalled")}</span>;
  return (
    <div className="flex flex-wrap gap-1.5">
      {platforms.map((p) => {
        const color = PLATFORM_COLORS[p] ?? "#8f96a3";
        return (
          <Badge
            key={p}
            variant="outline"
            className="gap-1.5 px-2 py-0 text-[10px] font-semibold"
            style={{ color, backgroundColor: `${color}1c`, borderColor: `${color}3d` }}
          >
            <span className="size-1.5 rounded-full" style={{ backgroundColor: color }} />
            {p}
          </Badge>
        );
      })}
    </div>
  );
}

function formatDateParts(ms: number): { date: string; time: string } {
  const value = new Date(ms);
  return {
    date: value.toLocaleDateString("zh-CN", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }),
    time: value.toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }),
  };
}

/** 合并列表的统一行：本机扫描结果 + 仅存在备份的游戏 */
interface SaveRow {
  key: string;
  id: number;
  title: string;
  zhCn: string | null;
  scanned?: ScannedGame;
  backup?: GameBackups;
  backupCount: number;
  backupSize: number | null;
  latestBackup: string | null;
}

function formatBackupStamp(value: string | null): string {
  if (!value) return "—";
  const [day, time] = value.split("_");
  return `${day.replace(/-/g, "/")}${time ? ` ${time.replace(/-/g, ":")}` : ""}`;
}

// ---------------------------------------------------------------- 备份标签页

export function BackupTab() {
  const { t, lang } = useI18n();
  const queryClient = useQueryClient();
  const backupCounts = useQuery({
    queryKey: ["backup-counts"],
    queryFn: () => invoke<Record<string, number>>("backup_counts"),
  });
  // 合并恢复页数据：有备份但当前扫描不到的游戏也必须保留入口
  const allBackups = useQuery({
    queryKey: ["all-backups"],
    queryFn: () => invoke<GameBackups[]>("list_all_backups"),
    staleTime: 30_000,
  });
  const backupById = new Map(
    (allBackups.data ?? []).map((game) => [game.wikiPageId, game] as const),
  );
  const {
    scanOutput, deepOutput, clearDeepResult, dismissedLeftoverIds, dismissLeftovers,
    scannedAt, scanning, runScan, setDeepResult,
  } = useScanStore();
  const [filter, setFilter] = useState("");
  const [customOpen, setCustomOpen] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [rowMenu, setRowMenu] = useState<{ x: number; y: number; row: SaveRow } | null>(null);
  // 右键菜单"恢复最新"的冲突确认
  const [conflict, setConflict] = useState<{ id: number; title: string; date: string; info: ConflictInfo } | null>(null);
  const [manageGameId, setManageGameId] = useState<number | null>(null);
  const [coverPickerRow, setCoverPickerRow] = useState<SaveRow | null>(null);
  const [fillingCovers, setFillingCovers] = useState(false);
  const managedGame =
    manageGameId != null ? (backupById.get(manageGameId) ?? null) : null;
  const sgdbStatus = useQuery({
    queryKey: ["sgdb-status"],
    queryFn: () => invoke<boolean>("steamgriddb_status"),
    staleTime: 30_000,
  });

  // 备份记录被全部删除后自动关闭弹窗，避免留下一个空记录框
  useEffect(() => {
    if (
      manageGameId != null &&
      allBackups.data &&
      !allBackups.data.some((game) => game.wikiPageId === manageGameId)
    ) {
      setManageGameId(null);
    }
  }, [manageGameId, allBackups.data]);

  // 深度扫描：全库解析（含已卸载游戏的残留存档），耗时约 1-3 分钟
  const deepScan = useMutation({
    mutationFn: () => invoke<ScanOutput>("scan_deep_games", { allAccounts: false }),
    onSuccess: (out) => {
      setDeepResult(out);
      const leftovers = out.games.filter((g) => !g.installPath).length;
      toast.success(t("saves.toast.deepDone", { count: out.games.length }), {
        description: t("saves.toast.deepDoneDesc", { count: leftovers }),
      });
    },
    onError: (e) => toast.error(t("saves.toast.deepFailed"), { description: String(e) }),
  });


  const invalidateAfterBackup = (id?: number) => {
    if (id != null) queryClient.invalidateQueries({ queryKey: ["backups", id] });
    queryClient.invalidateQueries({ queryKey: ["all-backups"] });
    queryClient.invalidateQueries({ queryKey: ["backup-counts"] });
  };

  const doBackup = useMutation({
    mutationFn: (id: number) =>
      invoke<string>("backup_game", { wikiPageId: id, allAccounts: false }),
    onSuccess: (instancePath, id) => {
      toast.success(t("saves.toast.backupDone"), { description: instancePath });
      invalidateAfterBackup(id);
    },
    onError: (e) => toast.error(t("saves.toast.backupFailed"), { description: String(e) }),
  });

  // 批量备份：单条 Rust 命令（一次 IPC，Rust 侧顺序执行，单个失败不中断）
  const batchBackupBulk = useMutation({
    mutationFn: (ids: number[]) => invoke<BatchResult[]>("batch_backup", { ids }),
    onSuccess: (results) => {
      const ok = results.filter((r) => r.ok);
      const failed = results.filter((r) => !r.ok);
      if (failed.length === 0) {
        toast.success(t("saves.toast.batchBackupDone", { count: ok.length }));
      } else {
        toast.warning(t("saves.toast.batchBackupPartial", { ok: ok.length, failed: failed.length }), {
          description: failed.map((r) => `${r.title}：${r.message}`).join("\n"),
        });
      }
      setSelected(new Set());
      invalidateAfterBackup();
    },
    onError: (e) => toast.error(t("saves.toast.batchBackupFailed"), { description: String(e) }),
  });

  // 批量恢复：每个游戏恢复其最新备份；本地较新的自动跳过
  const batchRestoreBulk = useMutation({
    mutationFn: (ids: number[]) => invoke<BatchResult[]>("batch_restore_latest", { ids }),
    onSuccess: (results) => {
      const ok = results.filter((r) => r.ok);
      const failed = results.filter((r) => !r.ok);
      if (failed.length === 0) {
        toast.success(t("saves.toast.batchRestoreDone", { count: ok.length }));
      } else {
        toast.warning(t("saves.toast.batchRestorePartial", { ok: ok.length, failed: failed.length }), {
          description: failed.map((r) => `${r.title}：${r.message}`).join("\n"),
        });
      }
      setSelected(new Set());
      invalidateAfterBackup();
    },
    onError: (e) => toast.error(t("saves.toast.batchRestoreFailed"), { description: String(e) }),
  });

  // 右键菜单 / 行操作：恢复最新备份（先查冲突）
  const restoreLatest = useMutation({
    mutationFn: (args: { id: number; date: string; force: boolean }) =>
      invoke<{ restoredPaths: number; errors: string[] }>("restore_backup", {
        wikiPageId: args.id,
        date: args.date,
        force: args.force,
      }),
    onSuccess: (result) => {
      if (result.errors.length > 0) {
        toast.warning(t("saves.toast.restorePartial", { count: result.restoredPaths }), {
          description: result.errors.join("\n"),
        });
      } else {
        toast.success(t("saves.toast.restoreDone", { count: result.restoredPaths }));
      }
      invalidateAfterBackup();
    },
    onError: (e) => {
      if (String(e).includes("CONFLICT")) return;
      toast.error(t("saves.toast.restoreFailed"), { description: String(e) });
    },
  });

  const startRestoreLatest = (id: number, title: string) => {
    invoke<{ wikiPageId: number; backups: { date: string }[] } | null>("list_backups", {
      wikiPageId: id,
    })
      .then((data) => {
        if (!data || data.backups.length === 0) {
          toast.info(t("saves.toast.noBackupYet", { title }));
          return;
        }
        const date = data.backups[0].date;
        return invoke<ConflictInfo>("check_restore_conflict", {
          wikiPageId: id,
          date,
        }).then((info) => {
          if (info.conflict) {
            setConflict({ id, title, date, info });
          } else {
            restoreLatest.mutate({ id, date, force: false });
          }
        });
      })
      .catch((e) => toast.error(t("saves.toast.opFailed"), { description: String(e) }));
  };

  const toggleSelect = (id: number, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  // 合并常规扫描与深度扫描的残留：常规结果为基础，深度独有（未安装）的追加在后，
  // 两者独立保存——常规扫描不会冲掉残留，直到用户主动清除或重新深度扫描
  // （store 写入时 scanOutput.deep 恒为 false、deepOutput.deep 恒为 true）
  // 行序：已安装在前、未安装残留统一排在最后（快扫自己也可能有残留——已卸载但存档还在）
  const quickGames = scanOutput?.games ?? [];
  const deepGames = deepOutput?.games ?? [];
  const quickGameIds = new Set(quickGames.map((g) => g.entry.wikiPageId));
  const quickInstalled = quickGames.filter((g) => g.installPath);
  // 常规扫描可能仍看得到 Steam 卸载后遗留的安装文件夹；清理后要持久隐藏，
  // 否则下一次普通扫描又把它带回来。
  const dismissedIds = new Set(dismissedLeftoverIds ?? []);
  const quickLeftover = quickGames.filter(
    (g) => !g.installPath && !dismissedIds.has(g.entry.wikiPageId),
  );
  const deepLeftover = deepGames.filter(
    (g) =>
      !g.installPath &&
      !quickGameIds.has(g.entry.wikiPageId) &&
      !dismissedIds.has(g.entry.wikiPageId),
  );
  const merged = [...quickInstalled, ...quickLeftover, ...deepLeftover];
  // 底部统计的"未安装残留"= 当前仍展示的快扫残留 + 深扫残留
  const leftoverCount = quickLeftover.length + deepLeftover.length;
  const leftoverDismissIds = Array.from(
    new Set([
      ...quickGames.filter((g) => !g.installPath).map((g) => g.entry.wikiPageId),
      ...deepGames.filter((g) => !g.installPath).map((g) => g.entry.wikiPageId),
    ]),
  );

  const games = merged.filter((g: ScannedGame) => {
    if (g.resolvedFiles.length === 0 && g.registryCandidates.length === 0) return false;
    if (!filter.trim()) return true;
    const q = filter.trim().toLowerCase();
    return (
      g.entry.title.toLowerCase().includes(q) ||
      (g.entry.zhCn ?? "").includes(filter.trim())
    );
  });

  const scannedRows: SaveRow[] = games.map((game) => {
    const id = game.entry.wikiPageId;
    const backup = backupById.get(id);
    return {
      key: `scanned-${id}`,
      id,
      title: game.entry.title,
      zhCn: game.entry.zhCn,
      scanned: game,
      backup,
      backupCount: backup?.backups.length ?? backupCounts.data?.[String(id)] ?? 0,
      backupSize: backup?.backupSize ?? (game.backupSize || null),
      latestBackup: backup?.latestBackup ?? null,
    };
  });

  const scannedIds = new Set(merged.map((g) => g.entry.wikiPageId));
  const backupOnlyRows: SaveRow[] = (allBackups.data ?? [])
    .filter((game) => !scannedIds.has(game.wikiPageId))
    .filter((game) => {
      if (!filter.trim()) return true;
      const q = filter.trim().toLowerCase();
      return game.title.toLowerCase().includes(q) || (game.zhCn ?? "").includes(filter.trim());
    })
    .map((game) => ({
      key: `backup-${game.wikiPageId}`,
      id: game.wikiPageId,
      title: game.title,
      zhCn: game.zhCn,
      backup: game,
      backupCount: game.backups.length,
      backupSize: game.backupSize,
      latestBackup: game.latestBackup,
    }));

  const backupOnlyTotal = (allBackups.data ?? []).filter(
    (game) => !scannedIds.has(game.wikiPageId),
  ).length;

  const rows: SaveRow[] = [...scannedRows, ...backupOnlyRows];

  type SaveGroup = "installed" | "leftover" | "backupOnly";
  const groupOf = (row: SaveRow): SaveGroup =>
    row.scanned ? (row.scanned.installPath ? "installed" : "leftover") : "backupOnly";
  const groupMeta: Record<SaveGroup, { label: string; dot: string }> = {
    installed: { label: t("saves.group.installed"), dot: "is-installed" },
    leftover: { label: t("saves.group.leftover"), dot: "is-leftover" },
    backupOnly: { label: t("saves.group.backupOnly"), dot: "is-available" },
  };
  const groupCounts: Record<SaveGroup, number> = { installed: 0, leftover: 0, backupOnly: 0 };
  for (const row of rows) groupCounts[groupOf(row)]++;

  const allSelected = rows.length > 0 && rows.every((row) => selected.has(row.id));

  // 路径的语义类型：兼容历史持久化数据（旧版本序列化为 content_kind）
  const pathKind = (f: { contentKind?: string; content_kind?: string }): string | undefined =>
    f.contentKind ?? f.content_kind;

  const buildRowMenuItems = (row: SaveRow): RowMenuItem[] => {
    const items: RowMenuItem[] = [];
    // 右键只保留行按钮没有提供的动作：打开文件夹。
    // 备份/恢复/记录在操作列已有按钮，换图可点封面，不重复进菜单。
    if (row.backupCount > 0) {
      items.push({
        label: ACTION_COPY.openBackupFolder.full,
        icon: FolderOpen,
        onSelect: () => {
          invoke("open_game_backup_folder", { wikiPageId: row.id }).catch((e) =>
            toast.error(t("saves.toast.openFailed"), { description: String(e) }),
          );
        },
      });
    }
    // 存档与设置分开打开（resolvedFiles 里两类路径混排，首个未必是存档）；
    // 深度扫描的未安装残留同样按此提供（残留存档在磁盘上真实存在，可直接打开）
    if (row.scanned) {
      const savesPath = row.scanned.resolvedFiles.find((f) => pathKind(f) === "saves")?.resolved;
      const configPath = row.scanned.resolvedFiles.find((f) => pathKind(f) === "config")?.resolved;
      if (savesPath) {
        items.push({
          label: ACTION_COPY.openSaveFolder.full,
          icon: FolderOpen,
          onSelect: () => {
            invoke("open_save_folder", { path: savesPath }).catch((e) =>
              toast.error(t("saves.toast.openFailed"), { description: String(e) }),
            );
          },
        });
      }
      if (configPath) {
        items.push({
          label: ACTION_COPY.openConfigFolder.full,
          icon: FolderOpen,
          onSelect: () => {
            invoke("open_save_folder", { path: configPath }).catch((e) =>
              toast.error(t("saves.toast.openFailed"), { description: String(e) }),
            );
          },
        });
      }
    }
    return items;
  };

  const selectedRows = rows.filter((row) => selected.has(row.id));
  const backupableCount = selectedRows.filter((row) => row.scanned).length;
  const restorableCount = selectedRows.filter((row) => row.backupCount > 0).length;

  const fillMissingCovers = () => {
    if (rows.length === 0) return;
    const seen = new Set<number>();
    const requests: CoverLookup[] = [];
    for (const row of rows) {
      if (seen.has(row.id)) continue;
      seen.add(row.id);
      requests.push({
        title: row.title,
        wikiPageId: row.id,
        steamId: row.scanned?.entry.steamId ?? null,
      });
    }
    setFillingCovers(true);
    invoke<CoverFillSummary>("steamgriddb_fill_missing", { requests })
      .then((summary) => {
        if (summary.filled > 0) refreshAllCovers();
        if (summary.failed > 0) {
          toast.warning(
            t("saves.toast.coverFillSummary", { ok: summary.filled, skipped: summary.skipped, failed: summary.failed }),
            { description: summary.errors.join("\n") },
          );
        } else {
          toast.success(
            summary.filled > 0
              ? t("saves.toast.coverFilled", { count: summary.filled })
              : t("saves.toast.coverAllExist"),
          );
        }
      })
      .catch((e) => toast.error(t("saves.toast.coverFailed"), { description: String(e) }))
      .finally(() => setFillingCovers(false));
  };

  return (
    // pb-20：给右下角悬浮扫描按钮留出滚动余量，最后一行可完整滚出遮挡区
    <div className="space-y-4 pb-20">
      {/* 搜索工具行：筛选 + 清理未安装游戏 + 自定义 */}
      <div className="flex items-center gap-2">
        <div className="relative max-w-md flex-1">
          <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder={t("saves.filter.placeholder")}
            className="pl-8"
          />
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            dismissLeftovers(leftoverDismissIds);
            clearDeepResult();
            toast.success(t("saves.toast.leftoverCleared"), {
              description: t("saves.toast.leftoverClearedDesc", { count: leftoverCount }),
            });
          }}
          disabled={leftoverCount === 0}
          title={
            leftoverCount > 0
              ? t("saves.tip.clearLeftover", { count: leftoverCount })
              : t("saves.tip.clearLeftoverNone")
          }
        >
          <Eraser className="mr-1.5 size-3.5" />
          {t("saves.action.clearLeftover")}{leftoverCount > 0 && ` (${leftoverCount})`}
        </Button>
        <Button size="sm" variant="outline" onClick={() => setCustomOpen(true)}>
          <FolderPlus className="mr-1.5 size-3.5" />
          {t("saves.action.customGames")}
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={fillMissingCovers}
          disabled={!sgdbStatus.data || fillingCovers || rows.length === 0}
          title={
            sgdbStatus.data
              ? t("saves.tip.fillCovers")
              : t("saves.tip.fillCoversNeedKey")
          }
        >
          {fillingCovers ? (
            <Loader2 className="mr-1.5 size-3.5 animate-spin" />
          ) : (
            <ImagePlus className="mr-1.5 size-3.5" />
          )}
          {fillingCovers ? t("saves.action.fillingCovers") : t("saves.action.fillCovers")}
        </Button>
      </div>

      {merged.length === 0 && !scanning && backupOnlyTotal === 0 && (
        <Card className="flex h-40 flex-col items-center justify-center gap-3 text-muted-foreground">
          <ScanSearch className="size-8" />
          <p>{t("saves.empty.hint")}</p>
        </Card>
      )}
      {scanning && merged.length === 0 && backupOnlyTotal === 0 && (
        <Card className="flex h-40 items-center justify-center gap-3 text-muted-foreground">
          <Loader2 className="size-5 animate-spin" /> {t("saves.empty.scanning")}
        </Card>
      )}

      {(merged.length > 0 || backupOnlyTotal > 0) && (
        <Card className="overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-12 pl-4">
                  <Checkbox
                    checked={allSelected}
                    onCheckedChange={(v) => {
                      if (v) setSelected(new Set(rows.map((row) => row.id)));
                      else setSelected(new Set());
                    }}
                  />
                </TableHead>
                <TableHead className="w-[31%]">{t("saves.col.game")}</TableHead>
                <TableHead className="w-[15%]">{t("saves.col.platform")}</TableHead>
                <TableHead className="w-[17%]">{t("saves.col.modified")}</TableHead>
                <TableHead className="w-[15%]">{t("saves.col.backups")}</TableHead>
                <TableHead className="pr-4 text-right">{t("saves.col.actions")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row, index) => {
                const group = groupOf(row);
                const showGroup = index === 0 || groupOf(rows[index - 1]) !== group;
                const modified = row.scanned?.latestModifiedMs
                  ? formatDateParts(row.scanned.latestModifiedMs)
                  : null;
                const backupStamp = formatBackupStamp(row.latestBackup);
                const [backupDate, backupTime] =
                  backupStamp === "—" ? ["—", ""] : backupStamp.split(" ");
                const namePair = gameNamePair(row.title, row.zhCn, lang);
                const displayName = namePair.main;
                const subtitle = [
                  namePair.sub,
                  row.scanned
                    ? row.scanned.resolvedFiles.length > 0
                      ? t("saves.row.saveCount", { count: row.scanned.resolvedFiles.length })
                      : row.scanned.registryCandidates.length > 0
                        ? t("saves.row.registry")
                        : t("saves.row.local")
                    : t("saves.row.backupOnly"),
                ]
                  .filter(Boolean)
                  .join(" · ");
                const backingUp =
                  doBackup.isPending && doBackup.variables === row.id;

                return (
                  <Fragment key={row.key}>
                    {showGroup && (
                      <TableRow className="gm-group-row">
                        <TableCell colSpan={6} className="py-2 pl-4">
                          <div className="flex items-center gap-2">
                            <span className={`gm-group-dot ${groupMeta[group].dot}`} />
                            <span>{groupMeta[group].label}</span>
                            <span className="text-[10px] font-medium tabular-nums text-muted-foreground/60">
                              {groupCounts[group]}
                            </span>
                          </div>
                        </TableCell>
                      </TableRow>
                    )}
                    <TableRow
                      className="gm-game-row"
                      onContextMenu={(e) => {
                        e.preventDefault();
                        if (buildRowMenuItems(row).length > 0) {
                          setRowMenu({ x: e.clientX, y: e.clientY, row });
                        }
                      }}
                    >
                      <TableCell className="pl-4" onClick={(e) => e.stopPropagation()}>
                        <Checkbox
                          checked={selected.has(row.id)}
                          onCheckedChange={(v) => toggleSelect(row.id, v === true)}
                        />
                      </TableCell>
                      <TableCell>
                        <div className="flex min-w-0 items-center gap-3">
                          <ActionTooltip content={ACTION_COPY.changeCover.full}>
                            <button
                              type="button"
                              className="gm-cover-trigger"
                              aria-label={ACTION_COPY.changeCover.full}
                              onClick={() => setCoverPickerRow(row)}
                            >
                              <GameCover
                                title={row.title}
                                wikiPageId={row.id}
                                steamId={row.scanned?.entry.steamId ?? null}
                                installPath={row.scanned?.installPath ?? null}
                              />
                              <span className="gm-cover-edit-badge">
                                <ImagePlus className="size-2.5" />
                              </span>
                            </button>
                          </ActionTooltip>
                          <div className="min-w-0">
                            <div className="truncate text-[0.84rem] font-semibold tracking-[-0.01em]">
                              {displayName}
                            </div>
                            <div className="mt-0.5 truncate text-[11px] text-muted-foreground/80">
                              {subtitle}
                            </div>
                          </div>
                        </div>
                      </TableCell>
                      <TableCell>
                        {row.scanned ? (
                          <PlatformBadges platforms={row.scanned.installedPlatforms} />
                        ) : (
                          <span className="text-[11px] text-muted-foreground">{t("saves.row.backupRecords")}</span>
                        )}
                      </TableCell>
                      <TableCell>
                        {modified ? (
                          <>
                            <div className="text-xs font-medium tabular-nums text-foreground/85">
                              {modified.date}
                            </div>
                            <div className="mt-0.5 text-[10px] tabular-nums text-muted-foreground/65">
                              {modified.time}
                            </div>
                          </>
                        ) : row.latestBackup ? (
                          <>
                            <div className="text-xs font-medium tabular-nums text-foreground/85">
                              {backupDate}
                            </div>
                            <div className="mt-0.5 text-[10px] tabular-nums text-muted-foreground/65">
                              {backupTime}
                            </div>
                          </>
                        ) : (
                          <span className="text-xs text-muted-foreground">—</span>
                        )}
                      </TableCell>
                      <TableCell>
                        <div className="text-xs font-medium tabular-nums text-foreground/85">
                          {row.backupCount > 0 ? t("saves.row.backupCount", { count: row.backupCount }) : t("saves.row.noBackup")}
                        </div>
                        <div className="mt-0.5 text-[10px] tabular-nums text-muted-foreground/65">
                          {row.backupCount > 0
                            ? formatBytes(row.backupSize ?? 0)
                            : formatBytes(row.scanned?.backupSize ?? 0)}
                        </div>
                      </TableCell>
                      <TableCell className="pr-4 text-right">
                        <div className="flex items-center justify-end gap-1.5">
                          <ActionTooltip
                            content={ACTION_COPY.backup.full}
                            disabled={!row.scanned || doBackup.isPending}
                          >
                            <Button
                              size="action"
                              aria-label={ACTION_COPY.backup.full}
                              onClick={() => doBackup.mutate(row.id)}
                              disabled={!row.scanned || doBackup.isPending}
                            >
                              {backingUp ? (
                                <Loader2 className="mr-1 size-3.5 animate-spin" />
                              ) : (
                                <Archive className="mr-1 size-3.5" />
                              )}
                              {ACTION_COPY.backup.short}
                            </Button>
                          </ActionTooltip>
                          <ActionTooltip
                            content={ACTION_COPY.restoreLatest.full}
                            disabled={row.backupCount === 0 || restoreLatest.isPending}
                          >
                            <Button
                              size="action"
                              variant="outline"
                              aria-label={ACTION_COPY.restoreLatest.full}
                              onClick={() => startRestoreLatest(row.id, row.title)}
                              disabled={row.backupCount === 0 || restoreLatest.isPending}
                            >
                              <RotateCcw className="mr-1 size-3.5" />
                              {ACTION_COPY.restoreLatest.short}
                            </Button>
                          </ActionTooltip>
                          <ActionTooltip
                            content={ACTION_COPY.manageBackups.full}
                            disabled={row.backupCount === 0}
                          >
                            <Button
                              size="action"
                              variant="outline"
                              aria-label={ACTION_COPY.manageBackups.full}
                              onClick={() => setManageGameId(row.id)}
                              disabled={row.backupCount === 0}
                            >
                              <History className="mr-1 size-3.5" />
                              {ACTION_COPY.manageBackups.short}
                            </Button>
                          </ActionTooltip>
                        </div>
                      </TableCell>
                    </TableRow>
                  </Fragment>
                );
              })}
              {rows.length === 0 && (
                <TableRow>
                  <TableCell colSpan={6} className="h-24 text-center text-muted-foreground">
                    {t("saves.empty.noMatch")}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
      )}

      {/* 列表底部统计：上次扫描时间 + 存档统计 */}
      {rows.length > 0 && !scanning && (
        <div className="flex items-center justify-center gap-2 pb-1 text-xs text-muted-foreground">
          {scannedAt && (
            <span>{t("saves.footer.lastScan", { time: new Date(scannedAt).toLocaleString() })}</span>
          )}
          {scannedAt && <span>·</span>}
          <span>
            {t("saves.footer.games", { count: rows.length })}
            {leftoverCount > 0 && t("saves.footer.leftover", { count: leftoverCount })}
            {backupOnlyRows.length > 0 && t("saves.footer.backupOnly", { count: backupOnlyRows.length })}
          </span>
        </div>
      )}

      <CustomGamesDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        onSaved={() => runScan()}
      />

      <BackupHistoryDialog
        open={manageGameId != null}
        game={managedGame}
        onOpenChange={(open) => !open && setManageGameId(null)}
        onChanged={invalidateAfterBackup}
      />

      <CoverPickerDialog
        open={coverPickerRow != null}
        onOpenChange={(open) => !open && setCoverPickerRow(null)}
        title={coverPickerRow?.title ?? ""}
        wikiPageId={coverPickerRow?.id ?? null}
        steamId={coverPickerRow?.scanned?.entry.steamId ?? null}
      />

      {/* 行右键菜单 */}
      {rowMenu && (
        <RowMenu
          x={rowMenu.x}
          y={rowMenu.y}
          items={buildRowMenuItems(rowMenu.row)}
          onClose={() => setRowMenu(null)}
        />
      )}

      {/* 右键"恢复最新"的冲突确认 */}
      <AlertDialog open={conflict != null} onOpenChange={(open) => !open && setConflict(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {conflict ? t("saves.conflict.title", { title: conflict.title }) : ""}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("saves.conflict.local", { time: conflict ? formatMs(conflict.info.localMs) : "" })}
              <br />
              {t("saves.conflict.backup", { time: conflict ? formatMs(conflict.info.backupMs) : "" })}
              <br />
              <br />
              {t("saves.conflict.question")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (conflict) restoreLatest.mutate({ id: conflict.id, date: conflict.date, force: true });
                setConflict(null);
              }}
            >
              {t("saves.conflict.overwrite")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 悬浮批量操作栏：选中游戏后出现在底部中央 */}
      {selectedRows.length > 0 && (
        <div className="fixed bottom-8 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-full border bg-background/95 px-5 py-2.5 shadow-xl backdrop-blur">
          <span className="text-sm font-medium">{t("saves.bulk.selected", { count: selectedRows.length })}</span>
          <Button
            size="sm"
            onClick={() =>
              batchBackupBulk.mutate(
                selectedRows.filter((row) => row.scanned).map((row) => row.id),
              )
            }
            disabled={batchBackupBulk.isPending || backupableCount === 0}
            title={t("saves.bulk.backupTip")}
          >
            {batchBackupBulk.isPending ? (
              <Loader2 className="mr-1.5 size-3.5 animate-spin" />
            ) : (
              <Archive className="mr-1.5 size-3.5" />
            )}
            {t("saves.bulk.backup")}{backupableCount > 0 ? ` (${backupableCount})` : ""}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() =>
              batchRestoreBulk.mutate(
                selectedRows.filter((row) => row.backupCount > 0).map((row) => row.id),
              )
            }
            disabled={batchRestoreBulk.isPending || restorableCount === 0}
            title={t("saves.bulk.restoreTip")}
          >
            {batchRestoreBulk.isPending ? (
              <Loader2 className="mr-1.5 size-3.5 animate-spin" />
            ) : (
              <RotateCcw className="mr-1.5 size-3.5" />
            )}
            {t("saves.bulk.restore")}{restorableCount > 0 ? ` (${restorableCount})` : ""}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            {t("saves.bulk.clear")}
          </Button>
        </div>
      )}

      {/* 悬空扫描按钮组：右下角横向排布，文字标签一眼可辨 */}
      <div className="fixed bottom-8 right-8 z-50 flex items-center gap-3">
        <button
          onClick={() => deepScan.mutate()}
          disabled={scanning || deepScan.isPending}
          title={t("saves.scan.deepTip")}
          className="flex h-11 items-center gap-2 rounded-full border bg-background px-4 text-sm font-medium shadow-xl shadow-black/25 transition-transform hover:scale-105 active:scale-95 disabled:opacity-70"
        >
          {deepScan.isPending ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <Telescope className="size-4" />
          )}
          {deepScan.isPending ? t("saves.scan.scanning") : t("saves.scan.deep")}
        </button>
        <button
          onClick={() => runScan()}
          disabled={scanning}
          title={scanning ? t("saves.scan.scanning") : t("saves.scan.localTip")}
          className="flex h-11 items-center gap-2 rounded-full bg-primary px-5 text-sm font-semibold text-primary-foreground shadow-xl shadow-black/25 transition-transform hover:scale-105 active:scale-95 disabled:opacity-70"
        >
          {scanning ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <ScanSearch className="size-5" />
          )}
          {scanning ? t("saves.scan.scanning") : t("saves.scan.local")}
        </button>
      </div>
    </div>
  );
}

function formatMs(ms: number | null): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString("zh-CN", { dateStyle: "short", timeStyle: "short" });
}
