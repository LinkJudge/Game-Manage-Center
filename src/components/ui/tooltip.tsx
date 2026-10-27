import * as React from "react";
import { Tooltip as TooltipPrimitive } from "radix-ui";
import { cn } from "cn";

function TooltipProvider({
  delayDuration = 320,
  skipDelayDuration = 80,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
  return (
    <TooltipPrimitive.Provider
      delayDuration={delayDuration}
      skipDelayDuration={skipDelayDuration}
      {...props}
    />
  );
}

function Tooltip(props: React.ComponentProps<typeof TooltipPrimitive.Root>) {
  return <TooltipPrimitive.Root {...props} />;
}

function TooltipTrigger(props: React.ComponentProps<typeof TooltipPrimitive.Trigger>) {
  return <TooltipPrimitive.Trigger {...props} />;
}

function TooltipContent({
  className,
  sideOffset = 8,
  children,
  ...props
}: React.ComponentProps<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        sideOffset={sideOffset}
        collisionPadding={10}
        className={cn(
          "z-[120] max-w-64 rounded-[0.65rem] border border-white/10 bg-zinc-950/92 px-3 py-1.5",
          "text-xs font-medium leading-relaxed text-zinc-50",
          "shadow-[0_8px_24px_rgba(0,0,0,0.28)] backdrop-blur-xl",
          "origin-(--radix-tooltip-content-transform-origin)",
          "data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0 data-[state=delayed-open]:zoom-in-95",
          "data-[state=instant-open]:animate-in data-[state=instant-open]:fade-in-0 data-[state=instant-open]:zoom-in-95",
          "data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95",
          className,
        )}
        {...props}
      >
        {children}
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  );
}

/** 禁用按钮的 pointer-events 会吞掉 hover，用一个可聚焦外壳承接 Tooltip。 */
function ActionTooltip({
  content,
  disabled = false,
  children,
}: {
  content: string;
  disabled?: boolean;
  children: React.ReactElement;
}) {
  // Radix Tooltip 默认在「指针悬停」和「键盘聚焦」两种情况下都会打开。
  // 但对话框打开时会把焦点自动放到第一个可聚焦元素上，正好命中带提示的按钮，
  // 于是鼠标还没动提示就自己弹出来了（戢翎 2026-10-28 报的问题）。
  // 这里改为受控：只有指针确实在触发器上时才允许打开，聚焦引起的打开请求直接忽略。
  const [open, setOpen] = React.useState(false);
  const pointerInside = React.useRef(false);

  const trigger = disabled ? (
    <span tabIndex={0} className="inline-flex rounded-[0.7rem] outline-none">
      {children}
    </span>
  ) : (
    children
  );

  return (
    <Tooltip
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          setOpen(false);
          return;
        }
        if (pointerInside.current) setOpen(true);
      }}
    >
      <TooltipTrigger
        asChild
        onPointerEnter={() => {
          pointerInside.current = true;
        }}
        onPointerLeave={() => {
          pointerInside.current = false;
        }}
      >
        {trigger}
      </TooltipTrigger>
      <TooltipContent>{content}</TooltipContent>
    </Tooltip>
  );
}

export {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
  TooltipProvider,
  ActionTooltip,
};

