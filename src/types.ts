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
  wikiPageId: number;
  installFolder: string | null;
  steamId: string | null;
  gogId: string | null;
  platforms: string[];
  zhCn: string | null;
  paths: SaveLocations;
}

export interface GamesPage {
  items: GameEntry[];
  total: number;
}

export interface AppInfo {
  name: string;
  version: string;
  coreVersion: string;
}

export interface ResolvedPath {
  template: string;
  finalTemplate: string;
  resolved: string;
  kind: "file" | "registry";
}

export interface AccountIds {
  steam: string | null;
  ubisoft: string | null;
  epic: string | null;
  xbox: string | null;
  rockstar: string | null;
  gog: string | null;
  ea: string | null;
}

export interface InstallRoot {
  path: string;
  platform: string;
}

export interface DetectedEnvironment {
  platform: string;
  installPaths: InstallRoot[];
  steamPath: string | null;
  ubisoftPath: string | null;
  accounts: AccountIds;
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
  wikiPageId: number;
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

export interface RestoreResult {
  restoredPaths: number;
  errors: string[];
}

export interface AppSettings { backupPath: string | null; maxBackups: number; compressBackups: boolean; autoBackupEnabled: boolean; closeToTray: boolean; }


export interface BatchResult {
  wikiPageId: number;
  title: string;
  ok: boolean;
  message: string | null;
}

export interface CustomPath { kind: string; path: string; }
export interface CustomGame { id: number; name: string; paths: CustomPath[]; }
