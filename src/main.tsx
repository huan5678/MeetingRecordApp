import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/globals.css";
import { applyTheme } from "@/stores/settingsStore";
import { DEFAULT_THEME } from "@/lib/constants";

// Apply the theme before first paint to avoid a flash. The store re-applies the
// persisted value once settings load.
applyTheme(DEFAULT_THEME);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
