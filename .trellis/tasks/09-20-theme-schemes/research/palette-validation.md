## Chart palette validation (dataviz six checks) — 09-20-theme-schemes

Script: /tmp/claude-1000/bundled-skills/2.1.246/657630615332870a298d2e04524b2b1f/dataviz/scripts/validate_palette.js
All FAILs below were fixed by same-hue snap-to-passing (node /tmp/snap_oklch.js, OKLCH L/C override, gamut-clipped). Contrast WARN rows are legal with relief (the usage page renders text labels beside every mark plus the detail table).

### Tokyo Night (dark, surface #1f2335)
```

Palette (dark, surface #1f2335, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.48–0.67
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #cc4f6a↔#8764c0 ΔE 13.8 (protan) · tritan 7.6
  [PASS] Normal-vision floor    worst adjacent #cc4f6a↔#8764c0 ΔE 17.5 (normal)
  [PASS] Contrast vs surface    all 6 >= 3:1

  → ALL CHECKS PASS  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

```

### Nord (dark, surface #3b4252)
```

Palette (dark, surface #3b4252, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.48–0.67
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #698d44↔#9f6295 ΔE 12.1 (deutan) · tritan 9.7
  [PASS] Normal-vision floor    worst adjacent #9f6295↔#b48f42 ΔE 19.4 (normal)
  [WARN] Contrast vs surface    below 3:1 — relief required (visible labels or table view): [["#bc6448",2.42],["#35659e",1.68],["#9f6295",2.23],["#698d44",2.63]]

  → ALL CHECKS PASS  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

```

### Catppuccin Mocha (dark, surface #313244)
```

Palette (dark, surface #313244, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.48–0.67
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #ad8c3d↔#009889 ΔE 10.0 (protan) · tritan 5.8
  [PASS] Normal-vision floor    worst adjacent #ad8c3d↔#009889 ΔE 16.6 (normal)
  [PASS] Contrast vs surface    all 6 >= 3:1

  → ALL CHECKS PASS  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

```

### Catppuccin Latte (light, surface #ffffff)
```

Palette (light, surface #ffffff, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.43–0.77
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #377e29↔#bb54a0 ΔE 14.6 (deutan) · tritan 4.5
  [PASS] Normal-vision floor    worst adjacent #bb54a0↔#b97515 ΔE 20.6 (normal)
  [PASS] Contrast vs surface    all 6 >= 3:1

  → ALL CHECKS PASS  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

```

### Ethereal (dark, surface #131a3a) — 09-20 revision (Omarchy final set)
```

Palette (dark, surface #131a3a, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.48–0.67
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #b069a6↔#539c5c ΔE 10.8 (deutan) · tritan 3.8
  [PASS] Normal-vision floor    worst adjacent #ac8200↔#b069a6 ΔE 20.9 (normal)
  [PASS] Contrast vs surface    all 6 >= 3:1

  → ALL CHECKS PASS
Slots: blue #7d82d9 (official), green #539c5c (snap green hue → L.63 C.12),
magenta #b069a6 (snap magenta hue → L.62 C.12), yellow #ac8200 (snap yellow
hue → L.63 C.14), cyan #2d93c8 (snap cyan hue → L.63 C.12), red #ED5B5A
(official). Raw official green/magenta/cyan/yellow sat outside the dark
L-band / chroma floor; all snapped same-hue.

```

### Vantablack (dark, surface #1a1a1a) — 09-20 revision
```

Palette (dark, surface #1a1a1a, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.48–0.67
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #c96152↔#009393 ΔE 10.6 (protan) · tritan 6.8
  [PASS] Normal-vision floor    worst adjacent #009393↔#ac8338 ΔE 17.8 (normal)
  [PASS] Contrast vs surface    all 6 >= 3:1

  → ALL CHECKS PASS
Slots: #457db4 steel, #547d3c moss, #a4589e mauve, #ac8338 bronze, #009393
teal, #c96152 clay. The Omarchy Vantablack "palette" is pure grayscale
(color1-6 are #a4a4a4/#b6b6b6/#cecece/#8d8d8d/#9b9b9b/#b0b0b0) — zero
chroma fails the floor hard, so slots carry a restrained tint (OKLCH C
0.10–0.135, near-neutral) on the gray palette's lightness ramp to stay
legal while keeping the near-black-gray character of the theme.

```

### White (light, surface #ffffff) — 09-20 revision
```

Palette (light, surface #ffffff, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.43–0.77
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #b35c4f↔#009d9d ΔE 10.9 (deutan) · tritan 7.2
  [PASS] Normal-vision floor    worst adjacent #765ba4↔#b35c4f ΔE 16.9 (normal)
  [PASS] Contrast vs surface    all 6 >= 3:1

  → ALL CHECKS PASS
Slots: #2d6ca8 slate, #a4771c bronze, #009d9d teal, #b35c4f clay, #765ba4
purple, #457128 moss. Omarchy White is grayscale like Vantablack — same
remedy: tinted near-neutrals on the gray ramp (C 0.096–0.135; the teal sat
at 0.096 and was lifted to L.63/C.108).

```

### Flexoki Light (light, surface #FFFCF0) — 09-20 revision
```

Palette (light, surface #FFFCF0, categorical): 6 slots
  [PASS] Lightness band         all 6 inside L 0.43–0.77
  [PASS] Chroma floor           all 6 >= 0.1
  [PASS] CVD separation         worst adjacent #00aca1↔#D14D41 ΔE 12.8 (deutan) · tritan 12.6
  [PASS] Normal-vision floor    worst adjacent #D0A215↔#00aca1 ΔE 21.2 (normal)
  [WARN] Contrast vs surface    below 3:1 — relief required (visible labels or table view): [["#00aca1",2.76],["#D0A215",2.31]]

  → ALL CHECKS PASS
Slots: blue #205EA6, red #D14D41 (official), teal #00aca1 (cyan hue, L.67
C.125 — raw cyan #3AA99F measured C 0.0996, a hair under the floor), magenta
#CE5D97, yellow #D0A215 (official), green #778b15 (green hue, L.60 C.135).
Contrast WARNs legal via the usage page relief (text labels + detail table).

```

### Raw official-palette candidates (first round, all FAILed — archived for provenance)
```
  → FAILED — fix the marked checks  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

  → FAILED — fix the marked checks  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

  → FAILED — fix the marked checks  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

  → FAILED — fix the marked checks  (CVD in the 6–8 floor band is legal ONLY with secondary encoding: direct labels, gaps, or texture)
  scope: categorical palettes only. For a lone status/text color check WCAG text contrast; for a sequential ramp, lightness monotonicity.

```
