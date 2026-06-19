/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Recording status accent (used by tray panel / mini-panel).
        recording: "#ef4444",
      },
    },
  },
  plugins: [],
};
