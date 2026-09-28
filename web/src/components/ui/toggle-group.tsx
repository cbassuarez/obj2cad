import * as React from "react";
import { ToggleGroup as ToggleGroupPrimitive } from "radix-ui";
import { cn } from "@/lib/utils";

/** Segmented control: a track with a raised selected segment. */
export function ToggleGroup({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Root>) {
  return (
    <ToggleGroupPrimitive.Root
      data-slot="toggle-group"
      className={cn("inline-flex items-stretch gap-0.5 rounded-[4px] bg-panel-2 p-[3px]", className)}
      {...props}
    />
  );
}

export function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Item>) {
  return (
    <ToggleGroupPrimitive.Item
      data-slot="toggle-group-item"
      className={cn(
        "inline-flex min-h-9 cursor-pointer items-center justify-center rounded-[3px] px-3 text-[13px] font-medium text-fg-3 transition-colors outline-none hover:text-fg",
        "data-[state=on]:bg-panel-solid data-[state=on]:text-fg data-[state=on]:shadow-[0_1px_2px_rgb(0_0_0/0.18),inset_0_0_0_1px_var(--line)]",
        className,
      )}
      {...props}
    />
  );
}
