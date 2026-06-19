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

const BASE =
  "inline-flex items-center justify-center gap-2 rounded-md font-medium " +
  "transition-colors focus:outline-none focus-visible:ring-2 " +
  "focus-visible:ring-blue-500 focus-visible:ring-offset-2 " +
  "dark:focus-visible:ring-offset-gray-950 disabled:cursor-not-allowed " +
  "disabled:opacity-50";

const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-blue-600 text-white hover:bg-blue-700 active:bg-blue-800",
  secondary:
    "border border-gray-300 bg-white text-gray-900 hover:bg-gray-50 " +
    "dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100 dark:hover:bg-gray-700",
  ghost:
    "text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-800",
  danger: "bg-recording text-white hover:bg-red-600 active:bg-red-700",
};

const SIZES: Record<Size, string> = {
  sm: "h-8 px-3 text-sm",
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
