"""把彩色图标转成「单色线稿」版本，供设置页页头用（随主题变色）。

做法：丢掉渐变/阴影定义，所有图形改成 fill:none + stroke:currentColor，
保留原有线宽与轮廓。这样图标就变成一条线稿，颜色由 CSS 的 color 决定，
浅色主题是深灰、深色主题是浅灰。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "public" / "icons"
OUT = ROOT / "src" / "icons" / "mono"

NAMES = ["settings", "save", "trainer", "mod", "extensions", "about"]

TAG_ATTRS = re.compile(r'(fill|stroke)="[^"]*"')
TAG_OPACITY = re.compile(r'\s*opacity="[^"]*"')


def to_mono(svg: str) -> str:
    # 去掉 defs（渐变、滤镜不再需要）
    svg = re.sub(r"<defs>.*?</defs>", "", svg, flags=re.DOTALL)
    # 去掉 filter 引用
    svg = re.sub(r'\sfilter="[^"]*"', "", svg)
    # 去掉透明度（线稿统一实色）
    svg = TAG_OPACITY.sub("", svg)

    def fix(m: re.Match[str]) -> str:
        return 'fill="none" stroke="currentColor"'

    svg = TAG_ATTRS.sub(fix, svg)
    return svg


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    for name in NAMES:
        src = SRC / f"{name}.svg"
        if not src.exists():
            print(f"[跳过] {src.name} 不存在")
            continue
        mono = to_mono(src.read_text(encoding="utf-8"))
        # 顶部分区没描边的地方补一个 stroke（保险）
        (OUT / f"{name}.svg").write_text(mono.strip() + "\n", encoding="utf-8")
        print(f"  {name}.svg → src/icons/mono/{name}.svg")
    return 0


if __name__ == "__main__":
    sys.exit(main())
