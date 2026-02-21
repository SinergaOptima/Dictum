import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./src/pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        dictum: {
          bg: "rgb(var(--bg) / <alpha-value>)",
          surface: "rgb(var(--card) / <alpha-value>)",
          border: "rgb(var(--border) / <alpha-value>)",
          accent: "rgb(var(--accent) / <alpha-value>)",
          "accent-hover": "rgb(var(--accent-glow) / <alpha-value>)",
          text: "rgb(var(--fg) / <alpha-value>)",
          muted: "rgb(var(--muted) / <alpha-value>)",
          partial: "rgb(var(--muted) / <alpha-value>)",
          final: "rgb(var(--fg) / <alpha-value>)",
          listening: "rgb(var(--good) / <alpha-value>)",
          stopped: "rgb(var(--danger) / <alpha-value>)",
          idle: "rgb(var(--muted) / <alpha-value>)",
          warm: "rgb(var(--accent-2) / <alpha-value>)",
          cool: "rgb(var(--accent-3) / <alpha-value>)",
        },
      },
      fontFamily: {
        display: ["var(--font-display)", "Georgia", "serif"],
        sans: ["var(--font-body)", "ui-sans-serif", "sans-serif"],
        mono: ["var(--font-mono)", "ui-monospace", "Cascadia Code", "monospace"],
      },
      borderRadius: {
        sm: "var(--radius-sm)",
        md: "var(--radius-md)",
        lg: "var(--radius-lg)",
        full: "var(--radius-full)",
      },
      boxShadow: {
        flush: "var(--shadow-flush)",
        soft: "var(--shadow-soft)",
        raised: "var(--shadow-raised)",
        floating: "var(--shadow-floating)",
        hard: "var(--shadow-hard)",
        inset: "var(--inset-shadow)",
        card: "var(--shadow-raised), var(--card-highlight), var(--card-edge)",
        glass: "var(--shadow-glass)",
        "glass-hover": "var(--shadow-glass-hover)",
      },
      animation: {
        pulse: "pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "fade-in": "fadeIn 0.2s ease-out",
        drift: "drift 25s ease-in-out infinite",
      },
      keyframes: {
        fadeIn: {
          "0%": { opacity: "0", transform: "translateY(4px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        drift: {
          "0%, 100%": { transform: "translate(0, 0) scale(1)" },
          "33%": { transform: "translate(0.4%, -0.25%) scale(1.004)" },
          "66%": { transform: "translate(-0.25%, 0.35%) scale(1.002)" },
        },
      },
    },
  },
  plugins: [],
};

export default config;
