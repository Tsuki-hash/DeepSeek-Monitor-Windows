import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { App } from "./App";
import { applyTheme, migrateLegacyTheme, readStoredTheme } from "./theme";

// One-time migration for the 1.3.0 light default (legacy installs carry a
// "dark" that the old versions wrote as their default), then apply the saved
// theme before first render to avoid a flash of the wrong skin.
migrateLegacyTheme();
applyTheme(readStoredTheme());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
