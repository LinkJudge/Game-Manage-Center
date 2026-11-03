#!/usr/bin/env python3
"""从简体词条生成繁体词条，输出 src/locales/zhHant.ts。

为什么用生成而不是手写两份：
  手写两份必然漂移（改一处忘一处），而 UI 文案还会继续增加。
  转换器（zhconv zh-tw）+ 术语修正表能保证"新增的简体词条自动有繁体"。

术语修正：zhconv 是逐词转换，港台习惯用词它管不了（它把「软件」转成「軟體」是对的，
但「设置」会转成「設置」而不是「設定」，「文件」还是「文件」而不是「檔案」）。
所以转换后再过一遍术语表。

用法：python scripts/i18n/gen_zh_hant.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

from zhconv import convert

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "src" / "locales" / "zhHant.ts"
SOURCES = [
    ROOT / "src" / "i18n.tsx",
    ROOT / "src" / "locales" / "saves.ts",
    ROOT / "src" / "locales" / "trainers.ts",
    ROOT / "src" / "locales" / "dialogs.ts",
    ROOT / "src" / "locales" / "settings.ts",
]

# 台湾/香港习惯用词（转换后替换；顺序：长的在前，避免被短的截断）
TERMS: list[tuple[str, str]] = [
    ("设置", "設定"), ("默认", "預設"), ("文件夾", "資料夾"), ("文件夹", "資料夾"),
    ("文件", "檔案"), ("网络", "網路"), ("支持", "支援"), ("信息", "資訊"),
    ("视频", "影片"), ("音频", "音訊"), ("用户", "使用者"), ("登录", "登入"),
    ("注销", "登出"), ("鼠标", "滑鼠"), ("键盘", "鍵盤"), ("内存", "記憶體"),
    ("硬盘", "硬碟"), ("服务器", "伺服器"), ("在线", "線上"), ("加载", "載入"),
    ("保存", "儲存"), ("搜索", "搜尋"), ("查找", "尋找"), ("刷新", "重新整理"),
    ("覆盖", "覆寫"), ("备份", "備份"), ("恢复", "還原"), ("复制", "複製"),
    ("粘贴", "貼上"), ("导出", "匯出"), ("导入", "匯入"), ("列表", "清單"),
    ("程序", "程式"), ("运行", "執行"), ("界面", "介面"), ("质量", "品質"),
    ("优化", "最佳化"), ("兼容", "相容"), ("选项卡", "分頁"), ("字体", "字型"),
    ("窗口", "視窗"), ("高级", "進階"), ("扩展", "擴充"), ("卸载", "解除安裝"),
    ("打印", "列印"), ("添加", "新增"), ("当前", "目前"), ("更新", "更新"),
    ("删除", "刪除"), ("路径", "路徑"), ("目录", "目錄"), ("进度", "進度"),
]

ENTRY = re.compile(r'^\s*"([A-Za-z0-9_.\-]+)":\s*"((?:[^"\\]|\\.)*)"\s*,?\s*$')


def extract(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        m = ENTRY.match(line)
        if not m:
            continue
        key, value = m.group(1), m.group(2)
        # 跳过英文词条所在文件里的 en 段：靠内容判断（含汉字才算中文词条）
        if not re.search(r"[\u4e00-\u9fff]", value):
            continue
        out[key] = value
    return out


def to_hant(text: str) -> str:
    converted = convert(text, "zh-tw")
    for simp, trad in TERMS:
        if simp in text:  # 只对原文里确实出现的词做替换，避免误伤
            converted = converted.replace(convert(simp, "zh-tw"), trad)
    return converted


def main() -> int:
    merged: dict[str, str] = {}
    for path in SOURCES:
        if not path.exists():
            print(f"[跳过] {path.name} 不存在")
            continue
        found = extract(path)
        print(f"  {path.name:14s} {len(found)} 条")
        merged.update(found)

    if not merged:
        print("[错误] 没提取到任何词条")
        return 1

    lines = [
        "/**",
        " * 繁体中文词条 —— **自动生成，请勿手改**。",
        " * 生成：python scripts/i18n/gen_zh_hant.py",
        " * 来源：src/i18n.tsx 与 src/locales/*.ts 里的简体词条，经 zhconv(zh-tw) + 术语修正表转换。",
        " */",
        "",
        "export const zhHant: Record<string, string> = {",
    ]
    changed = 0
    for key, value in merged.items():
        hant = to_hant(value)
        if hant != value:
            changed += 1
        lines.append(f'  "{key}": {json.dumps(hant, ensure_ascii=False)},')
    lines.append("};")
    lines.append("")

    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"\n共 {len(merged)} 条，其中 {changed} 条与简体不同")
    print(f"已写出 {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
