/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        background: "var(--background)",
        surface: "var(--surface)",
        foreground: "var(--foreground)",
        "muted-foreground": "var(--muted-foreground)",
        border: "var(--border)",
        primary: "var(--primary)",
        keep: "var(--keep)",
        reject: "var(--reject)",
        pending: "var(--pending)",
        warn: "var(--warn)",
        info: "var(--info)",
      },
    },
  },
  plugins: [],
};
