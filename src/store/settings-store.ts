import { create } from "zustand";
import { persist } from "zustand/middleware";

interface SettingsState {
  /// Supporter node URL chosen by the user; empty = the default node.
  nodeUrl: string;
  authToken: string;
  /// Lock the wallet when the app has been in the background for a while.
  lockOnBackground: boolean;
  /// UI language code, or "auto" for the phone's language.
  language: string;
  connected: boolean;
  network: string | null;
  blockHeight: number;
  setNodeUrl: (url: string) => void;
  setAuthToken: (token: string) => void;
  setLockOnBackground: (enabled: boolean) => void;
  setLanguage: (language: string) => void;
  setConnected: (connected: boolean, network?: string, blockHeight?: number) => void;
  setBlockHeight: (height: number) => void;
  reset: () => void;
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      nodeUrl: "",
      authToken: "",
      lockOnBackground: false,
      language: "auto",
      connected: false,
      network: null,
      blockHeight: 0,
      setNodeUrl: (nodeUrl) => set({ nodeUrl }),
      setAuthToken: (authToken) => set({ authToken }),
      setLockOnBackground: (lockOnBackground) => set({ lockOnBackground }),
      setLanguage: (language) => set({ language }),
      setConnected: (connected, network, blockHeight) =>
        set((state) => ({
          connected,
          network: network ?? (connected ? state.network : null),
          blockHeight: blockHeight ?? (connected ? state.blockHeight : 0),
        })),
      setBlockHeight: (blockHeight) => set({ blockHeight }),
      reset: () => set({ connected: false, network: null, blockHeight: 0 }),
    }),
    {
      name: "npt-settings",
      // Persist only real settings. `connected`/`network`/`blockHeight` describe
      // the live supporter connection, which is gone after an app restart.
      partialize: (state) => ({
        nodeUrl: state.nodeUrl,
        authToken: state.authToken,
        lockOnBackground: state.lockOnBackground,
        language: state.language,
      }),
    }
  )
);
