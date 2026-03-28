import React from "react";
import ReactDOM from "react-dom/client";
import { appWindow } from "@tauri-apps/api/window";
import ResultsWindow from "./ResultsWindow";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <ResultsWindow />
  </React.StrictMode>
);

// Show this window once React has mounted and painted
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    appWindow.show();
  });
});
