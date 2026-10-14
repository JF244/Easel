import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useApp, type Page } from "./lib/store";
import BrainPage from "./pages/BrainPage";
import CreatePage from "./pages/CreatePage";
import EditPage from "./pages/EditPage";
import AppsPage from "./pages/AppsPage";
import GalleryPage from "./pages/GalleryPage";
import SettingsPage from "./pages/SettingsPage";

const NAV: { id: Page; label: string; hint: string }[] = [
  { id: "brain", label: "Brain", hint: "Describe what you want; the brain plans and runs it" },
  { id: "create", label: "Create", hint: "Write a prompt and generate images yourself" },
  { id: "edit", label: "Edit", hint: "Change an image, or paint an area to redo" },
  { id: "gallery", label: "Gallery", hint: "Everything you've made, saved on this PC" },
  { id: "apps", label: "Apps", hint: "PhotoCraft and the other editors that replace the Adobe suite" },
  { id: "settings", label: "Settings", hint: "Keys, brains, ComfyUI and folders" },
];

export default function App() {
  const { page, setPage, loadSettings, refreshGallery, refreshApps, settingsError } = useApp();

  useEffect(() => {
    void loadSettings();
    void refreshGallery();
    void refreshApps();
  }, [loadSettings, refreshGallery, refreshApps]);

  // Coming back from PhotoCraft (or any app) picks up what you saved there.
  useEffect(() => {
    const unlisten = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        void refreshGallery();
        void refreshApps();
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [refreshGallery, refreshApps]);

  return (
    <div className="flex h-full">
      <nav aria-label="Main" className="flex w-44 shrink-0 flex-col gap-1 border-r border-line bg-panel p-3">
        <div className="mb-4 px-2 pt-1 text-lg font-semibold tracking-tight">
          Easel<span className="text-accent">.</span>
        </div>
        {NAV.map((item) => (
          <button
            key={item.id}
            type="button"
            title={item.hint}
            aria-current={page === item.id ? "page" : undefined}
            onClick={() => setPage(item.id)}
            className={`h-10 rounded-md px-3 text-left text-sm font-medium transition-colors ${
              page === item.id ? "bg-panel-2 text-ink" : "text-ink-2 hover:bg-panel-2/60 hover:text-ink"
            }`}
          >
            {item.label}
          </button>
        ))}
      </nav>
      <main className="min-w-0 flex-1 overflow-y-auto">
        {settingsError && (
          <p role="alert" className="m-4 rounded-md border border-danger/40 bg-danger/10 p-3 text-sm text-danger">
            Couldn't load your settings: {settingsError}
          </p>
        )}
        {/* These pages stay mounted when hidden, so chats, results, the edit
            mask and running jobs survive switching tabs. */}
        <div hidden={page !== "brain"} className="h-full">
          <BrainPage />
        </div>
        <div hidden={page !== "create"}>
          <CreatePage />
        </div>
        <div hidden={page !== "edit"}>
          <EditPage />
        </div>
        <div hidden={page !== "gallery"}>
          <GalleryPage />
        </div>
        {/* Apps and Settings reload each visit so they always show what's saved. */}
        {page === "apps" && <AppsPage />}
        {page === "settings" && <SettingsPage />}
      </main>
    </div>
  );
}
