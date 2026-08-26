// Statusbar - bottom strip of the main area. Left: a connectivity dot (green
// when the backend heartbeat is up, red when down) plus the number of enabled
// manifest services (and their ids); middle: compact CPU/MEM/DISK readings
// (hidden entirely while /api/stats is unreachable - graceful degradation);
// right: the gateway host as a COPY button (full URL to clipboard), a
// reset-layout button, and the theme/language toggles (they live here so the
// sidebar head stays brand + refresh + collapse).
// Pure presentation - all data arrives via props, no fetches (useStats in
// App.tsx owns the polling).

import { useEffect, useRef, useState } from "react";
import type { ServiceEntry, StatsSnapshot } from "./types";
import { fmt, t, type Lang } from "./i18n";
import { Icon } from "./icons";

interface Props {
  services: ServiceEntry[];
  lang: Lang;
  theme: "dark" | "light";
  online: boolean;
  stats?: StatsSnapshot;
  onResetLayout: () => void;
  onToggleTheme: () => void;
  onToggleLang: () => void;
}

/** Humanize a byte count with 1 decimal, B/K/M/G self-adaptive. */
function fmtBytes(n: number): string {
  if (n < 1024) return `${n}B`;
  const units = ["K", "M", "G", "T"];
  let v = n;
  let i = -1;
  do {
    v /= 1024;
    i++;
  } while (v >= 1024 && i < units.length - 1);
  return `${v.toFixed(1)}${units[i]}`;
}

/**
 * Copy text to the clipboard. `navigator.clipboard` only exists in secure
 * contexts (https or localhost) - LAN access via http://192.168.x.x has NO
 * clipboard API, so fall back to a hidden textarea + execCommand("copy")
 * (deprecated but universally supported). Returns false on failure.
 */
function copyText(text: string): boolean {
  if (navigator.clipboard && window.isSecureContext) {
    // Fire-and-forget promise; the caller's feedback path cannot await a
    // return type mix, so mirror the sync contract via a microtask no-op.
    void navigator.clipboard.writeText(text).catch(() => {});
    return true;
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.position = "fixed";
  ta.style.opacity = "0";
  document.body.appendChild(ta);
  ta.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  ta.remove();
  return ok;
}

export function Statusbar({
  services,
  lang,
  theme,
  online,
  stats,
  onResetLayout,
  onToggleTheme,
  onToggleLang,
}: Props): JSX.Element {
  const enabled = services.filter((s) => s.enabled);
  const ids = enabled.map((s) => s.id).join(" / ");

  // Copy feedback: swap in the "copied" label for 1.5s, then revert. Timer
  // ref survives re-renders and is cleared on unmount.
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(copyTimer.current), []);

  const onCopy = () => {
    if (!copyText(`${window.location.origin}/`)) return; // silent on failure
    setCopied(true);
    if (copyTimer.current) clearTimeout(copyTimer.current);
    copyTimer.current = setTimeout(() => setCopied(false), 1500);
  };

  const mem = stats
    ? stats.memTotalBytes != null
      ? `${fmtBytes(stats.memUsedBytes)} / ${fmtBytes(stats.memTotalBytes)}`
      : fmtBytes(stats.memUsedBytes) // no cgroup limit: absolute usage only
    : "";

  return (
    <footer className="statusbar" data-od-id="statusbar">
      <span className="seg">
        <span className={`dot${online ? "" : " down"}`} aria-hidden="true" />
        <span>{online ? fmt(lang, "statusAvail", enabled.length) : t(lang, "statusOffline")}</span>
        {ids && (
          <>
            <span aria-hidden="true">·</span>
            <span className="mono">{ids}</span>
          </>
        )}
      </span>
      {stats && (
        <span
          className="seg seg-stats mono"
          title={t(lang, "statsTip")}
          aria-label={t(lang, "statsTip")}
        >
          <span>CPU {stats.cpuPct.toFixed(0)}%</span>
          <span aria-hidden="true">·</span>
          <span>MEM {mem}</span>
          <span aria-hidden="true">·</span>
          <span>
            DISK {fmtBytes(stats.diskUsedBytes)} / {fmtBytes(stats.diskTotalBytes)}
          </span>
        </span>
      )}
      <span className="seg">
        <button
          className={`host-copy mono${copied ? " copied" : ""}`}
          title={t(lang, "copyUrl")}
          aria-label={t(lang, "copyUrl")}
          onClick={onCopy}
        >
          {window.location.host}
          {copied && <span className="copied-tag">{t(lang, "copied")}</span>}
        </button>
        <span aria-hidden="true">·</span>
        <span>{t(lang, "statusMounted")}</span>
        <button
          className="icon-btn"
          title={t(lang, "resetLayout")}
          aria-label={t(lang, "resetLayout")}
          onClick={onResetLayout}
        >
          <Icon name="reset" />
        </button>
        <button
          className="icon-btn"
          title={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
          aria-label={theme === "dark" ? t(lang, "toLight") : t(lang, "toDark")}
          onClick={onToggleTheme}
        >
          <Icon name={theme === "dark" ? "sun" : "moon"} />
        </button>
        <button
          className="icon-btn"
          title={t(lang, "switchLang")}
          aria-label={t(lang, "switchLang")}
          onClick={onToggleLang}
        >
          <Icon name="globe" />
        </button>
      </span>
    </footer>
  );
}
