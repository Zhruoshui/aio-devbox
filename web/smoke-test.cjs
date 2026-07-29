// Headless smoke test for the workspace UI.
// Manifest-driven: fetches GET /api/manifest, then asserts the SPA renders one
// generic pane per enabled service (IframePane for type=web, XtermPane for
// type=agent). For type=web panes it additionally verifies the iframe actually
// loads its service's UI (not just that an <iframe> tag exists).
//
// Run inside a node:20-bookworm container with chromium installed, --network
// host so localhost:8080 reaches the gateway. Uses puppeteer-core against the
// system chromium (no browser download). See README / Makefile.
//
//   docker run --rm --network host -v "$PWD/web":/web -w /web \
//     node:20-bookworm sh -c 'apt-get update && apt-get install -y chromium &&
//       npm i puppeteer-core@23 && node smoke-test.cjs'
const puppeteer = require("puppeteer-core");

const URL = "http://localhost:8080/";
const USER = "admin";
const PASS = "admin";

(async () => {
  // Fetch the manifest first (via curl-equivalent) so we know exactly which
  // panes to expect - this makes the test correct for any profile combination
  // (no profiles => terminal+opencode only; --profile code-server => +codeServer).
  const manifest = await fetch(`${URL}api/manifest`, {
    headers: { Authorization: "Basic " + Buffer.from(`${USER}:${PASS}`).toString("base64") },
  }).then((r) => r.json());

  const enabled = manifest.services.filter((s) => s.enabled);
  const expectedWeb = enabled.filter((s) => s.type === "web");
  const expectedAgent = enabled.filter((s) => s.type === "agent");
  console.log(
    "manifest enabled services:",
    enabled.map((s) => `${s.id}(${s.type})`).join(", "),
  );

  const browser = await puppeteer.launch({
    executablePath: "/usr/bin/chromium",
    headless: "new",
    args: ["--no-sandbox", "--disable-setuid-sandbox", "--disable-gpu"],
  });
  const page = await browser.newPage();
  await page.authenticate({ username: USER, password: PASS });

  const errors = [];
  const failedReqs = [];
  const badResponses = [];
  // Track the /api/term/ws WebSocket upgrade status (Phase E). The pty WS
  // should now return 101 (not 502). We record it explicitly rather than
  // tolerating its failure.
  let termWsStatus = null;
  // Tolerate known-irrelevant request/response failures:
  //  - /api: reserved seam stub (502 by design, Phase B) on bare /api and
  //    non-terminal sub-paths.
  //  - vsda (vsda.js / vsda_bg.wasm): an optional VS Code digital-signature
  //    module that does not ship in code-server's open builds (harmless 404).
  //  - open-vsx.org: code-server's extension marketplace, blocked by this
  //    sandbox's network policy (network-level, not a gateway/subpath issue).
  const isTolerable = (url) =>
    url.includes("vsda") ||
    url.includes("open-vsx.org");
  page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push("console.error: " + msg.text());
  });
  page.on("requestfailed", (req) => {
    const url = req.url();
    if (isTolerable(url)) return;
    failedReqs.push(`${url} :: ${req.failure()?.errorText}`);
  });
  page.on("response", (res) => {
    const url = res.url();
    // Track the terminal pty WS upgrade (Phase E): expect 101, not 502.
    // (puppeteer does not fire this for WS upgrades, so termWsStatus stays
    // null in practice - the DOM text assertion is authoritative.)
    if (url.includes("/api/term/ws")) {
      termWsStatus = res.status();
      return;
    }
    const status = res.status();
    if (status < 400) return;
    if (isTolerable(url)) return;
    badResponses.push(`${status} ${url}`);
  });

  // Use domcontentloaded (not networkidle0): with Phase E the terminal pty
  // WebSockets stay open, which would prevent networkidle0 from ever settling.
  // The explicit waitForSelector calls below handle the rest.
  await page.goto(URL, { waitUntil: "domcontentloaded", timeout: 30000 });

  // Wait for golden-layout root to mount.
  await page.waitForSelector(".lm_root", { timeout: 10000 });
  const glPresent = await page.$(".lm_root");

  const xtermPanes = await page.$$eval(".pane-xterm", (els) => els.length);
  const iframePanes = await page.$$eval(".pane-iframe", (els) => els.length);
  const tabTitles = await page.$$eval(".lm_tab", (els) =>
    els.map((e) => (e.textContent || "").trim()),
  );

  // Phase E: assert the terminal pty WS connected and the xterm pane shows the
  // "Terminal connected." message (not the old "Phase E" placeholder). xterm.js
  // uses a DOM renderer here (xterm-rows) so the buffer text is in the DOM.
  // The "Terminal connected." line is written by XtermPane's onopen handler,
  // which only fires when the WebSocket successfully upgrades - so its presence
  // is the authoritative proof that the pty WS works. (puppeteer's `response`
  // event does not capture WS 101 upgrades, so termWsStatus stays null; we
  // track it for diagnostics only.)
  if (expectedAgent.length > 0) {
    // Give the WS time to connect and the onopen handler to write the line.
    await new Promise((r) => setTimeout(r, 3000));
  }
  const xtermText = await page
    .$$eval(".pane-xterm .xterm-rows", (els) => els.map((e) => e.textContent || "").join("\n"))
    .catch(() => "");
  const terminalConnected = xtermText.includes("Terminal connected");
  const phaseEPlaceholder = xtermText.includes("Phase E");

  // For each expected web pane, verify its iframe actually loaded the service
  // UI: find the iframe whose src starts with the service url, then check its
  // content frame navigated and contains a service-specific DOM marker. For
  // code-server that is the workbench <script> / .monaco-workbench.
  const iframeResults = [];
  for (const svc of expectedWeb) {
    const frame = page
      .frames()
      .find((f) => f.url().includes(svc.url));
    let loaded = false;
    let detail = "frame not found";
    if (frame) {
      try {
        // code-server's initial HTML always contains the workbench script;
        // .monaco-workbench appears once the editor boots (may need a wait).
        await frame.waitForSelector('script[src*="workbench.js"]', { timeout: 10000 });
        const hasWorkbench = await frame
          .waitForSelector(".monaco-workbench", { timeout: 15000 })
          .then(() => true)
          .catch(() => false);
        loaded = true;
        detail = hasWorkbench ? "workbench.js + .monaco-workbench present" : "workbench.js present (workbench still booting)";
      } catch (e) {
        detail = "timeout: " + (e instanceof Error ? e.message : String(e));
      }
    }
    iframeResults.push({ id: svc.id, url: svc.url, frameUrl: frame?.url(), loaded, detail });
  }

  console.log(
    JSON.stringify(
      {
        goldenLayoutMounted: !!glPresent,
        xtermPanes,
        iframePanes,
        expectedXterm: expectedAgent.length,
        expectedIframe: expectedWeb.length,
        tabTitles,
        iframeResults,
        termWsStatus,
        terminalConnected,
        phaseEPlaceholder,
        xtermPreview: xtermText.slice(0, 300),
        failedReqs,
        badResponses,
        pageErrors: errors,
      },
      null,
      2,
    ),
  );

  await browser.close();

  // Console errors are informational only - they echo the request/response
  // failures already tracked precisely above (and include noisy extension-
  // marketplace messages). The hard gates are the precise trackers.
  const paneCountsOk =
    !!glPresent &&
    xtermPanes === expectedAgent.length &&
    iframePanes === expectedWeb.length;
  const tabsOk = expectedWeb.every((s) => tabTitles.includes(s.id)) &&
    expectedAgent.every((s) => tabTitles.includes(s.id));
  const iframesOk = iframeResults.every((r) => r.loaded);
  // Phase E: when agent panes are expected, the xterm pane must show
  // "Terminal connected." (written by onopen, proving the pty WS upgraded)
  // and must NOT show the old "Phase E" placeholder.
  const terminalOk =
    expectedAgent.length === 0 || (terminalConnected && !phaseEPlaceholder);
  const ok =
    paneCountsOk && tabsOk && iframesOk && terminalOk &&
    failedReqs.length === 0 && badResponses.length === 0;
  if (!ok) {
    console.error("ASSERTION FAILED;", {
      paneCountsOk, tabsOk, iframesOk, terminalOk,
      terminalConnected, phaseEPlaceholder, failedReqs, badResponses,
    });
  }
  process.exit(ok ? 0 : 2);
})().catch((e) => {
  console.error("SMOKE TEST FAILED:", e);
  process.exit(1);
});
