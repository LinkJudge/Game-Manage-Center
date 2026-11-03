"""把 src/icons/line/*.svg 渲染成一张预览图（浅色/深色两行），存到 .workbuddy/icon-render/。"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src" / "icons" / "line"
OUT = ROOT / ".workbuddy" / "icon-render" / "line-preview.html"
NAMES = ["settings", "save", "trainer", "mod", "extensions", "about"]
LABELS = ["通用", "存档", "修改器", "MOD", "扩展", "关于"]

CSS = """
 body{margin:0;font-family:"Microsoft YaHei",system-ui,sans-serif}
 .row{display:flex;gap:16px;padding:22px 26px;align-items:center}
 .light{background:#f6f7f9;color:#1f2328}
 .dark{background:#14171c;color:#e7e9ec}
 .item{display:flex;flex-direction:column;align-items:center;gap:6px;width:80px}
 .box{width:64px;height:64px;border-radius:15px;display:grid;place-items:center;
      background:color-mix(in oklab, currentColor 5%, transparent);
      border:1px solid color-mix(in oklab, currentColor 10%, transparent)}
 .box svg{width:34px;height:34px}
 .lbl{font-size:11px;opacity:.62}
"""


def row() -> str:
    out = []
    for name, label in zip(NAMES, LABELS):
        svg = (SRC / f"{name}.svg").read_text(encoding="utf-8")
        out.append(f'<div class="item"><div class="box">{svg}</div><span class="lbl">{label}</span></div>')
    return "\n".join(out)


if __name__ == "__main__":
    body = row()
    html = (
        "<!DOCTYPE html><html><head><meta charset='utf-8'><style>"
        + CSS
        + "</style></head><body>"
        + f'<div class="row light">{body}</div>'
        + f'<div class="row dark">{body}</div>'
        + "</body></html>"
    )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(html, encoding="utf-8")
    print(f"预览：{OUT}")
