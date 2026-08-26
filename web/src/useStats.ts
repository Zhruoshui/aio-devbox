// useStats - polls GET /api/stats every 3s and doubles as the backend
// heartbeat: any successful fetch -> online: true, any failure -> online:
// false (R3 reuses this channel instead of a dedicated probe endpoint).
// Poll period (3s) is coprime with the backend's 2s sample period so the
// displayed values don't sync into a sawtooth. Extracted from App.tsx per
// the "no fetches in Statusbar" contract; App passes results down as props.

import { useEffect, useRef, useState } from "react";
import type { StatsSnapshot } from "./types";

export interface StatsState {
  stats?: StatsSnapshot;
  online: boolean;
}

const POLL_MS = 3000;

export function useStats(): StatsState {
  // `online` starts true: the initial value is seeded by App from the manifest
  // fetch result, and the first stats poll corrects it within 3s anyway.
  const [state, setState] = useState<StatsState>({ online: true });
  const abortRef = useRef<AbortController>();

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | undefined;

    const poll = async () => {
      const ctrl = new AbortController();
      abortRef.current = ctrl;
      try {
        const r = await fetch("/api/stats", { signal: ctrl.signal });
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        const stats = (await r.json()) as StatsSnapshot;
        if (!cancelled) setState({ stats, online: true });
      } catch {
        // Backend unreachable / response bad: keep the last snapshot hidden
        // (R2.4 graceful degradation - the whole stats segment disappears)
        // and flip the status dot to down.
        if (!cancelled) setState((s) => ({ ...s, stats: undefined, online: false }));
      }
    };

    void poll();
    timer = setInterval(() => void poll(), POLL_MS);
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
      abortRef.current?.abort();
    };
  }, []);

  return state;
}
