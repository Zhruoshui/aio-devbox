// Icon sprite - inline SVG symbols, stroke style from the Kumo reference
// (same pattern and glyph set as web/src/icons.tsx, minus the workspace-only
// serviceIcon mapping). IconSprite is rendered once at the app root;
// <Icon name> references a symbol via <use href="#i-...">, which inherits
// `currentColor` so CSS owns the color per state.

export type IconName = keyof typeof PATHS;

// Symbol definitions: name -> inner SVG markup (viewBox 0 0 24 24, stroke
// currentColor). Kept as data so IconSprite stays a single static render.
const PATHS = {
  cube:
    '<path d="M12 2 3 7v10l9 5 9-5V7l-9-5z"/><path d="M3 7l9 5 9-5"/><path d="M12 12v10"/>',
  terminal: '<path d="m5 7 4 4-4 4"/><path d="M13 17h6"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  refresh: '<path d="M21 12a9 9 0 1 1-2.6-6.3"/><path d="M21 3v6h-6"/>',
  "chev-l": '<path d="m14 6-6 6 6 6"/>',
  "chev-r": '<path d="m10 6 6 6-6 6"/>',
  "chev-down": '<path d="m6 9 6 6 6-6"/>',
  x: '<path d="M6 6l12 12M18 6 6 18"/>',
  sun:
    '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z"/>',
  globe:
    '<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z"/>',
  dock:
    '<path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><path d="m10 17 5-5-5-5"/><path d="M15 12H3"/>',
  edit: '<path d="M17 3a2.83 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/>',
  trash: '<path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/>',
  play: '<path d="M8 5v14l11-7z" fill="currentColor" stroke="none"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="1" fill="currentColor" stroke="none"/>',
  restart:
    '<path d="M21 12a9 9 0 1 1-2.6-6.3"/><path d="M21 3v6h-6"/><path d="M12 8v4l3 2"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  "check-circle": '<path d="M22 11.1V12a10 10 0 1 1-5.9-9.1"/><path d="M22 4 12 14.01l-3-3"/>',
  alert: '<path d="M10.3 3.6 1.9 18a2 2 0 0 0 1.7 3h16.8a2 2 0 0 0 1.7-3L13.7 3.6a2 2 0 0 0-3.4 0z"/><path d="M12 9v4"/><circle cx="12" cy="16.5" r="0.5" fill="currentColor"/>',
  box:
    '<path d="M21 8v13H3V8"/><path d="M1 3h22v5H1z"/><path d="M10 12h4"/>',
  copy:
    '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
} as const;

/** Mount once (app root): the shared <symbol> sprite every <Icon> references. */
export function IconSprite(): JSX.Element {
  const symbols = (Object.keys(PATHS) as IconName[]).map((name) => (
    <symbol
      key={name}
      id={`i-${name}`}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      dangerouslySetInnerHTML={{ __html: PATHS[name] }}
    />
  ));
  return (
    <svg style={{ display: "none" }} aria-hidden="true">
      {symbols}
    </svg>
  );
}

/** Reference a sprite symbol. `large` selects the 20px display size. */
export function Icon({ name, large = false }: { name: IconName; large?: boolean }): JSX.Element {
  return (
    <svg className={large ? "icon-lg" : "icon"} aria-hidden="true">
      <use href={`#i-${name}`} />
    </svg>
  );
}
