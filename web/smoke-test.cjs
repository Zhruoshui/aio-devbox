// Headless smoke test for the workspace UI (sidebar + golden-layout).
//
// Manifest-driven: fetches GET /api/manifest, then asserts:
//   1. the sidebar shows exactly the enabled services (no dead buttons - a
//      service with enabled=false must NOT appear, e.g. opencode when the
//      binary is not baked into the image);
//   2. the terminal opens by default (single stack) and its pty WS connects;
//   3. sidebar buttons are LAUNCHERS: each click creates a NEW instance
//      (click twice -> two more tabs, numbered titles);
//   4. each enabled web service's button opens an instance whose iframe
//      actually loads the service UI;
//   5. instances close via the golden-layout tab close icon;
//   6. a user button can be registered via the UI form (POST /api/buttons),
//      launched, and deleted (DELETE /api/buttons/:id);
//   7. the statusbar shows CPU / MEM / DISK readings (GET /api/stats poll);
//   8. the golden-layout tab set survives a page reload (aio.layout restore).
//
// Run inside a node:20-bookworm container with chromium installed, --network
// host so localhost:8080 reaches the gateway. Uses puppeteer-core against the
// system chromium (no browser download). A prebuilt image `aio-smoke`
// (node:20-bookworm + chromium) speeds up reruns:
//
//   docker run --rm --network host -v "$PWD/web":/web -w /web aio-smoke \
//     node smoke-test.cjs

const puppeteer = require("puppeteer-core");

const URL = "http://localhost:8080/";
const USER = "admin";
const PASS = "admin";
const AUTH = { Authorization: "Basic " + Buffer.from(`${USER}:${PASS}`).toString("base64") };

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  // Fetch the manifest first so we know exactly which buttons to expect -
  // correct for any profile combination (no profiles => terminal only;
  // --profile code-server => +codeServer; opencode only if baked in).
  const manifest = await fetch(`${URL}api/manifest`, { headers: AUTH }).then((r) => r.json());
  const enabled = manifest.services.filter((s) => s.enabled);
  const disabled = manifest.services.filter((s) => !s.enabled);
  const expectedWeb = enabled.filter((s) => s.type === "web");
  console.log(
    "manifest enabled:",
    enabled.map((s) => `${s.id}(${s.type})`).join(", ") || "(none)",
    "| disabled (must NOT appear):",
    disabled.map((s) => s.id).join(", ") || "(none)",
  );

  const fs = require("fs");
  const executablePath =
    process.env.CHROMIUM_PATH ||
    ["/usr/bin/chromium", "/usr/bin/chromium-browser"].find((p) => fs.existsSync(p));
  if (!executablePath) {
    console.error("no chromium found; set CHROMIUM_PATH or install chromium");
    process.exit(1);
  }
  const browser = await puppeteer.launch({
    executablePath,
    headless: "new",
    args: ["--no-sandbox", "--disable-setuid-sandbox", "--disable-gpu"],
  });
  const page = await browser.newPage();
  // Desktop viewport: the UI's <=880px responsive guard collapses the sidebar
  // and the smoke assertions target the expanded layout.
  await page.setViewport({ width: 1440, height: 900 });
  await page.authenticate({ username: USER, password: PASS });

  const errors = [];
  const failedReqs = [];
  const badResponses = [];
  // Tolerate known-irrelevant failures:
  //  - /api: reserved seam stub (502 by design) on bare /api and non-real
  //    sub-paths.
  //  - vsda (vsda.js / vsda_bg.wasm): optional VS Code digital-signature
  //    module not shipped in code-server's open builds (harmless 404).
  //  - open-vsx.org: code-server's extension marketplace, blocked by this
  //    sandbox's network policy (network-level, not a gateway issue).
  //  - /vnc/package.json: noVNC's vnc.html fetches it for a version display;
  //    websockify does not serve it (harmless 404, present since Phase G).
  //  - code-server vscode-remote-resource: aborted (ERR_ABORTED) when a
  //    code-server tab/iframe is torn down mid-fetch - benign teardown noise.
  //  - /vnc/{vendor,core}/*.js: same teardown race when a noVNC iframe is
  //    closed or re-shown while its module scripts are still loading
  //    (ERR_ABORTED only - a 4xx on these paths is still a real failure).
  const isTolerable = (url) =>
    url.includes("vsda") ||
    url.includes("open-vsx.org") ||
    url.includes("vscode-remote-resource") ||
    url.endsWith("/vnc/package.json");
  const isTolerableTeardownAbort = (url) =>
    url.includes("/vnc/vendor/") || url.includes("/vnc/core/");
  page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push("console.error: " + msg.text());
  });
  page.on("requestfailed", (req) => {
    if (isTolerable(req.url())) return;
    if (req.failure()?.errorText === "net::ERR_ABORTED" && isTolerableTeardownAbort(req.url())) return;
    failedReqs.push(`${req.url()} :: ${req.failure()?.errorText}`);
  });
  page.on("response", (res) => {
    if (res.status() < 400 || isTolerable(res.url())) return;
    badResponses.push(`${res.status()} ${res.url()}`);
  });

  // domcontentloaded (not networkidle0): the terminal pty WebSocket stays open
  // and would prevent networkidle0 from ever settling.
  await page.goto(URL, { waitUntil: "domcontentloaded", timeout: 30000 });

  // --- 1. Sidebar reflects the manifest exactly ------------------------------
  await page.waitForSelector(".sidebar", { timeout: 10000 });
  const sidebarTitles = await page.$$eval(".launch-btn .launch-label", (els) =>
    els.map((e) => (e.textContent || "").trim()),
  );
  const expectedTitles = enabled.map((s) => s.label);
  const sidebarOk =
    sidebarTitles.length === expectedTitles.length &&
    expectedTitles.every((t) => sidebarTitles.includes(t)) &&
    disabled.every((s) => !sidebarTitles.includes(s.label));
  console.log("sidebar buttons:", JSON.stringify(sidebarTitles), "| sidebarOk:", sidebarOk);

  // --- 2. Terminal opens by default (single stack) + pty WS connects --------
  await page.waitForSelector(".lm_root", { timeout: 10000 });
  await sleep(3000); // let the pty WS connect and onopen write its line
  const defaultTabCount = await page.$$eval(".lm_tab", (els) => els.length);
  const defaultTabTitles = await page.$$eval(".lm_tab .lm_title", (els) =>
    els.map((e) => (e.textContent || "").trim()),
  );
  const xtermText = await page
    .$$eval(".pane-xterm .xterm-rows", (els) => els.map((e) => e.textContent || "").join("\n"))
    .catch(() => "");
  const terminalDefaultOk =
    defaultTabCount === 1 &&
    defaultTabTitles[0] === "Terminal" &&
    xtermText.includes("Terminal connected");
  console.log("default tabs:", JSON.stringify(defaultTabTitles), "| terminalDefaultOk:", terminalDefaultOk);

  // --- 3. Launcher semantics: each click creates a NEW instance -------------
  await page.click('.launch-btn[title^="Terminal"]');
  await sleep(500);
  await page.click('.launch-btn[title^="Terminal"]');
  await sleep(500);
  const launcherTitles = await page.$$eval(".lm_tab .lm_title", (els) =>
    els.map((e) => (e.textContent || "").trim()),
  );
  const xtermCount = await page.$$eval(".pane-xterm", (els) => els.length);
  const launcherOk =
    launcherTitles.length === 3 &&
    launcherTitles.includes("Terminal") &&
    launcherTitles.includes("Terminal (2)") &&
    launcherTitles.includes("Terminal (3)") &&
    xtermCount === 3;
  console.log("launcher tabs:", JSON.stringify(launcherTitles), "| launcherOk:", launcherOk);

  // --- 4. Each enabled web service button opens a loading iframe ------------
  const iframeResults = [];
  for (const svc of expectedWeb) {
    // {host} in a manifest url is substituted with window.location.hostname
    // by IframePane.tsx at render time (pi-web's published-port URL); the
    // browser here is on localhost, so mirror that substitution.
    const srcPrefix = svc.url.split("?")[0].replace("{host}", "localhost");
    await page.click(`.launch-btn[title^="${svc.label}"]`);
    await page.waitForSelector(`.pane-iframe[src^="${srcPrefix}"]`, { timeout: 10000 });
    // The iframe element exists immediately, but its frame starts at
    // about:blank and navigates asynchronously - wait for the real URL.
    const frame = await page
      .waitForFrame((f) => f.url().includes(srcPrefix), { timeout: 15000 })
      .catch(() => null);
    let loaded = false;
    let detail = "frame not found (timeout)";
    if (frame) {
      try {
        if (svc.id === "codeServer") {
          await frame.waitForSelector('script[src*="workbench.js"]', { timeout: 10000 });
          const hasWorkbench = await frame
            .waitForSelector(".monaco-workbench", { timeout: 15000 })
            .then(() => true)
            .catch(() => false);
          loaded = true;
          detail = hasWorkbench
            ? "workbench.js + .monaco-workbench present"
            : "workbench.js present (workbench still booting)";
        } else {
          await frame.waitForSelector("body", { timeout: 10000 });
          loaded = true;
          detail = "frame body loaded";
        }
      } catch (e) {
        detail = "timeout: " + (e instanceof Error ? e.message : String(e));
      }
    }
    iframeResults.push({ id: svc.id, loaded, detail });
  }
  const iframesOk = iframeResults.every((r) => r.loaded);
  console.log("iframeResults:", JSON.stringify(iframeResults));

  // --- 5. Close via the tab's close icon ------------------------------------
  // NB: golden-layout 2.6 renders the per-tab close icon as `.lm_close_tab`
  // inside `.lm_tab`; `.lm_close` is a header-level control that would close
  // the WHOLE stack - do not target it.
  const beforeClose = await page.$$eval(".lm_tab", (els) => els.length);
  const closeBtns = await page.$$(".lm_tab.lm_active .lm_close_tab");
  const closeTarget = closeBtns[0] ?? (await page.$(".lm_close_tab"));
  if (closeTarget) {
    await closeTarget.click();
    await sleep(500);
  }
  const afterClose = await page.$$eval(".lm_tab", (els) => els.length);
  const closeOk = closeTarget !== null && afterClose === beforeClose - 1;
  console.log("close:", { beforeClose, afterClose, closeOk });

  // --- 6. Register a user button, launch it, delete it ----------------------
  // RegisterButton now opens the Kumo-style modal dialog (register-btn in the
  // sidebar footer -> .dialog with #f-label / #f-cmd fields -> .btn-primary).
  await page.click(".register-btn");
  await page.waitForSelector(".overlay.open .dialog", { timeout: 5000 });
  await page.waitForSelector("#f-label", { visible: true, timeout: 5000 });
  await page.type("#f-label", "smokebtn");
  await page.type("#f-cmd", "echo smokeok-marker");
  await Promise.all([
    page.waitForResponse((r) => r.url().includes("/api/buttons") && r.request().method() === "POST"),
    page.click(".btn-primary"),
  ]);
  // The SPA refreshes the manifest after a successful POST; the new button is
  // visible once command_exists finds `echo` (/usr/bin/echo on the base image).
  await page.waitForSelector('.launch-btn[title^="smokebtn"]', { timeout: 10000 });
  await page.click('.launch-btn[title^="smokebtn"]');
  await sleep(2500); // pty runs the cmd; output lands in the xterm buffer
  const allXtermText = await page
    .$$eval(".pane-xterm .xterm-rows", (els) => els.map((e) => e.textContent || "").join("\n"))
    .catch(() => "");
  const userCmdRan = allXtermText.includes("smokeok-marker");

  // Delete via the API (the UI ✕ is hover-revealed; the API is the contract),
  // then refresh via the UI and assert the button is gone.
  const delRes = await fetch(`${URL}api/buttons/smokebtn`, { method: "DELETE", headers: AUTH });
  await page.click(".refresh-btn");
  await sleep(1000);
  const titlesAfterDelete = await page.$$eval(".launch-btn .launch-label", (els) =>
    els.map((e) => (e.textContent || "").trim()),
  );
  const deletedOk = delRes.ok && !titlesAfterDelete.includes("smokebtn");
  console.log("register/run/delete:", { userCmdRan, deletedOk, titlesAfterDelete });

  // --- 7. Statusbar resource readings (GET /api/stats poll) ---------------
  // The stats segment stays hidden until the first poll lands (up to ~3s +
  // render); CPU / MEM / DISK labels must all be present.
  await page.waitForSelector(".statusbar .seg-stats", { timeout: 10000 });
  const statsText = await page.$eval(
    ".statusbar .seg-stats",
    (e) => (e.textContent || "").replace(/\s+/g, " ").trim(),
  );
  const statsOk =
    statsText.includes("CPU") && statsText.includes("MEM") && statsText.includes("DISK");
  console.log("statusbar stats:", JSON.stringify(statsText), "| statsOk:", statsOk);

  // --- 8. Layout persistence: reload restores tabs --------------------------
  // Snapshot the current tab titles, reload, and expect the same set back
  // (order may differ; golden-layout restores content, not z-order).
  const titlesBeforeReload = await page.$$eval(".lm_tab .lm_title", (els) =>
    els.map((e) => (e.textContent || "").trim()).sort(),
  );
  await page.reload({ waitUntil: "domcontentloaded", timeout: 30000 });
  await page.waitForSelector(".lm_root", { timeout: 10000 });
  await page.waitForFunction(
    () => document.querySelectorAll(".lm_tab").length > 0,
    { timeout: 10000 },
  );
  await sleep(1500); // allow restored xterm panes to re-render
  const titlesAfterReload = await page.$$eval(".lm_tab .lm_title", (els) =>
    els.map((e) => (e.textContent || "").trim()).sort(),
  );
  const layoutRestoreOk =
    titlesAfterReload.length === titlesBeforeReload.length &&
    titlesBeforeReload.every((t, i) => titlesAfterReload[i] === t);
  console.log(
    "layout restore:",
    JSON.stringify({ before: titlesBeforeReload, after: titlesAfterReload }),
    "| layoutRestoreOk:",
    layoutRestoreOk,
  );

  await browser.close();

  const ok =
    sidebarOk &&
    terminalDefaultOk &&
    launcherOk &&
    iframesOk &&
    closeOk &&
    userCmdRan &&
    deletedOk &&
    statsOk &&
    layoutRestoreOk &&
    failedReqs.length === 0 &&
    badResponses.length === 0;
  console.log(
    JSON.stringify(
      {
        ok,
        sidebarOk,
        terminalDefaultOk,
        launcherOk,
        iframesOk,
        closeOk,
        userCmdRan,
        deletedOk,
        statsOk,
        layoutRestoreOk,
        failedReqs,
        badResponses,
        pageErrors: errors,
      },
      null,
      2,
    ),
  );
  if (!ok) console.error("ASSERTION FAILED");
  process.exit(ok ? 0 : 2);
})().catch((e) => {
  console.error("SMOKE TEST FAILED:", e);
  process.exit(1);
});
