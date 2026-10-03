import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { installGlobalErrorLogging } from "./lib/errors";
import { initTheme } from "./theme/theme";

// Apply the saved palette before the first render so nothing flashes.
initTheme();
// Uncaught JS errors go to the crash log too (render errors via error boundaries).
installGlobalErrorLogging();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
