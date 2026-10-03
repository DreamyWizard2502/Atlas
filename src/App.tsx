import { useEffect, useState } from "react";
import { CommandPalette } from "./components/CommandPalette";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ContextMenu, Dialogs, Toasts } from "./components/Overlays";
import { QuickLook } from "./components/QuickLook";
import { Onboarding, Rail, ScanBanner } from "./components/Shell";
import { Button, Empty, Spinner } from "./components/ui";
import { api, on, type ScanProgress } from "./lib/api";
import { formatCount } from "./lib/format";
import { isTypingTarget } from "./lib/hooks";
import { bumpData, getState, openQuickLook, setState, toast, useStore } from "./lib/store";
import { setTheme } from "./theme/theme";
import { Cleanup } from "./screens/Cleanup";
import { Dashboard } from "./screens/Dashboard";
import { Folders } from "./screens/Folders";
import { SettingsScreen } from "./screens/Settings";
import { Spaces } from "./screens/Spaces";
import { Timeline } from "./screens/Timeline";

export default function App() {
  const status = useStore((s) => s.status);
  const screen = useStore((s) => s.screen);
  const [booted, setBooted] = useState(false);

  const refreshStatus = () =>
    api
      .appStatus()
      .then((st) => setState({ status: st }))
      .finally(() => setBooted(true));

  useEffect(() => {
    refreshStatus();
    // Saved appearance is the source of truth; the local cache only avoids a flash.
    api
      .getSettings()
      .then((s) => setTheme({ mode: s.themeMode, light: s.lightPalette, dark: s.darkPalette }))
      .catch(() => {});
    let changeTimer: number | undefined;
    const subs = [
      on<ScanProgress>("scan://progress", (p) => setState({ scan: p })),
      on<ScanProgress>("scan://done", (p) => {
        setState({ scan: null });
        if (p.error) toast(`Indexing failed: ${p.error}`, { tone: "attention" });
        else if (!p.cancelled) toast(`Index up to date · ${formatCount(p.files)} files`, { tone: "success" });
        refreshStatus();
        bumpData();
      }),
      // The watcher can fire often; refresh widgets at most every 3 s.
      on<number>("index://changed", () => {
        if (changeTimer) return;
        changeTimer = window.setTimeout(() => {
          changeTimer = undefined;
          bumpData();
        }, 3000);
      }),
      on("cleanup://done", () => bumpData()),
    ];
    return () => {
      subs.forEach((p) => p.then((un) => un()));
      if (changeTimer) clearTimeout(changeTimer);
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const s = getState();
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setState({ palette: !s.palette, menu: null });
        return;
      }
      if (e.key === " " && !isTypingTarget(e.target) && !s.palette && !s.dialog && !s.quickLook && s.selection) {
        e.preventDefault();
        openQuickLook();
      }
      if (e.key === "Escape" && s.selection && !s.quickLook && !s.palette && !s.dialog && !s.menu && !isTypingTarget(e.target)) {
        setState({ selection: null });
      }
    };
    window.addEventListener("keydown", onKey);
    // Suppress the browser context menu outside our own menus.
    const onCtx = (e: MouseEvent) => {
      if (!isTypingTarget(e.target)) e.preventDefault();
    };
    window.addEventListener("contextmenu", onCtx);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onCtx);
    };
  }, []);

  if (!booted) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner size={20} />
      </div>
    );
  }

  if (status && !status.onboarded) {
    return (
      <>
        <Onboarding onDone={refreshStatus} />
        <Toasts />
      </>
    );
  }

  return (
    <div className="flex h-full">
      <Rail />
      <main className="flex min-w-0 flex-1 flex-col">
        <ScanBanner />
        <div className="min-h-0 flex-1">
          <ErrorBoundary
            name={`screen:${screen}`}
            resetKey={screen}
            fallback={(e, retry) => (
              <Empty
                title="This view hit a problem"
                body={`${e.message}. The rest of Atlas is fine; try again or switch views.`}
                action={<Button onClick={retry}>Try again</Button>}
              />
            )}
          >
            {screen === "dashboard" && <Dashboard />}
            {screen === "spaces" && <Spaces />}
            {screen === "timeline" && <Timeline />}
            {screen === "cleanup" && <Cleanup />}
            {screen === "folders" && <Folders />}
            {screen === "settings" && <SettingsScreen />}
          </ErrorBoundary>
        </div>
      </main>
      <ErrorBoundary name="command-palette" fallback={() => null}>
        <CommandPalette />
      </ErrorBoundary>
      <ErrorBoundary name="quick-look" fallback={() => null}>
        <QuickLook />
      </ErrorBoundary>
      <ErrorBoundary name="menus-and-dialogs" fallback={() => null}>
        <ContextMenu />
        <Dialogs />
      </ErrorBoundary>
      <Toasts />
    </div>
  );
}
