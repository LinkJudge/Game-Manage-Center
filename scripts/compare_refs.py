"""比对旧参考项目副本与上游 clone，找出「上游没有的文件」（即我们写入的痕迹）。"""

from pathlib import Path

BASE = Path(r"C:\Users\joeyl\Desktop")
OLD = BASE / ".ref-trash-20261102-154513"
NEW = BASE / "Project：Game-Manage-Center" / "Reference Projects"


def files(root: Path) -> set[str]:
    out = set()
    for p in root.rglob("*"):
        if ".git" in p.parts:
            continue
        if p.is_file():
            out.add(p.relative_to(root).as_posix())
    return out


for name in ("Game-Cheats-Manager", "Game-Save-Manager"):
    old_dir, new_dir = OLD / name, NEW / name
    if not old_dir.exists():
        print(f"{name}: 备份不存在")
        continue
    o, n = files(old_dir), files(new_dir)
    extra = sorted(o - n)
    missing = sorted(n - o)
    print(f"===== {name} =====")
    print(f"  旧 {len(o)} 个文件 / 新 {len(n)} 个文件")
    print(f"  旧有但上游没有：{len(extra)} 个")
    for f in extra[:12]:
        print(f"     + {f}")
    print(f"  上游有但旧副本没有：{len(missing)} 个（说明新版更全）")
    for f in missing[:5]:
        print(f"     - {f}")
    print()
