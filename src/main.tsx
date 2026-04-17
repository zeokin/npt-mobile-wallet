import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
// Initialize safe-area-insets plugin — sets --safe-area-inset-top/bottom CSS properties on Android
import "@saurl/tauri-plugin-safe-area-insets-css-api";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
