// CodeServerPane - the code-server pane (D4: unified entry + on-demand
// instance). The iframe side is the standard gateway-path web pane (the
// sandbox's own gateway strips /code-server/ and reverse-proxies); what this
// component adds is the START state machine in front of it:
//
//   probe (app:8200 via the mgr proxy) -> reachable? iframe right away
//     (adopted/stock stacks may have code-server already running, and mgr
//     never double-guesses - design §2.5)
//   not reachable -> "starting" placeholder -> POST the mgr service-start
//     (pulls up the profile-gated container) -> poll the probe until
//     reachable -> iframe.
//
// Design notes (design §2.5):
//   - closing the pane does NOT stop code-server (the editor session and
//     unsaved state survive; the explicit stop is the sandbox's own stop,
//     which still carries the full profile set - 契约 4). Unmount only
//     cancels the polling.
//   - a stopped sandbox never reaches this pane through the tree (greyed
//     out); the backend liveness guard is the backstop for a pane restored
//     from a saved layout after its sandbox stopped - its rejection lands
//     in the failed state below.
//   - poll cadence 1s, capped: the container create+start of a pre-built
//     image is seconds, and the app-side probe (400ms TCP timeout) is what
//     flips us to the iframe; a cap keeps a crash-looping service from
//     spinning the placeholder forever.
//
// The probe rides the sandbox's OWN /api/buttons/probe endpoint through
// mgr's per-sandbox proxy (api.ts probeSandboxPort): port 8200 is
// code-server on the app's shared netns (network_mode: service:app), so a
// listening port means the editor is actually up INSIDE the sandbox - not
// just that a container exists.
//
// The effect deliberately holds NO lang dependency: it stores only the
// backend's error text and renders localized chrome from props, so a
// language switch mid-start never restarts the machine (which would re-fire
// the start POST).

import { useEffect, useRef, useState } from "react";

import { probeSandboxPort, startSandboxService } from "../../../api";
import { t, type Lang } from "../../../i18n";
import { Icon } from "../../../icons";
import type { ServiceEntry } from "../types";
import { IframePane } from "./IframePane";

/** code-server's port on the app's shared netns (target app:8200 in the
 * sandbox's services.toml). */
const CODE_SERVER_PORT = 8200;
const PROBE_INTERVAL_MS = 1000;
/** 2 minutes of not-listening after a successful start call = give up with
 * the failure message (a healthy create+start binds within seconds). */
const MAX_PROBE_ATTEMPTS = 120;

type Phase =
  | { kind: "probing" }
  | { kind: "starting" }
  | { kind: "ready" }
  /** message is the BACKEND error text ("" = the poll-timeout case, whose
   * chrome is a local string - see the failed render). */
  | { kind: "failed"; message: string };

export function CodeServerPane({
  service,
  sandbox,
  lang,
}: {
  service: ServiceEntry;
  sandbox: string;
  lang: Lang;
}): JSX.Element {
  const [phase, setPhase] = useState<Phase>({ kind: "probing" });
  // Retry affordance for the failed state: bumping run re-runs the machine
  // (probe first again - the service may have come up in the meantime).
  const [run, setRun] = useState(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    let disposed = false;
    let stopped = false;
    let attempts = 0;

    const schedule = (fn: () => void, ms: number): void => {
      timerRef.current = setTimeout(fn, ms);
    };
    const fail = (message: string): void => {
      if (disposed || stopped) return;
      stopped = true;
      // Stop the already-scheduled poll too - a failed start must not keep
      // probing (and flip itself back to "starting").
      if (timerRef.current !== undefined) clearTimeout(timerRef.current);
      setPhase({ kind: "failed", message });
    };

    const poll = (): void => {
      probeSandboxPort(sandbox, CODE_SERVER_PORT)
        .then(({ listening }) => {
          if (disposed || stopped) return;
          if (listening) {
            setPhase({ kind: "ready" });
            return;
          }
          attempts += 1;
          if (attempts === 1) {
            // First miss: ask mgr to pull the service up, keep polling.
            // `up -d <svc>` is idempotent (docker.rs compose_service_up).
            setPhase({ kind: "starting" });
            startSandboxService(sandbox, "code-server").catch((e) => {
              fail(e instanceof Error ? e.message : String(e));
            });
          } else if (attempts >= MAX_PROBE_ATTEMPTS) {
            fail("");
            return;
          }
          schedule(poll, PROBE_INTERVAL_MS);
        })
        .catch((e) => {
          // The probe itself failing (proxy 502: sandbox stopped mid-flight)
          // is terminal - retrying against a stopped sandbox would spin.
          fail(e instanceof Error ? e.message : String(e));
        });
    };
    poll();

    return () => {
      disposed = true;
      if (timerRef.current !== undefined) clearTimeout(timerRef.current);
    };
  }, [sandbox, run]);

  if (phase.kind === "ready") {
    return <IframePane service={service} sandbox={sandbox} />;
  }

  if (phase.kind === "failed") {
    return (
      <div className="pane pane-note" role="alert">
        <p className="pane-note-error">
          {t(lang, "csStartFailed")}
          {phase.message === "" ? t(lang, "csNotReady") : phase.message}
        </p>
        <button className="btn btn-secondary btn-sm" onClick={() => setRun((r) => r + 1)}>
          {t(lang, "csRetry")}
        </button>
      </div>
    );
  }

  return (
    <div className="pane pane-note" role="status">
      <p>
        <span className="pane-note-spin">
          <Icon name="refresh" />
        </span>
        {t(lang, phase.kind === "probing" ? "csProbing" : "csStarting").replace(
          "{s}",
          service.label,
        )}
      </p>
    </div>
  );
}
