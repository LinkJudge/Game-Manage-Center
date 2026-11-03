"""生成设置页页头的单色图标预览（浅色/深色两个主题并排）。

图标用内联 SVG 嵌入，currentColor 才会跟随主题。
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MONO = ROOT / "src" / "icons" / "mono"
OUT = ROOT / ".workbuddy" / "icon-render" / "mono-preview.html"

NAMES = ["settings", "save", "trainer", "mod", "extensions", "about"]


def block() -> str:
    parts = []
    for n in NAMES:
        svg = (MONO / f"{n}.svg").read_text(encoding="utf-8")
        parts.append(f'<div class="box">{svg}</div>')
    return "\n".join(parts)


HTML = """<!DOCTYPE html><html><head><meta charset="utf-8">
<style>
 body{{margin:0;font-family:system-ui,sans-serif;display:flex;flex-direction:column}}
 .pane{{padding:28px 24px;display:flex;gap:18px;flex-wrap:wrap}}
 .light{{background:#f7f8fa;color:#1d2127}}
 .dark{{background:#14171c;color:#e7e9ec}}
 .box{{width:76px;height:76px;border-radius:17px;display:grid;place-items:center;
      background:color-mix(in oklab, currentColor 5%, transparent);
      border:1px solid color-mix(in oklab, currentColor 10%, transparent)}}
 .box svg{{width:40px;height:40px}}
</style></head><body>
<div class="pane light">{body}</div>
<div class="pane dark">{body}</div>
</body></html>
"""

if __name__ == "__main__":
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(HTML.format(body=block()), encoding="utf-8")
    print(f"预览已生成：{OUT}")
