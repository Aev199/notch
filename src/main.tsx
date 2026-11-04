import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { Editor } from "./editor/Editor";
import "./styles.css";
import "./editor/editor.css";

// No browser context menu (except in text fields: copy/paste), no accidental zoom.
window.addEventListener("contextmenu", (e) => {
  if (!(e.target as HTMLElement).closest?.("textarea, input")) e.preventDefault();
});
window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });

// The same bundle serves the island (main window) and the editor window.
const editorId = new URLSearchParams(window.location.search).get("editor");
if (editorId) document.documentElement.classList.add("is-editor");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{editorId ? <Editor initialId={editorId} /> : <App />}</React.StrictMode>,
);
