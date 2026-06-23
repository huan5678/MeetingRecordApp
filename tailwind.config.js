/** @type {import('tailwindcss').Config} */

// One truly hueless ramp. Every chromatic scale the app already uses is aliased
// to this, so the whole UI collapses to monochrome without touching components;
// accents that must invert per theme use the var-backed semantic tokens below.
const neutral = {
  50: "#fafafa",
  100: "#f5f5f5",
  200: "#e5e5e5",
  300: "#d4d4d4",
  400: "#a3a3a3",
  500: "#737373",
  600: "#525252",
  700: "#404040",
  800: "#262626",
  900: "#171717",
  950: "#0a0a0a",
};

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Semantic, theme-aware tokens (preferred for new/refined components).
        bg: "var(--bg)",
        surface: "var(--surface)",
        fg: "var(--fg)",
        muted: "var(--muted)",
        faint: "var(--faint)",
        line: "var(--line)",
        "line-strong": "var(--line-strong)",
        // Monochrome: a "live" state is signalled by motion + an inverted block,
        // never by hue, so the old red accent collapses to the foreground ink.
        recording: "var(--fg)",
        // Collapse every chromatic scale to the neutral ramp.
        gray: neutral,
        slate: neutral,
        zinc: neutral,
        neutral,
        stone: neutral,
        blue: neutral,
        sky: neutral,
        indigo: neutral,
        red: neutral,
        rose: neutral,
        orange: neutral,
        amber: neutral,
        yellow: neutral,
        green: neutral,
        emerald: neutral,
        teal: neutral,
      },
      fontFamily: {
        sans: ["var(--font-sans)"],
        display: ["var(--font-display)"],
        mono: ["var(--font-mono)"],
      },
      letterSpacing: {
        eyebrow: "0.18em",
      },
      // Sharp, editorial corners everywhere; keep `full` for dots/pulse.
      borderRadius: {
        none: "0",
        sm: "0",
        DEFAULT: "0",
        md: "0",
        lg: "0",
        xl: "0",
        "2xl": "0",
        "3xl": "0",
        full: "9999px",
      },
    },
  },
  plugins: [],
};
