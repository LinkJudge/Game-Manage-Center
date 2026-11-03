/**
 * 设置页分区图标：把戢翎给的 7 枚彩色图标（settings/save/trainer/mod/
 * extensions/about）重画成的线性版本。
 *
 * 为什么内联 SVG 而不是 <img>：线条用 stroke="currentColor"，
 * 只有内联到 DOM 里才能跟着主题的 color 变（浅色深灰 / 深色浅灰）。
 * <img src="x.svg"> 里的 currentColor 不生效，会固定成黑色。
 *
 * 双版本：≤24px 用 small/（去内部细节、加粗线条），>24px 用完整版。
 * 依据：16/20px 下完整版的软盘徽标、扩展加号会糊（2026-11-03 对比实测）。
 */
import { Settings as SettingsGlyph } from "lucide-react";
import saveRaw from "@/icons/line/save.svg?raw";
import trainerRaw from "@/icons/line/trainer.svg?raw";
import modRaw from "@/icons/line/mod.svg?raw";
import extensionsRaw from "@/icons/line/extensions.svg?raw";
import aboutRaw from "@/icons/line/about.svg?raw";
import saveSmallRaw from "@/icons/line/small/save.svg?raw";
import trainerSmallRaw from "@/icons/line/small/trainer.svg?raw";
import modSmallRaw from "@/icons/line/small/mod.svg?raw";
import extensionsSmallRaw from "@/icons/line/small/extensions.svg?raw";
import aboutSmallRaw from "@/icons/line/small/about.svg?raw";

type IconName = "save" | "trainer" | "mod" | "extensions" | "about";

const RAW: Record<IconName, string> = {
  save: saveRaw,
  trainer: trainerRaw,
  mod: modRaw,
  extensions: extensionsRaw,
  about: aboutRaw,
};

const RAW_SMALL: Record<IconName, string> = {
  save: saveSmallRaw,
  trainer: trainerSmallRaw,
  mod: modSmallRaw,
  extensions: extensionsSmallRaw,
  about: aboutSmallRaw,
};

/** 完整版与简化版的分界：≤24px 走简化版（标签栏 18.4px），>24px 走完整版（页头 34px） */
const SMALL_MAX_PX = 24;

function make(name: IconName) {
  return function LineIcon({ className }: { className?: string }) {
    // className 里的尺寸类（如 w-[18.4px]）无法在 SSR 前解析成像素，
    // 这里用容器查询的替代方案：直接按 CSS 变量判断不可行，改为渲染两个版本，
    // 由 CSS 按容器实际宽度显示其一（span 相互排斥）。
    return (
      <span
        className={["gm-line-icon", className].filter(Boolean).join(" ")}
        dangerouslySetInnerHTML={{
          __html:
            `<span class="gm-line-full">${RAW[name]}</span>` +
            `<span class="gm-line-small">${RAW_SMALL[name]}</span>`,
        }}
      />
    );
  };
}

/** 齿轮直接用顶栏「设置」按钮同款的 lucide Settings，线宽压到 1.6 与其余手绘图标一致 */
export function LineSettings({ className }: { className?: string }) {
  return <SettingsGlyph className={className} strokeWidth={1.6} aria-hidden="true" />;
}
export const LineSave = make("save");
export const LineTrainer = make("trainer");
export const LineMod = make("mod");
export const LineExtensions = make("extensions");
export const LineAbout = make("about");
export { SMALL_MAX_PX };
