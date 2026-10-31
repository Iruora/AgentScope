import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { App } from "./App";
import { applyInitialTheme } from "./lib/theme";
import { Popup } from "./views/Popup";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: Infinity, refetchOnWindowFocus: false } },
});

applyInitialTheme();

/** The same bundle serves the main window and the approval popup. */
const isPopup = getCurrentWindow().label === "popup";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      {isPopup ? <Popup /> : <App />}
    </QueryClientProvider>
  </StrictMode>,
);
