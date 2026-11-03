/**
 * 同一动作的统一文案：
 * - short：按钮上显示，尽量短
 * - full：右键菜单、悬浮提示统一使用，不再各处手写不同说明
 *
 * 通过 Proxy 按当前语言取值，调用方仍写 `ACTION_COPY.backup.full`，
 * 不必改成 hook 调用；组件只要订阅了 useI18n()，切换语言就会重新渲染。
 */
import { resolveLang } from "@/i18n";

type Entry = { short: string; full: string };
type Copy = Record<string, Entry>;

const zh: Copy = {
  backup: { short: "备份", full: "备份此游戏存档" },
  restoreLatest: { short: "恢复", full: "恢复最新备份" },
  manageBackups: { short: "记录", full: "管理备份记录" },
  openBackupFolder: { short: "备份目录", full: "打开备份文件夹" },
  openSaveFolder: { short: "存档目录", full: "打开存档文件夹" },
  openConfigFolder: { short: "设置目录", full: "打开设置文件夹" },
  changeCover: { short: "更换图标", full: "更换游戏图标" },
  launchTrainer: { short: "启动", full: "启动修改器" },
  openTrainerFolder: { short: "目录", full: "打开修改器目录" },
  deleteTrainer: { short: "删除", full: "删除修改器" },
  chooseSource: { short: "选择来源", full: "选择来源下载" },
  downloadTrainer: { short: "下载", full: "下载并安装修改器" },
  cancelDownload: { short: "取消", full: "取消下载" },
  externalTrainer: { short: "官网获取", full: "打开官方下载页" },
  updateTrainer: { short: "更新", full: "更新到最新版本" },
};

const en: Copy = {
  backup: { short: "Back up", full: "Back up this game's saves" },
  restoreLatest: { short: "Restore", full: "Restore the latest backup" },
  manageBackups: { short: "Records", full: "Manage backup records" },
  openBackupFolder: { short: "Backups", full: "Open the backup folder" },
  openSaveFolder: { short: "Saves", full: "Open the save folder" },
  openConfigFolder: { short: "Settings", full: "Open the settings folder" },
  changeCover: { short: "Change art", full: "Change game artwork" },
  launchTrainer: { short: "Launch", full: "Launch trainer" },
  openTrainerFolder: { short: "Folder", full: "Open the trainer folder" },
  deleteTrainer: { short: "Delete", full: "Delete trainer" },
  chooseSource: { short: "Choose source", full: "Choose a source to download" },
  downloadTrainer: { short: "Download", full: "Download and install trainer" },
  cancelDownload: { short: "Cancel", full: "Cancel download" },
  externalTrainer: { short: "Official site", full: "Open the official download page" },
  updateTrainer: { short: "Update", full: "Update to the latest version" },
};

const zhHant: Copy = {
  backup: { short: "備份", full: "備份此遊戲存檔" },
  restoreLatest: { short: "還原", full: "還原最新備份" },
  manageBackups: { short: "記錄", full: "管理備份記錄" },
  openBackupFolder: { short: "備份目錄", full: "開啟備份資料夾" },
  openSaveFolder: { short: "存檔目錄", full: "開啟存檔資料夾" },
  openConfigFolder: { short: "設定目錄", full: "開啟設定資料夾" },
  changeCover: { short: "更換圖示", full: "更換遊戲圖示" },
  launchTrainer: { short: "啟動", full: "啟動修改器" },
  openTrainerFolder: { short: "目錄", full: "開啟修改器目錄" },
  deleteTrainer: { short: "刪除", full: "刪除修改器" },
  chooseSource: { short: "選擇來源", full: "選擇來源下載" },
  downloadTrainer: { short: "下載", full: "下載並安裝修改器" },
  cancelDownload: { short: "取消", full: "取消下載" },
  externalTrainer: { short: "官網取得", full: "開啟官方下載頁" },
  updateTrainer: { short: "更新", full: "更新到最新版本" },
};

const DICTS: Record<string, Copy> = { zh, zhHant, en };

export const ACTION_COPY = new Proxy({} as Copy, {
  get(_target, prop: string) {
    const dict = DICTS[resolveLang()] ?? zh;
    return dict[prop] ?? zh[prop];
  },
});

export type ActionKey = keyof typeof zh;
