import { create } from "zustand";
import { persist } from "zustand/middleware";

interface SettingsState {
  nodeUrl: string;
  authToken: string;
  connected: boolean;
  network: string | null;
  blockHeight: number;
  setNodeUrl: (url: string) => void;
  setAuthToken: (token: string) => void;
  setConnected: (connected: boolean, network?: string, blockHeight?: number) => void;
  reset: () => void;
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      nodeUrl: "",
      authToken: "",
      connected: false,
      network: null,
      blockHeight: 0,
      setNodeUrl: (nodeUrl) => set({ nodeUrl }),
      setAuthToken: (authToken) => set({ authToken }),
      setConnected: (connected, network, blockHeight) =>
        set({ connected, network: network ?? null, blockHeight: blockHeight ?? 0 }),
      reset: () =>
        set({ connected: false, network: null, blockHeight: 0 }),
    }),
    {
      name: "npt-settings",
      // Persist only real settings. `connected`/`network`/`blockHeight` describe
      // the live supporter RPC connection, which is per-process and is gone
      // after an app restart or a wallet import — persisting them leaves a stale
      // `connected: true` that makes the wallet skip connecting and then fail to
      // sync. Reset them to defaults on each launch.
      partialize: (state) => ({
        nodeUrl: state.nodeUrl,
        authToken: state.authToken,
      }),
    }
  )
);
