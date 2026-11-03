"""扫描前端里没走 i18n 的中文字面量。

判断依据：字符串/JSX 文本里含汉字，且不是 `t("...")` 的参数、不在 locales/i18n 文件里。
输出按文件分组，供逐个补词条。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "src"
SKIP = {"locales", "i18n.tsx"}  # 以及 lib/actionCopy.ts（那是翻译表本身）
SKIP_FILES = {"src/lib/actionCopy.ts"}
CJK = re.compile(r"[\u4e00-\u9fff]")
LITERAL = re.compile(r"""(?<![A-Za-z_])["'`]([^"'`\n]*[\u4e00-\u9fff][^"'`\n]*)["'`]""")
JSX_TEXT = re.compile(r">\s*([^<>{}\n]*[\u4e00-\u9fff][^<>{}\n]*)\s*<")
COMMENT = re.compile(r"^\s*(//|/\*|\*|\{/\*)")


def main() -> int:
    hits: dict[str, list[tuple[int, str]]] = {}
    for path in sorted(ROOT.rglob("*")):
        if path.suffix not in (".tsx", ".ts"):
            continue
        if path.parent.name in SKIP or path.name in SKIP:
            continue
        rel = path.relative_to(ROOT.parent).as_posix()
        if rel in SKIP_FILES:
            continue
        for no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if COMMENT.match(line):
                continue
            found = []
            for m in LITERAL.finditer(line):
                text = m.group(1)
                # t("key") 的键名不含汉字，这里自然是排除的
                found.append(text.strip())
            for m in JSX_TEXT.finditer(line):
                found.append(m.group(1).strip())
            found = [f for f in found if CJK.search(f) and len(f) > 1]
            if found:
                hits.setdefault(rel, []).extend((no, f) for f in found)

    total = sum(len(v) for v in hits.values())
    print(f"发现 {total} 处未走 i18n 的中文字面量，分布在 {len(hits)} 个文件\n")
    for rel, items in hits.items():
        print(f"--- {rel}")
        for no, text in items:
            print(f"  {no:5d}  {text}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
