import * as React from "react"
import { cn } from "cn"
import { Progress as ProgressPrimitive } from "radix-ui"

function Progress({
  className,
  value,
  indeterminate,
  ...props
}: React.ComponentProps<typeof ProgressPrimitive.Root> & {
  /** 总量未知（服务端分块传输没给 Content-Length）时用往返滑动条代替百分比 */
  indeterminate?: boolean
}) {
  return (
    <ProgressPrimitive.Root
      data-slot="progress"
      data-indeterminate={indeterminate ? "" : undefined}
      className={cn(
        "relative flex h-1 w-full items-center overflow-x-hidden rounded-full bg-muted",
        className
      )}
      {...props}
    >
      <ProgressPrimitive.Indicator
        data-slot="progress-indicator"
        className={cn(
          "size-full flex-1 bg-primary transition-all",
          indeterminate && "gm-progress-indeterminate"
        )}
        style={indeterminate ? undefined : { transform: `translateX(-${100 - (value || 0)}%)` }}
      />
    </ProgressPrimitive.Root>
  )
}

export { Progress }
