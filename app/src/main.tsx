import "@fontsource-variable/plus-jakarta-sans";
import "./styles/app.css";
import { QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter } from "react-router-dom";
import { queryClient } from "./api";
import App from "./App";
import { ToastProvider } from "./components/ui";
import { applySavedTheme } from "./theme";

applySavedTheme();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <HashRouter>
          <App />
        </HashRouter>
      </ToastProvider>
    </QueryClientProvider>
  </StrictMode>,
);
