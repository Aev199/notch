import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// Overlay window: no browser context menu, no accidental zoom.
window.addEventListener("contextmenu", (e) => e.preventDefault());
window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
