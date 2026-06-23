/**
 * Button — the single button primitive. Variant + size only; everything else is
 * passed through. Dark-mode aware via Tailwind `dark:` classes.
 */

import { forwardRef, type ButtonHTMLAttributes } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
}

// Monochrome system. Focus uses the global `:focus-visible` outline (globals.css)
// so buttons don't suppress it.
const BASE =
  "inline-flex items-center justify-center gap-2 font-medium transition-colors " +
  "disabled:cursor-not-allowed disabled:opacity-40";

const VARIANTS: Record<Variant, string> = {
  // Inverted ink/paper — the loud, primary action.
  primary: "bg-fg text-bg hover:opacity-90 active:opacity-80",
  // Hairline outline — the quiet, secondary action.
  secondary: "border border-line-strong text-fg hover:bg-surface",
  ghost: "text-muted hover:bg-surface hover:text-fg",
  // No red in a monochrome system; destructive shares the inverted treatment.
  danger: "bg-fg text-bg hover:opacity-90 active:opacity-80",
};

const SIZES: Record<Size, string> = {
  sm: "h-8 px-3 text-[13px]",
  md: "h-10 px-4 text-sm",
  lg: "h-12 px-6 text-base",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = "primary", size = "md", className = "", type, ...rest }, ref) => (
    <button
      ref={ref}
      type={type ?? "button"}
      className={`${BASE} ${VARIANTS[variant]} ${SIZES[size]} ${className}`}
      {...rest}
    />
  ),
);
Button.displayName = "Button";
