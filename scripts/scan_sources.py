"""列出每个版本代码里出现的远程域名与关键地址（用于核对数据源）。"""

import re
import subprocess
from collections import Counter
from urllib.parse import urlparse

TAGS = ["v0.1.0", "v0.2.0", "v0.3.0", "v0.4.0", "HEAD"]
URL = re.compile(rb"https?://[A-Za-z0-9./_?=&%#:-]+")


def scan(tag: str) -> Counter:
    cnt: Counter = Counter()
    out = subprocess.run(
        ["git", "grep", "-h", "-o", "-E", r"https?://[^\"' )<>]+", tag, "--",
         "*.rs", "*.ts", "*.tsx", "*.js", "*.cjs", "*.json", "*.toml"],
        capture_output=True,
    ).stdout
    for m in URL.finditer(out):
        url = m.group(0).decode("utf-8", "replace").rstrip(".,;)")
        host = urlparse(url).netloc
        if host:
            cnt[host] += 1
    return cnt


for tag in TAGS:
    cnt = scan(tag)
    print(f"===== {tag} =====")
    if not cnt:
        print("  （代码里没有远程地址）")
    for host, n in cnt.most_common(12):
        print(f"  {host:34s} {n}")
    print()
