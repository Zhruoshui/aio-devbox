// Theme registry (09-20-theme-schemes) - the single list of color schemes
// the UI offers. App.tsx consumes it for the <html data-theme/data-mode>
// attributes and the rail's theme picker; XtermPane picks the ANSI palette
// up indirectly through the CSS tokens.
//
// Single source of truth split: the COLOR VALUES live only in styles.css
// (one [data-theme="<key>"] block per scheme; :root carries only the
// mode-independent primitives - fonts/spacing/radii/motion). Everything in
// this file is identity/metadata only. The `swatch` hexes are static preview
// samples for the picker's 5 dots (accent / fg / chart-1 / chart-2 /
// surface) and must mirror the corresponding token values.
//
// All schemes are Omarchy official themes (basecamp/omarchy
// themes/<name>/colors.toml, archived under the task's research dir);
// adding a 9th theme = one CSS block + one entry here + one i18n string -
// no component code changes (AC6).

import type { StringKey } from "./i18n";

export type ThemeMode = "light" | "dark";

export interface ThemeDef {
  /** Scheme key - written to <html data-theme> and localStorage. Must
   * match a styles.css [data-theme="<key>"] selector. */
  key: string;
  /** Picker label (i18n). */
  labelKey: StringKey;
  /** Static light/dark class of the scheme -> <html data-mode>. */
  mode: ThemeMode;
  /** 5 preview dots: accent / fg / chart-1 / chart-2 / surface (hex). */
  swatch: [string, string, string, string, string];
}

export const THEMES: ThemeDef[] = [
  {
    key: "tokyo-night",
    labelKey: "themeTokyoNight",
    mode: "dark",
    swatch: ["#7aa2f7", "#c0caf5", "#587dce", "#ca723b", "#24283b"],
  },
  {
    key: "catppuccin",
    labelKey: "themeCatppuccin",
    mode: "dark",
    swatch: ["#89b4fa", "#cdd6f4", "#507fcd", "#bb672a", "#313244"],
  },
  {
    key: "ethereal",
    labelKey: "themeEthereal",
    mode: "dark",
    swatch: ["#7d82d9", "#ffcead", "#7d82d9", "#539c5c", "#131a3a"],
  },
  {
    key: "nord",
    labelKey: "themeNord",
    mode: "dark",
    swatch: ["#81a1c1", "#eceff4", "#199cb8", "#bc6448", "#3b4252"],
  },
  {
    key: "vantablack",
    labelKey: "themeVantablack",
    mode: "dark",
    swatch: ["#8d8d8d", "#ffffff", "#457db4", "#547d3c", "#1a1a1a"],
  },
  {
    key: "catppuccin-latte",
    labelKey: "themeCatppuccinLatte",
    mode: "light",
    swatch: ["#1e66f5", "#4c4f69", "#1e66f5", "#d65b1f", "#ffffff"],
  },
  {
    key: "white",
    labelKey: "themeWhite",
    mode: "light",
    swatch: ["#6e6e6e", "#000000", "#2d6ca8", "#a4771c", "#f5f5f5"],
  },
  {
    key: "flexoki-light",
    labelKey: "themeFlexokiLight",
    mode: "light",
    swatch: ["#205ea6", "#100f0f", "#205ea6", "#d14d41", "#fffcf0"],
  },
];

/** Default when nothing (or nothing recognizable) is saved. */
export const DEFAULT_THEME = "tokyo-night";

/** The default theme's mode - mirrored statically in index.html's pre-paint
 * bootstrap (data-mode on <html>); keep the two in sync. */
export const DEFAULT_THEME_MODE: ThemeMode = "dark";

/**
 * Resolve a persisted THEME_KEY value to a ThemeDef. Back-compat: any
 * legacy value (pre-scheme "light"/"dark", the retired "kumo-*" keys, the
 * pre-rename "catppuccin-mocha") falls back to the default theme.
 */
export function resolveThemeKey(saved: string | null): ThemeDef {
  return (
    THEMES.find((th) => th.key === saved) ??
    THEMES.find((th) => th.key === DEFAULT_THEME) ??
    THEMES[0]
  );
}
