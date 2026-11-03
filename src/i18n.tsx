/**
 * 轻量 i18n：不引第三方库，语言设置存 localStorage（即时生效），
 * 同时由设置页写入 AppSettings 做持久化。
 *
 * 用法：
 *   const { t } = useI18n();
 *   <span>{t("nav.saves")}</span>
 *
 * 缺词回退顺序：当前语言 → 中文 → key 本身。
 * 这样英文版可以分批推进，没翻到的地方不会显示成键名。
 */
import { useSyncExternalStore } from "react";
import { savesEn, savesZh } from "@/locales/saves";
import { trainersEn, trainersZh } from "@/locales/trainers";
import { dialogsEn, dialogsZh } from "@/locales/dialogs";
import { settingsEn, settingsZh } from "@/locales/settings";
import { zhHant as zhHantLocale } from "@/locales/zhHant";
import { rustEn, rustZhHant } from "@/locales/rustMessages";

export type Lang = "zh" | "zhHant" | "en";
export type LocaleSetting = "system" | Lang;

const STORAGE_KEY = "gm-locale";

type Dict = Record<string, string>;

const baseZh: Dict = {
  "nav.saves": "存档",
  "nav.trainers": "修改器",
  "nav.settings": "设置",
  "nav.mods": "MOD",
  "app.name": "游戏管理中心",

  "saves.title": "存档",
  "saves.subtitle": "备份、恢复与历史记录集中管理 · 支持多平台、多账号",
  "trainers.title": "修改器",
  "trainers.subtitle": "一键下载安装单机游戏修改器 · 与本机游戏库联动，多来源持续接入",

  "common.save": "保存",
  "common.cancel": "取消",
  "common.close": "关闭",
  "common.refresh": "刷新",

  "settings.title": "设置",
  "settings.tab.general": "通用",
  "settings.tab.saves": "存档",
  "settings.tab.trainers": "修改器",
  "settings.tab.extensions": "扩展",
  "settings.tab.mods": "MOD",
  "mods.title": "MOD",
  "mods.subtitle": "安装、版本管理与冲突排查 · 功能规划中",
  "mods.planned.title": "规划中",
  "mods.planned.desc": "MOD 的安装、版本管理与冲突排查正在设计，后续版本开放。",
  "settings.mods.title": "MOD",
  "settings.mods.desc": "MOD 目录与加载相关设置",
  "settings.mods.planned": "规划中：这里会放 MOD 目录位置、加载顺序、冲突提示等设置。",
  "settings.tab.about": "关于",

  "settings.general.title": "通用",
  "settings.general.desc": "外观与全局行为",
  "settings.language.label": "语言",
  "settings.language.desc": "界面显示语言，切换后立即生效",
  "settings.language.system": "跟随系统",
  "settings.theme.label": "主题模式",
  "settings.theme.desc": "跟随系统、深色或浅色",
  "settings.theme.system": "跟随系统",
  "settings.theme.dark": "深色",
  "settings.theme.light": "浅色",
  "settings.autoBackup.label": "自动备份",
  "settings.autoBackup.desc": "存档变化或游戏退出时自动备份所有已安装游戏",
  "settings.autoScan.label": "自动扫描",
  "settings.autoScan.desc": "启动时或按间隔扫描本机游戏",
  "settings.autoScan.off": "关闭",
  "settings.autoScan.startup": "每次启动",
  "settings.autostart.label": "开机自动启动",
  "settings.autostart.desc": "随 Windows 登录自动运行",

  "settings.extensions.title": "扩展",
  "settings.extensions.desc": "游戏图标：SteamGridDB 选图、封面补全与本地缓存",
  "settings.extensions.sgdbKey.label": "SteamGridDB API Key",
  "settings.extensions.sgdbKey.desc":
    "用于非 Steam 游戏的图标搜索、手动选图与批量补全。Key 只保存在本机 settings.json，仅作为 Bearer 头发送给 SteamGridDB。",
  "settings.extensions.sgdbKey.placeholder": "粘贴 SteamGridDB API Key",
  "settings.extensions.sgdbKey.get": "获取 Key",
  "settings.extensions.sgdbKey.hint": "在 SteamGridDB 个人设置页申请 API Key 后填入，点“保存”生效。",
  "settings.extensions.cache.label": "图标缓存",
  "settings.extensions.cache.desc":
    "缓存 Steam 封面与程序图标。清理后，下次打开列表会重新获取；手动选择的图标会保留。",
  "settings.extensions.cache.clear": "清理图标缓存",
  "settings.extensions.tip":
    "存档与修改器列表右上角提供「补全图标」，只处理还没有本地封面的游戏；单个游戏也可以在行右键菜单中手动选图。",
};

const baseEn: Dict = {
  "nav.saves": "Saves",
  "nav.trainers": "Trainers",
  "nav.settings": "Settings",
  "nav.mods": "MODs",
  "app.name": "Game Manage Center",

  "saves.title": "Saves",
  "saves.subtitle": "Back up, restore and track save files of your local games",
  "trainers.title": "Trainers",
  "trainers.subtitle":
    "One-click download and install for single-player game trainers — matched against your local game library, with more sources on the way",

  "common.save": "Save",
  "common.cancel": "Cancel",
  "common.close": "Close",
  "common.refresh": "Refresh",

  "settings.title": "Settings",
  "settings.tab.general": "General",
  "settings.tab.saves": "Saves",
  "settings.tab.trainers": "Trainers",
  "settings.tab.extensions": "Extensions",
  "settings.tab.mods": "MODs",
  "mods.title": "MODs",
  "mods.subtitle": "Install, version management and conflict checks · planned",
  "mods.planned.title": "Planned",
  "mods.planned.desc": "MOD install, version management and conflict checks are being designed and will ship in a later release.",
  "settings.mods.title": "MODs",
  "settings.mods.desc": "MOD folder and loading options",
  "settings.mods.planned": "Planned: MOD folder location, load order and conflict warnings will live here.",
  "settings.tab.about": "About",

  "settings.general.title": "General",
  "settings.general.desc": "Appearance and global behaviour",
  "settings.language.label": "Language",
  "settings.language.desc": "Interface language, applied immediately",
  "settings.language.system": "Follow system",
  "settings.theme.label": "Theme",
  "settings.theme.desc": "Follow system, dark or light",
  "settings.theme.system": "Follow system",
  "settings.theme.dark": "Dark",
  "settings.theme.light": "Light",
  "settings.autoBackup.label": "Automatic backup",
  "settings.autoBackup.desc":
    "Back up all installed games when save files change or a game exits",
  "settings.autoScan.label": "Automatic scan",
  "settings.autoScan.desc": "Scan local games at startup or on an interval",
  "settings.autoScan.off": "Off",
  "settings.autoScan.startup": "Every launch",
  "settings.autostart.label": "Launch at startup",
  "settings.autostart.desc": "Start automatically when you sign in to Windows",

  "settings.extensions.title": "Extensions",
  "settings.extensions.desc": "Game artwork: SteamGridDB picking, cover filling and local cache",
  "settings.extensions.sgdbKey.label": "SteamGridDB API Key",
  "settings.extensions.sgdbKey.desc":
    "Used for artwork search, manual picking and batch filling of non-Steam games. The key is stored only in your local settings.json and sent to SteamGridDB as a Bearer token.",
  "settings.extensions.sgdbKey.placeholder": "Paste your SteamGridDB API Key",
  "settings.extensions.sgdbKey.get": "Get a key",
  "settings.extensions.sgdbKey.hint":
    "Create an API key in your SteamGridDB profile settings, paste it here and press Save.",
  "settings.extensions.cache.label": "Artwork cache",
  "settings.extensions.cache.desc":
    "Caches Steam covers and executable icons. After clearing, the next list load fetches them again; artwork you picked manually is kept.",
  "settings.extensions.cache.clear": "Clear artwork cache",
  "settings.extensions.tip":
    "Both the Saves and Trainers lists have a “Fill artwork” action in the top-right corner that only handles games without a cached cover. You can also pick one manually from a game's context menu.",
};

/** 各页面按模块拆分字典，新增文案往对应文件里加即可 */
const zh: Dict = { ...baseZh, ...savesZh, ...trainersZh, ...dialogsZh, ...settingsZh };
const en: Dict = { ...baseEn, ...savesEn, ...trainersEn, ...dialogsEn, ...settingsEn };

/** 繁体：以简体为底再覆盖转换结果，漏转的词条会自动回退到简体，不会显示键名 */
const zhHant: Dict = { ...zh, ...zhHantLocale };

const DICTS: Record<Lang, Dict> = { zh, zhHant, en };

function readStored(): LocaleSetting {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    if (value === "zh" || value === "zhHant" || value === "en" || value === "system") return value;
  } catch {
    // localStorage 不可用时按跟随系统处理
  }
  return "system";
}

let current: LocaleSetting = readStored();
const listeners = new Set<() => void>();

export function getLocaleSetting(): LocaleSetting {
  return current;
}

export function setLocaleSetting(value: LocaleSetting): void {
  if (value === current) return;
  current = value;
  try {
    localStorage.setItem(STORAGE_KEY, value);
  } catch {
    // 忽略写入失败：本次会话内仍然生效
  }
  listeners.forEach((fn) => fn());
}

function subscribe(callback: () => void): () => void {
  listeners.add(callback);
  return () => listeners.delete(callback);
}

/** 把设置值解析成实际语言；system 用浏览器语言判断（zh-TW / zh-HK / zh-Hant 走繁体） */
export function resolveLang(setting: LocaleSetting = current): Lang {
  if (setting === "zh" || setting === "zhHant" || setting === "en") return setting;
  const nav = typeof navigator === "undefined" ? "zh" : navigator.language || "zh";
  const lower = nav.toLowerCase();
  if (lower.startsWith("zh")) {
    return /hant|tw|hk|mo/.test(lower) ? "zhHant" : "zh";
  }
  return "en";
}

/** 日期/数字格式化用的 BCP-47 标签：繁体模式用 zh-TW，别让日期还是简体格式 */
export function localeTag(lang: Lang = resolveLang()): string {
  if (lang === "en") return "en-US";
  return lang === "zhHant" ? "zh-TW" : "zh-CN";
}

function format(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (match, name) =>
    name in vars ? String(vars[name]) : match,
  );
}

export function translate(
  key: string,
  vars?: Record<string, string | number>,
  lang: Lang = resolveLang(),
): string {
  const primary = DICTS[lang][key];
  if (primary !== undefined) return format(primary, vars);
  const fallback = zh[key];
  if (fallback !== undefined) return format(fallback, vars);
  return key;
}

const RUST_TABLES: Record<string, { table: Record<string, string>; keys: string[] }> = {
  zhHant: {
    table: rustZhHant,
    keys: Object.keys(rustZhHant).sort((a, b) => b.length - a.length),
  },
  en: {
    table: rustEn,
    keys: Object.keys(rustEn).sort((a, b) => b.length - a.length),
  },
};

/**
 * 后端错误文案：按当前语言查表转换。
 * Rust 的错误是硬编码简体中文（"恢复失败：..."），静态部分能翻，动态拼接的部分原样保留。
 * 未登记的错误（日志类、测试断言）原样返回，不会显示成键名。
 */
export function errText(error: unknown): string {
  const raw =
    typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
  const lang = resolveLang();
  const entry = RUST_TABLES[lang];
  if (!entry) return raw;
  const exact = entry.table[raw];
  if (exact) return exact;
  for (const key of entry.keys) {
    if (key.length >= 3 && raw.startsWith(key)) {
      return entry.table[key] + raw.slice(key.length);
    }
  }
  return raw;
}

export function useLocaleSetting(): LocaleSetting {
  return useSyncExternalStore(subscribe, () => current, () => current);
}

/**
 * 游戏名的主副两行。
 *
 * 回退规则（戢翎 2026-11-02 定）：
 *   简体模式：name_zh → name_zh_hant → name_en
 *   繁体模式：name_zh_hant → name_en（**不回退简体**，宁可显示英文）
 *   英文模式：name_en
 *
 * 副标题：主名不是英文原名时，把英文原名放副标题；主名就是英文时不显示副标题
 * （英文界面完全不显示副标题，戢翎 2026-10-28 要求）。
 */
export function gameNamePair(
  title: string,
  zhName: string | null | undefined,
  lang: Lang,
  zhHantName?: string | null,
): { main: string; sub: string | null } {
  const en = (title ?? "").trim();
  const zh = (zhName ?? "").trim();
  const hant = (zhHantName ?? "").trim();

  if (lang === "en") {
    return { main: en || zh || hant, sub: null };
  }

  const main = lang === "zhHant" ? hant || en : zh || hant || en;
  if (!main || main.toLowerCase() === en.toLowerCase()) {
    return { main: main || en, sub: null };
  }
  return { main, sub: en };
}

/** 只要显示名时的简写 */
export function gameName(
  title: string,
  zhName: string | null | undefined,
  lang: Lang,
): string {
  return gameNamePair(title, zhName, lang).main;
}

export function useI18n(): {
  lang: Lang;
  setting: LocaleSetting;
  t: (key: string, vars?: Record<string, string | number>) => string;
} {
  const setting = useLocaleSetting();
  const lang = resolveLang(setting);
  return {
    lang,
    setting,
    t: (key, vars) => translate(key, vars, lang),
  };
}
