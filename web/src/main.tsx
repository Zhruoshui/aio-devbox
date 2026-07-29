import { createRoot } from "react-dom/client";
import { App } from "./App";

const el = document.getElementById("root");
if (!el) throw new Error("root element #root not found");

// No StrictMode: golden-layout is imperative and owns DOM in a container; the
// StrictMode double-invoke of effects in dev would create two GoldenLayout
// instances against the same container. Production builds are unaffected, but
// keeping this single-invoked avoids the footgun in dev too.
createRoot(el).render(<App />);
