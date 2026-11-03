"""按戢翎给的 7 枚彩色图标的构图，重画 24×24 线性图标（第二版：优化比例与细节密度）。

第一版的问题：
  - 齿轮齿距是硬凑的点序列，齿肩略歪
  - 软盘里塞了徽标 + 两条文字线，18px 下会糊
  - 修改器的滑轨与旋钮没做断开，看起来像卡里三个点
  - MOD 中心的连接短线拼成了「+」，和扩展图标的加号撞车

这一版：齿轮用齿顶直线 + 齿根圆弧；软盘去掉文字线、缩小徽标；
滑轨在旋钮处断开；MOD 去掉连接线只留四宫格；扩展保持模块+插脚+加号。
"""

from __future__ import annotations

import math
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "src" / "icons" / "line"

HEAD = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" '
    'stroke="currentColor" stroke-width="1.6" stroke-linecap="round" '
    'stroke-linejoin="round">\n'
)
TAIL = "</svg>\n"

CX = CY = 12.0


def polar(ang: float, r: float) -> tuple[float, float]:
    rad = math.radians(ang)
    return CX + r * math.cos(rad), CY + r * math.sin(rad)


def gear_path(teeth: int = 6, r_tip: float = 9.6, r_root: float = 6.9,
              half_tip: float = 12.0, flank: float = 4.0) -> str:
    """齿顶直线 + 齿侧斜线 + 齿根圆弧。"""
    step = 360 / teeth
    cmds: list[str] = []
    for i in range(teeth):
        a = -90 + i * step
        p1 = polar(a - half_tip, r_tip)          # 齿顶左
        p2 = polar(a + half_tip, r_tip)          # 齿顶右
        p3 = polar(a + half_tip + flank, r_root)  # 落回齿根
        p4 = polar(a + step - half_tip - flank, r_root)  # 齿根圆弧终点
        if i == 0:
            cmds.append(f"M {p1[0]:.2f} {p1[1]:.2f}")
        else:
            cmds.append(f"L {p1[0]:.2f} {p1[1]:.2f}")
        cmds.append(f"L {p2[0]:.2f} {p2[1]:.2f}")
        cmds.append(f"L {p3[0]:.2f} {p3[1]:.2f}")
        cmds.append(f"A {r_root:.2f} {r_root:.2f} 0 0 1 {p4[0]:.2f} {p4[1]:.2f}")
    cmds.append("Z")
    return " ".join(cmds)


def svg(body: str) -> str:
    return HEAD + body.strip() + "\n" + TAIL


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    icons: dict[str, str] = {}

    # 1) 通用：六齿齿轮 + 中心孔
    icons["settings"] = svg(
        f'<path d="{gear_path()}"/>\n'
        '  <circle cx="12" cy="12" r="3.05"/>'
    )

    # 2) 存档：软盘 + 右下角备份徽标（去掉内部文字线，18px 下更干净）
    icons["save"] = svg(
        '<path d="M4.2 5.2A2 2 0 0 1 6.2 3.2h8.6l4.9 4.9v10.7a2 2 0 0 1-2 2H6.2a2 2 0 0 1-2-2Z"/>\n'
        '  <path d="M8.6 3.4v4.3h5.6V3.4"/>\n'
        '  <circle cx="16.1" cy="16.1" r="4.1"/>\n'
        '  <path d="M14.1 15.2a2.4 2.4 0 0 1 3.9-.2"/>\n'
        '  <path d="M18.1 17.0a2.4 2.4 0 0 1-3.9.2"/>'
    )

    # 3) 修改器：三条滑轨 + 旋钮（去掉面板外框，18px 下更清楚；
    #    实测有框版本在 18px 会糊成一个带线的方块）
    icons["trainer"] = svg(
        '  <path d="M3.5 7h5.6M12.5 7h8"/>\n'
        '  <circle cx="10.9" cy="7" r="1.95"/>\n'
        '  <path d="M3.5 12h7.3M14.3 12h6.2"/>\n'
        '  <circle cx="12.7" cy="12" r="1.95"/>\n'
        '  <path d="M3.5 17h2.7M9.7 17h10.8"/>\n'
        '  <circle cx="8" cy="17" r="1.95"/>'
    )

    # 4) MOD：四宫格（右上为主块，略大），去掉中心连接线避免与「加号」混淆
    icons["mod"] = svg(
        '<rect x="3.6" y="3.6" width="7.2" height="7.2" rx="2"/>\n'
        '  <rect x="3.6" y="13.2" width="7.2" height="7.2" rx="2"/>\n'
        '  <rect x="13.2" y="13.2" width="7.2" height="7.2" rx="2"/>\n'
        '  <rect x="12.9" y="3.3" width="7.8" height="7.8" rx="2.2" stroke-width="2"/>'
    )

    # 5) 扩展：模块本体 + 顶部两插脚 + 中间加号 + 底部卡口
    icons["extensions"] = svg(
        '<path d="M6.4 7.4h11.2a2.4 2.4 0 0 1 2.4 2.4v6.1a4.2 4.2 0 0 1-4.2 4.2H8.2a4.2 4.2 0 0 1-4.2-4.2v-6.1a2.4 2.4 0 0 1 2.4-2.4Z"/>\n'
        '  <path d="M9.6 7.2V3.4M14.4 7.2V3.4"/>\n'
        '  <path d="M12 10.9v5.4M9.3 13.6h5.4"/>\n'
        '  <path d="M11.1 19.9v1.3h1.8v-1.3"/>'
    )

    # 6) 关于：圆 + i
    icons["about"] = svg(
        '<circle cx="12" cy="12" r="8.7"/>\n'
        '  <circle cx="12" cy="8.3" r="1.05" fill="currentColor" stroke="none"/>\n'
        '  <path d="M12 11.3v5.3" stroke-width="1.75"/>'
    )

    for name, content in icons.items():
        (OUT / f"{name}.svg").write_text(content, encoding="utf-8")
        print(f"  {name}.svg  {len(content)} 字节")

    # ---------- 小尺寸专用版（src/icons/line/small/*.svg）----------
    # 32px 及以下细节会挤（软盘徽标、滑轨旋钮、扩展加号糊成一团）。
    # 原则：只留一个能认出语义的主轮廓 + 一处最大特征，stroke 加粗到 1.9。
    SMALL = OUT / "small"
    SMALL.mkdir(parents=True, exist_ok=True)
    SH = ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" '
          'stroke="currentColor" stroke-width="1.9" stroke-linecap="round" '
          'stroke-linejoin="round">\n')
    small_icons = {
        # 齿轮本身在小尺寸仍清晰，中心孔略缩
        "settings": f'<path d="{gear_path(r_tip=9.8, r_root=7.2)}"/>\n  <circle cx="12" cy="12" r="2.6"/>',
        # 软盘：只留盘体 + 顶部快门（戢翎 11-03：右下角圆在小尺寸看不清还显脏）
        "save": '  <path d="M4.2 5.2A2 2 0 0 1 6.2 3.2h8.6l4.9 4.9v10.7a2 2 0 0 1-2 2H6.2a2 2 0 0 1-2-2Z"/>\n'
                '  <path d="M8.6 3.4v4.3h5.6V3.4"/>\n'
                '  <rect x="7.4" y="12.8" width="9.2" height="6.6" rx="1.4"/>',
        # 三条滑轨保留，旋钮缩小
        "trainer": '  <path d="M3.5 7h5.6M12.5 7h8"/>\n  <circle cx="10.9" cy="7" r="1.7"/>\n'
                   '  <path d="M3.5 12h7.3M14.3 12h6.2"/>\n  <circle cx="12.7" cy="12" r="1.7"/>\n'
                   '  <path d="M3.5 17h2.7M9.7 17h10.8"/>\n  <circle cx="8" cy="17" r="1.7"/>',
        # 四宫格：完全统一（戢翎 11-03：简化就不要右上角加黑）
        "mod": '  <rect x="3.6" y="3.6" width="7.2" height="7.2" rx="2"/>\n'
               '  <rect x="3.6" y="13.2" width="7.2" height="7.2" rx="2"/>\n'
               '  <rect x="13.2" y="13.2" width="7.2" height="7.2" rx="2"/>\n'
               '  <rect x="13.2" y="3.6" width="7.2" height="7.2" rx="2"/>',
        # 扩展去掉底部卡口（小尺寸看不清），保留插脚 + 加号
        "extensions": '  <path d="M6.4 7.4h11.2a2.4 2.4 0 0 1 2.4 2.4v6.1a4.2 4.2 0 0 1-4.2 4.2H8.2a4.2 4.2 0 0 1-4.2-4.2v-6.1a2.4 2.4 0 0 1 2.4-2.4Z"/>\n'
                      '  <path d="M9.6 7.2V3.4M14.4 7.2V3.4"/>\n'
                      '  <path d="M12 10.9v5.4M9.3 13.6h5.4"/>',
        "about": '  <circle cx="12" cy="12" r="8.7"/>\n'
                 '  <circle cx="12" cy="8.3" r="1.15" fill="currentColor" stroke="none"/>\n'
                 '  <path d="M12 11.3v5.3" stroke-width="2"/>',
    }
    for name, body in small_icons.items():
        (SMALL / f"{name}.svg").write_text(SH + body + "\n</svg>\n", encoding="utf-8")
        print(f"  small/{name}.svg")


if __name__ == "__main__":
    main()
