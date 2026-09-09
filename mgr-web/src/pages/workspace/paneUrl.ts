// paneUrl - per-sandbox pane URL resolution (design §2.4): the single place
// that decouples workbench pane URLs from the mgr.localhost origin the
// workbench SPA assumed (web/src did everything same-origin). Four rules:
//
//   relative gateway path (/code-server/, /vnc/...)
//     -> http://sbx-<name>.mgr.localhost + url     (the sandbox's OWN gateway
//        strips the prefix and reverse-proxies; D3/D4, zero backend changes)
//   /preview/<port>/... (user web buttons)
//     -> /api/sbx/<name>/preview/...               (SAME ORIGIN on mgr,
//        through mgr's per-sandbox proxy - avoids iframe mixed content and
//        leaves one seam for future auth; the proxy forwards HTTP+SSE+WS)
//   absolute url (piWeb under mgr: PI_WEB_URL is the subdomain form)
//     -> verbatim                                    (契约 3: pi-web needs its
//        .localhost-suffix public hostname, never a bare alias)
//   agent/terminal pty WS
//     -> ws(s)://<mgr origin>/api/sbx/<name>/api/term/ws?cmd=...  (proxied)
//
// Sandbox names are validated slugs [a-z0-9-] on the backend (routes.rs
// validate_name), so interpolating them into URLs needs no escaping.

import type { ServiceEntry } from "./types";

/** The sandbox's own gateway origin (total-gateway subdomain; same fixed
 * identity as Sandbox.entry_url / caddy.rs render / composegen aliases). */
export function sandboxGatewayOrigin(sandbox: string): string {
  return `http://sbx-${sandbox}.mgr.localhost`;
}

/** Resolve a web-type service's iframe src for a sandbox. */
export function urlFor(service: ServiceEntry, sandbox: string): string | undefined {
  const url = service.url;
  if (url === undefined) return undefined;
  if (/^https?:\/\//i.test(url)) {
    // Absolute URL (piWeb). Under mgr this is already the piweb subdomain
    // (composegen's PI_WEB_URL override); the {host} substitution only
    // survives on adopted stacks without PI_WEB_URL, where substituting the
    // hostname mgr-web was reached on is the legacy behavior we keep.
    return url.replace("{host}", window.location.hostname);
  }
  if (url.startsWith("/preview/")) {
    // User web button: the app's dev-server proxy, same-origin on mgr.
    return `/api/sbx/${sandbox}${url}`;
  }
  return `${sandboxGatewayOrigin(sandbox)}${url}`;
}

/** Build the proxied pty WebSocket URL for an agent-type service command. */
export function termWsUrl(sandbox: string, cmd: string): string {
  const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${window.location.host}/api/sbx/${sandbox}/api/term/ws?cmd=${encodeURIComponent(cmd)}`;
}
