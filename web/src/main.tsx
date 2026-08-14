import { createRoot } from "react-dom/client";
import { App } from "./App";

const el = document.getElementById("root");
if (!el) throw new Error("root element #root not found");

// No StrictMode: XtermPane opens a WebSocket + Terminal inside an effect; the
// dev-only double-invoke would connect/teardown the pty twice in quick
// succession. Production builds are unaffected either way.
createRoot(el).render(<App />);
