# Kumi Changelog

## 1.1.0

### Hi-Fi Audio, Live DSP & True 2 Vrms Line-Out
- **Live 24-band post-DSP RTA spectrum:** 60/96 fps vsync-interpolated spline drawn behind the EQ curve across *Home*, *Presets*, *Bands*, and *Effects*. Computed inside the case DSP via incremental per-stage FFT steps (zero audio glitches on app launch, minimize, or resume), works at up to 384 kHz bit-perfect, needs no `RECORD_AUDIO` permission, and uses 0% CPU in the background.
- **EBU R128 LUFS, True-Peak (`dBTP`) & hardware Auto-Level:** live loudness metering on *Home* and *Sound -> Effects*, plus optional on-case Auto-Level (attenuate-only under Android software volume so it never fights volume buttons, and peak-guarded up to `+4 dB` under UAC2 hardware volume).
- **Two-Layer Parametric EQ:** stack a fixed **Headphone Base (AutoEQ)** curve under all 4 persistent EQ slots, plus a quick **5-band Tone layer**, **Subsonic high-pass**, and **Mono-Bass** crossover.
- **True 2 Vrms Line-Out mode:** locks the CS43198 into fixed `2 Vrms` (`FixedHv`) + `0 dB` output and sets full `0 dBFS` Android media output on confirm (restoring your previous headphone volume when turned off).
- **Upgraded metering & graphs:** 30 Hz polled / 60–96 fps vsync-rendered asymmetric PPM stereo VU meters and taller zero-scroll EQ graphs.

### Instant Search, Hardware Media Gestures & Physical Keyboard UX
- **Instant Home Type-to-Search:** start typing on any Home screen to open a zero-animation app search overlay and press `Enter` to launch (no root required).
- **Native HID Consumer double-tap media controls in firmware:** `2x Shift` previous, `2x Backspace` next, `2x Space` play/pause — works with the screen off, in Doze, and on Android, macOS, Windows, Linux, and iOS.
- **System Shortcuts screen:** optional **Backspace-as-Back** outside text fields with 4-layer text-field guards and per-app exclusions, plus Speed Keys.
- **Full keyboard navigation inside Kumi:** quick card keys (`Q-R`), `Enter` tabs, `W/S` & `J/K` scrolling, `Space` EQ A/B, and bottom shortcut hints.

### Kumi Strip IME — 20 Typing Languages & 13 UI Languages
- **13 full app UI languages** (`en, de, es, fr, it, ja, ko, pl, pt, ru, uk, zh-CN, zh-TW`) with Android Per-App Language support.
- **20 Kumi Strip typing languages:** bundled English, Russian, Ukrainian, Japanese (Mozc), Korean (2-beolsik Hangul), Simplified & Traditional Chinese (Pinyin) plus 13 downloadable European dictionary packs.
- **Typing haptics & inset fixes:** configurable typing vibration (`Off / Smart / All keys`, `12 / 20 / 40 / 65 ms`) and fixed `WRAP_CONTENT` IME insets.

### Updates, Reliability & Security
- **Streamlined Updates screen & in-app changelog reader:** shows App and Case status side by side above the fold with actionable updates first, collapsible older versions, and formatted release notes with clickable links.
- **Glitch-free OTA & playback:** caches crash-log diagnostics in RAM (zero SPI flash stalls during streaming), eliminates unnecessary bulk-IN topic timeouts on Android MUSB hosts, automatically mutes the CS43198 DAC and pauses Android media playback while flashing case firmware, and fixes OTA trial-boot with headphones plugged in.
- **Stable / Developer (RC) update channels**, fixed 36-key Caps Word presets (`D+F` / `F+J`), and a new orthographic MindOne + Snap-In Case illustration.
- **Hardened release firmware (zero daily friction):** release slots disable lab/debug commands and UART sniffing; Ed25519 signed A/B OTA with monotonic anti-rollback; signature-protected launcher bridge and fail-closed APK signature checks; lockscreen guards, RAM-only clipboard (1 h TTL), opt-in WebUSB BOS, SipHash-2-4 USB serial, and a `Shift + Backspace` (2 s) physical-presence guard for service BootROM entry.

## 1.0.1

### Firmware
- **Fixed false Safe Mode triggers:** confirmed-slot boot health tracks monotonic wall time across USB suspend (10 s) instead of USB SOF frames, and clears `unhealthy_starts` before intentional `auto_uac`/mouse re-enumeration and host-requested normal reboots.

### Companion Android App
- **Guided 2-step Kumi Strip setup:** tracks both Android enablement and active keyboard selection and automatically opens the keyboard picker on return.
- **Direct update navigation:** tapping update banners jumps straight to the **Updates** tab and automatically resumes APK installation after granting "Install unknown apps".

## 1.0.0

- Initial public release of Kumi Rust firmware, open control protocol, CLI, emulator test suite, and companion Android app for the iKKO MindOne Snap-In Case.
