import { create } from "zustand";
import type { SendProgress, SendResult } from "../api/rpc";

export interface SendRequestInfo {
  recipient: string;
  amount: string;
  fee: string;
  /// Key type of the recipient address ("ec_hybrid", …), if known.
  recipientType: string | null;
}

/// The send in progress. Lives outside the Send screen, so leaving the screen
/// while a transaction is being proven neither loses the result nor the
/// record of the sent transaction. Not persisted.
interface SendState {
  phase: "idle" | "sending" | "done";
  request: SendRequestInfo | null;
  startedAt: number;
  progress: SendProgress | null;
  result: SendResult | null;
  begin: (request: SendRequestInfo) => void;
  setProgress: (progress: SendProgress) => void;
  finish: (result: SendResult) => void;
  fail: () => void;
  reset: () => void;
}

export const useSendStore = create<SendState>()((set) => ({
  phase: "idle",
  request: null,
  startedAt: 0,
  progress: null,
  result: null,
  begin: (request) =>
    set({ phase: "sending", request, startedAt: Date.now(), progress: null, result: null }),
  setProgress: (progress) => set({ progress }),
  finish: (result) => set({ phase: "done", result }),
  fail: () => set({ phase: "idle", progress: null }),
  reset: () => set({ phase: "idle", request: null, startedAt: 0, progress: null, result: null }),
}));

/// Set while the QR scanner is open, so that leaving the app for the camera
/// does not count as "app went to background".
export const scannerState = { active: false };
