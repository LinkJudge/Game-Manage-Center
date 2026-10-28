import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import {
  Download,
  ExternalLink,
  FolderOpen,
  Gamepad2,
  ImagePlus,
  Layers3,
  Loader2,
  PackageCheck,
  PackageX,
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
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { RowMenu, type RowMenuItem } from "@/components/rowMenu";
import { GameCover, refreshAllCovers } from "@/components/gameCover";
import { CoverPickerDialog } from "@/components/coverPickerDialog";
import { TrainerSourceBadge, compareTrainerHits } from "@/components/trainerSource";
import { TrainerSourcePicker } from "@/components/trainerSourcePicker";
import { ActionTooltip } from "@/components/ui/tooltip";
import { ACTION_COPY } from "@/lib/actionCopy";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useScanStore, useTrainerStore } from "@/store";
import { gameName, gameNamePair, resolveLang, translate, useI18n } from "@/i18n";
import type {
  CoverFillSummary,
  CoverLookup,
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
function openExternalLink(url: string) {
  import("@tauri-apps/plugin-opener")
    .then(({ openUrl }) => openUrl(url))
    .catch((e) => toast.error(translate("trainers.toast.openFailed"), { description: String(e) }));
}

const DOWNLOAD_CANCELLED = "__TRAINER_DOWNLOAD_CANCELLED__";

/** 名称归一化：忽略大小写、全半角、空格与标点 */
function normalizeGameName(value: string | null | undefined): string {
  return (value ?? "")
    .normalize("NFKC")
    .toLowerCase()
    .replace(/[™®]/g, "")
    .replace(/[^a-z0-9\u4e00-\u9fff]/g, "");
}

/** 宽松匹配游戏名：完全相同，或较长名称包含较短名称且覆盖比例足够 */
function gameNamesMatch(left: string, right: string): boolean {
  if (!left || !right) return false;
  if (left === right) return true;
  const shorter = left.length <= right.length ? left : right;
  const longer = left.length <= right.length ? right : left;
  return shorter.length >= 4 && longer.includes(shorter) && shorter.length / longer.length >= 0.55;
}

// ---------------------------------------------------------------- 行模型与操作单元格

/**
 * 结果表的统一行：搜索/匹配视图里 hit 是目录条目、installedOf 是该条目当前
 * 装着的最新包；「只看已安装」视图里 hit 为空，信息全部来自 manifest。
 */
interface ResultRow {
  key: string;
  hit: TrainerHit | null;
  /** 折叠后的本地游戏显示名（优先游戏库中文名） */
  displayName?: string;
  /** 可选的多个来源；仅默认匹配视图的未安装行使用 */
  sources?: TrainerHit[];
  /** 副行文字：匹配视图是"来自 <游戏标题>"，搜索视图是中文名时的英文名 */
  subline?: string;
  installedOf?: InstalledTrainer;
  update?: TrainerUpdateInfo;
}

function rowName(row: ResultRow): string {
  if (row.displayName) return row.displayName;
  const lang = resolveLang();
  if (row.hit) return gameNamePair(row.hit.gameName, row.hit.zhName, lang).main;
  const m = row.installedOf?.manifest;
  return m ? gameNamePair(m.gameName, m.zhName, lang).main : "—";
}

function rowSub(row: ResultRow): string {
  if (row.subline) return row.subline;
  const lang = resolveLang();
  const m = row.installedOf?.manifest;
  if (row.hit) return gameNamePair(row.hit.gameName, row.hit.zhName, lang).sub ?? "";
  if (m) return gameNamePair(m.gameName, m.zhName, lang).sub ?? "";
  return "";
}

/** 行内下载/解压进度（download → extract → done 事件驱动） */
function RowProgress({
  progress,
  onCancel,
}: {
  progress: TrainerProgress;
  onCancel: () => void;
}) {
  const { t: tr } = useI18n();
  const pct = progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : 0;
  if (progress.stage === "cancelling") {
    return (
      <div className="flex items-center justify-end gap-1.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> {tr("trainers.progress.cancelling")}
      </div>
    );
  }
  if (progress.stage === "extract") {
    return (
      <div className="flex items-center justify-end gap-1.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> {tr("trainers.progress.extracting")}
      </div>
    );
  }
  const cancelButton = (
    <ActionTooltip content={ACTION_COPY.cancelDownload.full}>
      <Button
        size="icon-sm"
        variant="outline"
        className="size-7 rounded-[0.7rem]"
        aria-label={ACTION_COPY.cancelDownload.full}
        onClick={onCancel}
      >
        <X className="size-3.5" />
      </Button>
    </ActionTooltip>
  );
  // 连接/重定向阶段：还没有字节进度
  if (progress.total === 0 && progress.downloaded === 0) {
    return (
      <div className="flex items-center justify-end gap-2 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> {tr("trainers.progress.connecting")}
        {cancelButton}
      </div>
    );
  }
  return (
    <div className="ml-auto flex items-center justify-end gap-2">
      <div className="w-32 text-right">
        <Progress value={pct} className="h-1.5" />
        <div className="mt-0.5 text-[10px] text-muted-foreground">
          {pct}% · {formatBytes(progress.downloaded)}
        </div>
      </div>
      {cancelButton}
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
  onChooseSource,
  onCancel,
}: {
  row: ResultRow;
  progress?: TrainerProgress;
  onDownload: (hit: TrainerHit) => void;
  onUpdate: (u: TrainerUpdateInfo) => void;
  onLaunch: (t: InstalledTrainer) => void;
  onOpenFolder: (t: InstalledTrainer) => void;
  onDeleteRequest: (t: InstalledTrainer) => void;
  onChooseSource: (row: ResultRow) => void;
  onCancel: (trainerId: string) => void;
}) {
  if (progress) return <RowProgress progress={progress} onCancel={() => onCancel(progress.trainerId)} />;

  if (row.installedOf) {
    const t = row.installedOf;
    return (
      <div className="space-x-1 text-right">
        <ActionTooltip content={ACTION_COPY.launchTrainer.full}>
          <Button
            size="action"
            aria-label={ACTION_COPY.launchTrainer.full}
            onClick={() => onLaunch(t)}
          >
            <Play className="mr-1 size-3.5" />
            {ACTION_COPY.launchTrainer.short}
          </Button>
        </ActionTooltip>
        <ActionTooltip content={ACTION_COPY.openTrainerFolder.full}>
          <Button
            size="action"
            variant="outline"
            aria-label={ACTION_COPY.openTrainerFolder.full}
            onClick={() => onOpenFolder(t)}
          >
            <FolderOpen className="size-3.5" />
          </Button>
        </ActionTooltip>
        {row.update && (
          <ActionTooltip content={ACTION_COPY.updateTrainer.full}>
            <Button
              size="action"
              variant="outline"
              aria-label={ACTION_COPY.updateTrainer.full}
              onClick={() => onUpdate(row.update!)}
            >
              <RefreshCw className="mr-1 size-3.5" />
              {ACTION_COPY.updateTrainer.short}
            </Button>
          </ActionTooltip>
        )}
        <ActionTooltip content={ACTION_COPY.deleteTrainer.full}>
          <Button
            size="action"
            variant="outline"
            className="text-destructive hover:text-destructive"
            aria-label={ACTION_COPY.deleteTrainer.full}
            onClick={() => onDeleteRequest(t)}
          >
            <Trash2 className="size-3.5" />
          </Button>
        </ActionTooltip>
      </div>
    );
  }

  if (row.sources && row.sources.length > 1) {
    return (
      <ActionTooltip content={ACTION_COPY.chooseSource.full}>
        <Button
          size="action"
          aria-label={ACTION_COPY.chooseSource.full}
          onClick={() => onChooseSource(row)}
        >
          <Layers3 className="mr-1 size-3.5" />
          {ACTION_COPY.chooseSource.short}（{row.sources.length}）
        </Button>
      </ActionTooltip>
    );
  }

  const hit = row.hit;
  if (hit?.externalUrl) {
    return (
      <ActionTooltip content={ACTION_COPY.externalTrainer.full}>
        <Button
          size="action"
          variant="outline"
          aria-label={ACTION_COPY.externalTrainer.full}
          onClick={() => openExternalLink(hit.externalUrl!)}
        >
          <ExternalLink className="mr-1 size-3.5" />
          {ACTION_COPY.externalTrainer.short}
        </Button>
      </ActionTooltip>
    );
  }
  return (
    <ActionTooltip content={ACTION_COPY.downloadTrainer.full} disabled={!hit?.downloadUrl}>
      <Button
        size="action"
        aria-label={ACTION_COPY.downloadTrainer.full}
        disabled={!hit?.downloadUrl}
        onClick={() => hit && onDownload(hit)}
      >
        <Download className="mr-1 size-3.5" />
        {ACTION_COPY.downloadTrainer.short}
      </Button>
    </ActionTooltip>
  );
}

// ---------------------------------------------------------------- 修改器标签页

export function TrainersTab() {
  const { t: tr, lang } = useI18n();
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
  const [orphanOnly, setOrphanOnly] = useState(false);
  // 来源筛选："all" 或某个 sourceId
  const [sourceFilter, setSourceFilter] = useState<string>("all");
  const [coverPickerRow, setCoverPickerRow] = useState<ResultRow | null>(null);
  const [sourcePickerRow, setSourcePickerRow] = useState<ResultRow | null>(null);
  const [fillingCovers, setFillingCovers] = useState(false);
  const sgdbStatus = useQuery({
    queryKey: ["sgdb-status"],
    queryFn: () => invoke<boolean>("steamgriddb_status"),
    staleTime: 30_000,
  });
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
    setOrphanOnly(false);
    setSearching(true);
    const t = setTimeout(() => {
      const seq = ++searchSeq.current;
      invoke<TrainerHit[]>("search_trainers", { query: q, limit: 60 })
        .then((hits) => {
          if (seq === searchSeq.current) setHits(hits);
        })
        .catch((e) => {
          if (seq === searchSeq.current) toast.error(tr("trainers.toast.searchFailed"), { description: String(e) });
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
        setProgress((prev) => {
          const current = prev[p.trainerId];
          return {
            ...prev,
            [p.trainerId]: {
              ...p,
              stage: current?.stage === "cancelling" ? "cancelling" : p.stage,
            },
          };
        });
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
        toast.success(tr("trainers.toast.installed"), { description: t.manifest.gameName });
        refreshInstalled();
      })
      .catch((e) => {
        if (String(e).includes(DOWNLOAD_CANCELLED)) {
          toast.info(tr("trainers.toast.cancelled"), { description: hit.gameName });
        } else {
          toast.error(tr("trainers.toast.downloadFailed"), { description: String(e) });
        }
        setProgress((prev) => {
          const next = { ...prev };
          delete next[hit.id];
          return next;
        });
      });
  };

  const cancelDownload = (trainerId: string) => {
    setProgress((prev) => {
      const current = prev[trainerId];
      if (!current) return prev;
      return {
        ...prev,
        [trainerId]: { ...current, stage: "cancelling" },
      };
    });

    const requestCancel = (attempt: number) => {
      invoke<boolean>("cancel_trainer_download", { trainerId })
        .then((found) => {
          if (found) return;
          if (attempt < 2) {
            window.setTimeout(() => requestCancel(attempt + 1), 150);
            return;
          }
          setProgress((prev) => {
            const next = { ...prev };
            delete next[trainerId];
            return next;
          });
          toast.info(tr("trainers.toast.downloadEnded"));
        })
        .catch((e) => {
          setProgress((prev) => {
            const current = prev[trainerId];
            if (!current) return prev;
            return {
              ...prev,
              [trainerId]: { ...current, stage: "download" },
            };
          });
          toast.error(tr("trainers.toast.cancelFailed"), { description: String(e) });
        });
    };
    requestCancel(0);
  };

  const chooseSource = (hit: TrainerHit) => {
    setSourcePickerRow(null);
    if (hit.downloadUrl) {
      startDownload(hit);
    } else if (hit.externalUrl) {
      openExternalLink(hit.externalUrl);
    }
  };

  const startUpdate = (u: TrainerUpdateInfo) => {    setProgress((prev) => ({
      ...prev,
      [u.trainerId]: { trainerId: u.trainerId, stage: "download", downloaded: 0, total: 0 },
    }));
    invoke<InstalledTrainer>("update_trainer", {
      trainerId: u.trainerId,
      downloadUrl: u.newDownload.url,
      oldFolder: u.oldFolder,
    })
      .then(() => {
        toast.success(tr("trainers.toast.updated"), {
          description: gameName(u.gameName, u.zhName, lang),
        });
        refreshInstalled();
      })
      .catch((e) => {
        if (String(e).includes(DOWNLOAD_CANCELLED)) {
          toast.info(tr("trainers.toast.updateCancelled"), {
            description: gameName(u.gameName, u.zhName, lang),
          });
        } else {
          toast.error(tr("trainers.toast.updateFailed"), { description: String(e) });
        }
        setProgress((prev) => {
          const next = { ...prev };
          delete next[u.trainerId];
          return next;
        });
      });
  };

  const launch = (t: InstalledTrainer) => {
    invoke("launch_installed_trainer", { folder: t.folder }).catch((e) =>
      toast.error(tr("trainers.toast.launchFailed"), { description: String(e) }),
    );
  };

  const openFolder = (t: InstalledTrainer) => {
    invoke("open_save_folder", { path: t.folder }).catch((e) =>
      toast.error(tr("trainers.toast.openFailed"), { description: String(e) }),
    );
  };

  const confirmDelete = () => {
    const t = pendingDelete;
    setPendingDelete(null);
    if (!t) return;
    invoke("delete_installed_trainer", { folder: t.folder })
      .then(() => {
        toast.success(tr("trainers.toast.deleted"), { description: t.manifest.gameName });
        refreshInstalled();
      })
      .catch((e) => toast.error(tr("trainers.toast.deleteFailed"), { description: String(e) }));
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

  // 右键只保留一个行按钮之外的快捷入口；启动/更新/删除/选来源/换图标
  // 都能直接在行内点到，不再重复堆进菜单。
  const buildRowMenuItems = (row: ResultRow): RowMenuItem[] => {
    if (!row.installedOf) return [];
    const t = row.installedOf;
    return [
      {
        label: ACTION_COPY.openTrainerFolder.full,
        icon: FolderOpen,
        onSelect: () => openFolder(t),
      },
    ];
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
    .flatMap((m): ResultRow[] => {
      const orderedHits = [...m.matches].sort(compareTrainerHits);
      const installedHits = orderedHits
        .map((hit) => ({ hit, trainer: findInstalled(hit.id) }))
        .filter((item) => item.trainer);

      // 没有任何已安装版本 → 同游戏折叠成一行；来源先隐藏，下载时再选择
      if (installedHits.length === 0) {
        const primary = orderedHits[0];
        const pair = gameNamePair(m.title, m.zhName ?? primary.zhName, lang);
        return [
          {
            key: `match-${m.title}`,
            hit: primary,
            displayName: pair.main,
            sources: orderedHits,
            subline: pair.sub
              ? pair.sub
              : orderedHits.length > 1
                ? tr("trainers.source.chooseCount", { count: orderedHits.length })
                : primary.gameName,
          },
        ];
      }

      // 已安装的来源必须逐行保留，避免隐藏已安装信息
      return orderedHits.map((hit) => {
        const installedOf = findInstalled(hit.id);
        const sameGameName =
          normalizeGameName(m.title) === normalizeGameName(hit.gameName);
        return {
          key: `${m.title}-${hit.id}`,
          hit,
          subline: sameGameName
            ? hit.gameName
            : tr("trainers.source.from", { game: hit.gameName, source: m.title }),
          installedOf,
          update: installedOf ? updateOf(installedOf.folder) : undefined,
        };
      });
    });

  // 残留修改器：修改器还装着，但本机扫描不到对应游戏。
  // 优先用扫描结果的已安装路径 + 名称归一化匹配；再用目录匹配结果兜底，
  // 避免“Monster Hunter: World / World: Iceborne”这类标点/副标题差异误判。
  const scanDataReady = scanOutput !== null;
  const installedGames = (scanOutput?.games ?? []).filter((g) => g.installPath);
  // 目录匹配还在进行时先不出残留结论，避免把“尚未核对完”误报成残留
  const orphanCheckReady = scanDataReady && (installedGames.length === 0 || matchQuery.isSuccess);
  const installedGameAliases = installedGames.flatMap((g) =>
    [
      g.entry.title,
      g.entry.zhCn,
      g.installPath?.split(/[\\/]/).filter(Boolean).pop(),
    ]
      .map(normalizeGameName)
      .filter((name): name is string => Boolean(name)),
  );
  const matchedInstalledTrainerIds = new Set(
    (matchQuery.data ?? [])
      .filter((m) =>
        [m.title, m.zhName].some((name) => {
          const normalized = normalizeGameName(name);
          return normalized &&
            installedGameAliases.some((local) => gameNamesMatch(normalized, local));
        }),
      )
      .flatMap((m) => m.matches.map((hit) => hit.id)),
  );
  const orphanedInstalled = orphanCheckReady
    ? installed.filter((t) => {
        const { trainerId, gameName, zhName } = t.manifest;
        if (matchedInstalledTrainerIds.has(trainerId)) return false;
        const trainerAliases = [gameName, zhName]
          .map(normalizeGameName)
          .filter((name): name is string => Boolean(name));
        return !trainerAliases.some((name) =>
          installedGameAliases.some((local) => gameNamesMatch(name, local)),
        );
      })
    : [];
  const orphanFolders = new Set(orphanedInstalled.map((t) => t.folder));
  const residualRows: ResultRow[] = orphanedInstalled.map((t) => ({
    key: t.folder,
    hit: null,
    installedOf: t,
    update: updateOf(t.folder),
  }));

  const toggleOnlyInstalled = () => {
    const next = !onlyInstalled;
    setOnlyInstalled(next);
    setOrphanOnly(false);
    setSelected(new Set());
    if (next) setQuery("");
  };

  const toggleOrphanOnly = () => {
    const next = !orphanOnly;
    setOrphanOnly(next);
    setOnlyInstalled(false);
    setSelected(new Set());
    if (next) setQuery("");
  };

  // 视图行：搜索词 > 残留修改器 > 只看已安装 > 默认匹配视图
  const searching_ = query.trim().length > 0;
  let rows: ResultRow[] = [];
  if (searching_) {
    rows = hits.map((hit) => {
      const installedOf = findInstalled(hit.id);
      return {
        key: hit.id,
        hit,
        subline: gameNamePair(hit.gameName, hit.zhName, resolveLang()).sub ?? undefined,
        installedOf,
        update: installedOf ? updateOf(installedOf.folder) : undefined,
      };
    });
  } else if (orphanOnly) {
    rows = residualRows;
  } else if (onlyInstalled) {
    rows = installed.map((t) => ({
      key: t.folder,
      hit: null,
      installedOf: t,
      update: updateOf(t.folder),
    }));
  } else {
    // 默认视图必须带上“已装但游戏未检测到”的残留修改器，否则下载完的
    // 未安装游戏修改器会从页面上消失。
    rows = [...matchRows, ...residualRows];
  }

  // 扫描过（或有搜索/已安装修改器）就显示表格；从未扫描才显示引导卡片
  const showTable =
    searching_ ||
    searching ||
    onlyInstalled ||
    orphanOnly ||
    scannedGames.length > 0 ||
    (scanDataReady && installed.length > 0);

  // 与存档页一致：已安装在前、可下载居中、残留统一排在最后
  type TrainerGroup = "installed" | "available" | "residual";
  const groupOfRow = (row: ResultRow): TrainerGroup => {
    if (!row.installedOf) return "available";
    return orphanFolders.has(row.installedOf.folder) ? "residual" : "installed";
  };
  const groupOrder: Record<TrainerGroup, number> = {
    installed: 0,
    available: 1,
    residual: 2,
  };
  const sortedRows = [...rows].sort(
    (a, b) => groupOrder[groupOfRow(a)] - groupOrder[groupOfRow(b)],
  );
  // 来源筛选：只看带 hit 的行；纯已安装行（无来源信息）始终保留，
  // 否则筛选「小幸」时本地的风灵修改器会整个消失，看起来像丢了。
  const orderedRows =
    sourceFilter === "all"
      ? sortedRows
      : sortedRows.filter((row) => {
          const ids = [
            row.hit?.sourceId,
            ...(row.sources?.map((s) => s.sourceId) ?? []),
          ].filter((x): x is string => Boolean(x));
          if (ids.length === 0) return true;
          return ids.includes(sourceFilter);
        });
  const groupCounts: Record<TrainerGroup, number> = { installed: 0, available: 0, residual: 0 };
  for (const row of orderedRows) groupCounts[groupOfRow(row)]++;
  const groupMeta: Record<TrainerGroup, { label: string; dot: string }> = {
    installed: { label: tr("trainers.group.installed"), dot: "is-installed" },
    available: { label: tr("trainers.group.available"), dot: "is-available" },
    residual: { label: tr("trainers.group.residual"), dot: "is-leftover" },
  };

  const localGameFor = (gameName: string, zhName: string | null | undefined) => {
    const names = [gameName, zhName]
      .map(normalizeGameName)
      .filter((name): name is string => Boolean(name));
    return (scanOutput?.games ?? []).find((game) => {
      const locals = [game.entry.title, game.entry.zhCn]
        .map(normalizeGameName)
        .filter((name): name is string => Boolean(name));
      return names.some((name) => locals.some((local) => gameNamesMatch(name, local)));
    });
  };
  const pickerGameTitle = coverPickerRow
    ? (coverPickerRow.hit?.gameName ??
      coverPickerRow.installedOf?.manifest.gameName ??
      rowName(coverPickerRow))
    : "";
  const pickerLocalGame = coverPickerRow
    ? localGameFor(
        pickerGameTitle,
        coverPickerRow.hit?.zhName ??
          coverPickerRow.installedOf?.manifest.zhName ??
          null,
      )
    : undefined;
  // 默认匹配视图的请求进行中（加载中行只在对应视图下出现，不与空状态叠加）
  const matchPending =
    !searching_ && !onlyInstalled && !orphanOnly && (matchQuery.isLoading || matchQuery.isFetching);

  // 多选：批量下载未安装的、批量删除已安装的（顺序执行，单条失败不中断）
  const selectedRows = rows.filter((r) => selected.has(r.key));
  const downloadableCount = selectedRows.filter((r) => r.hit?.downloadUrl && !r.installedOf).length;
  const deletableCount = selectedRows.filter((r) => r.installedOf).length;
  const batchSelectableRows = rows.filter(
    (row) => !(row.sources && row.sources.length > 1),
  );
  const allSelected =
    batchSelectableRows.length > 0 &&
    batchSelectableRows.every((r) => selected.has(r.key));

  const fillMissingCovers = () => {
    const seen = new Set<string>();
    const requests: CoverLookup[] = [];
    for (const row of orderedRows) {
      const manifest = row.installedOf?.manifest;
      const coverTitle = row.hit?.gameName ?? manifest?.gameName ?? rowName(row);
      const localGame = localGameFor(
        coverTitle,
        row.hit?.zhName ?? manifest?.zhName ?? null,
      );
      const dedupeKey = `${localGame?.entry.pcgwId ?? ""}|${coverTitle.toLowerCase()}`;
      if (seen.has(dedupeKey)) continue;
      seen.add(dedupeKey);
      requests.push({
        title: coverTitle,
        pcgwId: localGame?.entry.pcgwId ?? null,
        steamId: localGame?.entry.steamId ?? null,
      });
    }
    if (requests.length === 0) return;
    setFillingCovers(true);
    invoke<CoverFillSummary>("steamgriddb_fill_missing", { requests })
      .then((summary) => {
        if (summary.filled > 0) refreshAllCovers();
        if (summary.failed > 0) {
          toast.warning(
            tr("trainers.toast.coverFillSummary", { ok: summary.filled, skipped: summary.skipped, failed: summary.failed }),
            { description: summary.errors.join("\n") },
          );
        } else {
          toast.success(
            summary.filled > 0
              ? tr("trainers.toast.coverFilled", { count: summary.filled })
              : tr("trainers.toast.coverAllExist"),
          );
        }
      })
      .catch((e) => toast.error(tr("trainers.toast.coverFailed"), { description: String(e) }))
      .finally(() => setFillingCovers(false));
  };

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
      toast.warning(tr("trainers.toast.batchDownloadPartial", { ok, failed: failed.length }), {
        description: failed.join("\n"),
      });
    } else {
      toast.success(tr("trainers.toast.batchDownloadDone", { count: ok }));
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
      toast.warning(tr("trainers.toast.batchDeletePartial", { ok, failed: failed.length }), {
        description: failed.join("\n"),
      });
    } else {
      toast.success(tr("trainers.toast.batchDeleted", { count: ok }));
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
            placeholder={tr("trainers.filter.placeholder")}
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
        {sources.length > 1 && (
          <ActionTooltip content={tr("trainers.filter.sourceTip")}>
            <div className="flex items-center gap-0.5 rounded-[0.7rem] border p-0.5">
              <button
                type="button"
                onClick={() => setSourceFilter("all")}
                className={`rounded-[0.55rem] px-2 py-1 text-xs transition-colors ${
                  sourceFilter === "all"
                    ? "bg-foreground/10 font-medium text-foreground"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {tr("trainers.filter.sourceAll")}
              </button>
              {sources.map((src) => {
                const short =
                  src.id === "fling"
                    ? "风灵"
                    : src.id === "fling_archive"
                      ? "旧档"
                      : src.id === "xiaoxing"
                        ? "小幸"
                        : src.title.slice(0, 2);
                const active = sourceFilter === src.id;
                return (
                  <button
                    key={src.id}
                    type="button"
                    title={`${src.title}（${src.count}）`}
                    onClick={() => setSourceFilter(active ? "all" : src.id)}
                    className={`rounded-[0.55rem] px-2 py-1 text-xs transition-colors ${
                      active
                        ? "bg-foreground/10 font-medium text-foreground"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    {short}
                  </button>
                );
              })}
            </div>
          </ActionTooltip>
        )}
        <ActionTooltip content={tr("trainers.filter.onlyInstalledTip")}>
          <Button
            size="sm"
            variant={onlyInstalled ? "default" : "outline"}
            onClick={toggleOnlyInstalled}
          >
            <PackageCheck className="mr-1.5 size-3.5" />
            {tr("trainers.filter.onlyInstalled", { count: installed.length })}
          </Button>
        </ActionTooltip>
        <ActionTooltip
          content={
            orphanCheckReady
              ? tr("trainers.filter.residualTip")
              : scanDataReady
                ? tr("trainers.filter.checkingTip")
                : tr("trainers.filter.needScanTip")
          }
          disabled={!orphanCheckReady}
        >
          <Button
            size="sm"
            variant={orphanOnly ? "default" : "outline"}
            onClick={toggleOrphanOnly}
            disabled={!orphanCheckReady}
            className={
              orphanOnly
                ? "border-transparent bg-[#ff9f0a] text-white hover:bg-[#e68f00]"
                : undefined
            }
          >
            <PackageX className="mr-1.5 size-3.5" />
            {tr("trainers.filter.residual")}{orphanCheckReady ? `（${orphanedInstalled.length}）` : ""}
          </Button>
        </ActionTooltip>
        <ActionTooltip
          content={
            sgdbStatus.data
              ? tr("trainers.tip.fillCovers")
              : tr("trainers.tip.fillCoversNeedKey")
          }
          disabled={!sgdbStatus.data || fillingCovers || orderedRows.length === 0}
        >
          <Button
            size="sm"
            variant="outline"
            onClick={fillMissingCovers}
            disabled={!sgdbStatus.data || fillingCovers || orderedRows.length === 0}
          >
            {fillingCovers ? (
              <Loader2 className="mr-1.5 size-3.5 animate-spin" />
            ) : (
              <ImagePlus className="mr-1.5 size-3.5" />
            )}
            {fillingCovers ? tr("trainers.action.filling") : tr("trainers.action.fillCovers")}
          </Button>
        </ActionTooltip>
      </div>

      {orphanOnly && (
        <div className="gm-orphan-banner">
          <span className="gm-orphan-banner-icon">
            <PackageX className="size-4" />
          </span>
          <div className="min-w-0">
            <p className="text-xs font-semibold text-foreground">{tr("trainers.notice.title")}</p>
            <p className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">
              {tr("trainers.notice.desc")}
            </p>
          </div>
        </div>
      )}

      {showTable ? (
        <Card className="overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-12 pl-4">
                  <Checkbox
                    checked={batchSelectableRows.length > 0 && allSelected}
                    onCheckedChange={(v) => {
                      if (v) setSelected(new Set(batchSelectableRows.map((r) => r.key)));
                      else setSelected(new Set());
                    }}
                  />
                </TableHead>
                <TableHead className="w-[30%]">{tr("trainers.col.game")}</TableHead>
                <TableHead className="w-[12%]">{tr("trainers.col.source")}</TableHead>
                <TableHead>{tr("trainers.col.pack")}</TableHead>
                <TableHead className="w-[15%]">{tr("trainers.col.updated")}</TableHead>
                <TableHead className="w-[13%]">{tr("trainers.col.status")}</TableHead>
                <TableHead className="pr-4 text-right">{tr("trainers.col.actions")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {orderedRows.map((row, index) => {
                const manifest = row.installedOf?.manifest;
                const name = rowName(row);
                const coverTitle =
                  row.hit?.gameName ?? manifest?.gameName ?? name;
                const localGame = localGameFor(
                  coverTitle,
                  row.hit?.zhName ?? manifest?.zhName ?? null,
                );
                const rowGroup = groupOfRow(row);
                const showGroup =
                  index === 0 || groupOfRow(orderedRows[index - 1]) !== rowGroup;
                const subline = rowSub(row) || (manifest ? tr("trainers.subline.installed") : tr("trainers.subline.catalog"));
                const optionsCount =
                  row.hit?.optionsCount ?? manifest?.optionsCount ?? null;
                const gameVersion = row.hit?.gameVersion ?? manifest?.gameVersion ?? null;
                const sizeBytes = row.hit?.sizeBytes ?? manifest?.sizeBytes ?? null;
                const updatedAt = row.hit?.updatedAt ?? manifest?.dateAdded ?? null;
                const isOrphan = row.installedOf
                  ? orphanFolders.has(row.installedOf.folder)
                  : false;
                const statusLabel = isOrphan
                  ? tr("trainers.status.residual")
                  : row.installedOf
                    ? row.update
                      ? tr("trainers.status.update")
                      : tr("trainers.status.installed")
                    : row.sources
                      ? tr("trainers.status.available")
                      : row.hit?.externalUrl
                        ? tr("trainers.status.manual")
                        : tr("trainers.status.downloadable");
                const statusClass = isOrphan
                  ? "is-orphan"
                  : row.installedOf
                    ? row.update
                      ? "is-update"
                      : "is-installed"
                    : "is-available";
                return (
                  <Fragment key={row.key}>
                    {showGroup && (
                      <TableRow className="gm-group-row">
                        <TableCell colSpan={7} className="py-2 pl-4">
                          <div className="flex items-center gap-2">
                            <span className={`gm-group-dot ${groupMeta[rowGroup].dot}`} />
                            <span>{groupMeta[rowGroup].label}</span>
                            <span className="text-[10px] font-medium tabular-nums text-muted-foreground/60">
                              {groupCounts[rowGroup]}
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
                        checked={selected.has(row.key)}
                        disabled={!!(row.sources && row.sources.length > 1)}
                        onCheckedChange={(v) => toggleSelect(row.key, v === true)}
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
                              title={coverTitle}
                              pcgwId={localGame?.entry.pcgwId ?? null}
                              steamId={localGame?.entry.steamId ?? null}
                              installPath={localGame?.installPath ?? null}
                            />
                            <span className="gm-cover-edit-badge">
                              <ImagePlus className="size-2.5" />
                            </span>
                          </button>
                        </ActionTooltip>
                        <div className="min-w-0">
                          <div className="truncate text-[0.84rem] font-semibold tracking-[-0.01em]">
                            {name}
                          </div>
                          <div className="mt-0.5 truncate text-[11px] text-muted-foreground/80">
                            {subline}
                          </div>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell>
                      {row.sources && !row.installedOf ? (
                        <span className="text-[11px] text-muted-foreground/60">—</span>
                      ) : (
                        <TrainerSourceBadge sourceId={row.hit?.sourceId ?? manifest?.sourceId ?? ""} />
                      )}
                    </TableCell>
                    <TableCell>
                      <div className="text-xs font-medium tabular-nums text-foreground/85">
                        {optionsCount != null ? tr("trainers.row.options", { count: optionsCount }) : tr("trainers.row.optionsUnknown")}
                      </div>
                      <div className="mt-0.5 text-[10px] tabular-nums text-muted-foreground/65">
                        {gameVersion ?? tr("trainers.row.versionUnknown")} · {formatBytes(sizeBytes)}
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="text-xs font-medium tabular-nums text-foreground/85">
                        {formatDate(updatedAt)}
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-col items-start gap-1">
                        <span className={`gm-trainer-status ${statusClass}`}>
                          <span className="gm-trainer-status-dot" />
                          {statusLabel}
                        </span>
                        <span className="text-[10px] tabular-nums text-muted-foreground/60">
                          {manifest ? tr("trainers.row.installedAt", { date: formatDate(manifest.installedAt) }) : tr("trainers.row.notInstalled")}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="pr-4 text-right">
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
                        onChooseSource={setSourcePickerRow}
                        onCancel={cancelDownload}
                      />
                    </TableCell>
                    </TableRow>
                  </Fragment>
                );
              })}
              {rows.length === 0 && !searching && !matchPending && (
                <TableRow>
                  <TableCell colSpan={7} className="h-20 text-center text-muted-foreground">
                    {orphanOnly
                      ? tr("trainers.empty.noResidual")
                      : onlyInstalled
                        ? tr("trainers.empty.noInstalled")
                        : searching_
                          ? tr("trainers.empty.noMatch")
                          : scannedGames.length === 0
                            ? tr("trainers.empty.noScan")
                            : tr("trainers.empty.noCatalog")}
                  </TableCell>
                </TableRow>
              )}
              {(searching || matchPending) && (
                <TableRow>
                  <TableCell colSpan={7} className="h-20 text-center text-muted-foreground">
                    <Loader2 className="mr-2 inline size-4 animate-spin" />
                    {searching ? tr("trainers.loading.searching") : tr("trainers.loading.matching")}
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Card>
      ) : (
        <Card className="flex h-48 flex-col items-center justify-center gap-3 text-muted-foreground">
          <Gamepad2 className="size-8" />
          <p>{tr("trainers.empty.hint")}</p>
        </Card>
      )}

      {/* 底部：来源徽章 + 说明（与存档页底部统计行同款居中样式） */}
      <div className="flex flex-wrap items-center justify-center gap-2 pb-1 text-xs text-muted-foreground">
        {sources.map((s) => (
          <TrainerSourceBadge key={s.id} sourceId={s.id} />
        ))}
        <span>
          {tr("trainers.footer.catalog", { count: sources.reduce((n, s) => n + s.count, 0), installed: installed.length })}
          {orphanCheckReady
            ? tr("trainers.footer.residual", { count: orphanedInstalled.length })
            : tr("trainers.footer.residualNeedScan")}
          {tr("trainers.footer.disclaimer")}
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

      <CoverPickerDialog
        open={coverPickerRow != null}
        onOpenChange={(open) => !open && setCoverPickerRow(null)}
        title={pickerGameTitle}
        pcgwId={pickerLocalGame?.entry.pcgwId ?? null}
        steamId={pickerLocalGame?.entry.steamId ?? null}
      />

      <TrainerSourcePicker
        open={sourcePickerRow != null}
        gameTitle={sourcePickerRow ? rowName(sourcePickerRow) : ""}
        hits={sourcePickerRow?.sources ?? []}
        onOpenChange={(open) => !open && setSourcePickerRow(null)}
        onChoose={chooseSource}
      />

      {/* 悬浮批量操作栏：选中后出现在底部中央（与存档页同款） */}
      {selected.size > 0 && (
        <div className="fixed bottom-8 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-full border bg-background/95 px-5 py-2.5 shadow-xl backdrop-blur">
          <span className="text-sm font-medium">{tr("trainers.bulk.selected", { count: selected.size })}</span>
          <ActionTooltip
            content={tr("trainers.bulk.downloadTip")}
            disabled={batchRunning || downloadableCount === 0}
          >
          <Button
            size="sm"
            onClick={batchDownload}
            disabled={batchRunning || downloadableCount === 0}
          >
            {batchRunning ? (
              <Loader2 className="mr-1.5 size-3.5 animate-spin" />
            ) : (
              <Download className="mr-1.5 size-3.5" />
            )}
            {tr("trainers.bulk.download")}{downloadableCount > 0 ? `（${downloadableCount}）` : ""}
          </Button>
          </ActionTooltip>
          <ActionTooltip
            content={tr("trainers.bulk.deleteTip")}
            disabled={deletableCount === 0}
          >
          <Button
            size="sm"
            variant="outline"
            className="text-destructive hover:text-destructive"
            onClick={() => setBatchDeleteAsk(true)}
            disabled={deletableCount === 0}
          >
            <Trash2 className="mr-1.5 size-3.5" />
            {tr("trainers.bulk.delete")}{deletableCount > 0 ? `（${deletableCount}）` : ""}
          </Button>
          </ActionTooltip>
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            {tr("trainers.bulk.clear")}
          </Button>
        </div>
      )}

      {/* 删除确认 */}
      <AlertDialog open={pendingDelete != null} onOpenChange={(open) => !open && setPendingDelete(null)}>
        <AlertDialogContent className="sm:max-w-lg">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {tr("trainers.delete.title", {
                name: pendingDelete
                  ? gameName(
                      pendingDelete.manifest.gameName,
                      pendingDelete.manifest.zhName,
                      lang,
                    )
                  : "",
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {tr("trainers.delete.desc")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="break-all rounded-md bg-muted px-3 py-2 font-mono text-xs leading-relaxed text-muted-foreground">
            {pendingDelete?.folder}
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel>{tr("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={confirmDelete}
            >
              {tr("common.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 批量删除确认 */}
      <AlertDialog open={batchDeleteAsk} onOpenChange={(open) => !open && setBatchDeleteAsk(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{tr("trainers.delete.batchTitle", { count: deletableCount })}</AlertDialogTitle>
            <AlertDialogDescription>
              {tr("trainers.delete.batchDesc")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{tr("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={batchDelete}
            >
              {tr("common.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
