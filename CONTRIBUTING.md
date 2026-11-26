<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Contributing

This is a small, independent non-commercial project licensed under `PolyForm-Noncommercial-1.0.0`
(see [LICENSE](LICENSE)). Reports and patches are welcome; please make them easy to act on. For
commercial licensing or collaboration, reach out to Ilia Snezhkov at `iasnezhkov@nonbiri.dev`
([nonbiri.dev/about](https://nonbiri.dev/about)).

## The one rule that matters most

🔴 **Never commit firmware images, flash dumps, signing keys, or built binaries.** Tests that need
a stock dump read it from a path given in `KUMI_STOCK_DUMP`. CI rejects `.apk`, `.so`, `.bin`,
`.elf`, `.kumi`, `.key`, absolute paths from a developer machine, and non-English documentation or
code outside Android localization resources and IME language tables.

## Before reporting a problem

State which of these you have, because the answer changes completely:

- the **iKKO Snap case** on an iKKO MindOne (stock Android 15 or LineageOS), or
- **another device built on the SpaceTouch SPV5048** — the chip and chip-independent crates should
  transfer; the board crate, the keyboard link, and the flash map will not.

Then include:

| | |
|---|---|
| Firmware | `device/info` output, or "stock" with the version the case reports |
| App / Host | Kumi app version (if applicable), phone or computer, Android / OS version |
| What you ran | the exact commands or steps, not a description of them |
| What happened | the actual output, trimmed but not paraphrased |

For a case crash, include the crash record (`kumi crash`) and the boot journal.

## Patches

- One change per commit, with a message that says **why** — the diff already says what. Where the
  code differs from what the stock firmware does, the message says what the stock firmware did and
  what evidence made us change it.
- English only in code, documentation, and commit messages (Android UI translations under
  `app/app/src/main/res/values-*` and IME language/dictionary data are the only exceptions).
- Do not add explanatory code comments. Keep SPDX license headers; explain design decisions in
  commit messages or documentation.
- Every hardware statement carries its evidence and confidence (`measured`, `stock`, `assumed`).
- Never write a case's flash in a test or a tool without a plan that `kumi-install` has checked.

## Before you send

```sh
cargo fmt --check
(cd firmware && cargo fmt --check)
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo target-build
(cd firmware && cargo clippy --release -- -D warnings)
(cd app && ./gradlew testDebugUnitTest lint)
python3 tools/tree-lint.py
```

## What this project will not take

- Firmware images, flash dumps, signing keys, or prebuilt binaries (only the SHA-256 verified Gradle
  wrapper and reproducible open-corpus IME dictionaries built via `app/tools/` are permitted).
- Code copied from vendor firmware or another tree without a note saying where it came from and
  under what license.
- Changes that write flash outside the checked install and update plans.
