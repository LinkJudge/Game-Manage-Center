import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import {
  Archive,
  FolderOpen,
  Loader2,
  Pin,
  PinOff,
  RotateCcw,
  Search,
  Settings2,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import type { BatchResult, ConflictInfo, GameBackups } from "@/types";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function formatBackupDate(date: string): string {
  const [day, time] = date.split("_");
  return `${day.replace(/-/g, "/")}${time ? ` ${time.replace(/-/g, ":")}` : ""}`;
}

function formatMs(ms: number | null): string {
  if (!ms) return "—";
  return new Date(ms).toLocaleString("zh-CN", { dateStyle: "short", timeStyle: "short" });
}

// ---------------------------------------------------------------- 恢复标签页（与备份页同风格）

export function RestoreTab() {
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [rowMenu, setRowMenu] = useState<{ x: number; y: number; game: GameBackups } | null>(null);
  const [manageGameId, setManageGameId] = useState<number | null>(null);
  const [deleteAsk, setDeleteAsk] = useState<{ id: number; title: string; date: string } | null>(null);
  // "恢复最新"或记录管理里的恢复，冲突时弹出确认
  const [conflict, setConflict] = useState<{
    id: number;
    title: string;
    date: string;
    info: ConflictInfo;
  } | null>(null);

  const allBackups = useQuery({
    queryKey: ["all-backups"],
    queryFn: () => invoke<GameBackups[]>("list_all_backups"),
    staleTime: 30_000,
  });

  const games = allBackups.data ?? [];
  const filtered = games.filter((g) => {
    if (!filter.trim()) return true;
    const q = filter.trim().toLowerCase();
    return g.title.toLowerCase().includes(q) || (g.zhCn ?? "").includes(filter.trim());
  });
  const allSelected = filtered.length > 0 && filtered.every((g) => selected.has(g.wikiPageId));
  // 备份记录对话框实时派生：删除后随查询刷新，全删完自动关闭
  const managedGame =
    manageGameId != null ? (games.find((g) => g.wikiPageId === manageGameId) ?? null) : null;

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["all-backups"] });
    // 计数刷新让备份页右键的"打开备份文件夹"同步消失/出现
    queryClient.invalidateQueries({ queryKey: ["backup-counts"] });
  };

  const toggleSelect = (id: number, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  // 恢复指定游戏的指定备份（force=true 时无视本地较新的冲突）
  const restore = useMutation({
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
      invalidate();
    },
    onError: (e) => {
      if (String(e).includes("CONFLICT")) return;
      toast.error("恢复失败", { description: String(e) });
    },
  });

  const startRestore = (game: GameBackups, date: string) => {
    invoke<ConflictInfo>("check_restore_conflict", { wikiPageId: game.wikiPageId, date })
      .then((info) => {
        if (info.conflict) {
          setConflict({ id: game.wikiPageId, title: game.title, date, info });
        } else {
          restore.mutate({ id: game.wikiPageId, date, force: false });
        }
      })
      .catch((e) => toast.error("检查冲突失败", { description: String(e) }));
  };

  const doDelete = useMutation({
    mutationFn: (args: { id: number; date: string }) =>
      invoke<void>("delete_backup", { wikiPageId: args.id, date: args.date }),
    onSuccess: () => {
      toast.success("备份已删除");
      invalidate();
    },
    onError: (e) => toast.error("删除失败", { description: String(e) }),
  });

  const doPin = useMutation({
    mutationFn: (args: { id: number; date: string; permanent: boolean }) =>
      invoke<void>("set_backup_permanent", {
        wikiPageId: args.id,
        date: args.date,
        permanent: args.permanent,
      }),
    onSuccess: () => {
      toast.success("已更新固定状态");
      invalidate();
    },
    onError: (e) => toast.error("操作失败", { description: String(e) }),
  });

  // 批量恢复：每个游戏恢复其最新备份；本地较新的自动跳过
  const batchRestore = useMutation({
    mutationFn: (ids: number[]) => invoke<BatchResult[]>("batch_restore_latest", { ids }),
    onSuccess: (results) => {
      const ok = results.filter((r) => r.ok);
      const failed = results.filter((r) => !r.ok);
      if (failed.length === 0) {
        toast.success(`批量恢复完成（${ok.length} 个游戏）`, {
          description: ok.map((r) => `${r.title}：${r.message}`).join("\n"),
        });
      } else {
        toast.warning(`批量恢复：${ok.length} 成功，${failed.length} 跳过/失败`, {
          description: failed.map((r) => `${r.title}：${r.message}`).join("\n"),
        });
      }
      setSelected(new Set());
      invalidate();
    },
    onError: (e) => toast.error("批量恢复失败", { description: String(e) }),
  });

  // 取最新备份日期（空数组防护：后端契约保证非空，但坏了不该让界面直接崩）
  const startRestoreLatest = (game: GameBackups) => {
    const latest = game.backups[0];
    if (!latest) {
      toast.info(`${game.title} 还没有备份`);
      return;
    }
    startRestore(game, latest.date);
  };

  const buildRowMenuItems = (game: GameBackups): RowMenuItem[] => {
    const items: RowMenuItem[] = [
      {
        label: "恢复最新备份",
        icon: RotateCcw,
        onSelect: () => startRestoreLatest(game),
      },
      { label: "管理备份记录", icon: Settings2, onSelect: () => setManageGameId(game.wikiPageId) },
    ];
    items.push({
      label: "打开备份文件夹",
      icon: FolderOpen,
      onSelect: () => {
        invoke("open_game_backup_folder", { wikiPageId: game.wikiPageId }).catch((e) =>
          toast.error("打开失败", { description: String(e) }),
        );
      },
    });
    return items;
  };

  if (allBackups.isLoading) {
    return (
      <div className="flex h-40 items-center justify-center text-muted-foreground">
        <Loader2 className="mr-2 size-4 animate-spin" /> 加载备份记录……
      </div>
    );
  }

  if (games.length === 0) {
    return (
      <Card className="flex h-40 flex-col items-center justify-center gap-3 text-muted-foreground">
        <RotateCcw className="size-8" />
        <p>还没有任何备份。去「备份」标签页扫描并备份你的游戏。</p>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      <div className="relative max-w-md">
        <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
        <Input
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="筛选游戏（支持中文名）"
          className="pl-8"
        />
      </div>

      <Card className="overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-10">
                <Checkbox
                  checked={allSelected}
                  onCheckedChange={(v) => {
                    if (v) setSelected(new Set(filtered.map((g) => g.wikiPageId)));
                    else setSelected(new Set());
                  }}
                />
              </TableHead>
              <TableHead className="w-[36%]">游戏</TableHead>
              <TableHead>备份数</TableHead>
              <TableHead>最新备份</TableHead>
              <TableHead>大小</TableHead>
              <TableHead className="text-right">操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.map((game) => (
              <TableRow
                key={game.wikiPageId}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setRowMenu({ x: e.clientX, y: e.clientY, game });
                }}
              >
                <TableCell onClick={(e) => e.stopPropagation()}>
                  <Checkbox
                    checked={selected.has(game.wikiPageId)}
                    onCheckedChange={(v) => toggleSelect(game.wikiPageId, v === true)}
                  />
                </TableCell>
                <TableCell>
                  <div className="font-medium">{game.title}</div>
                  {game.zhCn && (
                    <div className="text-xs text-muted-foreground">{game.zhCn}</div>
                  )}
                </TableCell>
                <TableCell className="font-mono text-xs">{game.backups.length}</TableCell>
                <TableCell className="font-mono text-xs">{game.latestBackup}</TableCell>
                <TableCell className="font-mono text-xs">
                  {formatBytes(game.backupSize ?? 0)}
                </TableCell>
                <TableCell className="space-x-1 text-right">
                  <Button
                    size="sm"
                    onClick={() => startRestoreLatest(game)}
                    disabled={restore.isPending}
                  >
                    {restore.isPending && restore.variables?.id === game.wikiPageId ? (
                      <Loader2 className="mr-1 size-3.5 animate-spin" />
                    ) : (
                      <RotateCcw className="mr-1 size-3.5" />
                    )}
                    恢复
                  </Button>
                  <Button size="sm" variant="outline" onClick={() => setManageGameId(game.wikiPageId)}>
                    <Archive className="mr-1 size-3.5" />
                    记录
                  </Button>
                </TableCell>
              </TableRow>
            ))}
            {filtered.length === 0 && (
              <TableRow>
                <TableCell colSpan={6} className="h-24 text-center text-muted-foreground">
                  没有匹配的游戏
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Card>

      {/* 底部统计 */}
      {games.length > 0 && (
        <div className="flex items-center justify-center pb-1 text-xs text-muted-foreground">
          {games.length} 个游戏有备份
        </div>
      )}

      {/* 备份记录管理：单游戏逐份恢复 / 固定 / 删除 */}
      <Dialog open={managedGame != null} onOpenChange={(open) => !open && setManageGameId(null)}>
        <DialogContent className="sm:max-w-[760px]">
          <DialogHeader>
            <DialogTitle>备份记录：{managedGame?.title ?? ""}</DialogTitle>
            <DialogDescription>
              恢复、固定或删除该游戏的每一份备份。固定（Pin）的备份不会被轮换清理。
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[55vh] overflow-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[150px]">备份时间</TableHead>
                  <TableHead className="w-[80px]">大小</TableHead>
                  <TableHead className="w-[70px]">状态</TableHead>
                  <TableHead className="w-[130px] text-right">操作</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {(managedGame?.backups ?? []).map((b) => {
                  return (
                  <TableRow key={b.date} className="transition-colors">
                    <TableCell>
                      <div className="font-mono text-xs">{formatBackupDate(b.date)}</div>
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {formatBytes(b.backupSize ?? 0)}
                    </TableCell>
                    <TableCell>
                      {b.isPermanent ? (
                        <Badge className="gap-1 text-[10px]">
                          <Pin className="size-3" />
                          固定
                        </Badge>
                      ) : (
                        <Badge variant="secondary" className="text-[10px]">
                          滚动
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          size="icon"
                          variant="outline"
                          className="size-8"
                          title="恢复此备份"
                          onClick={() => managedGame && startRestore(managedGame, b.date)}
                        >
                          <RotateCcw className="size-3.5" />
                        </Button>
                        <Button
                          size="icon"
                          variant="outline"
                          className="size-8"
                          title={b.isPermanent ? "取消固定" : "永久固定（不被轮换清理）"}
                          onClick={() =>
                            managedGame &&
                            doPin.mutate({
                              id: managedGame.wikiPageId,
                              date: b.date,
                              permanent: !b.isPermanent,
                            })
                          }
                        >
                          {b.isPermanent ? <PinOff className="size-3.5" /> : <Pin className="size-3.5" />}
                        </Button>
                        <Button
                          size="icon"
                          variant="outline"
                          className="size-8 text-destructive hover:bg-destructive/10"
                          title="删除此备份（不可恢复）"
                          onClick={() =>
                            setDeleteAsk(
                              managedGame && { id: managedGame.wikiPageId, title: managedGame.title, date: b.date },
                            )
                          }
                        >
                          <Trash2 className="size-3.5" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </div>
        </DialogContent>
      </Dialog>

      {/* 删除确认 */}
      <AlertDialog open={deleteAsk != null} onOpenChange={(open) => !open && setDeleteAsk(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除这份备份？</AlertDialogTitle>
            <AlertDialogDescription>
              {deleteAsk
                ? `${deleteAsk.title} · ${formatBackupDate(deleteAsk.date)}，删除后无法恢复。`
                : ""}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() => {
                if (deleteAsk) doDelete.mutate({ id: deleteAsk.id, date: deleteAsk.date });
                setDeleteAsk(null);
              }}
            >
              删除
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
            onClick={() => batchRestore.mutate([...selected])}
            disabled={batchRestore.isPending}
          >
            {batchRestore.isPending ? (
              <Loader2 className="mr-1.5 size-3.5 animate-spin" />
            ) : (
              <RotateCcw className="mr-1.5 size-3.5" />
            )}
            批量恢复
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            取消选择
          </Button>
        </div>
      )}

      {/* 行右键菜单 */}
      {rowMenu && (
        <RowMenu
          x={rowMenu.x}
          y={rowMenu.y}
          items={buildRowMenuItems(rowMenu.game)}
          onClose={() => setRowMenu(null)}
        />
      )}

      {/* 冲突确认 */}
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
                if (conflict) restore.mutate({ id: conflict.id, date: conflict.date, force: true });
                setConflict(null);
              }}
            >
              覆盖恢复
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
