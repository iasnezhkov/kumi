<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Kumi — Companion Android App

Android companion app (**Kumi**, package `org.mindone.snapcase`) for the iKKO Snap keyboard and
CS43198 DAC case on the iKKO MindOne. Works on **stock Android 15 (API 35)** and **LineageOS 23/24
(Android 16/17)** without root. Built with Kotlin, Jetpack Compose, and Material 3.

Controls both **Kumi firmware** (`1209:0001`, vendor interface `FF/4B/01`) and **stock firmware**
(`0661:0881`, experimental legacy view), including guided backup, installation, signed A/B updates,
and bit-for-bit return to stock through the immutable boot ROM service mode (`0666:0888`).

## Highlights

- **Guided Stock → Kumi installer &amp; return to stock** — runs the exact Rust `kumi-flasher` /
  `kumi-install` code through `libkumi_jni.so`: verifies Ed25519 `.kumi` bundles before touching
  USB, takes a double-read verified 4 MiB flash backup with `fsync`, writes a power-fail-resumable
  plan, and restores factory firmware bit-for-bit at any time.
- **Audio &amp; DSP control** — 10-band parametric EQ (4 flash slots + 16 built-in presets with
  instant interactive frequency-response curve, active-band silhouette, and auto preamp from
  `kumi-dsp`), ISO 226 dynamic loudness, spatial crossfeed, soft peak limiter, Class-H &amp; 1 Vrms
  LV output stage, DAC digital filters, volume limit, L/R balance, DSD Direct, live vertical &amp;
  horizontal stereo VU meters, DC + 1 kHz AC headphone impedance measurement, and L/R/Phase test
  tones. Audio controls remain editable even when the 3.5 mm jack is unplugged and the USB DAC is
  asleep.
- **Hardware keys &amp; layers** — 4 in-case key layers, tap-hold, one-shot modifiers, Caps Word,
  8 combos, HID mouse keys, 8 macros, one-tap Clicks / BlackBerry / Russian 36-key hardware presets,
  and Stay-awake toggle.
- **Kumi Strip IME** — compact 46 dp bottom autocomplete bar for physical keyboard typing (works on
  Android 15–17 without shifting the app viewport), built-in physical layout translator
  (`EN` / `RU` / `UK` / `JA` built in, plus `ES` / `DE` / `FR` / `IT` / `PT` / `PL` / `NL` / `TR` /
  `CS` / `SV` / `FI` / `RO` / `HU` with on-device downloadable 25k-word dictionaries, switched via
  `Ctrl+Space`, `Shift+Space`, `Alt+Shift`, or strip tap), accent-aware suggestions and
  language-specific quick extras and long-press variants, bundled offline MIT FrequencyWords
  dictionaries (EN/RU/UK) and BSD-3-Clause Google Mozc Japanese Romaji → Hiragana/Katakana/Kanji
  converter (all refreshable on demand), emoji search, clipboard history, text snippets, and
  Meta+letter Speed Keys.
- **App UI languages** — English, Russian, Spanish, Japanese, German, French, Portuguese, Italian,
  Ukrainian, Polish, Simplified Chinese and Korean, selectable per app in Android system settings.
  `python3 -I tools/check_l10n.py` verifies key and placeholder parity of every `values-*` file.
- **Simulated demo mode** — runs the full install, update, audio, EQ, keymap, and return-to-stock
  code paths in-process without a physical case attached.

## Build

```sh
cd app
./gradlew testDebugUnitTest assembleDebug lint
./gradlew assembleSystem
```

- `local.properties` (`sdk.dir`) is local and gitignored.
- The Gradle wrapper verifies its distribution checksum (`distributionSha256Sum` in
  `gradle/wrapper/gradle-wrapper.properties`).
- Build variants:
  - `debug` — standalone build without `sharedUserId`, installs on any Android 15+ phone (USB Host
    access to Kumi and BROM service mode works without root).
  - `release` — standalone release build (signed when `kumi.keystoreFile` / `KUMI_KEYSTORE_*` is provided).
  - `system` — optional ROM build adding `android:sharedUserId="android.uid.system"`
    (`app/src/system/AndroidManifest.xml`), unsigned for platform signing in LineageOS.

| Component | Version |
|---|---|
| Gradle (wrapper) | 9.8.2 |
| Android Gradle Plugin | 9.5.0 |
| Kotlin / Compose compiler | 2.4.21 |
| Compose BOM | 2026.09.00 (Material 3 1.4.0) |
| compileSdk / targetSdk / minSdk | 37.0 / 36 / 35 |
| JDK for Gradle | JDK 21+ (Java 17 bytecode) |

The native library `libkumi_jni.so` (`arm64-v8a`) is compiled automatically by the `cargoBuildKumi`
Gradle task from `crates/kumi-jni` (Rust profile `android`, target `aarch64-linux-android`, Android
NDK `28.2.13676358`).

| Gradle property (`-P…`) | Purpose |
|---|---|
| `kumi.releaseKey` | 64-hex Ed25519 public key used to verify `.kumi` packages; without it, flashing is disabled and demo mode remains available |
| `kumi.packagesDir` | directory containing signed `.kumi` bundles to embed in `assets/kumi/` (bundles are never committed to Git) |
| `kumi.releasesRepo` | GitHub repository checked for firmware updates (default `iasnezhkov/kumi`, configurable in app settings) |
| `kumi.workspace` | override path to the Rust Cargo workspace (auto-detected as `..` in the standalone repo or `../kumi` in monorepo layout) |
| `kumi.skipNative` | `true` to run JVM unit tests and lint without building `libkumi_jni.so` |
| `kumi.ndkDir`, `kumi.rustup` | custom paths to Android NDK and `rustup` |

## Architecture

| Package | Responsibility |
|---|---|
| `org.mindone.snapcase.kumi` | `KumiNative` JNI bridge, `KumiClient` typed RPCs, `KumiSession` state machine, `KumiEqPresets`, `KumiKeymapPresets`, `SnapLiveMirror` |
| `org.mindone.snapcase.ime` | `SnapStripImeService`, `ImeKeyProcessor`, `PhysicalLayoutTranslator`, `RomajiKanaConverter`, `LocaleVariations`, `EmojiCatalog`, `ClipboardStore`, `QuickLauncher` |
| `org.mindone.snapcase.ime.dict` | `SnapDictBinary` (EN/RU/UK + downloadable languages), `JapaneseDict` (Mozc `SNAPJA01`), `SuggestionEngine`, `PersonalDictionary`, `DictionaryPackManager` |
| `org.mindone.snapcase.transport` | Android USB Host transport (`UsbCasePort`, `ServiceMode`), optional stock hidraw transport |
| `org.mindone.snapcase.ui` | Material 3 Compose screens (`KumiHome`, `KumiAudio`, `KumiEq`, `KumiKeys`, `KumiFirmware`, `KumiDeveloper`, `ImeSettingsScreen`, `SnapStripBar`) |
| `org.mindone.snapcase.remap` | Accessibility service for Speed Keys and optional Android-side key remapping |

JSON models and enum contracts are pinned against `crates/kumi-jni/tests/fixtures/contract.json`
and verified in both Rust and JVM unit tests (`ContractTest`).

## Rebuilding offline dictionaries

Bundled dictionaries in `app/src/main/assets/dict/` are reproducible from open corpora:

```sh
python3 tools/build_dict.py en_50k.txt app/src/main/assets/dict/en.snapdict 15000 latin
python3 tools/build_dict.py ru_50k.txt app/src/main/assets/dict/ru.snapdict 15000 cyrillic
python3 tools/build_dict.py uk_50k.txt app/src/main/assets/dict/uk.snapdict 12000 cyrillic
python3 tools/build_ja_dict.py /tmp/mozc-cache app/src/main/assets/dict/ja.snapja 35000
```

## Network &amp; security boundaries

- Network requests use HTTPS (port 443) only, strictly scoped to GitHub release APIs
  (`api.github.com`, `github.com`, `objects.githubusercontent.com`,
  `release-assets.githubusercontent.com`) and on-demand open dictionary downloads
  (`raw.githubusercontent.com/hermitdave/FrequencyWords`, `raw.githubusercontent.com/google/mozc`).
- Downloaded `.kumi` packages are strictly Ed25519-verified (`verify_strict`) before entering the
  installer list and verified a second time before flashing.
- Flash-writing commands against stock firmware (`SET 0x01`, `0x04`, `0x05`) are never sent; see
  [STOCK-CONTROL.md](STOCK-CONTROL.md).

## License

Licensed under **PolyForm Noncommercial License 1.0.0** ([LICENSE](LICENSE)). Third-party notices
for AOSP keyboard layouts, FrequencyWords, Google Mozc, Unicode Emoji, and Gradle Wrapper are
listed in `THIRD_PARTY_NOTICES.md`.
