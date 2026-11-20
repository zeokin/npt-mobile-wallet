import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { sendTransaction, type SendProgress } from "../api/rpc";
import { useSendStore, type SendRequestInfo } from "../store/send-store";
import { useWalletStore } from "../store/wallet-store";

export type SendFailure =
  | { kind: "lustration"; threshold: string }
  | { kind: "password"; message: string }
  | { kind: "error"; message: string };

/// Build, prove and submit a transaction. Progress and the result go to the
/// send store; the sent transaction is recorded in the history even if the
/// Send screen was left meanwhile. Resolves to `null` on success.
export async function runSend(
  request: SendRequestInfo,
  pin: string,
  acceptLustrations: boolean
): Promise<SendFailure | null> {
  useSendStore.getState().begin(request);
  let unlisten: UnlistenFn | null = null;
  try {
    unlisten = await listen<SendProgress>("send_state", (event) =>
      useSendStore.getState().setProgress(event.payload)
    );
  } catch {
    /* progress display is optional */
  }
  try {
    const result = await sendTransaction(
      pin,
      request.recipient,
      request.amount,
      request.fee,
      acceptLustrations
    );
    useWalletStore.getState().addOutgoingTx({
      recipient: request.recipient,
      amount: request.amount,
      fee: request.fee,
      timestamp: Date.now(),
      status: "pending",
      addition_record_hexes: result.addition_record_hexes ?? [],
      txid: result.txid,
      outputs: result.outputs,
    });
    useSendStore.getState().finish(result);
    return null;
  } catch (e) {
    const message = String(e);
    useSendStore.getState().fail();
    const lustration = message.match(/LUSTRATION_REQUIRED:(\d+)/);
    if (lustration) return { kind: "lustration", threshold: lustration[1] };
    if (/^Wrong password|Too many attempts/.test(message)) return { kind: "password", message };
    return { kind: "error", message };
  } finally {
    unlisten?.();
  }
}
