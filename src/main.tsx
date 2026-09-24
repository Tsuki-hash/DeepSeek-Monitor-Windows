import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { App } from "./App";
import { applyTheme, readStoredTheme } from "./theme";

// Apply the saved theme before first render to avoid a flash of the wrong skin.
applyTheme(readStoredTheme());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
