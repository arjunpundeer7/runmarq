import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { setNativeApi } from "./services/native";
import "./styles/app.css";

async function boot() {
  // Dev-only: `?mock` renders the UI against sample data outside Tauri.
  if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
    const { createMockNative } = await import("./services/mock");
    const params = new URLSearchParams(location.search);
    setNativeApi(
      createMockNative({
        active: params.has("empty") ? [] : undefined,
        connected: !params.has("offline"),
        preferences: { onboardingCompleted: !params.has("onboarding") },
      }),
    );
  }
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void boot();
