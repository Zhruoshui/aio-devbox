// Minimal i18n - a flat string table per language plus a `t` lookup.
// Deliberately no framework: the UI surface is small and chrome-only (service
// labels themselves come from the manifest and stay server-authored).
// `{n}` placeholders are filled by `fmt` (no plural rules needed yet).

export type Lang = "zh-CN" | "en";

export const LANGS: readonly Lang[] = ["zh-CN", "en"];

type Strings = typeof STRINGS["zh-CN"];

const STRINGS: Record<Lang, Record<string, string>> = {
  "zh-CN": {
    brand: "AIO 沙箱",
    groupWeb: "Web 工具",
    groupTui: "终端与 Agent",
    groupCustom: "自定义",
    refresh: "刷新服务清单",
    collapse: "折叠侧边栏",
    expand: "展开侧边栏",
    toLight: "切换到浅色主题",
    toDark: "切换到深色主题",
    switchLang: "Switch to English",
    register: "注册按钮",
    openInstanceSuffix: "，点击打开新实例",
    removePrefix: "移除 ",
    sidebarEmpty: "暂无可用按钮。启动一个 compose profile 或安装工具。",
    dialogTitle: "注册自定义按钮",
    dialogSub: "按钮会作为「终端 + 命令」运行在 app 容器中，并持久保存到工作区卷，重建容器后仍然可用。",
    fieldLabel: "按钮名称",
    fieldLabelPh: "例如：跟踪日志",
    fieldCmd: "启动命令",
    fieldCmdPh: "例如：make logs",
    fieldCmdHint: "在登录 shell 的 PATH 中执行；命令真实存在时按钮才会出现。",
    cancel: "取消",
    submit: "添加按钮",
    errLabel: "请填写按钮名称。",
    errCmd: "请填写启动命令。",
    errFailed: "注册失败，请重试。",
    loading: "正在加载工作区…",
    loadFailed: "加载清单失败：",
    noButtons: "没有可用按钮。启动一个 compose profile（例如 --profile code-server）或把场景烘焙进镜像。",
    statusAvail: "{n} 个服务可用",
    statusMounted: "工作区卷已挂载",
    statusOffline: "后端连接中断",
    copied: "已复制",
    resetLayout: "重置布局",
    statsTip: "CPU / 内存 / 磁盘（容器视角）",
    copyUrl: "复制工作台地址",
    popin: "返回工作区",
  },
  en: {
    brand: "AIO sandbox",
    groupWeb: "Web tools",
    groupTui: "Terminals & agents",
    groupCustom: "Custom",
    refresh: "Refresh services",
    collapse: "Collapse sidebar",
    expand: "Expand sidebar",
    toLight: "Switch to light theme",
    toDark: "Switch to dark theme",
    switchLang: "切换到中文",
    register: "Register button",
    openInstanceSuffix: " - click to open a new instance",
    removePrefix: "Remove ",
    sidebarEmpty: "No buttons yet. Start a compose profile or install a tool.",
    dialogTitle: "Register a custom button",
    dialogSub: "Buttons run as “terminal + command” inside the app container and persist to the workspace volume, surviving container rebuilds.",
    fieldLabel: "Button name",
    fieldLabelPh: "e.g. Tail logs",
    fieldCmd: "Command",
    fieldCmdPh: "e.g. make logs",
    fieldCmdHint: "Runs on the login shell's PATH; the button only appears when the command actually exists.",
    cancel: "Cancel",
    submit: "Add button",
    errLabel: "Button name is required.",
    errCmd: "Command is required.",
    errFailed: "Registration failed, try again.",
    loading: "Loading workspace…",
    loadFailed: "Failed to load manifest: ",
    noButtons: "No buttons available. Start a compose profile (e.g. --profile code-server) or bake a scenario into the image.",
    statusAvail: "{n} services available",
    statusMounted: "workspace volume mounted",
    statusOffline: "backend unreachable",
    copied: "copied",
    resetLayout: "Reset layout",
    statsTip: "CPU / memory / disk (container view)",
    copyUrl: "Copy workspace URL",
    popin: "Dock back to workspace",
  },
};

/** Look up a string for the active language (falls back to the key). */
export function t(lang: Lang, key: keyof Strings): string {
  return STRINGS[lang][key] ?? key;
}

/** Fill a `{n}` placeholder in a looked-up string. */
export function fmt(lang: Lang, key: keyof Strings, n: number): string {
  return t(lang, key).replace("{n}", String(n));
}
