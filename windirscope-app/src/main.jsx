import React from "react";
import ReactDOM from "react-dom/client";
import { appWindow } from "@tauri-apps/api/window";
import App from "./App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);

// Show the window once React has mounted and painted the first frame
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    appWindow.show();
  });
});
