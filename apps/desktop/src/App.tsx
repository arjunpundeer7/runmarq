import { useEffect } from "react";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Shell } from "./components/Shell";
import { Notice, Spinner } from "./components/controls";
import { History } from "./pages/History";
import { Insights } from "./pages/Insights";
import { Integrations } from "./pages/Integrations";
import { Onboarding } from "./pages/Onboarding";
import { Overview } from "./pages/Overview";
import { RunDetail } from "./pages/RunDetail";
import { Settings } from "./pages/Settings";
import { Usage } from "./pages/Usage";
import { useApp } from "./stores/app";

export function App() {
  const ready = useApp((s) => s.ready);
  const fatal = useApp((s) => s.fatal);
  const prefs = useApp((s) => s.preferences);
  const route = useApp((s) => s.route);
  const init = useApp((s) => s.init);

  useEffect(() => {
    void init();
  }, [init]);

  if (!ready) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }
  if (fatal) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="max-w-md">
          <Notice tone="danger">{fatal}</Notice>
        </div>
      </div>
    );
  }
  if (prefs && !prefs.onboardingCompleted) return <Onboarding />;

  return (
    <Shell>
      <ErrorBoundary resetKey={JSON.stringify(route)}>
      <div key={route.page === "run" ? route.id : route.page} className="rm-rise">
        {route.page === "overview" && <Overview />}
        {route.page === "history" && <History />}
        {route.page === "integrations" && <Integrations />}
        {route.page === "insights" && <Insights />}
        {route.page === "usage" && <Usage />}
        {route.page === "settings" && <Settings />}
        {route.page === "run" && <RunDetail id={route.id} />}
      </div>
      </ErrorBoundary>
    </Shell>
  );
}
