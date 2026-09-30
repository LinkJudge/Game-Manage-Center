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
  const MENU_W = 208;
  const MENU_H = items.length * 36 + 12;
  const left = Math.min(x, window.innerWidth - MENU_W - 8);
  const top = Math.min(y, window.innerHeight - MENU_H - 8);

  return (
    <div
      ref={ref}
      className="fixed z-[100] min-w-[200px] rounded-xl border bg-popover p-1.5 text-popover-foreground shadow-xl"
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
            "flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors " +
            (item.danger
              ? "text-destructive hover:bg-destructive/10"
              : "hover:bg-foreground/5")
          }
        >
          <item.icon className="size-4 opacity-70" />
          {item.label}
        </button>
      ))}
    </div>
  );
}
