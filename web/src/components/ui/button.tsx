import * as React from "react";
import { Slot } from "radix-ui";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 cursor-pointer items-center justify-center gap-2 whitespace-nowrap font-medium transition-[background,color,border-color,opacity,transform] duration-150 outline-none select-none active:translate-y-px disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-accent font-semibold text-accent-fg hover:bg-accent-hover",
        secondary: "border border-line bg-panel-solid text-fg hover:bg-panel-2",
        ghost: "text-fg-2 hover:bg-panel-2 hover:text-fg",
        link: "h-auto p-0 font-semibold text-accent underline-offset-4 hover:underline",
      },
      size: {
        sm: "h-8 rounded-[3px] px-3 text-[13px]",
        md: "h-10 rounded-[4px] px-4 text-[14px]",
        lg: "h-12 rounded-[4px] px-6 text-[15px]",
        icon: "size-10 rounded-[4px]",
        "icon-sm": "size-8 rounded-[3px]",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

export interface ButtonProps extends React.ComponentProps<"button">, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

export function Button({ className, variant, size, asChild = false, ...props }: ButtonProps) {
  const Comp = asChild ? Slot.Root : "button";
  return <Comp data-slot="button" className={cn(buttonVariants({ variant, size, className }))} {...props} />;
}

export { buttonVariants };
