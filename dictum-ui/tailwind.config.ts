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
          bg: "rgb(var(--bg))",
          surface: "rgb(var(--card))",
          border: "rgb(var(--border))",
          accent: "rgb(var(--accent))",
          "accent-hover": "rgb(var(--accent-3))",
          text: "rgb(var(--fg))",
          muted: "rgb(var(--muted))",
          partial: "rgb(var(--muted))",
          final: "rgb(var(--fg))",
          listening: "rgb(var(--good))",
          stopped: "rgb(var(--danger))",
          idle: "rgb(var(--muted))",
          warm: "rgb(var(--accent-2))",
          cool: "rgb(var(--accent-3))",
        },
      },
      fontFamily: {
        display: ["var(--font-display)", "ui-sans-serif", "sans-serif"],
        sans: ["var(--font-body)", "ui-sans-serif", "sans-serif"],
        mono: ["var(--font-mono)", "ui-monospace", "Cascadia Code", "monospace"],
      },
      borderRadius: {
        sm: "var(--radius-sm)",
        md: "var(--radius-md)",
        lg: "var(--radius-lg)",
      },
      boxShadow: {
        flush: "var(--shadow-flush)",
        raised: "var(--shadow-raised)",
        floating: "var(--shadow-floating)",
        card: "var(--shadow-raised), var(--card-highlight), var(--card-edge)",
      },
      animation: {
        pulse: "pulse 2s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "fade-in": "fadeIn 0.2s ease-out",
        drift: "drift 18s ease-in-out infinite",
      },
      keyframes: {
        fadeIn: {
          "0%": { opacity: "0", transform: "translateY(4px)" },
          "100%": { opacity: "1", transform: "translateY(0)" },
        },
        drift: {
          "0%, 100%": { transform: "translateY(0px)" },
          "50%": { transform: "translateY(-6px)" },
        },
      },
    },
  },
  plugins: [],
};

export default config;
