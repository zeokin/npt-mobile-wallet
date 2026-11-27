# NPT Mobile Wallet

NPT is a privacy-preserving mobile wallet for [Neptune Cash](https://github.com/Neptune-Crypto/neptune-core). It is built with Tauri (Rust + React) and targets Android (iOS support is planned but requires a macOS build host).

All private keys are derived and stored locally on the device. Transactions are built and signed locally; only `ProofCollection`-backed transactions ever leave the app. The wallet communicates with a neptune-core "supporter" node via JSON-RPC for chain state; the supporter never learns which addresses belong to the wallet.

## Project structure

- `src/` — React + TypeScript frontend
  - `screens/` — page components (Wallet, Send, History, Settings, Unlock, …)
  - `store/` — Zustand state (wallet, settings, send in progress)
  - `api/rpc.ts` — Tauri invoke wrappers
  - `lib/` — shared logic: exact amount math, sync, pending transactions, send, node, error texts
  - `hooks/` — React hooks (session guard and background lock, screenshot protection, pull to refresh)
  - `components/` — sheets (addresses, pending transaction, history details, change password, language) and the wallet timeline
  - `components/ui/` — shared UI components (NavBar, bottom sheet, QR code, toggle, …)
  - `i18n/` — UI translations: `en.ts` is the source; every other language has the same keys (checked by TypeScript)
- `src-tauri/` — Rust backend (neptune-core crates 0.20.x, Triton VM 9)
  - `src/lib.rs` — Tauri commands and app state
  - `src/session.rs` — unlocked-wallet session, auto-lock clock, password-attempt limiter
  - `src/seed.rs` — BIP39 mnemonic + AES-256-GCM + Argon2id KDF, atomic seed file writes
  - `src/keys.rs` — local key derivation, per-session key cache, address validation
  - `src/sync.rs` — privacy-preserving UTXO scan (flag-based + local decryption, block cache)
  - `src/send.rs` — send pipeline (input selection, membership proofs, retries, submission)
  - `src/meta.rs` — per-wallet address counters ("New address"), removed and funded addresses
  - `src/android.rs` — Android-only helpers via JNI (screenshot protection, clipboard)
  - `src/transaction.rs` — ProofCollection proving for sends
  - `src/rpc.rs` — JSON-RPC client for supporter node
  - `gen/android/` — generated Android project (gradle, manifest, Kotlin shims)
- `scripts/` — build and check scripts (styles for old WebViews, translation checks)

## How it works

**Sync.** The wallet derives its keys locally and asks the supporter's UTXO
index for the blocks that carry announcements for those keys
(`utxoindex_blockHeightsByFlags`). Each such block is downloaded once and its
public parts are cached on the device, so later syncs only fetch new blocks.
Announcements are decrypted locally; a UTXO counts only if it is locked to one
of the wallet's keys and is really an output of its block. One batched call
(`archival_batchAreBloomIndicesSet`) tells which UTXOs are spent. Every key
type (generation, EC-hybrid, viewing, symmetric/change) is scanned over a
look-ahead window that widens automatically when funds are found near its end,
so seeds used in neptune-core or the desktop wallet show their full balance.
Time-locked funds are shown as locked rather than hidden. Any RPC failure fails
the sync instead of silently showing a wrong balance.

**Send.** Inputs are selected largest-first (at most 10). Lustration of old
inputs is decided from the tip header before anything about the inputs is sent
to the node. Membership proofs and the tip header are taken from the same block,
the transaction is validated locally, then a `ProofCollection` is proven for the
tip's consensus rule set. If the node answers that the transaction is no longer
confirmable (new blocks arrived during proving), it is rebuilt and proven again,
up to three times. Progress is emitted to the UI as `send_state` events.

**Addresses.** Receiving addresses are derived from the seed by index. "New
address" hands out the next index; an address the user added can be removed
from the list if it never received coins. A removed address keeps being
scanned, so a later payment to it still shows up (and lists it again).

**Languages.** The UI follows the phone's language when it is one of the
supported ones (English, Chinese, Korean, Japanese, Russian, Spanish,
Portuguese, French, German, Turkish, Vietnamese, Indonesian), or the language
chosen in Settings. The seed words stay English (BIP39 word list). To add a
language, copy `src/i18n/en.ts`, translate the values and register it in
`src/i18n/index.ts`. TypeScript rejects missing or unknown keys;
`python3 scripts/check-translations.py src/i18n/en.ts src/i18n/<code>.ts` also
checks placeholders, bold markers and untranslated terms.

**Old WebViews.** The UI runs in the phone's "Android System WebView", which
can be years older than the phone's Android version: phones that do not get
Play Store updates, emulators, and the devices that app stores review on.
Tailwind 4 needs a 2023 browser engine (Chrome 111); on an older one all of its
styles are ignored and the app shows as bare HTML. So `npm run build` ends with
`scripts/legacy-css.mjs`, which appends a copy of the styles rewritten for old
engines. Only engines without `color-mix()` read that copy; current ones skip
it. The JavaScript is built for Chrome 69 (`build.target` in `vite.config.ts`).
The app is checked on Chromium 74 to 110 and on a current one, and looks the
same on all of them. Two rules keep it that way:

- Text next to an icon in a flex container with a `gap-*` class goes in a
  `<span>` (old engines turn the gap into margins, and a margin needs an
  element). `npm run check:gap` lists the places that break this; CI runs it.
- The build fails if the styles contain something an old engine cannot read
  and `legacy-css.mjs` has no rule for it yet. Add the rule there.

## Run in development mode on a Debian/Ubuntu system

The fastest way to iterate on UI changes is to run the frontend in a browser — the Tauri backend is mocked so you see the layout but not real wallet functionality.

```bash
# Install Node.js (22.x or newer)
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt install -y nodejs

# Install system dependencies for Tauri desktop (needed even for dev)
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
    librsvg2-dev build-essential libglib2.0-dev

# Install Rust via rustup if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install JS dependencies
npm install

# Frontend only (fastest — for UI-only work in a browser at http://localhost:1420)
npm run dev

# Full desktop app (Rust backend + WebView, real wallet)
npm run tauri dev
```

## Run on an Android emulator or device

Mobile builds require the Android SDK and NDK.

```bash
# Install Android SDK + NDK (via Android Studio or sdkmanager)
# Set environment variables
export ANDROID_HOME="$HOME/android-sdk"
export NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
export JAVA_HOME="/path/to/jdk-17"
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"

# Install Android Rust targets
rustup target add aarch64-linux-android x86_64-linux-android

# Initialise the Android project (first time only)
cargo tauri android init

# Start an emulator or connect a device via USB (`adb devices` should list it)
# Run the app with hot reload
cargo tauri android dev
```

Genymotion, LDPlayer, and Android Studio's emulators all work. For x86\_64
emulators (LDPlayer, most Android Studio images) add `--target x86_64` to
the build commands below.

## Build release APK

```bash
# Build release APK for both aarch64 (real phones) and x86_64 (emulators)
cargo tauri android build --target aarch64 --target x86_64
```

The unsigned APK is produced at:

```
src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk
```

### Signing the APK

Android requires APKs to be signed before installation. Create a keystore once:

```bash
keytool -genkey -v -keystore npt-wallet.keystore -alias npt -keyalg RSA -keysize 2048 -validity 10000
```

Sign the built APK (uses the v2/v3 signing scheme — v1 alone is not accepted on modern devices):

```bash
APK_DIR=src-tauri/gen/android/app/build/outputs/apk/universal/release
$ANDROID_HOME/build-tools/36.0.0/zipalign -f 4 \
    "$APK_DIR/app-universal-release-unsigned.apk" \
    "$APK_DIR/npt-wallet-aligned.apk"
$ANDROID_HOME/build-tools/36.0.0/apksigner sign \
    --ks npt-wallet.keystore --ks-key-alias npt \
    --out "$APK_DIR/npt-wallet-signed.apk" \
    "$APK_DIR/npt-wallet-aligned.apk"
```

**Keep the keystore safe** — all future updates must be signed with the same key, otherwise users will not be able to update the app.

## Supporter node

The wallet needs a neptune-core node with the following RPC namespaces enabled:
`node`, `chain`, `wallet`, `archival`, `utxoindex`. The `personal` namespace is
**not** required and the wallet never calls it — it is privacy-violating and
would require `--unsafe-rpc` on the node, which is never acceptable for a
third-party supporter.

To run your own supporter node:

```bash
git clone https://github.com/Neptune-Crypto/neptune-core
cd neptune-core
cargo run --release -- \
    --listen-rpc=<public-ip-or-localhost>:9797 \
    --rpc-modules "node,chain,wallet,archival,utxoindex" \
    --utxo-index
```

Then set the supporter URL in the wallet's Settings screen.

No secrets are shared between the node and the wallet. A malicious supporter
cannot steal funds; at worst it can refuse to serve requests or feed stale
chain data. Use a TLS-terminating reverse proxy (Caddy, nginx) if you want to
protect against passive eavesdroppers on the network path.

## Security

All private key material is encrypted with AES-256-GCM; the key is derived
from the user's password via Argon2id (64 MB memory, 3 iterations). The seed
file lives in the app's sandboxed data directory and is written atomically.

While unlocked, the decrypted wallet entropy and derived keys stay in the Rust
process (wiped when the wallet locks); the password is not kept and is asked
for again to send or to reveal the seed phrase. Wrong passwords are limited
(five attempts, then an escalating cooldown that survives app restarts). The
wallet locks after three minutes without activity, measured on a clock that
keeps running while the phone sleeps. Spending secrets of UTXOs never reach the
WebView. Android cloud backup and device-to-device transfer of app data are
disabled; restore a wallet on a new phone from its seed phrase.

See the source of [`src-tauri/src/seed.rs`](src-tauri/src/seed.rs) for the
encryption details.
