import { currentAddresses, syncWallet } from "../api/rpc";
import { useSettingsStore } from "../store/settings-store";
import { useWalletStore } from "../store/wallet-store";
import { ensureConnected, isConnectionError } from "./node";
import { resolvePendingTx, type PendingResolution } from "./pending";

export interface SyncOutcome {
  /// UTXOs received since the previous sync (change excluded).
  newlyReceived: number;
  pending: PendingResolution;
}

let inFlight: Promise<SyncOutcome> | null = null;

/// Refresh the receiving addresses (a sync can find funds at new indices).
export async function refreshAddresses(): Promise<void> {
  try {
    useWalletStore.getState().setAddresses(await currentAddresses());
  } catch {
    /* keep the cached addresses */
  }
}

/// Sync the wallet with the node. Concurrent calls share one sync.
export function syncNow(): Promise<SyncOutcome> {
  if (inFlight) return inFlight;
  inFlight = (async () => {
    const wallet = useWalletStore.getState();
    wallet.setSyncStatus("syncing");
    try {
      await ensureConnected();
      const known = new Set([
        ...wallet.utxos.map((u) => u.aocl_leaf_index),
        ...wallet.lockedUtxos.map((u) => u.aocl_leaf_index),
      ]);
      const result = await syncWallet();
      useWalletStore.getState().applySync(result);
      if (result.tip_height) {
        useSettingsStore.getState().setBlockHeight(result.tip_height);
      }
      const received = [...result.utxos, ...(result.locked_utxos ?? [])].filter(
        (u) => !u.is_change && !known.has(u.aocl_leaf_index)
      );
      await refreshAddresses();
      let pending: PendingResolution = "none";
      try {
        pending = await resolvePendingTx();
      } catch {
        /* checked again on the next sync */
      }
      return { newlyReceived: received.length, pending };
    } catch (e) {
      const message = String(e);
      useWalletStore.getState().setSyncStatus("error", message);
      if (isConnectionError(message)) {
        // Reconnect on the next attempt.
        useSettingsStore.getState().setConnected(false);
      }
      throw e;
    } finally {
      inFlight = null;
    }
  })();
  return inFlight;
}

export function isSyncing(): boolean {
  return inFlight !== null;
}
