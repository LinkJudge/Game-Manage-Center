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
  // 注意：不要在 Tooltip 层做「只认 hover」的受控改造。
  // 受控会把 Provider 的 delayDuration 绕掉，导致提示一碰就弹（2026-10-28 试过，戢翎要求统一）。
  // 弹窗自动聚焦导致的误弹，改为在对应 Dialog 上用 onOpenAutoFocus 阻止自动聚焦。
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {disabled ? (
          <span tabIndex={0} className="inline-flex rounded-[0.7rem] outline-none">
            {children}
          </span>
        ) : (
          children
        )}
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

