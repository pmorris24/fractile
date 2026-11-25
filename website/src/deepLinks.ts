// Shared deep-link parser, metadata registry, and dark-mode web bounce renderer
// for https://anoma.ly/spaceman/open/, /labs/<slug>, /features/<slug>, and /settings/<pane>.
//
// Used by:
// - website/worker.ts (dynamic Cloudflare Worker responses with per-slug OG metadata)
// - website/scripts/prerender.mjs (static HTML pages emitted into dist/)
// - website/vite.config.ts (local dev server middleware)

import { DOWNLOAD_URL, MIN_MACOS_LABEL, RELEASE_VERSION, SITE_URL } from "./release.ts";

export type DemoKind =
  | "scrolling-strip"
  | "workspaces"
  | "rules"
  | "shortcuts"
  | "gaps"
  | "hud"
  | "playground"
  | "tiling";

export interface BounceMeta {
  badge: "Labs" | "Feature" | "Settings";
  heading: string;
  ogTitle: string;
  description: string;
  demo: DemoKind;
}

export interface ParsedBounceTarget {
  kind: "labs" | "features" | "settings";
  pane?: string;
  slug?: string;
  focus?: string;
  /** Canonical strict `spaceman://...` URL (guaranteed `[a-z0-9-/]` only). */
  schemeUrl: string;
  /** Canonical path relative to `/spaceman` (e.g. `/labs/scrolling-strip`). */
  webPath: string;
  meta: BounceMeta;
}

const SLUG_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const MAX_SLUG_LEN = 64;
const MAX_INPUT_LEN = 200;

const KNOWN_LABS: Record<string, BounceMeta> = {
  "scrolling-strip": {
    badge: "Labs",
    heading: "Scrolling Strip",
    ogTitle: "Scrolling Strip — Spaceman Labs",
    description:
      "A horizontal strip of windows you glide across with smooth trackpad momentum. Open directly in Spaceman Settings › Labs.",
    demo: "scrolling-strip",
  },
  playground: {
    badge: "Labs",
    heading: "Rune Playground",
    ogTitle: "Rune Playground — Spaceman Labs",
    description:
      "Inspect live Spaceman Settings scenes, HUD layouts, and onboarding flows in isolation.",
    demo: "playground",
  },
};

const KNOWN_FEATURES: Record<string, BounceMeta> = {
  "scrolling-strip": KNOWN_LABS["scrolling-strip"],
  playground: KNOWN_LABS.playground,
  workspaces: {
    badge: "Feature",
    heading: "Workspaces & Displays",
    ogTitle: "Workspaces — Spaceman",
    description:
      "Ten numbered workspaces plus opt-in named workspaces A–Z, routed cleanly across your displays.",
    demo: "workspaces",
  },
  rules: {
    badge: "Feature",
    heading: "Ordered Window Rules",
    ogTitle: "Window Rules — Spaceman",
    description:
      "Send apps to their workspace automatically, float utilities, or ignore helper panels with first-match rules.",
    demo: "rules",
  },
  shortcuts: {
    badge: "Feature",
    heading: "Keyboard Shortcuts",
    ogTitle: "Shortcuts — Spaceman",
    description:
      "Option-first shortcuts for workspaces, directional focus, resizing, fullscreen layout, and floating windows.",
    demo: "shortcuts",
  },
  gaps: {
    badge: "Feature",
    heading: "Gaps & Max Width",
    ogTitle: "Gaps & Max Width — Spaceman",
    description:
      "Tune outer margins, inner tile gaps, and comfortable single-window max width on ultrawide displays.",
    demo: "gaps",
  },
  "max-width": {
    badge: "Feature",
    heading: "Gaps & Max Width",
    ogTitle: "Max Width — Spaceman",
    description:
      "Keep a single focused window centered at a readable width on wide monitors while multi-window splits fill the screen.",
    demo: "gaps",
  },
  hud: {
    badge: "Feature",
    heading: "Workspace HUD",
    ogTitle: "Workspace HUD — Spaceman",
    description:
      "A crisp non-activating capsule showing every display's workspaces and app icons as you switch.",
    demo: "hud",
  },
};

const KNOWN_SETTINGS: Record<string, BounceMeta> = {
  general: {
    badge: "Settings",
    heading: "General Settings",
    ogTitle: "General Settings — Spaceman",
    description:
      "Automatic tiling, Workspace HUD style, Launch at login, update channel, and your config.toml file.",
    demo: "hud",
  },
  rules: {
    badge: "Settings",
    heading: "Window Rules",
    ogTitle: "Rules — Spaceman Settings",
    description:
      "Assign apps to workspaces, float windows, or ignore them with ordered first-match rules.",
    demo: "rules",
  },
  workspaces: {
    badge: "Settings",
    heading: "Workspaces",
    ogTitle: "Workspaces — Spaceman Settings",
    description:
      "Route workspaces 1–10 and named workspaces A–Z to your main, secondary, tertiary, or preferred display.",
    demo: "workspaces",
  },
  shortcuts: {
    badge: "Settings",
    heading: "Shortcuts",
    ogTitle: "Shortcuts — Spaceman Settings",
    description:
      "View every Option keyboard shortcut and three-finger trackpad gesture in Spaceman.",
    demo: "shortcuts",
  },
  labs: {
    badge: "Labs",
    heading: "Spaceman Labs",
    ogTitle: "Spaceman Labs",
    description:
      "Preview opt-in window-management experiments in Spaceman. Deep links open and highlight only — nothing is enabled automatically.",
    demo: "scrolling-strip",
  },
  layout: {
    badge: "Settings",
    heading: "Layout Settings",
    ogTitle: "Layout Settings — Spaceman",
    description:
      "Configure split behavior, tile gaps, and max-width presentation in Spaceman.",
    demo: "gaps",
  },
  activity: {
    badge: "Settings",
    heading: "Activity & Diagnostics",
    ogTitle: "Activity — Spaceman Settings",
    description:
      "Inspect title-free arrangement diagnostics, workspace recall history, and live process vitals.",
    demo: "tiling",
  },
};

export const PRERENDER_TARGETS: readonly string[] = [
  "/labs",
  ...Object.keys(KNOWN_LABS).map((slug) => `/labs/${slug}`),
  "/settings",
  ...Object.keys(KNOWN_SETTINGS).map((pane) => `/settings/${pane}`),
  ...Object.keys(KNOWN_FEATURES).map((slug) => `/features/${slug}`),
];

export function isValidSlug(value: string): boolean {
  return value.length > 0 && value.length <= MAX_SLUG_LEN && SLUG_RE.test(value);
}

function titleCaseSlug(slug: string): string {
  return slug
    .split("-")
    .map((part) => (part ? part[0].toUpperCase() + part.slice(1) : ""))
    .join(" ");
}

/**
 * Parses a route path under `/spaceman` (like `/labs/scrolling-strip`,
 * `/features/gaps`, `/settings/workspaces`, or `/open/?to=labs/scrolling-strip`)
 * into a strictly validated bounce target. Returns `null` if the path is not a
 * bounce route, or `{ invalid: true }` when a bounce route carries an invalid slug.
 */
export function resolveBounceRoute(
  path: string,
  searchParams: URLSearchParams,
): { kind: "ok"; target: ParsedBounceTarget } | { kind: "invalid" } | null {
  const clean = path.replace(/\/+$/, "") || "/";
  if (clean === "/open") {
    const rawTo = searchParams.get("to");
    if (!rawTo) {
      return { kind: "ok", target: defaultSettingsTarget("/open/") };
    }
    const parsed = parseTargetString(rawTo);
    return parsed ? { kind: "ok", target: parsed } : { kind: "invalid" };
  }
  if (
    clean === "/labs" ||
    clean.startsWith("/labs/") ||
    clean === "/features" ||
    clean.startsWith("/features/") ||
    clean === "/settings" ||
    clean.startsWith("/settings/")
  ) {
    const parsed = parseTargetString(clean.slice(1));
    return parsed ? { kind: "ok", target: parsed } : { kind: "invalid" };
  }
  return null;
}

export function parseTargetString(rawInput: string): ParsedBounceTarget | null {
  const trimmed = rawInput.trim();
  if (!trimmed || trimmed.length > MAX_INPUT_LEN) return null;

  let body = trimmed;
  if (body.toLowerCase().startsWith("spaceman://")) {
    body = body.slice("spaceman://".length);
  } else if (body.includes("://") || body.includes(":")) {
    return null;
  }

  // Strip query or fragment safely.
  body = body.split("#")[0].split("?")[0].replace(/^\/+|\/+$/g, "");
  if (!body) return null;

  const segments = body.split("/").map((seg) => seg.toLowerCase());
  if (segments.some((seg) => !isValidSlug(seg))) return null;

  const [host, first, second, ...extra] = segments;
  if (extra.length > 0) return null;

  if (host === "labs") {
    if (second !== undefined) return null;
    if (!first) {
      return {
        kind: "labs",
        schemeUrl: "spaceman://labs",
        webPath: "/labs",
        meta: KNOWN_SETTINGS.labs,
      };
    }
    const meta: BounceMeta = KNOWN_LABS[first] ?? {
      badge: "Labs",
      heading: titleCaseSlug(first),
      ogTitle: `${titleCaseSlug(first)} — Spaceman Labs`,
      description:
        "Open this experiment directly in Spaceman Settings › Labs. Deep links open and highlight only — nothing is enabled automatically.",
      demo: "scrolling-strip",
    };
    return {
      kind: "labs",
      slug: first,
      schemeUrl: `spaceman://labs/${first}`,
      webPath: `/labs/${first}`,
      meta,
    };
  }

  if (host === "features") {
    if (!first || second !== undefined) return null;
    const meta: BounceMeta = KNOWN_FEATURES[first] ?? {
      badge: "Feature",
      heading: titleCaseSlug(first),
      ogTitle: `${titleCaseSlug(first)} — Spaceman`,
      description:
        "Open this feature directly in Spaceman Settings. Deep links open and highlight only — nothing changes your config.",
      demo: "tiling",
    };
    return {
      kind: "features",
      slug: first,
      schemeUrl: `spaceman://features/${first}`,
      webPath: `/features/${first}`,
      meta,
    };
  }

  if (host === "settings") {
    if (!first) {
      return defaultSettingsTarget("/settings");
    }
    const baseMeta: BounceMeta = KNOWN_SETTINGS[first] ?? {
      badge: "Settings",
      heading: titleCaseSlug(first),
      ogTitle: `${titleCaseSlug(first)} — Spaceman Settings`,
      description: "Open Spaceman Settings directly to this pane.",
      demo: "tiling",
    };
    const schemeUrl = second ? `spaceman://settings/${first}/${second}` : `spaceman://settings/${first}`;
    const webPath = second ? `/settings/${first}/${second}` : `/settings/${first}`;
    const meta: BounceMeta = second
      ? {
          ...baseMeta,
          heading: `${baseMeta.heading} › ${titleCaseSlug(second)}`,
          ogTitle: `${titleCaseSlug(second)} — Spaceman Settings`,
        }
      : baseMeta;
    return {
      kind: "settings",
      pane: first,
      focus: second,
      schemeUrl,
      webPath,
      meta,
    };
  }

  return null;
}

function defaultSettingsTarget(webPath: string): ParsedBounceTarget {
  return {
    kind: "settings",
    schemeUrl: "spaceman://settings",
    webPath,
    meta: {
      badge: "Settings",
      heading: "Spaceman Settings",
      ogTitle: "Open in Spaceman",
      description:
        "Open Spaceman directly to Settings, window rules, workspaces, shortcuts, or Labs.",
      demo: "tiling",
    },
  };
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function renderMiniDemo(kind: DemoKind): string {
  switch (kind) {
    case "scrolling-strip":
      return `<div class="demo demo-strip" aria-hidden="true">
        <div class="strip-viewport">
          <div class="strip-track">
            <div class="win win-dim"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-gold"></span><span></span><span class="short"></span></div></div>
            <div class="win win-active"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-blue"></span><span></span><span class="mid"></span><span></span></div></div>
            <div class="win"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-teal"></span><span></span><span class="short"></span></div></div>
            <div class="win win-dim"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-violet"></span><span></span><span class="mid"></span></div></div>
          </div>
        </div>
        <div class="strip-dots"><span></span><span class="on"></span><span></span><span></span></div>
      </div>`;
    case "workspaces":
      return `<div class="demo demo-ws" aria-hidden="true">
        <div class="ws-stage">
          <div class="win win-ws-a"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-blue"></span><span></span><span class="short"></span></div></div>
          <div class="win win-ws-b"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-teal"></span><span></span><span class="mid"></span></div></div>
        </div>
        <div class="hud-pill">
          <span class="hud-slot active"><b>1</b><em class="dot a-blue"></em><em class="dot a-teal"></em></span>
          <span class="hud-slot"><b>2</b><em class="dot a-gold"></em></span>
          <span class="hud-slot"><b>3</b><em class="dot a-violet"></em><em class="dot a-green"></em></span>
        </div>
      </div>`;
    case "rules":
      return `<div class="demo demo-rules" aria-hidden="true">
        <div class="rule-list">
          <div class="rule-row rule-hi"><span class="rule-app"><em class="app-sq a-blue"></em>Ghostty</span><span class="rule-dest">Workspace 1</span></div>
          <div class="rule-row"><span class="rule-app"><em class="app-sq a-violet"></em>Slack</span><span class="rule-dest">Workspace 6</span></div>
          <div class="rule-row"><span class="rule-app"><em class="app-sq a-gold"></em>System Settings</span><span class="rule-dest rule-float">Float</span></div>
        </div>
      </div>`;
    case "shortcuts":
      return `<div class="demo demo-keys" aria-hidden="true">
        <div class="key-grid">
          <div class="key-row"><span>Switch workspace</span><span class="keys"><kbd>⌥</kbd><kbd>1…0</kbd></span></div>
          <div class="key-row key-hi"><span>Focus tile</span><span class="keys"><kbd>⌥</kbd><kbd>H</kbd><kbd>J</kbd><kbd>K</kbd><kbd>L</kbd></span></div>
          <div class="key-row"><span>Fullscreen layout</span><span class="keys"><kbd>⌥</kbd><kbd>⇧</kbd><kbd>F</kbd></span></div>
        </div>
      </div>`;
    case "gaps":
      return `<div class="demo demo-gaps" aria-hidden="true">
        <div class="gaps-frame">
          <div class="win"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-teal"></span><span></span><span class="short"></span></div></div>
          <div class="gaps-right">
            <div class="win"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-blue"></span><span></span></div></div>
            <div class="win"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-gold"></span><span class="short"></span></div></div>
          </div>
        </div>
      </div>`;
    case "hud":
      return `<div class="demo demo-hud" aria-hidden="true">
        <div class="hud-pill hud-lg">
          <span class="hud-slot"><b>1</b><em class="dot a-gold"></em><em class="dot a-blue"></em></span>
          <span class="hud-slot active"><b>2</b><em class="dot a-teal"></em><em class="dot a-violet"></em><em class="dot a-green"></em></span>
          <span class="hud-slot"><b>6</b><em class="dot a-orange"></em></span>
        </div>
      </div>`;
    case "playground":
      return `<div class="demo demo-play" aria-hidden="true">
        <div class="play-shell">
          <div class="play-side"><span></span><span class="on"></span><span></span><span></span></div>
          <div class="play-main">
            <div class="play-card"><span class="accent a-blue"></span><span class="line"></span></div>
            <div class="play-card"><span class="accent a-teal"></span><span class="line short"></span></div>
          </div>
        </div>
      </div>`;
    case "tiling":
    default:
      return `<div class="demo demo-tiling" aria-hidden="true">
        <div class="tile-grid">
          <div class="win"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-blue"></span><span></span><span class="mid"></span></div></div>
          <div class="tile-col">
            <div class="win"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-gold"></span><span></span></div></div>
            <div class="win"><div class="win-bar"><i></i><i></i><i></i></div><div class="win-lines"><span class="accent a-teal"></span><span class="short"></span></div></div>
          </div>
        </div>
      </div>`;
  }
}

export function renderBounceHtml(
  target: ParsedBounceTarget,
  options?: { allowQueryOverride?: boolean; invalidNote?: boolean },
): string {
  const canonicalUrl = `${SITE_URL}${target.webPath.replace(/^\//, "")}`;
  const ogImage = `${SITE_URL}og-image.png`;
  const iconUrl = `${SITE_URL}icon-192.png`;
  const title = escapeHtml(target.meta.ogTitle);
  const description = escapeHtml(target.meta.description);
  const heading = escapeHtml(target.meta.heading);
  const badge = escapeHtml(target.meta.badge);
  const schemeUrl = escapeHtml(target.schemeUrl);
  const demoHtml = renderMiniDemo(target.meta.demo);
  const allowQueryOverride = Boolean(options?.allowQueryOverride);
  const invalidNote = Boolean(options?.invalidNote);

  const clientRegistry = JSON.stringify({
    labs: Object.fromEntries(
      Object.entries(KNOWN_LABS).map(([k, v]) => [k, { badge: v.badge, heading: v.heading, description: v.description }]),
    ),
    features: Object.fromEntries(
      Object.entries(KNOWN_FEATURES).map(([k, v]) => [k, { badge: v.badge, heading: v.heading, description: v.description }]),
    ),
    settings: Object.fromEntries(
      Object.entries(KNOWN_SETTINGS).map(([k, v]) => [k, { badge: v.badge, heading: v.heading, description: v.description }]),
    ),
  });

  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" />
    <title>${title}</title>
    <meta name="description" content="${description}" />
    <meta name="color-scheme" content="dark" />
    <meta name="theme-color" content="#0a0b0d" />
    <link rel="canonical" href="${escapeHtml(canonicalUrl)}" />
    <link rel="icon" href="/spaceman/favicon.ico" sizes="32x32" />
    <link rel="icon" href="/spaceman/icon-192.png" sizes="192x192" type="image/png" />
    <link rel="apple-touch-icon" href="/spaceman/apple-touch-icon.png" />
    <meta property="og:type" content="website" />
    <meta property="og:site_name" content="Spaceman" />
    <meta property="og:locale" content="en_US" />
    <meta property="og:url" content="${escapeHtml(canonicalUrl)}" />
    <meta property="og:title" content="${title}" />
    <meta property="og:description" content="${description}" />
    <meta property="og:image" content="${escapeHtml(ogImage)}" />
    <meta property="og:image:type" content="image/png" />
    <meta property="og:image:width" content="1200" />
    <meta property="og:image:height" content="630" />
    <meta property="og:image:alt" content="Spaceman for macOS — ${heading}" />
    <meta name="twitter:card" content="summary_large_image" />
    <meta name="twitter:creator" content="@kitlangton" />
    <meta name="twitter:title" content="${title}" />
    <meta name="twitter:description" content="${description}" />
    <meta name="twitter:image" content="${escapeHtml(ogImage)}" />
    <meta name="twitter:image:alt" content="Spaceman for macOS — ${heading}" />
    <style>
      :root {
        color-scheme: dark;
        font-family: "InterVariable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        background: #0a0b0d;
        color: #f3f4f6;
      }
      * { box-sizing: border-box; }
      body {
        margin: 0;
        min-height: 100svh;
        display: flex;
        flex-direction: column;
        background:
          radial-gradient(640px 420px at 50% 0%, rgba(120, 175, 255, 0.08), transparent 70%),
          radial-gradient(520px 380px at 80% 90%, rgba(150, 215, 180, 0.05), transparent 72%),
          #0a0b0d;
        -webkit-font-smoothing: antialiased;
      }
      header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        height: 68px;
        padding: 0 24px;
        max-width: 960px;
        width: 100%;
        margin: 0 auto;
      }
      .brand {
        display: inline-flex;
        align-items: center;
        gap: 10px;
        color: #f5f5f7;
        text-decoration: none;
        font-size: 15px;
        font-weight: 600;
        letter-spacing: -0.015em;
      }
      .brand img {
        width: 28px;
        height: 28px;
        border-radius: 23%;
        box-shadow: 0 2px 8px rgba(0, 0, 0, 0.45), inset 0 0 0 1px rgba(255, 255, 255, 0.14);
      }
      .nav-link {
        font-size: 13px;
        color: #9a9ca3;
        text-decoration: none;
        transition: color 150ms ease;
      }
      .nav-link:hover { color: #f3f4f6; }
      main {
        flex: 1;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px 20px 56px;
      }
      .card {
        width: 100%;
        max-width: 480px;
        border-radius: 26px;
        background: rgba(255, 255, 255, 0.03);
        box-shadow:
          inset 0 0 0 1px rgba(255, 255, 255, 0.08),
          0 28px 64px -24px rgba(0, 0, 0, 0.82);
        padding: 26px;
        display: flex;
        flex-direction: column;
        gap: 20px;
      }
      .top-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
      }
      .status {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 12.5px;
        font-weight: 500;
        color: #c4c6cc;
      }
      .pulse {
        width: 8px;
        height: 8px;
        border-radius: 999px;
        background: #4fb3a7;
        box-shadow: 0 0 0 0 rgba(79, 179, 167, 0.45);
        animation: pulse 1.8s ease-out infinite;
      }
      @keyframes pulse {
        0% { box-shadow: 0 0 0 0 rgba(79, 179, 167, 0.45); }
        70% { box-shadow: 0 0 0 7px rgba(79, 179, 167, 0); }
        100% { box-shadow: 0 0 0 0 rgba(79, 179, 167, 0); }
      }
      .badge {
        display: inline-flex;
        align-items: center;
        height: 24px;
        padding: 0 10px;
        border-radius: 999px;
        font-size: 11.5px;
        font-weight: 600;
        letter-spacing: 0.01em;
        color: #dfe1e5;
        background: rgba(255, 255, 255, 0.07);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.09);
      }
      .head { display: flex; flex-direction: column; gap: 8px; }
      h1 {
        margin: 0;
        font-size: 28px;
        line-height: 1.12;
        font-weight: 600;
        letter-spacing: -0.03em;
        color: #f3f4f6;
      }
      .desc {
        margin: 0;
        font-size: 15px;
        line-height: 1.55;
        color: #9a9ca3;
      }
      .demo {
        position: relative;
        aspect-ratio: 16 / 9;
        width: 100%;
        border-radius: 16px;
        overflow: hidden;
        background: linear-gradient(135deg, #2d3642, #171a20);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
        padding: 14px;
        display: flex;
        flex-direction: column;
        justify-content: center;
      }
      .win {
        background: #1b1d22;
        border-radius: 8px;
        overflow: hidden;
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.09), 0 10px 24px rgba(0, 0, 0, 0.45);
        display: flex;
        flex-direction: column;
      }
      .win-bar {
        height: 14px;
        background: rgba(255, 255, 255, 0.04);
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 0 7px;
      }
      .win-bar i {
        width: 4px;
        height: 4px;
        border-radius: 999px;
        background: rgba(255, 255, 255, 0.22);
        display: block;
      }
      .win-lines {
        padding: 10px;
        display: flex;
        flex-direction: column;
        gap: 6px;
      }
      .win-lines span {
        height: 4px;
        border-radius: 999px;
        background: rgba(255, 255, 255, 0.14);
        width: 82%;
      }
      .win-lines span.mid { width: 64%; }
      .win-lines span.short { width: 46%; }
      .win-lines span.accent { width: 42%; height: 5px; }
      .a-blue { background: #4f8ad6 !important; }
      .a-teal { background: #4fb3a7 !important; }
      .a-gold { background: #e9b93f !important; }
      .a-violet { background: #8a74d1 !important; }
      .a-green { background: #5cb86e !important; }
      .a-orange { background: #d9803a !important; }
      /* Scrolling strip demo */
      .strip-viewport {
        flex: 1;
        overflow: hidden;
        display: flex;
        align-items: center;
      }
      .strip-track {
        display: flex;
        gap: 10px;
        width: 175%;
        animation: strip-glide 6.4s cubic-bezier(0.45, 0.05, 0.55, 0.95) infinite alternate;
      }
      .strip-track .win { flex: 0 0 38%; height: 122px; }
      .strip-track .win-active {
        box-shadow: inset 0 0 0 1.5px rgba(79, 138, 214, 0.75), 0 12px 28px rgba(0, 0, 0, 0.55);
      }
      .strip-track .win-dim { opacity: 0.65; }
      @keyframes strip-glide {
        0%, 15% { transform: translateX(0%); }
        45%, 55% { transform: translateX(-24%); }
        85%, 100% { transform: translateX(-44%); }
      }
      .strip-dots {
        margin-top: 10px;
        display: flex;
        justify-content: center;
        gap: 6px;
      }
      .strip-dots span {
        width: 5px;
        height: 5px;
        border-radius: 999px;
        background: rgba(255, 255, 255, 0.25);
      }
      .strip-dots span.on {
        width: 18px;
        background: rgba(255, 255, 255, 0.8);
      }
      /* Workspaces & HUD demo */
      .ws-stage {
        flex: 1;
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 10px;
        margin-bottom: 10px;
      }
      .hud-pill {
        align-self: center;
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 4px;
        border-radius: 12px;
        background: rgba(20, 21, 24, 0.92);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1), 0 10px 24px rgba(0, 0, 0, 0.45);
      }
      .hud-lg { transform: scale(1.18); }
      .hud-slot {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        height: 24px;
        padding: 0 8px;
        border-radius: 8px;
        font-size: 11px;
        color: rgba(255, 255, 255, 0.45);
      }
      .hud-slot.active {
        color: #fff;
        background: rgba(255, 255, 255, 0.14);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12);
      }
      .dot { width: 10px; height: 10px; border-radius: 3px; display: inline-block; }
      /* Rules demo */
      .rule-list { display: flex; flex-direction: column; gap: 8px; }
      .rule-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 10px 12px;
        border-radius: 10px;
        background: rgba(18, 20, 24, 0.84);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
        font-size: 12.5px;
      }
      .rule-hi {
        background: rgba(28, 32, 40, 0.92);
        box-shadow: inset 0 0 0 1px rgba(79, 138, 214, 0.55);
      }
      .rule-app { display: inline-flex; align-items: center; gap: 8px; color: #f3f4f6; font-weight: 500; }
      .app-sq { width: 14px; height: 14px; border-radius: 4px; display: inline-block; }
      .rule-dest { font-size: 11.5px; color: #9a9ca3; }
      .rule-float { color: #e9b93f; }
      /* Shortcuts demo */
      .key-grid { display: flex; flex-direction: column; gap: 8px; }
      .key-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 9px 12px;
        border-radius: 10px;
        background: rgba(18, 20, 24, 0.84);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
        font-size: 12.5px;
        color: #c4c6cc;
      }
      .key-hi { box-shadow: inset 0 0 0 1px rgba(79, 179, 167, 0.5); color: #f3f4f6; }
      .keys { display: inline-flex; gap: 4px; }
      kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 22px;
        height: 22px;
        padding: 0 6px;
        border-radius: 6px;
        background: rgba(255, 255, 255, 0.08);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12), 0 1px 0 rgba(0, 0, 0, 0.4);
        font-family: inherit;
        font-size: 11px;
        color: #f3f4f6;
      }
      /* Gaps & Tiling demo */
      .gaps-frame, .tile-grid {
        flex: 1;
        display: grid;
        grid-template-columns: 1.15fr 1fr;
        gap: 10px;
        height: 100%;
      }
      .gaps-frame {
        padding: 6px;
        animation: gaps-breathe 4.4s ease-in-out infinite alternate;
      }
      @keyframes gaps-breathe {
        0% { gap: 4px; padding: 2px; }
        100% { gap: 14px; padding: 10px; }
      }
      .gaps-right, .tile-col { display: grid; grid-template-rows: 1fr 1fr; gap: inherit; }
      /* Playground demo */
      .play-shell {
        flex: 1;
        display: grid;
        grid-template-columns: 88px 1fr;
        gap: 10px;
      }
      .play-side {
        background: rgba(14, 15, 18, 0.85);
        border-radius: 10px;
        padding: 10px 8px;
        display: flex;
        flex-direction: column;
        gap: 7px;
      }
      .play-side span { height: 10px; border-radius: 5px; background: rgba(255, 255, 255, 0.08); }
      .play-side span.on { background: rgba(79, 138, 214, 0.55); }
      .play-main { display: flex; flex-direction: column; gap: 10px; }
      .play-card {
        flex: 1;
        border-radius: 10px;
        background: rgba(24, 26, 31, 0.92);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
        padding: 12px;
        display: flex;
        flex-direction: column;
        gap: 8px;
      }
      .play-card .accent { width: 38%; height: 6px; border-radius: 999px; display: block; }
      .play-card .line { width: 76%; height: 4px; border-radius: 999px; background: rgba(255, 255, 255, 0.16); display: block; }
      .play-card .line.short { width: 52%; }
      .uri {
        font-family: "SFMono-Regular", "SF Mono", ui-monospace, Menlo, monospace;
        font-size: 12.5px;
        color: #c4c6cc;
        background: rgba(0, 0, 0, 0.36);
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
        border-radius: 12px;
        padding: 10px 13px;
        overflow-x: auto;
        white-space: nowrap;
      }
      .actions {
        display: flex;
        flex-wrap: wrap;
        gap: 10px;
      }
      .btn {
        flex: 1 1 180px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        height: 44px;
        padding: 0 20px;
        border-radius: 999px;
        font-size: 14.5px;
        font-weight: 500;
        letter-spacing: -0.01em;
        text-decoration: none;
        transition: background-color 150ms ease, transform 150ms ease;
      }
      .btn:active { transform: scale(0.98); }
      .btn-primary {
        background: #f3f4f6;
        color: #0d0e11;
        box-shadow: 0 1px 1px rgba(0, 0, 0, 0.35), 0 10px 24px -10px rgba(255, 255, 255, 0.25);
      }
      .btn-primary:hover { background: #ffffff; }
      .btn-secondary {
        background: rgba(255, 255, 255, 0.08);
        color: #f3f4f6;
        box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.11);
      }
      .btn-secondary:hover { background: rgba(255, 255, 255, 0.14); }
      .footnote {
        margin: 0;
        font-size: 12.5px;
        line-height: 1.5;
        color: #6b6e76;
      }
      .warn {
        margin: 0;
        font-size: 13px;
        color: #e9b93f;
      }
      @media (prefers-reduced-motion: reduce) {
        .pulse, .strip-track, .gaps-frame { animation: none; }
      }
    </style>
  </head>
  <body>
    <header>
      <a class="brand" href="${escapeHtml(SITE_URL)}">
        <img src="${escapeHtml(iconUrl)}" alt="" width="28" height="28" />
        <span>Spaceman</span>
      </a>
      <a class="nav-link" href="${escapeHtml(SITE_URL)}">spaceman · v${escapeHtml(RELEASE_VERSION)}</a>
    </header>
    <main>
      <section class="card" aria-labelledby="bounce-title">
        <div class="top-row">
          <span class="status" id="bounce-status"><span class="pulse" aria-hidden="true"></span><span id="bounce-status-text">Opening Spaceman…</span></span>
          <span class="badge" id="bounce-badge">${badge}</span>
        </div>
        <div class="head">
          <h1 id="bounce-title">${heading}</h1>
          <p class="desc" id="bounce-desc">${description}</p>
        </div>
        ${invalidNote ? '<p class="warn">That link target wasn’t recognized, so this will open Spaceman Settings instead.</p>' : ""}
        ${demoHtml}
        <div class="uri" id="bounce-uri">${schemeUrl}</div>
        <div class="actions">
          <a class="btn btn-primary" id="bounce-open" href="${schemeUrl}">Open in Spaceman</a>
          <a class="btn btn-secondary" href="${escapeHtml(DOWNLOAD_URL)}">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 1.75v7.5M3.75 6.25 7 9.5l3.25-3.25M2.25 12.25h9.5" /></svg>
            Download Spaceman
          </a>
        </div>
        <p class="footnote">
          ${escapeHtml(MIN_MACOS_LABEL)} · Apple silicon. Deep links only open Settings and highlight this section — they never modify your configuration.
        </p>
      </section>
    </main>
    <script>
      (function () {
        var SLUG_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
        var schemeUrl = ${JSON.stringify(target.schemeUrl)};
        var allowQuery = ${allowQueryOverride ? "true" : "false"};
        var registry = ${clientRegistry};

        function titleCase(slug) {
          return slug.split("-").map(function (p) { return p ? p[0].toUpperCase() + p.slice(1) : ""; }).join(" ");
        }

        function parseClientTo(raw) {
          if (!raw || raw.length > 200) return null;
          var body = raw.trim();
          if (body.toLowerCase().indexOf("spaceman://") === 0) body = body.slice(11);
          else if (body.indexOf("://") !== -1 || body.indexOf(":") !== -1) return null;
          body = body.split("#")[0].split("?")[0].replace(/^\\/+|\\/+$/g, "");
          if (!body) return null;
          var parts = body.split("/").map(function (s) { return s.toLowerCase(); });
          if (parts.length > 3) return null;
          for (var i = 0; i < parts.length; i++) {
            if (!parts[i] || parts[i].length > 64 || !SLUG_RE.test(parts[i])) return null;
          }
          var host = parts[0], first = parts[1], second = parts[2];
          if (host === "labs" && !second) {
            var lm = first ? registry.labs[first] : registry.settings.labs;
            return {
              url: first ? "spaceman://labs/" + first : "spaceman://labs",
              badge: lm ? lm.badge : "Labs",
              heading: lm ? lm.heading : titleCase(first),
              description: lm ? lm.description : "Open this experiment directly in Spaceman Settings › Labs."
            };
          }
          if (host === "features" && first && !second) {
            var fm = registry.features[first];
            return {
              url: "spaceman://features/" + first,
              badge: fm ? fm.badge : "Feature",
              heading: fm ? fm.heading : titleCase(first),
              description: fm ? fm.description : "Open this feature directly in Spaceman Settings."
            };
          }
          if (host === "settings") {
            var sm = first ? registry.settings[first] : null;
            var u = first ? (second ? "spaceman://settings/" + first + "/" + second : "spaceman://settings/" + first) : "spaceman://settings";
            return {
              url: u,
              badge: sm ? sm.badge : "Settings",
              heading: first ? (second ? (sm ? sm.heading : titleCase(first)) + " › " + titleCase(second) : (sm ? sm.heading : titleCase(first))) : "Spaceman Settings",
              description: sm ? sm.description : "Open Spaceman Settings directly to this pane."
            };
          }
          return null;
        }

        if (allowQuery) {
          var toParam = new URLSearchParams(window.location.search).get("to");
          if (toParam) {
            var parsed = parseClientTo(toParam);
            if (parsed) {
              schemeUrl = parsed.url;
              var openEl = document.getElementById("bounce-open");
              var uriEl = document.getElementById("bounce-uri");
              var badgeEl = document.getElementById("bounce-badge");
              var titleEl = document.getElementById("bounce-title");
              var descEl = document.getElementById("bounce-desc");
              if (openEl) openEl.setAttribute("href", parsed.url);
              if (uriEl) uriEl.textContent = parsed.url;
              if (badgeEl) badgeEl.textContent = parsed.badge;
              if (titleEl) titleEl.textContent = parsed.heading;
              if (descEl) descEl.textContent = parsed.description;
              document.title = parsed.heading + " — Spaceman";
            }
          }
        }

        var isMac = /Mac/i.test(navigator.platform || navigator.userAgent || "") && !/iPhone|iPad|iPod/i.test(navigator.userAgent || "");
        var statusText = document.getElementById("bounce-status-text");
        if (isMac) {
          window.setTimeout(function () {
            window.location.href = schemeUrl;
          }, 160);
          window.setTimeout(function () {
            if (statusText) statusText.textContent = "Not installed yet? Download below, or click Open in Spaceman.";
          }, 1800);
        } else if (statusText) {
          statusText.textContent = "Spaceman runs on macOS (Apple silicon)";
        }
      })();
    </script>
  </body>
</html>
`;
}
