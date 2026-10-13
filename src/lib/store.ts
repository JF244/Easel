import { create } from "zustand";
import { api, errorMessage, type GalleryItem, type Settings } from "./api";

export type Page = "brain" | "create" | "edit" | "gallery" | "settings";

interface AppStore {
  page: Page;
  settings: Settings | null;
  settingsError: string | null;
  gallery: GalleryItem[];
  /** The image the Edit page opens with. */
  editPath: string | null;
  setPage: (page: Page) => void;
  loadSettings: () => Promise<void>;
  setSettings: (settings: Settings) => void;
  refreshGallery: () => Promise<void>;
  addToGallery: (items: GalleryItem[]) => void;
  removeFromGallery: (path: string) => void;
  editImage: (path: string) => void;
}

export const useApp = create<AppStore>((set) => ({
  page: "brain",
  settings: null,
  settingsError: null,
  gallery: [],
  editPath: null,
  setPage: (page) => set({ page }),
  loadSettings: async () => {
    try {
      set({ settings: await api.getSettings(), settingsError: null });
    } catch (err) {
      set({ settingsError: errorMessage(err) });
    }
  },
  setSettings: (settings) => set({ settings }),
  refreshGallery: async () => {
    try {
      set({ gallery: await api.galleryList() });
    } catch {
      // The gallery page shows its own message when the folder can't be read.
    }
  },
  addToGallery: (items) => set((s) => ({ gallery: [...items, ...s.gallery.filter((g) => !items.some((i) => i.path === g.path))] })),
  removeFromGallery: (path) => set((s) => ({ gallery: s.gallery.filter((g) => g.path !== path) })),
  editImage: (path) => set({ editPath: path, page: "edit" }),
}));
