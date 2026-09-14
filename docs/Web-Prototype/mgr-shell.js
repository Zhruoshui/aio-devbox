/* mgr-web 重设计 · 共享 shell 脚本
 * 1) 注入图标 sprite（对应 icons.tsx 的 <use href="#i-*">）
 * 2) 渲染 48px 图标栏（对应 App.tsx 的 aside.sidebar → 新 .rail）
 * 3) 主题持久化：沿用真实应用的 localStorage 键 mgr.theme 与 data-mode 钩子 */
(function () {
  const P = (d) => `<path d="${d}"/>`;
  const ICONS = {
    cube: P("M12 2.5 3.5 7v10l8.5 4.5 8.5-4.5V7z") + P("M3.5 7 12 11.5 20.5 7M12 11.5v10"),
    grid: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
    terminal: P("m5 7 5 5-5 5M12 17h7"),
    layers: P("m12 3 9 5-9 5-9-5z") + P("m3 13 9 5 9-5"),
    sliders: P("M4 7h9M17 7h3M4 12h3M11 12h9M4 17h11M19 17h1") + '<circle cx="15" cy="7" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="17" r="2"/>',
    chart: P("M4 20V11M10 20V5M16 20v-7M2 20h20"),
    sun: '<circle cx="12" cy="12" r="4"/>' + P("M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"),
    moon: P("M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"),
    globe: '<circle cx="12" cy="12" r="9"/>' + P("M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"),
    "chev-r": P("m9 6 6 6-6 6"),
    "chev-l": P("m15 6-6 6 6 6"),
    "chev-d": P("m6 9 6 6 6-6"),
    play: P("M7 5v14l11-7z"),
    stop: '<rect x="6" y="6" width="12" height="12" rx="1.5"/>',
    restart: P("M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5"),
    edit: P("M4 20h4L18 10l-4-4L4 16z") + P("m13 7 4 4"),
    trash: P("M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v6M14 11v6"),
    plus: P("M12 5v14M5 12h14"),
    x: P("m6 6 12 12M18 6 6 18"),
    code: P("m8 7-5 5 5 5M16 7l5 5-5 5"),
    browser: '<rect x="3" y="4" width="18" height="16" rx="2"/>' + P("M3 9h18"),
    chat: P("M4 5h16v11H9l-5 4z"),
    dock: P("M14 4h6v16h-6M4 12h10M10 8l4 4-4 4"),
    refresh: P("M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5"),
    reset: P("M4 12a8 8 0 1 0 2.3-5.7M4 4v5h5"),
    search: '<circle cx="11" cy="11" r="6"/>' + P("m20 20-4.5-4.5"),
    more: '<circle cx="5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="19" cy="12" r="1.3"/>',
    check: P("m5 12 5 5 9-10"),
    external: P("M14 4h6v6M20 4l-9 9M18 14v6H4V6h6"),
    popout: P("M9 4H4v16h16v-5M14 4h6v6M20 4l-9 9"),
    maximise: '<rect x="4" y="4" width="16" height="16" rx="2"/>' + P("M4 9h16"),
    cpu: '<rect x="6" y="6" width="12" height="12" rx="2"/><rect x="10" y="10" width="4" height="4"/>' + P("M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"),
    key: '<circle cx="8" cy="15" r="4"/>' + P("m11 12 9-9M17 3l3 3M14 6l3 3"),
    panel: '<rect x="3" y="4" width="18" height="16" rx="2"/>' + P("M9 4v16"),
    info: '<circle cx="12" cy="12" r="9"/>' + P("M12 11v5M12 8h.01"),
    alert: P("M12 3 2.5 20h19z") + P("M12 10v4M12 17h.01"),
    copy: '<rect x="9" y="9" width="11" height="11" rx="2"/>' + P("M5 15V5h10"),
    link: P("M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1.5 1.5M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1.5-1.5"),
    box: P("M12 2.5 3.5 7v10l8.5 4.5 8.5-4.5V7z") + P("M3.5 7 12 11.5 20.5 7M12 11.5v10"),
    desktop: '<rect x="3" y="4" width="18" height="12" rx="2"/>' + P("M8 20h8M12 16v4"),
    clock: '<circle cx="12" cy="12" r="9"/>' + P("M12 7v5l3 2"),
    eye: P("M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12z") + '<circle cx="12" cy="12" r="3"/>',
    eyeoff: P("M3 3l18 18M10.6 10.6a3 3 0 0 0 4.2 4.2M6.5 6.7C4 8.3 2 12 2 12s3.5 6 10 6c1.6 0 3-.3 4.2-.9M9.9 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7s-.9 1.6-2.5 3.2"),
    bolt: P("M13 2 4 14h7l-1 8 9-12h-7z"),
    list: P("M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"),
    filter: P("M3 5h18l-7 8v6l-4 2v-8z"),
    arrowr: P("M5 12h14M13 6l6 6-6 6"),
    home: P("M3 11 12 4l9 7v9H3z") + P("M9 20v-6h6v6"),
  };
  const sprite = document.createElement("svg");
  sprite.setAttribute("aria-hidden", "true");
  sprite.setAttribute("style", "position:absolute;width:0;height:0;overflow:hidden");
  sprite.innerHTML = Object.entries(ICONS)
    .map(([k, v]) => `<symbol id="i-${k}" viewBox="0 0 24 24">${v}</symbol>`)
    .join("");
  document.body.prepend(sprite);
  window.icon = (name, cls) => `<svg class="icon${cls ? " " + cls : ""}" aria-hidden="true"><use href="#i-${name}"/></svg>`;

  // ── 图标栏（对应 App.tsx 的 Page 联合：workspace | sandboxes | images | models | usage）
  const NAV = [
    { id: "workspace", icon: "grid", label: "工作区", href: "workspace.html" },
    { id: "sandboxes", icon: "terminal", label: "沙箱列表", href: "sandbox-list.html" },
    { id: "images", icon: "layers", label: "镜像（本轮未重设计）", href: "index.html#scope" },
    { id: "models", icon: "sliders", label: "模型配置", href: "models.html" },
    { id: "usage", icon: "chart", label: "用量（本轮未重设计）", href: "index.html#scope" },
  ];
  const page = document.body.dataset.page || "";
  const mount = document.querySelector("[data-rail]");
  if (mount) {
    const dark = document.documentElement.dataset.mode === "dark";
    mount.className = "rail";
    mount.setAttribute("aria-label", "主导航");
    mount.innerHTML =
      `<a class="rail-brand" href="index.html" title="Sandbox 管理器 · 总览" data-od-id="rail-brand">${window.icon("cube")}</a>` +
      `<nav class="rail-nav" data-od-id="rail-nav">` +
      NAV.map(
        (n) =>
          `<a class="rail-btn${n.id === page ? " active" : ""}" href="${n.href}" data-tip="${n.label}" aria-label="${n.label}"${n.id === page ? ' aria-current="page"' : ""} data-nav="${n.id}">${window.icon(n.icon)}</a>`,
      ).join("") +
      `</nav>` +
      `<div class="rail-foot" data-od-id="rail-foot">` +
      `<button class="rail-btn" id="theme-btn" data-tip="${dark ? "切换到浅色主题" : "切换到深色主题"}" aria-label="${dark ? "切换到浅色主题" : "切换到深色主题"}">${window.icon(dark ? "sun" : "moon")}</button>` +
      `<button class="rail-btn" data-tip="Switch to English" aria-label="Switch to English" title="原型仅含中文文案">${window.icon("globe")}</button>` +
      `</div>`;
    const tb = mount.querySelector("#theme-btn");
    tb.addEventListener("click", () => {
      const next = document.documentElement.dataset.mode === "dark" ? "light" : "dark";
      document.documentElement.dataset.mode = next;
      localStorage.setItem("mgr.theme", next);
      const d = next === "dark";
      tb.innerHTML = window.icon(d ? "sun" : "moon");
      tb.dataset.tip = d ? "切换到浅色主题" : "切换到深色主题";
      tb.setAttribute("aria-label", tb.dataset.tip);
      document.dispatchEvent(new CustomEvent("mgr:theme", { detail: next }));
    });
  }

  // 通用：点击外部关闭 .menu
  document.addEventListener("click", (e) => {
    document.querySelectorAll(".menu.open").forEach((m) => {
      if (!m.contains(e.target) && !e.target.closest("[data-more],[data-menu-trigger]")) m.classList.remove("open");
    });
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      document.querySelectorAll(".menu.open").forEach((m) => m.classList.remove("open"));
      document.querySelectorAll(".overlay.open").forEach((o) => o.classList.remove("open"));
      document.querySelectorAll(".drawer.open").forEach((o) => o.classList.remove("open"));
    }
  });
})();
