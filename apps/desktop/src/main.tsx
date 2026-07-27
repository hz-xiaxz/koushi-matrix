import React from "react";
import { createRoot } from "react-dom/client";

import "./bootErrorCapture";
import { App } from "./App";
import { installJsErrorCapture } from "./domain/jsErrorLog";
import "katex/dist/katex.min.css";
import "./styles.css";

installJsErrorCapture(window);

createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
