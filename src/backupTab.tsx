import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  Archive,
  Eraser,
  FolderOpen,
  FolderPlus,
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
import { useScanStore } from "@/store";
import type { ConflictInfo, BatchResult, ScanOutput, ScannedGame } from "@/types";
import { CustomGamesDialog } from "@/customGamesDialog";

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
  if (platforms.length === 0) return <span className="text-xs text-muted-foreground">未安装</span>;
  return (
    <div className="flex flex-wrap gap-1">
      {platforms.map((p) => {
        const color = PLATFORM_COLORS[p] ?? "#8f96a3";
        return (
          <Badge
            key={p}
            variant="outline"
            className="text-[10px] px-1.5 py-0 font-medium"
            style={{ color, backgroundColor: `${color}1f`, borderColor: `${color}45` }}
          >
            {p}
          </Badge>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------- 备份标签页

export function BackupTab() {
  const queryClient = useQueryClient();
  const backupCounts = useQuery({
    queryKey: ["backup-counts"],
    queryFn: () => invoke<Record<string, number>>("backup_counts"),
  });
  const {
    scanOutput, deepOutput, clearDeepResult, scannedAt, scanning, runScan,
    setDeepResult,
  } = useScanStore();
  const [filter, setFilter] = useState("");
  const [customOpen, setCustomOpen] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [rowMenu, setRowMenu] = useState<{ x: number; y: number; game: ScannedGame } | null>(null);
  // 右键菜单"恢复最新"的冲突确认
  const [conflict, setConflict] = useState<{ id: number; title: string; date: string; info: ConflictInfo } | null>(null);

  // 深度扫描：全库解析（含已卸载游戏的残留存档），耗时约 1-3 分钟
  const deepScan = useMutation({
    mutationFn: () => invoke<ScanOutput>("scan_deep_games", { allAccounts: false }),
    onSuccess: (out) => {
      setDeepResult(out);
      const leftovers = out.games.filter((g) => !g.installPath).length;
      toast.success(`深度扫描完成：${out.games.length} 个游戏有存档`, {
        description: `其中 ${leftovers} 个是未安装游戏的残留存档`,
      });
    },
    onError: (e) => toast.error("深度扫描失败", { description: String(e) }),
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
      toast.success("备份完成", { description: instancePath });
      invalidateAfterBackup(id);
    },
    onError: (e) => toast.error("备份失败", { description: String(e) }),
  });

  // 批量备份：单条 Rust 命令（一次 IPC，Rust 侧顺序执行，单个失败不中断）
  const batchBackupBulk = useMutation({
    mutationFn: (ids: number[]) => invoke<BatchResult[]>("batch_backup", { ids }),
    onSuccess: (results) => {
      const ok = results.filter((r) => r.ok);
      const failed = results.filter((r) => !r.ok);
      if (failed.length === 0) {
        toast.success(`批量备份完成（${ok.length} 个游戏）`);
      } else {
        toast.warning(`批量备份：${ok.length} 成功，${failed.length} 失败`, {
          description: failed.map((r) => `${r.title}：${r.message}`).join("\n"),
        });
      }
      setSelected(new Set());
      invalidateAfterBackup();
    },
    onError: (e) => toast.error("批量备份失败", { description: String(e) }),
  });

  // 右键菜单：恢复最新备份（先查冲突）
  const restoreLatest = useMutation({
    mutationFn: (args: { id: number; date: string; force: boolean }) =>
      invoke<{ restoredPaths: number; errors: string[] }>("restore_backup", {
        wikiPageId: args.id,
        date: args.date,
        force: args.force,
      }),
    onSuccess: (result) => {
      if (result.errors.length > 0) {
        toast.warning(`恢复了 ${result.restoredPaths} 个路径，部分失败`, {
          description: result.errors.join("\n"),
        });
      } else {
        toast.success(`恢复完成（${result.restoredPaths} 个路径）`);
      }
      invalidateAfterBackup();
    },
    onError: (e) => {
      if (String(e).includes("CONFLICT")) return;
      toast.error("恢复失败", { description: String(e) });
    },
  });

  const startRestoreLatest = (game: ScannedGame) => {
    invoke<{ wikiPageId: number; backups: { date: string }[] } | null>("list_backups", {
      wikiPageId: game.entry.wikiPageId,
    })
      .then((data) => {
        if (!data || data.backups.length === 0) {
          toast.info(`${game.entry.title} 还没有备份`);
          return;
        }
        const date = data.backups[0].date;
        return invoke<ConflictInfo>("check_restore_conflict", {
          wikiPageId: game.entry.wikiPageId,
          date,
        }).then((info) => {
          if (info.conflict) {
            setConflict({ id: game.entry.wikiPageId, title: game.entry.title, date, info });
          } else {
            restoreLatest.mutate({ id: game.entry.wikiPageId, date, force: false });
          }
        });
      })
      .catch((e) => toast.error("操作失败", { description: String(e) }));
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
  const quickInstalled = quickGames.filter((g) => g.installPath);
  const quickLeftover = quickGames.filter((g) => !g.installPath);
  const deepLeftover = deepGames.filter(
    (g) =>
      !g.installPath &&
      !quickGames.some((q) => q.entry.wikiPageId === g.entry.wikiPageId),
  );
  const merged = [...quickInstalled, ...quickLeftover, ...deepLeftover];
  // 底部统计的"未安装残留"= 快扫残留 + 深扫残留；
  // 「清理」按钮只清深扫结果（快扫残留重扫即回，清了也没意义）
  const leftoverCount = quickLeftover.length + deepLeftover.length;

  const games = merged.filter((g: ScannedGame) => {
    if (g.resolvedFiles.length === 0 && g.registryCandidates.length === 0) return false;
    if (!filter.trim()) return true;
    const q = filter.trim().toLowerCase();
    return (
      g.entry.title.toLowerCase().includes(q) ||
      (g.entry.zhCn ?? "").includes(filter.trim())
    );
  });
  const allSelected = games.length > 0 && games.every((g) => selected.has(g.entry.wikiPageId));

  // 路径的语义类型：兼容历史持久化数据（旧版本序列化为 content_kind）
  const pathKind = (f: { contentKind?: string; content_kind?: string }): string | undefined =>
    f.contentKind ?? f.content_kind;

  const buildRowMenuItems = (game: ScannedGame): RowMenuItem[] => {
    const items: RowMenuItem[] = [
      { label: "备份此游戏", icon: Archive, onSelect: () => doBackup.mutate(game.entry.wikiPageId) },
      { label: "恢复最新备份", icon: RotateCcw, onSelect: () => startRestoreLatest(game) },
    ];
    // 存档与设置分开打开（resolvedFiles 里两类路径混排，首个未必是存档）；
    // 深度扫描的未安装残留同样按此提供（残留存档在磁盘上真实存在，可直接打开）
    const savesPath = game.resolvedFiles.find((f) => pathKind(f) === "saves")?.resolved;
    const configPath = game.resolvedFiles.find((f) => pathKind(f) === "config")?.resolved;
    if (savesPath) {
      items.push({
        label: "打开存档文件夹",
        icon: FolderOpen,
        onSelect: () => {
          invoke("open_save_folder", { path: savesPath }).catch((e) =>
            toast.error("打开失败", { description: String(e) }),
          );
        },
      });
    }
    if (configPath) {
      items.push({
        label: "打开设置文件夹",
        icon: FolderOpen,
        onSelect: () => {
          invoke("open_save_folder", { path: configPath }).catch((e) =>
            toast.error("打开失败", { description: String(e) }),
          );
        },
      });
    }
    // 只有已备份的游戏才显示（命令层也不再有创建目录的副作用）
    if ((backupCounts.data?.[String(game.entry.wikiPageId)] ?? 0) > 0) {
      items.push({
        label: "打开备份文件夹",
        icon: FolderOpen,
        onSelect: () => {
          invoke("open_game_backup_folder", { wikiPageId: game.entry.wikiPageId }).catch((e) =>
            toast.error("打开失败", { description: String(e) }),
          );
        },
      });
    }
    return items;
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
            placeholder="筛选游戏（支持中文名）"
            className="pl-8"
          />
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            clearDeepResult();
            toast.success("已清理未安装游戏的残留记录");
          }}
          disabled={deepLeftover.length === 0}
          title={
            deepLeftover.length > 0
              ? `清除 ${deepLeftover.length} 个未安装游戏的残留存档记录（不会删除存档文件）`
              : "当前没有未安装游戏的残留记录（先执行深度扫描）"
          }
        >
          <Eraser className="mr-1.5 size-3.5" />
          清理未安装游戏{deepLeftover.length > 0 && `（${deepLeftover.length}）`}
        </Button>
        <Button size="sm" variant="outline" onClick={() => setCustomOpen(true)}>
          <FolderPlus className="mr-1.5 size-3.5" />
          自定义游戏
        </Button>
      </div>

      {merged.length === 0 && !scanning && (
        <Card className="flex h-40 flex-col items-center justify-center gap-3 text-muted-foreground">
          <ScanSearch className="size-8" />
          <p>点击右下角的「扫描本机」按钮，找到你电脑上所有带存档的游戏</p>
        </Card>
      )}
      {scanning && merged.length === 0 && (
        <Card className="flex h-40 items-center justify-center gap-3 text-muted-foreground">
          <Loader2 className="size-5 animate-spin" /> 正在扫描存档路径并统计大小……
        </Card>
      )}

      {merged.length > 0 && (
        <Card className="overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-10">
                  <Checkbox
                    checked={allSelected}
                    onCheckedChange={(v) => {
                      if (v) setSelected(new Set(games.map((g) => g.entry.wikiPageId)));
                      else setSelected(new Set());
                    }}
                  />
                </TableHead>
                <TableHead className="w-[28%]">游戏</TableHead>
                <TableHead>已安装平台</TableHead>
                <TableHead>存档大小</TableHead>
                <TableHead>最近修改</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {games.map((scanned) => {
                return (
                  <TableRow
                    key={scanned.entry.wikiPageId}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      setRowMenu({ x: e.clientX, y: e.clientY, game: scanned });
                    }}
                  >
                    <TableCell onClick={(e) => e.stopPropagation()}>
                      <Checkbox
                        checked={selected.has(scanned.entry.wikiPageId)}
                        onCheckedChange={(v) => toggleSelect(scanned.entry.wikiPageId, v === true)}
                      />
                    </TableCell>
                    <TableCell>
                      <div className="font-medium">{scanned.entry.title}</div>
                      {scanned.entry.zhCn && (
                        <div className="text-xs text-muted-foreground">{scanned.entry.zhCn}</div>
                      )}
                    </TableCell>
                    <TableCell>
                      <PlatformBadges platforms={scanned.installedPlatforms} />
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {formatBytes(scanned.backupSize)}
                    </TableCell>
                    <TableCell className="text-xs">
                      {scanned.latestModifiedMs
                        ? new Date(scanned.latestModifiedMs).toLocaleString("zh-CN", {
                            dateStyle: "short",
                            timeStyle: "short",
                          })
                        : "—"}
                    </TableCell>
                    <TableCell className="space-x-1 text-right">
                      <Button
                        size="sm"
                        onClick={() => doBackup.mutate(scanned.entry.wikiPageId)}
                        disabled={doBackup.isPending}
                      >
                        {doBackup.isPending && doBackup.variables === scanned.entry.wikiPageId ? (
                          <Loader2 className="mr-1 size-3.5 animate-spin" />
                        ) : (
                          <Archive className="mr-1 size-3.5" />
                        )}
                        备份
                      </Button>
                    </TableCell>
                  </TableRow>
                );
              })}
              {games.length === 0 && (
                <TableRow>
                  <TableCell colSpan={6} className="h-24 text-center text-muted-foreground">
                    没有匹配的游戏
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
      )}

      {/* 列表底部统计：上次扫描时间 + 存档统计 */}
      {merged.length > 0 && !scanning && (
        <div className="flex items-center justify-center gap-2 pb-1 text-xs text-muted-foreground">
          {scannedAt && (
            <span>上次扫描：{new Date(scannedAt).toLocaleString("zh-CN")}</span>
          )}
          {scannedAt && <span>·</span>}
          <span>
            {games.length} 个游戏有存档
            {leftoverCount > 0 && `（含 ${leftoverCount} 个未安装残留）`}
          </span>
        </div>
      )}

      <CustomGamesDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        onSaved={() => runScan()}
      />

      {/* 行右键菜单 */}
      {rowMenu && (
        <RowMenu
          x={rowMenu.x}
          y={rowMenu.y}
          items={buildRowMenuItems(rowMenu.game)}
          onClose={() => setRowMenu(null)}
        />
      )}

      {/* 右键"恢复最新"的冲突确认 */}
      <AlertDialog open={conflict != null} onOpenChange={(open) => !open && setConflict(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {conflict ? `${conflict.title}：检测到本地存档比备份更新` : ""}
            </AlertDialogTitle>
            <AlertDialogDescription>
              本地存档时间：{conflict ? formatMs(conflict.info.localMs) : ""}
              <br />
              备份存档时间：{conflict ? formatMs(conflict.info.backupMs) : ""}
              <br />
              <br />
              确定要用备份覆盖本地较新的存档吗？
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (conflict) restoreLatest.mutate({ id: conflict.id, date: conflict.date, force: true });
                setConflict(null);
              }}
            >
              覆盖恢复
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 悬浮批量操作栏：选中游戏后出现在底部中央 */}
      {selected.size > 0 && (
        <div className="fixed bottom-8 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-full border bg-background/95 px-5 py-2.5 shadow-xl backdrop-blur">
          <span className="text-sm font-medium">已选 {selected.size} 个</span>
          <Button
            size="sm"
            onClick={() => batchBackupBulk.mutate([...selected])}
            disabled={batchBackupBulk.isPending}
          >
            {batchBackupBulk.isPending ? (
              <Loader2 className="mr-1.5 size-3.5 animate-spin" />
            ) : (
              <Archive className="mr-1.5 size-3.5" />
            )}
            批量备份
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            取消选择
          </Button>
        </div>
      )}

      {/* 悬空扫描按钮组：右下角横向排布，文字标签一眼可辨 */}
      <div className="fixed bottom-8 right-8 z-50 flex items-center gap-3">
        <button
          onClick={() => deepScan.mutate()}
          disabled={scanning || deepScan.isPending}
          title="深度扫描：解析全部 5.2 万个游戏的存档路径（约 1-3 分钟），可找回已卸载游戏的残留存档"
          className="flex h-11 items-center gap-2 rounded-full border bg-background px-4 text-sm font-medium shadow-xl shadow-black/25 transition-transform hover:scale-105 active:scale-95 disabled:opacity-70"
        >
          {deepScan.isPending ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <Telescope className="size-4" />
          )}
          {deepScan.isPending ? "扫描中…" : "深度扫描"}
        </button>
        <button
          onClick={() => runScan()}
          disabled={scanning}
          title={scanning ? "扫描中……" : "扫描本机已安装游戏"}
          className="flex h-11 items-center gap-2 rounded-full bg-primary px-5 text-sm font-semibold text-primary-foreground shadow-xl shadow-black/25 transition-transform hover:scale-105 active:scale-95 disabled:opacity-70"
        >
          {scanning ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <ScanSearch className="size-5" />
          )}
          {scanning ? "扫描中…" : "扫描本机"}
        </button>
      </div>
    </div>
  );
}

function formatMs(ms: number | null): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString("zh-CN", { dateStyle: "short", timeStyle: "short" });
}
