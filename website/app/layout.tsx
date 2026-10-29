import "./globals.css";
import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import { Geist, Geist_Mono } from "next/font/google";
import { Analytics } from "@vercel/analytics/next";

const geistSans = Geist({
  subsets: ["latin"],
  variable: "--font-geist-sans",
  display: "swap",
});

const geistMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-geist-mono",
  display: "swap",
});

export const metadata: Metadata = {
  metadataBase: new URL("https://agent-webmcp.vercel.app"),
  title: "agent-webmcp · ultra-light WebMCP browser CLI",
  description:
    "A small engine that turns any web page into an agent's toolkit: page tools, authored custom tools, and codemode over CLI and MCP.",
  icons: {
    icon: [{ url: "/orkestrate-icon.svg", type: "image/svg+xml" }],
    apple: "/orkestrate-apple-touch-icon.png",
  },
  openGraph: {
    title: "agent-webmcp · ultra-light WebMCP browser CLI",
    description:
      "A small engine that turns any web page into an agent's toolkit: page tools, authored custom tools, and codemode over CLI and MCP.",
    url: "https://agent-webmcp.vercel.app",
    siteName: "agent-webmcp",
    type: "website",
    images: [
      {
        url: "/opengraph-image",
        width: 1200,
        height: 630,
        alt: "agent-webmcp — turn the web into your agent's toolkit",
      },
    ],
  },
  twitter: {
    card: "summary_large_image",
    title: "agent-webmcp · ultra-light WebMCP browser CLI",
    description:
      "A small engine that turns any web page into an agent's toolkit: page tools, authored custom tools, and codemode over CLI and MCP.",
    images: ["/opengraph-image"],
  },
};

export const viewport: Viewport = {
  themeColor: "#000000",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html
      lang="en"
      className={`${geistSans.variable} ${geistMono.variable}`}
      suppressHydrationWarning
    >
      {/* Natively styled site — tell Dark Reader to leave the DOM alone.
          Otherwise it injects data-darkreader-* attrs on <html> before
          hydration, causing React hydration-mismatch errors. (Rendered
          explicitly rather than via metadata.other, which drops empty
          values.) suppressHydrationWarning absorbs any other extension's
          one-time attribute injection on this element. */}
      <meta name="darkreader-lock" />
      <body>
        {children}
        <Analytics />
      </body>
    </html>
  );
}
