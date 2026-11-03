"""从 Gemini 导出的图标工作台 HTML 里提取 7 枚 SVG，写成 public/icons/*.svg。

# 来源：设计稿 HTML（gemini-code-*.html，运行时用 --html 指定路径）
用法：python scripts/extract_icons.py [--html 路径]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "public" / "icons"
DEFAULT_HTML = Path("gemini-code.html")  # 运行时用 --html 指定设计稿 HTML

# HTML 里的 id → 落盘文件名
NAME_MAP = {
    "icon-main": "app.svg",
    "icon-save": "save.svg",
    "icon-trainer": "trainer.svg",
    "icon-mod": "mod.svg",
    "icon-settings": "settings.svg",
    "icon-ext": "extensions.svg",
    "icon-about": "about.svg",
}

SVG_BLOCK = re.compile(r'<svg id="(icon-[a-z]+)"[^>]*>(.*?)</svg>', re.DOTALL)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--html", default=str(DEFAULT_HTML))
    args = ap.parse_args()

    html = Path(args.html).read_text(encoding="utf-8")
    found = {m.group(1): m.group(0) for m in SVG_BLOCK.finditer(html)}
    print(f"从 HTML 提取到 {len(found)} 个 svg：{sorted(found)}")

    missing = [k for k in NAME_MAP if k not in found]
    if missing:
        print(f"[错误] 缺少：{missing}")
        return 1

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for svg_id, filename in NAME_MAP.items():
        svg = found[svg_id]
        # 去掉 id 属性，避免同页多份时冲突；补上 xmlns 与 viewBox 已存在
        svg = svg.replace(f'id="{svg_id}" ', "", 1).replace(f'id="{svg_id}"', "", 1)
        target = OUT_DIR / filename
        target.write_text(svg.strip() + "\n", encoding="utf-8")
        print(f"  {svg_id:14s} → {filename:16s} {len(svg):5d} 字节")

    return 0


if __name__ == "__main__":
    sys.exit(main())
