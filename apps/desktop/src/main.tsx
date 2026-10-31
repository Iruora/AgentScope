import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { App } from "./App";
import { applyInitialTheme } from "./lib/theme";
import { Popup } from "./views/Popup";
import { Widget } from "./views/Widget";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: Infinity, refetchOnWindowFocus: false } },
});

applyInitialTheme();

/** The same bundle serves the main window, the approval popup and the mini widget. */
const label = getCurrentWindow().label;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      {label === "popup" ? <Popup /> : label === "widget" ? <Widget /> : <App />}
    </QueryClientProvider>
  </StrictMode>,
);
