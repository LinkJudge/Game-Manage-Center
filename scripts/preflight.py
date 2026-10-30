#!/usr/bin/env python3
"""上传前合规自检。

只检查**会被 git 跟踪的文件**（untracked / ignored 的不上传，不用管），
外加 git 历史里的凭据泄露。

检查项：
  1. 硬编码凭据（GitHub token / OpenAI key / AWS key / 私钥）
  2. 凭据类文件名（.env / secret / *.pem / *.key）
  3. 硬编码的个人路径（C:\\Users\\<name>）
  4. 疑似违规措辞（破解 / 绕过授权 / 外挂）
  5. 超过 GitHub 建议阈值的文件（>50 MB 告警，>100 MB 会被拒）
  6. git 历史里是否出现过凭据（-S 搜索）

用法：python scripts/preflight.py
退出码：0 = 通过（可能有告警）；1 = 有必须处理的问题
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# (名称, 正则, 说明)
SECRET_PATTERNS = [
    ("GitHub Token", re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}"), "吊销并改用环境变量"),
    ("OpenAI Key", re.compile(r"sk-[A-Za-z0-9]{24,}"), "吊销并改用环境变量"),
    ("AWS Access Key", re.compile(r"AKIA[0-9A-Z]{16}"), "吊销"),
    ("Slack Token", re.compile(r"xox[baprs]-[A-Za-z0-9-]{10,}"), "吊销"),
    ("私钥", re.compile(r"-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY"), "删除并换 key"),
]

CREDENTIAL_FILES = re.compile(
    r"(^|/)(\.env(\..*)?|.*\.(pem|key|p12|pfx)|secret.*\.json|credential.*\.json)$",
    re.I,
)

# 单字母用户名（x / j）是单元测试里的占位符，真实用户名基本 ≥2 字符；
# Public / Default 是系统内置目录，都不算个人信息泄露
PRIVATE_PATH = re.compile(
    r"C:\\+Users\\+\\(?!(?:Public|Default|[A-Za-z])\\)[A-Za-z0-9_.-]+\\+", re.I
)

# 项目里允许出现的"敏感词"是正面声明（明确写了不绕过授权），
# 只报警那些像是在宣传违规用法的组合
BAD_PHRASES = [
    re.compile(r"破解版|破解补丁|PJ版"),
    re.compile(r"绕过.{0,6}(验证|授权|付费|检测)"),
    re.compile(r"外挂|作弊器下载"),
]

TEXT_EXT = {
    ".rs", ".ts", ".tsx", ".js", ".cjs", ".mjs", ".py", ".json", ".md",
    ".toml", ".yaml", ".yml", ".css", ".html", ".sh", ".bat", ".ps1",
}


def git(*args: str) -> str:
    out = subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    return out.stdout


def tracked_files() -> list[Path]:
    files = []
    for line in git("ls-files").splitlines():
        if line.strip():
            files.append(ROOT / line)
    return files


def main() -> int:
    errors: list[str] = []
    warnings: list[str] = []
    files = tracked_files()
    print(f"检查 {len(files)} 个受跟踪文件\n")

    # 1 & 2：凭据文件与内容
    big = []
    for path in files:
        rel = path.relative_to(ROOT).as_posix()
        if CREDENTIAL_FILES.search(rel):
            errors.append(f"[凭据文件] {rel} 不应入库")
            continue
        try:
            size = path.stat().st_size
        except OSError:
            continue
        if size > 100 * 1024 * 1024:
            errors.append(f"[超大文件] {rel}  {size / 1024 / 1024:.0f} MB > 100 MB，GitHub 会拒收")
        elif size > 50 * 1024 * 1024:
            big.append(f"[大文件] {rel}  {size / 1024 / 1024:.0f} MB（建议走 Release 附件）")

        if path.suffix.lower() not in TEXT_EXT:
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue

        for name, pattern, advice in SECRET_PATTERNS:
            if pattern.search(text):
                errors.append(f"[疑似{name}] {rel} —— {advice}")

        for match in PRIVATE_PATH.finditer(text):
            errors.append(f"[个人路径] {rel}  含 {match.group(0)}")
            break

        for phrase in BAD_PHRASES:
            found = phrase.search(text)
            if found:
                # 明确写了"不绕过/不实现"的属于正面声明，放过
                context = text[max(0, found.start() - 12): found.end() + 12]
                if "不" in context[:14] or "禁止" in context:
                    continue
                warnings.append(f"[措辞] {rel}  含「{found.group(0)}」，确认语境是否合适")

    warnings.extend(big)

    # 3：git 历史里的凭据
    for name, pattern, _ in SECRET_PATTERNS[:3]:
        # 用 -G 传正则；-S 是固定字符串，拿 "gh" 去搜会命中所有含 github 的提交
        hits = git("log", "--all", "-G", pattern.pattern, "--oneline").strip()
        if hits:
            errors.append(f"[历史] {name} 可能出现在提交中：{hits.splitlines()[0]}")

    # 输出
    if errors:
        print("必须处理：")
        for line in errors:
            print("  ✗ " + line)
        print()
    if warnings:
        print("需要确认：")
        for line in warnings:
            print("  ! " + line)
        print()
    if not errors and not warnings:
        print("✓ 未发现凭据、个人路径、超大文件或违规措辞。")

    print("\n提示：这些是自动能查的部分。下列内容需要人判断——")
    print("  · 第三方数据/素材的许可与署名（见 README「许可」章节）")
    print("  · 打包进安装包的第三方二进制（如 7-Zip）是否附了许可文本")
    print("  · 是否与所引用的网站/作者存在官方关联的误解")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
