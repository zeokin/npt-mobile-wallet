# NPT Mobile Wallet

NPT is a privacy-preserving mobile wallet for [Neptune Cash](https://github.com/Neptune-Crypto/neptune-core). It is built with Tauri (Rust + React) and targets Android (iOS support is planned but requires a macOS build host).

All private keys are derived and stored locally on the device. Transactions are built and signed locally; only `ProofCollection`-backed transactions ever leave the app. The wallet communicates with a neptune-core "supporter" node via JSON-RPC for chain state; the supporter never learns which addresses belong to the wallet.

## Project structure

- `src/` — React + TypeScript frontend
  - `screens/` — page components (Wallet, Send, History, Settings, Unlock, …)
  - `store/` — Zustand state (wallet, settings)
  - `api/rpc.ts` — Tauri invoke wrappers
  - `hooks/` — React hooks (session guard)
  - `components/ui/` — shared UI components (NavBar, logo, …)
- `src-tauri/` — Rust backend
  - `src/lib.rs` — Tauri commands, app state, session logic
  - `src/seed.rs` — BIP39 mnemonic + AES-256-GCM + Argon2id KDF
  - `src/keys.rs` — local key derivation
  - `src/sync.rs` — privacy-preserving UTXO scan (flag-based + local decryption)
  - `src/rpc.rs` — JSON-RPC client for supporter node
  - `src/transaction.rs` — STARK proof generation for sends
  - `gen/android/` — generated Android project (gradle, manifest, Kotlin shims)
- `leveldb-sys/`, `systemstat-stub/` — stubs so `neptune-cash` compiles on mobile

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
file lives in the app's sandboxed data directory.

See the source of [`src-tauri/src/seed.rs`](src-tauri/src/seed.rs) for the
encryption details.
