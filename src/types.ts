// 与 gm-core 的 serde 序列化结构一一对应（rename_all = "camelCase"）
// specta 接入后此文件改为生成产物

export interface SaveLocations {
  win: string[];
  reg: string[];
  mac: string[];
  linux: string[];
}

export interface GameEntry {
  title: string;
  pcgwId: number;
  installFolder: string | null;
  steamId: string | null;
  gogId: string | null;
  platforms: string[];
  zhCn: string | null;
  paths: SaveLocations;
}

export interface ResolvedPath {
  template: string;
  finalTemplate: string;
  resolved: string;
  kind: "file" | "registry";
  contentKind?: string; // saves | config | reg（语义类型）
}

export interface ScannedGame {
  entry: GameEntry;
  installPath: string | null;
  installedPlatforms: string[];
  resolvedFiles: ResolvedPath[];
  registryCandidates: ResolvedPath[];
  backupSize: number;
  latestModifiedMs: number | null;
}

export interface ScanOutput {
  games: ScannedGame[];
  scannedRoots: number;
  detectedFolders: number;
  deep: boolean;
}

export interface BackupPathEntry {
  folderName: string;
  template: string;
  kind: string; // file | folder | reg（存储形态）
  installFolder: string | null;
  contentKind?: string | null; // saves | config | reg（语义类型；旧备份无此字段）
}

export interface BackupSummary {
  date: string; // YYYY-MM-DD_HH-mm
  title: string;
  zhCn: string | null;
  backupSize: number | null;
  backupPaths: BackupPathEntry[];
  isPermanent: boolean;
  customName: string;
}

export interface GameBackups {
  pcgwId: number;
  latestBackup: string;
  title: string;
  zhCn: string | null;
  backupSize: number | null;
  backups: BackupSummary[];
}

export interface ConflictInfo {
  conflict: boolean;
  localMs: number | null;
  backupMs: number | null;
}

export interface AppSettings {
  backupPath: string | null;
  maxBackups: number;
  compressBackups: boolean;
  autoBackupEnabled: boolean;
  closeToTray: boolean;
  trainerPath: string | null;
  steamgriddbApiKey: string | null;
  locale: string | null;
  webdavUrl: string | null;
  webdavUsername: string | null;
  webdavPassword: string | null;
  webdavEnabled: boolean;
}


export interface BatchResult {
  pcgwId: number;
  title: string;
  ok: boolean;
  message: string | null;
}

export interface CustomPath { kind: string; path: string; }
export interface CustomGame { id: number; name: string; paths: CustomPath[]; }

// ---------------------------------------------------------------- 修改器模块

export interface TrainerSourceSummary {
  id: string;
  title: string;
  homepage: string;
  count: number;
}

export interface TrainerHit {
  sourceId: string;
  id: string;
  gameName: string;
  zhName: string | null;
  pageUrl: string;
  updatedAt: string | null;
  optionsCount: number | null;
  gameVersion: string | null;
  downloadUrl: string | null;
  externalUrl: string | null;
  sizeBytes: number | null;
  downloadCount: number | null;
  totalDownloads: number;
}

export interface TrainerGameQuery {
  title: string;
  zhName: string | null;
}

export interface GameTrainerMatch {
  title: string;
  zhName: string | null;
  matches: TrainerHit[];
}

export interface TrainerManifest {
  schema: string;
  trainerId: string;
  sourceId: string;
  gameName: string;
  zhName: string | null;
  file: string;
  downloadUrl: string;
  pageUrl: string;
  dateAdded: string | null;
  gameVersion: string | null;
  optionsCount: number | null;
  sizeBytes: number | null;
  installedAt: string;
  primaryExe: string | null;
  files: string[];
}

export interface InstalledTrainer {
  manifest: TrainerManifest;
  folder: string;
}

export interface TrainerDownload {
  file: string;
  url: string;
  dateAdded: string | null;
  sizeBytes: number | null;
  downloadCount: number | null;
  gameVersion: string | null;
  optionsCount: number | null;
}

export interface TrainerUpdateInfo {
  trainerId: string;
  gameName: string;
  zhName: string | null;
  oldFolder: string;
  newDownload: TrainerDownload;
}

export interface TrainerProgress {
  trainerId: string;
  stage: "download" | "extract" | "done" | "cancelling";
  downloaded: number;
  total: number;
}

// ---------------------------------------------------------------- 游戏封面

export interface SgdbCandidate {
  id: number;
  url: string;
  thumb: string;
  width: number;
  height: number;
  style: string | null;
  mime: string;
  author: string | null;
}

export interface SgdbOptions {
  gameId: number;
  gameName: string;
  candidates: SgdbCandidate[];
}

export interface CoverLookup {
  title: string;
  pcgwId: number | null;
  steamId: string | null;
}

export interface CoverFillSummary {
  total: number;
  filled: number;
  skipped: number;
  failed: number;
  errors: string[];
}
