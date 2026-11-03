"""设置页六个分区页头的截图对照清单（戢翎要求彩色图标优先）。"""

ICONS = {
    "general": ("SlidersHorizontal（线性）", None),
    "saves": ("save.svg（彩色：蓝色软盘+绿色对勾）", "/icons/save.svg"),
    "trainers": ("trainer.svg（彩色）", "/icons/trainer.svg"),
    "mods": ("mod.svg（彩色）", "/icons/mod.svg"),
    "extensions": ("Blocks（线性）", None),
    "about": ("Info（线性）", None),
}
for tab, (desc, src) in ICONS.items():
    print(f"{tab:12s} {desc:44s} iconSrc={src}")
