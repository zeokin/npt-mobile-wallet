import { connectNode, type ConnectionInfo } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";

/// The public supporter node used unless the user configures another one.
export const DEFAULT_NODE_URL = "https://wallet.neptunefundamentals.org";

/** The node URL in use (the user's choice, or the default). */
export function currentNodeUrl(): string {
  return useSettingsStore.getState().nodeUrl.trim() || DEFAULT_NODE_URL;
}

/** Connect the backend to the configured node and record the result. */
export async function connectToNode(): Promise<ConnectionInfo> {
  const info = await connectNode(currentNodeUrl());
  useSettingsStore.getState().setConnected(true, info.network, info.block_height);
  return info;
}

/** Connect unless this app session is already connected. */
export async function ensureConnected(): Promise<void> {
  if (!useSettingsStore.getState().connected) {
    await connectToNode();
  }
}

/** Whether an error message means the node could not be reached. */
export function isConnectionError(message: string): boolean {
  return /connect|did not answer|not connected|HTTP error|timed out|network/i.test(message);
}
