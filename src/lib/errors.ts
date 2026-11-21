import { t, type MessageKey } from "../i18n";

// The backend reports errors in English. Known messages are shown in the
// selected language (numbers and amounts kept); anything else as it is.

type Rule = [RegExp, (m: RegExpMatchArray) => string];

const simple = (key: MessageKey) => () => t(key);

const RULES: Rule[] = [
  // Password and attempt limit (see session.rs).
  [/^Wrong password\. Too many attempts — locked for (\d+) seconds/, (m) => t("errors.lockedFor", { s: m[1] })],
  [/^Too many attempts\. Wait (\d+) seconds/, (m) => t("errors.waitSeconds", { s: m[1] })],
  [/^Wrong password\. (\d+) attempts remaining/, (m) => t("errors.wrongPasswordRemaining", { n: m[1] })],
  [/^Wrong password\b/, simple("errors.wrongPassword")],
  [/^Wallet file is damaged/, simple("errors.walletDamaged")],
  [/^Session expired/, simple("errors.sessionExpired")],
  [/^Wallet is locked/, simple("errors.walletLocked")],
  [/^Wallet already exists/, simple("errors.walletExists")],
  [/^The new password must be different/, simple("errors.samePassword")],

  // Node connection.
  [/^Not connected to supporter/, simple("errors.notConnected")],
  [/did not answer in time|timed out/i, simple("errors.nodeTimeout")],
  [/^Cannot reach node|could not connect to the node/i, simple("errors.nodeUnreachable")],
  [/^Node URL must start with/, simple("errors.badUrl")],
  [/^The node does not have the UTXO index enabled/, simple("errors.noUtxoIndex")],
  [/^Unknown network: (.+)/, (m) => t("errors.unknownNetwork", { network: m[1] })],

  // Amounts and coins.
  [
    /^Insufficient balance: spendable (\S+) NPT, but this transaction needs (\S+) NPT \(amount plus fee\)\.(?: (\S+) NPT is still time-locked\.)?/,
    (m) => {
      const text = t("errors.insufficient", { spendable: m[1], needed: m[2] });
      return m[3] ? `${text} ${t("errors.insufficientLocked", { locked: m[3] })}` : text;
    },
  ],
  [
    /^This amount needs (\d+) coins \(UTXOs\), but a phone can prove at most (\d+)/,
    (m) => t("errors.tooManyInputs", { n: m[1], max: m[2] }),
  ],
  [/^Amount must be greater than zero/, simple("errors.amountZero")],
  [/^(The fee|Fee) must not be negative/, simple("errors.feeNegative")],
  [/^A transaction is already being prepared/, simple("errors.alreadySending")],

  // Addresses.
  [/^Not a valid (\w+) address/, (m) => t("errors.invalidAddress", { network: m[1] })],
  [/^Invalid address/, simple("errors.invalidAddressShort")],
  [/^Address #(\d+) has received coins/, (m) => t("errors.addressFunded", { index: m[1] })],
  [/^The main address \(#0\) cannot be removed/, simple("errors.mainAddress")],
  [/^Address #(\d+) is not in the list/, (m) => t("errors.addressNotListed", { index: m[1] })],
  [/^At most (\d+) addresses per type/, (m) => t("errors.tooManyAddresses", { n: m[1] })],

  // Seed phrase.
  [/^Expected 18 words, got (\d+)/, (m) => t("errors.wordCount", { n: m[1] })],
  [/^Invalid mnemonic/, simple("errors.invalidMnemonic")],
  [/^These 18 words are not a valid seed phrase/, simple("errors.checksum")],

  // Node answers to a submitted transaction (see send.rs).
  [/time is in the future/, simple("errors.futureDated")],
  [/time is too old/, simple("errors.tooOld")],
  [/^The network requires older coins in this transaction to be revealed/, simple("errors.missingLustration")],
  [/lustration limit reached/, simple("errors.lustrationLimit")],
  [/^The transaction has more inputs or outputs than the network allows/, simple("errors.tooBig")],
  [/^The node rejected the transaction as invalid/, simple("errors.invalidTx")],
  [/^The node could not accept this transaction in (\d+) attempts/, (m) => t("errors.notConfirmable", { n: m[1] })],
  [/^The node did not accept the transaction/, simple("errors.notAccepted")],
  [/^The node refused the transaction: (.+)/, (m) => t("errors.refused", { detail: m[1] })],
  [/^Could not submit the transaction: (.+)/, (m) => t("errors.submitFailed", { detail: m[1] })],

  [/^Clipboard access is only available on the phone/, simple("errors.clipboardPhoneOnly")],
];

/// The message of an error from the backend, in the selected language.
export function errorText(error: unknown): string {
  const message = String(error).replace(/^Error:\s*/, "").trim();
  for (const [pattern, render] of RULES) {
    const m = message.match(pattern);
    if (m) return render(m);
  }
  return message;
}
