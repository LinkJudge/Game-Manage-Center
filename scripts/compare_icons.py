"""对比新旧打包图标：同尺寸并排放大，量化边缘锐度与留白比例。"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
OLD = ROOT / ".workbuddy" / "icon-backup" / "tauri-icons-old"
NEW = ROOT / "src-tauri" / "icons"
OUT = ROOT / ".workbuddy" / "icon-render"

SIZES = [32, 128]
SCALE = 4  # 放大倍数，便于肉眼看清边缘


def load(root: Path, size: int) -> Image.Image | None:
    name = {32: "32x32.png", 128: "128x128.png"}[size]
    p = root / name
    if not p.exists():
        return None
    return Image.open(p).convert("RGBA")


def pad_ratio(im: Image.Image) -> float:
    """图标本体（非全透明）占画布的比例，用来看留白是否过多。"""
    alpha = im.getchannel("A")
    box = alpha.getbbox()
    if not box:
        return 0.0
    w = box[2] - box[0]
    return w / im.width


def edge_sharpness(im: Image.Image) -> float:
    """灰度梯度均值，越大边缘越锐（相对比较用）。"""
    g = im.convert("L")
    px = g.load()
    w, h = g.size
    total, count = 0, 0
    for y in range(h):
        for x in range(w - 1):
            total += abs(px[x, y] - px[x + 1, y])
            count += 1
    return total / count if count else 0.0


def main() -> None:
    for size in SIZES:
        old, new = load(OLD, size), load(NEW, size)
        if not old or not new:
            print(f"{size}: 缺文件，跳过")
            continue
        print(f"=== {size}×{size} ===")
        print(f"  实体占比  旧 {pad_ratio(old)*100:.1f}%  新 {pad_ratio(new)*100:.1f}%")
        print(f"  边缘梯度  旧 {edge_sharpness(old):.2f}   新 {edge_sharpness(new):.2f}")

        # 并排：左旧右新，各自最近邻放大
        big = size * SCALE
        canvas = Image.new("RGBA", (big * 2 + 24, big), (255, 255, 255, 255))
        canvas.paste(old.resize((big, big), Image.NEAREST), (0, 0), old.resize((big, big), Image.NEAREST))
        canvas.paste(new.resize((big, big), Image.NEAREST), (big + 24, 0), new.resize((big, big), Image.NEAREST))
        out = OUT / f"compare-{size}.png"
        canvas.save(out)
        print(f"  对比图（左旧 / 右新）: {out}")
        print()


if __name__ == "__main__":
    main()
