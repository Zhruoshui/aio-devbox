import { createRoot } from "react-dom/client";
import { App } from "./App";
// styles.css is imported by App.tsx (token layer + page classes); the
// designer's shared component layer (ported from docs/Web-Prototype) must
// come AFTER it so its component classes win the cascade.
import "./components.css";

const el = document.getElementById("root");
if (!el) throw new Error("root element #root not found");

// No StrictMode: golden-layout is imperative and owns DOM in a container; the
// StrictMode double-invoke of effects in dev would create two GoldenLayout
// instances against the same container. Production builds are unaffected, but
// keeping this single-invoked avoids the footgun in dev too. (Same reason as
// the workbench SPA, web/src/main.tsx.)
createRoot(el).render(<App />);
