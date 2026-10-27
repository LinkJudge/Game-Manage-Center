#!/usr/bin/env python3
"""构建额外修改器来源目录。

来源：
- FLiNG Trainer Archive：archive.flingtrainer.com 的公开旧档表，直接 RAR。
- 小幸修改器：xiaoxingjie.com WordPress 公开文章接口，提供官网/网盘页链接。

输出：
- src-tauri/resources/trainers/fling_archive.json
- src-tauri/resources/trainers/xiaoxing.json
"""

from __future__ import annotations

import hashlib
import html
import json
import re
import sys
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = ROOT / "src-tauri" / "resources" / "trainers"
CACHE_DIR = Path(__file__).resolve().parent / ".cache"
UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124 Safari/537.36"
ARCHIVE_URL = "https://archive.flingtrainer.com/download_chs.html"
XIAOXING_API = "https://www.xiaoxingjie.com/wp-json/wp/v2/posts?per_page=100&_fields=id,date,link,title,content"
XIAOXING_HOME = "https://www.xiaoxingjie.com"


def fetch(url: str, timeout: int = 120) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def cached(name: str, url: str) -> bytes:
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    path = CACHE_DIR / name
    if path.exists() and path.stat().st_size > 0:
        return path.read_bytes()
    data = fetch(url)
    path.write_bytes(data)
    return data


def strip_tags(text: str) -> str:
    return html.unescape(re.sub(r"<[^>]+>", "", text)).strip()


def parse_size(text: str) -> int | None:
    match = re.search(r"([\d.]+)\s*(KB|MB|GB)", text, re.I)
    if not match:
        return None
    value = float(match.group(1))
    unit = match.group(2).upper()
    factor = {"KB": 1024, "MB": 1024**2, "GB": 1024**3}[unit]
    return round(value * factor)


def slugify(text: str, suffix: str = "") -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    if not slug:
        slug = "entry"
    if suffix:
        slug = f"{slug}-{suffix}"
    return slug[:90]


def archive_catalog() -> dict:
    page = cached("fling-archive.html", ARCHIVE_URL).decode("utf-8", errors="ignore")
    rows = re.findall(
        r'<tr>\s*<td><a href="(files/[^"]+)"[^>]*>(.*?)</a></td>\s*<td>(.*?)</td>',
        page,
        re.S,
    )
    entries = []
    for href, raw_title, raw_size in rows:
        title = strip_tags(raw_title)
        cleaned = re.sub(
            r"\s+Trainer(?:\s+(?:Updated|Fixed).*)?$", "", title, flags=re.I
        ).strip()
        plus = re.search(r"\s+Plus\s+(\d+)\b", cleaned, re.I)
        before = cleaned[: plus.start()].strip() if plus else cleaned
        version_match = re.search(r"\s+(v\.?\d.*)$", before, re.I)
        if version_match:
            game_name = before[: version_match.start()].strip()
            game_version = version_match.group(1).strip()
        else:
            game_name = before
            game_version = None
        game_name = game_name.replace("_", ": ").strip()
        if not game_name:
            continue
        updated_match = re.search(r"Updated\s+(\d{4})\.(\d{1,2})\.(\d{1,2})", title, re.I)
        updated_at = (
            f"{updated_match.group(1)}-{int(updated_match.group(2)):02d}-{int(updated_match.group(3)):02d}"
            if updated_match
            else None
        )
        filename = urllib.parse.unquote(href.split("/", 1)[1])
        file_stem = re.sub(r"\.(rar|zip|7z|exe)$", "", filename, flags=re.I)
        digest = hashlib.sha1(filename.encode("utf-8")).hexdigest()[:8]
        slug = slugify(game_name, digest)
        url = urllib.parse.urljoin(ARCHIVE_URL, urllib.parse.quote(href))
        entries.append(
            {
                "id": f"fling_archive/{slug}",
                "slug": slug,
                "gameName": game_name,
                "zhName": None,
                "pageUrl": ARCHIVE_URL,
                "updatedAt": updated_at,
                "downloads": [
                    {
                        "file": file_stem,
                        "url": url,
                        "dateAdded": updated_at,
                        "sizeBytes": parse_size(raw_size),
                        "downloadCount": None,
                        "gameVersion": game_version,
                        "optionsCount": int(plus.group(1)) if plus else None,
                    }
                ],
            }
        )

    entries.sort(key=lambda item: item["gameName"].lower())
    if len(entries) < 300:
        raise RuntimeError(f"FLiNG 归档条目异常少：{len(entries)}")
    return {
        "schema": "gm.trainers/1",
        "generatedAt": datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z"),
        "source": {
            "id": "fling_archive",
            "title": "FLiNG Trainer Archive",
            "homepage": "https://archive.flingtrainer.com",
            "listUrl": ARCHIVE_URL,
        },
        "stats": {"count": len(entries)}, 
        "trainers": entries,
    }


def choose_names(title: str) -> tuple[str, str | None, str | None]:
    parts = [part.strip() for part in re.split(r"\s*/\s*", title) if part.strip()]
    version = None
    match = re.search(r"(?:v(?:ersion)?\s*)(\d+(?:\.\d+)+)", title, re.I)
    if match:
        version = match.group(1)

    chinese = None
    english = None
    for part in parts:
        has_latin = bool(re.search(r"[A-Za-z]{3,}", part))
        has_cjk = bool(re.search(r"[\u3400-\u9fff]", part))
        cleaned = re.sub(
            r"(多功能修改器|全功能修改器|全版本?[^\s]*?项修改器|[一二三四五六七八九十百零]+项修改器|"
            r"辅助器|Trainer|English\s+Version|チート)",
            " ",
            part,
            flags=re.I,
        )
        cleaned = re.sub(r"(?:v(?:ersion)?\s*)\d+(?:\.\d+)+", " ", cleaned, flags=re.I)
        cleaned = re.sub(r"\s+\d+(?:\.\d+)+\s*$", " ", cleaned)
        cleaned = re.sub(r"\s+", " ", cleaned).strip(" -_/")
        cleaned = re.sub(r"^[\d一二三四五六七八九十百零]+项\s*", "", cleaned)
        if not cleaned:
            continue
        if has_cjk and chinese is None:
            chinese = cleaned
        if has_latin and not has_cjk and english is None:
            english = cleaned
        elif has_latin and has_cjk and english is None:
            # “轩辕剑柒 / 轩辕剑7 多功能修改器 / Xuan-Yuan...” 这类只在纯英文段取名
            continue

    if english:
        return english, chinese, version
    if chinese:
        return chinese, chinese, version
    fallback = re.sub(r"Trainer|修改器|辅助器", " ", title, flags=re.I)
    fallback = re.sub(r"\s+", " ", fallback).strip()
    return fallback, None, version


def xiaoxing_catalog() -> dict:
    posts = json.loads(cached("xiaoxing-posts.json", XIAOXING_API).decode("utf-8"))
    entries = []
    download_hosts = ("pan.baidu.com", "mediafire.com", "1drv.ms", "share.weiyun.com", "vdisk.weibo.com")
    for post in posts:
        title = strip_tags(post["title"]["rendered"])
        if not re.search(r"修改器|Trainer|チート", title, re.I):
            continue
        links = [
            html.unescape(link)
            for link in re.findall(r'href=["\']([^"\']+)', post["content"]["rendered"])
        ]
        if not any(any(host in link for host in download_hosts) for link in links):
            continue

        game_name, zh_name, version = choose_names(title)
        if not game_name:
            continue
        date = post["date"][:10]
        entries.append(
            {
                "id": f"xiaoxing/post-{post['id']}",
                "slug": f"post-{post['id']}",
                "gameName": game_name,
                "zhName": zh_name,
                "pageUrl": post["link"],
                "updatedAt": date,
                "downloads": [
                    {
                        "file": title,
                        "url": post["link"],
                        "dateAdded": date,
                        "sizeBytes": None,
                        "downloadCount": None,
                        "gameVersion": version,
                        "optionsCount": None,
                        "external": True,
                    }
                ],
            }
        )

    entries.sort(key=lambda item: item["gameName"].lower())
    if not entries:
        raise RuntimeError("小幸修改器没有解析到条目")
    return {
        "schema": "gm.trainers/1",
        "generatedAt": datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z"),
        "source": {
            "id": "xiaoxing",
            "title": "XiaoXing Trainer",
            "homepage": XIAOXING_HOME,
            "listUrl": XIAOXING_HOME,
        },
        "stats": {"count": len(entries)},
        "trainers": entries,
    }


def write(name: str, data: dict) -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    path = OUT_DIR / name
    path.write_text(
        json.dumps(data, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    print(f"{name}: {len(data['trainers'])} 条 -> {path}")


def main() -> int:
    only = sys.argv[1] if len(sys.argv) > 1 else "all"
    if only in ("all", "archive"):
        write("fling_archive.json", archive_catalog())
    if only in ("all", "xiaoxing"):
        write("xiaoxing.json", xiaoxing_catalog())
    return 0


if __name__ == "__main__":
    sys.exit(main())
