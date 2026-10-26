import { useCallback, useEffect, useRef, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import {
  Download,
  FolderOpen,
  Gamepad2,
  Loader2,
  PackageCheck,
  Play,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { RowMenu, type RowMenuItem } from "@/components/rowMenu";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useScanStore, useTrainerStore } from "@/store";
import type {
  GameTrainerMatch,
  InstalledTrainer,
  TrainerGameQuery,
  TrainerHit,
  TrainerProgress,
  TrainerSourceSummary,
  TrainerUpdateInfo,
} from "@/types";

function formatBytes(bytes: number | null): string {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString("zh-CN");
}

// 来源显示名（新增来源时在此补一行；UI 层预留，来源自身的 title 是英文名）
const SOURCE_LABELS: Record<string, string> = {
  fling: "风灵月影",
};

function SourceBadge({ sourceId }: { sourceId: string }) {
  const label = SOURCE_LABELS[sourceId] ?? sourceId;
  const color = sourceId === "fling" ? "#e0684b" : "#8f96a3";
  return (
    <Badge
      variant="outline"
      className="text-[10px] px-1.5 py-0 font-medium"
      style={{ color, backgroundColor: `${color}1f`, borderColor: `${color}45` }}
    >
      {label}
    </Badge>
  );
}

// ---------------------------------------------------------------- 行模型与操作单元格

/**
 * 结果表的统一行：搜索/匹配视图里 hit 是目录条目、installedOf 是该条目当前
 * 装着的最新包；「只看已安装」视图里 hit 为空，信息全部来自 manifest。
 */
interface ResultRow {
  key: string;
  hit: TrainerHit | null;
  /** 副行文字：匹配视图是"来自 <游戏标题>"，搜索视图是中文名时的英文名 */
  subline?: string;
  installedOf?: InstalledTrainer;
  update?: TrainerUpdateInfo;
}

function rowName(row: ResultRow): string {
  if (row.hit) return row.hit.zhName ?? row.hit.gameName;
  const m = row.installedOf?.manifest;
  return m ? (m.zhName ?? m.gameName) : "—";
}

function rowSub(row: ResultRow): string {
  if (row.subline) return row.subline;
  const m = row.installedOf?.manifest;
  if (row.hit && row.hit.zhName) return row.hit.gameName;
  if (m?.zhName) return m.gameName;
  return "";
}

/** 行内下载/解压进度（download → extract → done 事件驱动） */
function RowProgress({ progress }: { progress: TrainerProgress }) {
  const pct = progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : 0;
  if (progress.stage === "extract") {
    return (
      <div className="flex items-center justify-end gap-1.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> 解压安装中…
      </div>
    );
  }
  // 连接/重定向阶段：还没有字节进度
  if (progress.total === 0 && progress.downloaded === 0) {
    return (
      <div className="flex items-center justify-end gap-1.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> 连接中…
      </div>
    );
  }
  return (
    <div className="ml-auto w-32 text-right">
      <Progress value={pct} className="h-1.5" />
      <div className="mt-0.5 text-[10px] text-muted-foreground">
        {pct}% · {formatBytes(progress.downloaded)}
      </div>
    </div>
  );
}

/** 操作列：未装 → 下载；已装 → 启动/目录/更新/删除；下载中 → 进度 */
function ActionsCell({
  row,
  progress,
  onDownload,
  onUpdate,
  onLaunch,
  onOpenFolder,
  onDeleteRequest,
}: {
  row: ResultRow;
  progress?: TrainerProgress;
  onDownload: (hit: TrainerHit) => void;
  onUpdate: (u: TrainerUpdateInfo) => void;
  onLaunch: (t: InstalledTrainer) => void;
  onOpenFolder: (t: InstalledTrainer) => void;
  onDeleteRequest: (t: InstalledTrainer) => void;
}) {
  if (progress) return <RowProgress progress={progress} />;

  if (row.installedOf) {
    const t = row.installedOf;
    return (
      <div className="space-x-1 text-right">
        <Button size="sm" onClick={() => onLaunch(t)} title="启动修改器">
          <Play className="mr-1 size-3.5" />
          启动
        </Button>
        <Button size="sm" variant="outline" onClick={() => onOpenFolder(t)} title="打开修改器所在目录">
          <FolderOpen className="size-3.5" />
        </Button>
        {row.update && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => onUpdate(row.update!)}
            title={`更新到 ${row.update.newDownload.gameVersion ?? "最新包"}，旧包自动清除`}
          >
            <RefreshCw className="mr-1 size-3.5" />
            更新
          </Button>
        )}
        <Button
          size="sm"
          variant="outline"
          className="text-destructive hover:text-destructive"
          onClick={() => onDeleteRequest(t)}
          title="删除这个修改器包"
        >
          <Trash2 className="size-3.5" />
        </Button>
      </div>
    );
  }

  const hit = row.hit;
  return (
    <Button
      size="sm"
      disabled={!hit?.downloadUrl}
      onClick={() => hit && onDownload(hit)}
      title={hit && hit.totalDownloads > 1 ? `共 ${hit.totalDownloads} 个包，先装最新` : undefined}
    >
      <Download className="mr-1 size-3.5" />
      下载
    </Button>
  );
}

// ---------------------------------------------------------------- 修改器标签页

export function TrainersTab() {
  const scanOutput = useScanStore((s) => s.scanOutput);
  // 搜索词持久化：切到别的标签页再回来不丢
  const query = useTrainerStore((s) => s.trainerQuery);
  const setQuery = useTrainerStore((s) => s.setTrainerQuery);
  const [hits, setHits] = useState<TrainerHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [sources, setSources] = useState<TrainerSourceSummary[]>([]);
  const [installed, setInstalled] = useState<InstalledTrainer[]>([]);
  const [updates, setUpdates] = useState<TrainerUpdateInfo[]>([]);
  const [progress, setProgress] = useState<Record<string, TrainerProgress>>({});
  const [pendingDelete, setPendingDelete] = useState<InstalledTrainer | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [batchRunning, setBatchRunning] = useState(false);
  const [batchDeleteAsk, setBatchDeleteAsk] = useState(false);
  const [onlyInstalled, setOnlyInstalled] = useState(false);
  // 行右键菜单（与存档/恢复页同款浮层）
  const [rowMenu, setRowMenu] = useState<{ x: number; y: number; row: ResultRow } | null>(null);

  const refreshInstalled = useCallback(() => {
    invoke<InstalledTrainer[]>("list_installed_trainers")
      .then(setInstalled)
      .catch(() => {});
    invoke<TrainerUpdateInfo[]>("check_trainer_updates")
      .then(setUpdates)
      .catch(() => {});
  }, []);

  useEffect(() => {
    refreshInstalled();
    invoke<TrainerSourceSummary[]>("list_trainer_sources").then(setSources).catch(() => {});
  }, [refreshInstalled]);

  // 搜索（300ms 防抖；清空输入即回空；切回标签页时恢复的词会自动重搜）
  // seq 守卫：已发出的旧查询响应晚到时直接丢弃，不覆盖新词的结果
  const searchSeq = useRef(0);
  useEffect(() => {
    const q = query.trim();
    setSelected(new Set()); // 换一批结果，选择作废
    if (!q) {
      setHits([]);
      setSearching(false);
      return;
    }
    setOnlyInstalled(false); // 搜索优先于「只看已安装」
    setSearching(true);
    const t = setTimeout(() => {
      const seq = ++searchSeq.current;
      invoke<TrainerHit[]>("search_trainers", { query: q, limit: 60 })
        .then((hits) => {
          if (seq === searchSeq.current) setHits(hits);
        })
        .catch((e) => {
          if (seq === searchSeq.current) toast.error("搜索失败", { description: String(e) });
        })
        .finally(() => {
          if (seq === searchSeq.current) setSearching(false);
        });
    }, 300);
    return () => clearTimeout(t);
  }, [query]);

  // 下载进度事件（Rust 侧流式回传）
  useEffect(() => {
    const un = listen<TrainerProgress>("trainer-download-progress", (e) => {
      const p = e.payload;
      if (p.stage === "done") {
        setProgress((prev) => {
          const next = { ...prev };
          delete next[p.trainerId];
          return next;
        });
        refreshInstalled();
      } else {
        setProgress((prev) => ({ ...prev, [p.trainerId]: p }));
      }
    });
    return () => {
      un.then((fn) => fn());
    };
  }, [refreshInstalled]);

  const startDownload = (hit: TrainerHit) => {
    if (!hit.downloadUrl) return;
    // 立即把按钮换成进度条（连接/重定向阶段后端还没有字节进度）
    setProgress((prev) => ({
      ...prev,
      [hit.id]: { trainerId: hit.id, stage: "download", downloaded: 0, total: 0 },
    }));
    invoke<InstalledTrainer>("download_trainer", {
      trainerId: hit.id,
      downloadUrl: hit.downloadUrl,
    })
      .then((t) => {
        toast.success("修改器已安装", { description: t.manifest.gameName });
        refreshInstalled();
      })
      .catch((e) => {
        toast.error("下载失败", { description: String(e) });
        setProgress((prev) => {
          const next = { ...prev };
          delete next[hit.id];
          return next;
        });
      });
  };

  const startUpdate = (u: TrainerUpdateInfo) => {
    setProgress((prev) => ({
      ...prev,
      [u.trainerId]: { trainerId: u.trainerId, stage: "download", downloaded: 0, total: 0 },
    }));
    invoke<InstalledTrainer>("update_trainer", {
      trainerId: u.trainerId,
      downloadUrl: u.newDownload.url,
      oldFolder: u.oldFolder,
    })
      .then(() => {
        toast.success("已更新到最新包", { description: u.zhName ?? u.gameName });
        refreshInstalled();
      })
      .catch((e) => {
        toast.error("更新失败", { description: String(e) });
        setProgress((prev) => {
          const next = { ...prev };
          delete next[u.trainerId];
          return next;
        });
      });
  };

  const launch = (t: InstalledTrainer) => {
    invoke("launch_installed_trainer", { folder: t.folder }).catch((e) =>
      toast.error("启动失败", { description: String(e) }),
    );
  };

  const openFolder = (t: InstalledTrainer) => {
    invoke("open_save_folder", { path: t.folder }).catch((e) =>
      toast.error("打开失败", { description: String(e) }),
    );
  };

  const confirmDelete = () => {
    const t = pendingDelete;
    setPendingDelete(null);
    if (!t) return;
    invoke("delete_installed_trainer", { folder: t.folder })
      .then(() => {
        toast.success("已删除", { description: t.manifest.gameName });
        refreshInstalled();
      })
      .catch((e) => toast.error("删除失败", { description: String(e) }));
  };

  const toggleSelect = (key: string, checked: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
  };

  // 每个条目当前装着的最新包（同名多包时 scan 已按安装时间降序，取首个）
  const findInstalled = (trainerId: string): InstalledTrainer | undefined =>
    installed.find((t) => t.manifest.trainerId === trainerId);
  const updateOf = (folder: string): TrainerUpdateInfo | undefined =>
    updates.find((u) => u.oldFolder === folder);

  // 行右键菜单项：与操作列按钮同一套动作，按行的状态裁剪（无可动作的行不弹菜单）
  const buildRowMenuItems = (row: ResultRow): RowMenuItem[] => {
    const items: RowMenuItem[] = [];
    if (row.installedOf) {
      const t = row.installedOf;
      items.push({ label: "启动修改器", icon: Play, onSelect: () => launch(t) });
      if (row.update) {
        const u = row.update;
        items.push({ label: "更新到最新包", icon: RefreshCw, onSelect: () => startUpdate(u) });
      }
      items.push({ label: "打开所在目录", icon: FolderOpen, onSelect: () => openFolder(t) });
      items.push({
        label: "删除此修改器",
        icon: Trash2,
        danger: true,
        onSelect: () => setPendingDelete(t),
      });
    } else if (row.hit?.downloadUrl) {
      const hit = row.hit;
      items.push({ label: "下载安装", icon: Download, onSelect: () => startDownload(hit) });
    }
    return items;
  };

  // 默认视图：已安装游戏 ↔ 修改器目录实时匹配（随扫描结果自动刷新）
  const scannedGames: TrainerGameQuery[] = (scanOutput?.games ?? []).map((g) => ({
    title: g.entry.title,
    zhName: g.entry.zhCn,
  }));
  const matchQuery = useQuery({
    queryKey: ["trainer-matches", scannedGames],
    queryFn: () => invoke<GameTrainerMatch[]>("match_trainers", { games: scannedGames }),
    enabled: scannedGames.length > 0,
    // 重新扫描后键值变化，先留住上一批结果，避免表格闪空
    placeholderData: keepPreviousData,
  });
  const matchRows: ResultRow[] = (matchQuery.data ?? [])
    .filter((m) => m.matches.length > 0)
    .flatMap((m) =>
      m.matches.map((hit) => {
        const installedOf = findInstalled(hit.id);
        return {
          key: `${m.title}-${hit.id}`,
          hit,
          subline:
            m.title !== hit.gameName ? `${hit.gameName} · 来自 ${m.title}` : hit.gameName,
          installedOf,
          update: installedOf ? updateOf(installedOf.folder) : undefined,
        };
      }),
    );

  const toggleOnlyInstalled = () => {
    const next = !onlyInstalled;
    setOnlyInstalled(next);
    setSelected(new Set());
    if (next) setQuery("");
  };

  // 视图行：搜索词 > 只看已安装 > 默认匹配视图
  const searching_ = query.trim().length > 0;
  let rows: ResultRow[] = [];
  if (searching_) {
    rows = hits.map((hit) => {
      const installedOf = findInstalled(hit.id);
      return {
        key: hit.id,
        hit,
        subline: hit.zhName ? hit.gameName : undefined,
        installedOf,
        update: installedOf ? updateOf(installedOf.folder) : undefined,
      };
    });
  } else if (onlyInstalled) {
    rows = installed.map((t) => ({
      key: t.folder,
      hit: null,
      installedOf: t,
      update: updateOf(t.folder),
    }));
  } else {
    rows = matchRows;
  }

  // 扫描过（或有搜索）就显示表格；从未扫描才显示引导卡片
  const showTable = searching_ || searching || onlyInstalled || scannedGames.length > 0;
  // 默认匹配视图的请求进行中（加载中行只在对应视图下出现，不与空状态叠加）
  const matchPending =
    !searching_ && !onlyInstalled && (matchQuery.isLoading || matchQuery.isFetching);

  // 多选：批量下载未安装的、批量删除已安装的（顺序执行，单条失败不中断）
  const selectedRows = rows.filter((r) => selected.has(r.key));
  const downloadableCount = selectedRows.filter((r) => r.hit?.downloadUrl && !r.installedOf).length;
  const deletableCount = selectedRows.filter((r) => r.installedOf).length;
  const allSelected = rows.length > 0 && rows.every((r) => selected.has(r.key));

  const batchDownload = async () => {
    const targets = selectedRows.filter((r) => r.hit?.downloadUrl && !r.installedOf);
    if (!targets.length) return;
    setBatchRunning(true);
    let ok = 0;
    const failed: string[] = [];
    for (const r of targets) {
      try {
        await invoke("download_trainer", {
          trainerId: r.hit!.id,
          downloadUrl: r.hit!.downloadUrl,
        });
        ok++;
      } catch (e) {
        failed.push(`${r.hit!.gameName}：${String(e)}`);
      }
    }
    setBatchRunning(false);
    setSelected(new Set());
    refreshInstalled();
    if (failed.length) {
      toast.warning(`批量下载：${ok} 成功，${failed.length} 失败`, {
        description: failed.join("\n"),
      });
    } else {
      toast.success(`批量下载完成（${ok} 个修改器）`);
    }
  };

  const batchDelete = async () => {
    const targets = selectedRows.filter((r) => r.installedOf);
    setBatchDeleteAsk(false);
    if (!targets.length) return;
    let ok = 0;
    const failed: string[] = [];
    for (const r of targets) {
      try {
        await invoke("delete_installed_trainer", { folder: r.installedOf!.folder });
        ok++;
      } catch (e) {
        failed.push(`${r.installedOf!.manifest.gameName}：${String(e)}`);
      }
    }
    setSelected(new Set());
    refreshInstalled();
    if (failed.length) {
      toast.warning(`批量删除：${ok} 成功，${failed.length} 失败`, {
        description: failed.join("\n"),
      });
    } else {
      toast.success(`已删除 ${ok} 个修改器`);
    }
  };

  return (
    <div className={selected.size > 0 ? "space-y-4 pb-20" : "space-y-4"}>
      {/* 搜索工具行 */}
      <div className="flex items-center gap-2">
        <div className="relative max-w-md flex-1">
          <Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索修改器（游戏名支持中英文）"
            className="pl-8 pr-8"
          />
          {query && (
            <button
              onClick={() => setQuery("")}
              className="absolute right-2 top-2.5 text-muted-foreground hover:text-foreground"
            >
              <X className="size-4" />
            </button>
          )}
        </div>
        <Button
          size="sm"
          variant={onlyInstalled ? "default" : "outline"}
          onClick={toggleOnlyInstalled}
          title="只显示已安装的修改器"
        >
          <PackageCheck className="mr-1.5 size-3.5" />
          只看已安装{installed.length > 0 ? `（${installed.length}）` : ""}
        </Button>
      </div>

      {showTable ? (
        <Card className="overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-10">
                  <Checkbox
                    checked={rows.length > 0 && allSelected}
                    onCheckedChange={(v) => {
                      if (v) setSelected(new Set(rows.map((r) => r.key)));
                      else setSelected(new Set());
                    }}
                  />
                </TableHead>
                <TableHead className="w-[24%]">游戏</TableHead>
                <TableHead>来源</TableHead>
                <TableHead>选项</TableHead>
                <TableHead>游戏版本</TableHead>
                <TableHead>大小</TableHead>
                <TableHead>更新日期</TableHead>
                <TableHead className="w-16">状态</TableHead>
                <TableHead>安装时间</TableHead>
                <TableHead className="text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => {
                const manifest = row.installedOf?.manifest;
                return (
                  <TableRow
                    key={row.key}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      if (buildRowMenuItems(row).length > 0) {
                        setRowMenu({ x: e.clientX, y: e.clientY, row });
                      }
                    }}
                  >
                    <TableCell onClick={(e) => e.stopPropagation()}>
                      <Checkbox
                        checked={selected.has(row.key)}
                        onCheckedChange={(v) => toggleSelect(row.key, v === true)}
                      />
                    </TableCell>
                    <TableCell>
                      <div className="font-medium">{rowName(row)}</div>
                      {rowSub(row) && (
                        <div className="text-xs text-muted-foreground">{rowSub(row)}</div>
                      )}
                    </TableCell>
                    <TableCell>
                      <SourceBadge sourceId={row.hit?.sourceId ?? manifest?.sourceId ?? ""} />
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {row.hit?.optionsCount ?? manifest?.optionsCount ?? "—"}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {row.hit?.gameVersion ?? manifest?.gameVersion ?? "—"}
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {formatBytes(row.hit?.sizeBytes ?? manifest?.sizeBytes ?? null)}
                    </TableCell>
                    <TableCell className="text-xs">
                      {row.hit?.updatedAt ?? manifest?.dateAdded ?? "—"}
                    </TableCell>
                    <TableCell>
                      {row.installedOf && (
                        <Badge variant="secondary" className="text-[10px] px-1.5 py-0">
                          已安装
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-xs">
                      {manifest ? formatDate(manifest.installedAt) : "—"}
                    </TableCell>
                    <TableCell className="text-right">
                      <ActionsCell
                        row={row}
                        progress={
                          progress[(row.hit?.id ?? manifest?.trainerId) ?? ""]
                        }
                        onDownload={startDownload}
                        onUpdate={startUpdate}
                        onLaunch={launch}
                        onOpenFolder={openFolder}
                        onDeleteRequest={setPendingDelete}
                      />
                    </TableCell>
                  </TableRow>
                );
              })}
              {rows.length === 0 && !searching && !matchPending && (
                <TableRow>
                  <TableCell colSpan={10} className="h-20 text-center text-muted-foreground">
                    {onlyInstalled
                      ? "还没有已安装的修改器"
                      : searching_
                        ? "没有匹配的结果"
                        : scannedGames.length === 0
                          ? "还没有已安装游戏的记录——先到存档页点「扫描本机」"
                          : "已安装的游戏暂无目录内修改器"}
                  </TableCell>
                </TableRow>
              )}
              {(searching || matchPending) && (
                <TableRow>
                  <TableCell colSpan={10} className="h-20 text-center text-muted-foreground">
                    <Loader2 className="mr-2 inline size-4 animate-spin" />
                    {searching ? "搜索中……" : "匹配中……"}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
      ) : (
        <Card className="flex h-48 flex-col items-center justify-center gap-3 text-muted-foreground">
          <Gamepad2 className="size-8" />
          <p>先到存档页点「扫描本机」，这里会自动列出你每个游戏可用的修改器；也可以直接搜索</p>
        </Card>
      )}

      {/* 底部：来源徽章 + 说明（与存档页底部统计行同款居中样式） */}
      <div className="flex flex-wrap items-center justify-center gap-2 pb-1 text-xs text-muted-foreground">
        {sources.map((s) => (
          <SourceBadge key={s.id} sourceId={s.id} />
        ))}
        <span>
          目录收录 {sources.reduce((n, s) => n + s.count, 0)} 个修改器 · 已安装 {installed.length} 个
          （下载自来源官网，杀软可能误报，请自行甄别）
        </span>
      </div>

      {/* 行右键菜单（与存档/恢复页同款） */}
      {rowMenu && (
        <RowMenu
          x={rowMenu.x}
          y={rowMenu.y}
          items={buildRowMenuItems(rowMenu.row)}
          onClose={() => setRowMenu(null)}
        />
      )}

      {/* 悬浮批量操作栏：选中后出现在底部中央（与存档页同款） */}
      {selected.size > 0 && (
        <div className="fixed bottom-8 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-full border bg-background/95 px-5 py-2.5 shadow-xl backdrop-blur">
          <span className="text-sm font-medium">已选 {selected.size} 个</span>
          <Button
            size="sm"
            onClick={batchDownload}
            disabled={batchRunning || downloadableCount === 0}
            title="批量下载选中且未安装的修改器"
          >
            {batchRunning ? (
              <Loader2 className="mr-1.5 size-3.5 animate-spin" />
            ) : (
              <Download className="mr-1.5 size-3.5" />
            )}
            批量下载{downloadableCount > 0 ? `（${downloadableCount}）` : ""}
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="text-destructive hover:text-destructive"
            onClick={() => setBatchDeleteAsk(true)}
            disabled={deletableCount === 0}
            title="批量删除选中的已安装修改器"
          >
            <Trash2 className="mr-1.5 size-3.5" />
            批量删除{deletableCount > 0 ? `（${deletableCount}）` : ""}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            取消选择
          </Button>
        </div>
      )}

      {/* 删除确认 */}
      <AlertDialog open={pendingDelete != null} onOpenChange={(open) => !open && setPendingDelete(null)}>
        <AlertDialogContent className="sm:max-w-lg">
          <AlertDialogHeader>
            <AlertDialogTitle>
              删除 {pendingDelete?.manifest.zhName ?? pendingDelete?.manifest.gameName} 的修改器？
            </AlertDialogTitle>
            <AlertDialogDescription>
              将删除该修改器的包目录及其全部文件，不影响游戏本体的任何数据。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="break-all rounded-md bg-muted px-3 py-2 font-mono text-xs leading-relaxed text-muted-foreground">
            {pendingDelete?.folder}
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={confirmDelete}
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 批量删除确认 */}
      <AlertDialog open={batchDeleteAsk} onOpenChange={(open) => !open && setBatchDeleteAsk(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除 {deletableCount} 个已安装的修改器？</AlertDialogTitle>
            <AlertDialogDescription>
              将删除选中修改器的包目录及全部文件，不影响游戏本体的任何数据。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={batchDelete}
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
