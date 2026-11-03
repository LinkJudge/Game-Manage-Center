#!/usr/bin/env python3
"""把 Rust 侧的中文字面量转成繁体，生成前端可用的映射表。

为什么这么做：后端错误是硬编码的中文（`format!("恢复失败：{e}")`），直接进 Toast 的
description。与其把 200+ 处错误全改成错误码（改动面太大、容易漏），不如在前端展示时
按映射表转一次：静态部分能转，动态拼接部分（`{e}`）原样保留。

生成：python scripts/i18n/gen_rust_messages.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

from zhconv import convert

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "src" / "locales" / "rustMessages.ts"
EN_JSON = Path(__file__).resolve().parent / "rust_en.json"

CJK = re.compile(r"[\u4e00-\u9fff]")
LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"')

TERMS: list[tuple[str, str]] = [
    ("设置", "設定"), ("默认", "預設"), ("文件夾", "資料夾"), ("文件夹", "資料夾"),
    ("文件", "檔案"), ("网络", "網路"), ("支持", "支援"), ("信息", "資訊"),
    ("用户", "使用者"), ("登录", "登入"), ("注销", "登出"), ("内存", "記憶體"),
    ("硬盘", "硬碟"), ("服务器", "伺服器"), ("在线", "線上"), ("加载", "載入"),
    ("保存", "儲存"), ("搜索", "搜尋"), ("刷新", "重新整理"), ("备份", "備份"),
    ("恢复", "還原"), ("复制", "複製"), ("粘贴", "貼上"), ("列表", "清單"),
    ("程序", "程式"), ("运行", "執行"), ("界面", "介面"), ("质量", "品質"),
    ("优化", "最佳化"), ("兼容", "相容"), ("字体", "字型"), ("窗口", "視窗"),
    ("高级", "進階"), ("扩展", "擴充"), ("卸载", "解除安裝"), ("添加", "新增"),
    ("当前", "目前"), ("路径", "路徑"), ("目录", "目錄"), ("进度", "進度"),
]


def to_hant(text: str) -> str:
    out = convert(text, "zh-tw")
    for simp, trad in TERMS:
        if simp in text:
            out = out.replace(convert(simp, "zh-tw"), trad)
    return out


def main() -> int:
    found: dict[str, str] = {}
    for path in sorted((ROOT / "src-tauri").rglob("*.rs")):
        if "target" in path.parts:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for match in LITERAL.finditer(text):
            value = match.group(1)
            if len(value) < 2 or not CJK.search(value) or value in found:
                continue
            found[value] = to_hant(value)

    lines = [
        "/**",
        " * Rust 侧中文字面量 → 繁体中文 —— **自动生成，请勿手改**。",
        " * 生成：python scripts/i18n/gen_rust_messages.py",
        " * 用途：后端错误直接进 Toast 的 description，前端展示前按这张表转一次。",
        " */",
        "",
        "export const rustZhHant: Record<string, string> = {",
    ]
    for simp, hant in found.items():
        lines.append(f"  {json.dumps(simp, ensure_ascii=False)}: {json.dumps(hant, ensure_ascii=False)},")
    lines.append("};")
    lines.append("")

    # 英文：人工翻译表（未登记的条目在英文模式下原样显示中文）
    import json as _json

    manual = _json.loads(EN_JSON.read_text(encoding="utf-8")) if EN_JSON.exists() else {}
    manual.pop("_comment", None)
    lines.append("/** Rust 错误文案的英文翻译（人工维护，见 scripts/i18n/rust_en.json） */")
    lines.append("export const rustEn: Record<string, string> = {")
    missing = []
    for simp in found:
        en = manual.get(simp)
        if en is None:
            missing.append(simp)
            continue
        lines.append(f"  {_json.dumps(simp, ensure_ascii=False)}: {_json.dumps(en, ensure_ascii=False)},")
    lines.append("};")
    lines.append("")

    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"繁体 {len(found)} 条；英文已翻译 {len(found) - len(missing)} 条，缺 {len(missing)} 条")
    print(f"已写出 {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
