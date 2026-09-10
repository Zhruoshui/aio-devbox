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
//     -> verbatim (+ port-follow; 契约 3: pi-web needs its .localhost-suffix
//        public hostname, never a bare alias)
//   agent/terminal pty WS
//     -> ws(s)://<mgr origin>/api/sbx/<name>/api/term/ws?cmd=...  (proxied)
//
// Rules 1 and 3 pass through withMgrPort: URLs aimed at the total gateway
// carry the host port the browser itself used (09-10-mgr-subdomain-port-
// follow); rule 2 and rule 4 are same-origin and need nothing.
//
// Sandbox names are validated slugs [a-z0-9-] on the backend (routes.rs
// validate_name), so interpolating them into URLs needs no escaping.

import type { ServiceEntry } from "./types";

/** The host port the browser used to reach the mgr UI, if any. `""` for the
 * default port (80 on http, or file/other schemes) — i.e. the classic
 * host-port-80 deployment, where URLs must stay port-less. */
function browserPort(): string {
  return window.location.port;
}

/** Port-following for `*.mgr.localhost` URLs (design §1 of
 * 09-10-mgr-subdomain-port-follow): when the browser reaches the mgr UI
 * through a non-default host port (e.g. `sbx ports --publish 8081:80`), every
 * subdomain URL must carry that same port — `*.mgr.localhost` resolves to
 * 127.0.0.1, and a port-less URL would hit host :80 which may not be
 * published at all. Only appends when the URL has NO explicit port; URLs
 * that already carry one (piWeb's `http://{host}:30141/` legacy form after
 * `{host}` substitution) pass through untouched. Port-less form is preserved
 * verbatim when the browser itself used the default port (80), so the
 * classic deployment keeps its canonical URLs. */
export function withMgrPort(url: string): string {
  const port = browserPort();
  if (!port || port === "80") return url;
  // Match `scheme://host` followed by end-of-string, `/`, `?` or `#` — i.e.
  // an origin with NO port. `[^/:?#]+` cannot cross a `:`, so an explicit
  // port (e.g. `http://x:30141/`) never matches and is left alone.
  return url.replace(/^(https?:\/\/[^/:?#]+)(?=$|[/?#])/, `$1:${port}`);
}

/** The sandbox's own gateway origin (total-gateway subdomain; same fixed
 * identity as Sandbox.entry_url / caddy.rs render / composegen aliases). */
export function sandboxGatewayOrigin(sandbox: string): string {
  return withMgrPort(`http://sbx-${sandbox}.mgr.localhost`);
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
    // Either way the subdomain form goes through the total gateway, so it
    // needs the same port-following as sandboxGatewayOrigin (an explicit
    // port in the legacy form is preserved by withMgrPort).
    return withMgrPort(url.replace("{host}", window.location.hostname));
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
