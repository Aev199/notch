import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { Editor } from "./editor/Editor";
import { Capture } from "./capture/Capture";
import "./styles.css";
import { MotionRoot } from "./lib/MotionRoot";
import "./editor/editor.css";

// No browser context menu (except in text fields: copy/paste), no accidental zoom.
window.addEventListener("contextmenu", (e) => {
  if (!(e.target as HTMLElement).closest?.("textarea, input")) e.preventDefault();
});
window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });

// The same bundle serves the island (main window), the editor window and
// the screen capture window.
const query = new URLSearchParams(window.location.search);
const editorId = query.get("editor");
const capture = query.has("capture");
if (editorId) document.documentElement.classList.add("is-editor");
if (capture) document.documentElement.classList.add("is-capture");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <MotionRoot>{capture ? <Capture /> : editorId ? <Editor initialId={editorId} /> : <App />}</MotionRoot>
  </React.StrictMode>,
);
