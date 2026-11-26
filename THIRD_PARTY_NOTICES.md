<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Third-Party Notices

This repository contains Kumi firmware, libraries, tools, and the companion Android app under
[LICENSE](LICENSE) (`PolyForm-Noncommercial-1.0.0`), together with the following third-party
components and compiled open-corpus assets that retain their respective upstream licenses.

## Android Open Source Project — Keyboard Layout Overlays (`Apache-2.0`)

- **Files:** `app/app/src/main/res/raw/*.kcm`
- **Source:** Android Open Source Project (`frameworks/base/packages/InputDevices`)
- **License:** Apache License 2.0 (<https://www.apache.org/licenses/LICENSE-2.0>)

## FrequencyWords (`MIT`)

- **Files:** `app/app/src/main/assets/dict/en.snapdict`, `ru.snapdict`, `uk.snapdict` (reproducible
  via `app/tools/build_dict.py`)
- **Source:** <https://github.com/hermitdave/FrequencyWords> (2018 OpenSubtitles word frequency
  lists)
- **License:** MIT License

## Google Mozc (`BSD-3-Clause`)

- **Files:** `app/app/src/main/assets/dict/ja.snapja` (reproducible via
  `app/tools/build_ja_dict.py`)
- **Source:** <https://github.com/google/mozc> (`src/data/dictionary_oss/dictionary00..09.txt`)
- **Copyright:** Copyright 2010–2026, Google Inc.
- **License:** BSD 3-Clause License

## Unicode Emoji / CLDR Data (`Unicode-3.0`)

- **Files:** `app/app/src/main/assets/dict/emoji.tsv`
- **License:** Unicode License v3 (<https://www.unicode.org/license.txt>)

## Gradle Wrapper (`Apache-2.0`)

- **Files:** `app/gradlew`, `app/gradlew.bat`, `app/gradle/wrapper/*`
- **Source:** Gradle (<https://gradle.org>), distribution verified by `distributionSha256Sum` in
  `app/gradle/wrapper/gradle-wrapper.properties`
- **License:** Apache License 2.0
