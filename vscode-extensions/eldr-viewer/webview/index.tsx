import React from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import type { ViewerFile } from "../src/viewerTypes";

declare global {
  interface Window {
    __SURTR_VIEWER_DATA__: ViewerFile;
  }
}

const container = document.getElementById("root");
if (!container) {
  throw new Error("Missing root element");
}

createRoot(container).render(<App viewer={window.__SURTR_VIEWER_DATA__} />);
