/**
 * 修改器显示名规则（戢翎 2026-11-03 定）：
 *
 *   1. 默认用**来源原名**（风灵是英文、小幸本身就是中文）
 *   2. 如果原名已经是目标语言（含汉字的原名在中文界面），直接用
 *   3. 如果原名不是目标语言，用**匹配到的游戏库官方名**兜底
 *   4. 兜底也没有，就还是原名
 *
 * 和游戏名的规则不同：游戏名"中文优先、英文作副标题"，
 * 修改器反过来——原名是主体，译文只是兜底，所以副标题放原名。
 */
import type { Lang } from "@/i18n";

const CJK = /[\u4e00-\u9fff]/;

export interface TrainerNameSource {
  /** 来源站点的原名 */
  gameName: string;
  /** 游戏库官方简体名 */
  zhName?: string | null;
  /** 游戏库官方繁体名 */
  zhHant?: string | null;
  /** 匹配到的游戏库官方英文名 */
  officialName?: string | null;
  /** 已安装列表回填的官方名（manifest 没有 officialName，用这两个字段） */
  zhNameResolved?: string | null;
  zhHantResolved?: string | null;
}

export function trainerNameParts(
  hit: TrainerNameSource,
  lang: Lang,
): { main: string; sub: string | null } {
  const src = (hit.gameName ?? "").trim();
  const isCJK = CJK.test(src);

  let main = src;
  const zhS = (hit.zhName ?? hit.zhNameResolved ?? "").trim();
  const zhT = (hit.zhHant ?? hit.zhHantResolved ?? "").trim();
  const official = (hit.officialName ?? "").trim();

  if (lang === "en") {
    // 原名是中文（小幸等来源）→ 换成官方英文名
    if (isCJK) main = official || src;
  } else if (lang === "zhHant") {
    // 原名是英文 → 官方繁体优先，其次简体，最后原名
    if (!isCJK) main = zhT || zhS || src;
  } else {
    if (!isCJK) main = zhS || src;
  }

  main = main || src;
  // 主名被翻译过 → 原名作副标题；否则不显示副标题
  return { main, sub: main !== src ? src : null };
}
