import { useEffect, useRef } from "react";
import type { LucideIcon } from "lucide-react";

export interface RowMenuItem {
  label: string;
  icon: LucideIcon;
  danger?: boolean;
  onSelect: () => void;
}

/** 行右键菜单：跟随鼠标位置的无边框浮层，点击外部 / Esc 关闭 */
export function RowMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: RowMenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onClose);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onClose);
    };
  }, [onClose]);

  // 边界翻转：靠右/靠底时向内收
  const MENU_W = 188;
  const MENU_H = items.length * 32 + 8;
  const left = Math.min(x, window.innerWidth - MENU_W - 8);
  const top = Math.min(y, window.innerHeight - MENU_H - 8);

  return (
    <div
      ref={ref}
      className="fixed z-[100] min-w-[184px] rounded-[0.8rem] border bg-popover p-1 text-popover-foreground shadow-lg"
      style={{ left, top }}
    >
      {items.map((item) => (
        <button
          key={item.label}
          onClick={() => {
            item.onSelect();
            onClose();
          }}
          className={
            "flex h-8 w-full items-center gap-2 rounded-[0.55rem] px-2.5 text-[0.8rem] transition-colors " +
            (item.danger
              ? "text-destructive hover:bg-destructive/10"
              : "hover:bg-foreground/5")
          }
        >
          <item.icon className="size-3.5 opacity-70" />
          {item.label}
        </button>
      ))}
    </div>
  );
}
