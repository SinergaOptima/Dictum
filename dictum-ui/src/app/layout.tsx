import type { Metadata } from "next";
import { Fraunces, Epilogue, Fira_Code } from "next/font/google";
import "./globals.css";

const displayFont = Fraunces({
  subsets: ["latin"],
  variable: "--font-display",
  axes: ["opsz"],
});

const bodyFont = Epilogue({
  subsets: ["latin"],
  variable: "--font-body",
});

const monoFont = Fira_Code({
  subsets: ["latin"],
  variable: "--font-mono",
});

export const metadata: Metadata = {
  title: "Dictum",
  description: "Local-first, ultra-low-latency voice-to-text",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="h-full">
      <body
        className={`${displayFont.variable} ${bodyFont.variable} ${monoFont.variable} h-full antialiased`}
      >
        {children}
      </body>
    </html>
  );
}
