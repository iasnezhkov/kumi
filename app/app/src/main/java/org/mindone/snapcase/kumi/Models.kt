// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

data class KumiVersion(val major: Int, val minor: Int, val patch: Int) : Comparable<KumiVersion> {
    override fun compareTo(other: KumiVersion): Int = compareValuesBy(this, other, { it.major }, { it.minor }, { it.patch })
    override fun toString() = "$major.$minor.$patch"
    fun toJson(): JSONObject = JSONObject().put("major", major).put("minor", minor).put("patch", patch)

    companion object {
        private val PATTERN = Regex("""^v?(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:[-+].*)?$""")
        private val EMBEDDED = Regex("""(?:^|[^0-9])v?(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:[^0-9]|$)""")

        fun parse(text: String): KumiVersion? = PATTERN.matchEntire(text.trim())?.destructured?.let { (a, b, c) ->
            KumiVersion(a.toInt(), b.toInt(), c.toInt())
        }

        fun extractFromFilename(filename: String): KumiVersion? {
            val base = filename.substringBeforeLast('.')
            return EMBEDDED.find(base)?.destructured?.let { (a, b, c) ->
                KumiVersion(a.toInt(), b.toInt(), c.toInt())
            }
        }

        fun from(json: JSONObject) = KumiVersion(json.getInt("major"), json.getInt("minor"), json.getInt("patch"))
    }
}

data class Capabilities(
    val audioOut: Boolean,
    val audioIn: Boolean,
    val dsd: Boolean,
    val parametricEq: Boolean,
    val keymap: Boolean,
    val layers: Boolean,
    val battery: Boolean,
    val persistentSettings: Boolean,
    val firmwareUpdate: Boolean,
    val developer: Boolean,
    val lab: Boolean,
    val otherBits: Long = 0,
) {
    companion object {
        val NAMES = listOf(
            "audio_out", "audio_in", "dsd", "parametric_eq", "keymap", "layers", "battery", "persistent_settings", "firmware_update", "developer",
            "lab",
        )

        fun from(j: JSONObject): Capabilities {
            val flag = { name: String -> j.optBoolean(name, false) }
            return Capabilities(
                flag("audio_out"), flag("audio_in"), flag("dsd"), flag("parametric_eq"), flag("keymap"), flag("layers"), flag("battery"),
                flag("persistent_settings"), flag("firmware_update"), flag("developer"), flag("lab"), j.optLong("other_bits", 0),
            )
        }
    }
}

data class DeviceInfo(
    val protocol: Int,
    val firmware: KumiVersion,
    val build: String,
    val chipId: Long,
    val flashJedec: List<Int>,
    val serial: List<Int>?,
    val runningFromRam: Boolean,
    val capabilities: Capabilities,
) {
    val serialText: String?
        get() = serial?.let { s -> if (s.all { it in 0x21..0x7E }) s.map { it.toChar() }.joinToString("") else s.joinToString("") { "%02x".format(it) } }

    companion object {
        fun from(j: JSONObject) = DeviceInfo(
            j.getInt("protocol"), KumiVersion.from(j.getJSONObject("firmware")), j.getString("build"), j.getLong("chip_id"),
            j.getJSONArray("flash_jedec").ints(), j.optJSONArray("serial")?.ints(), j.getBoolean("running_from_ram"),
            Capabilities.from(j.getJSONObject("capabilities")),
        )
    }
}

enum class Jack { Empty, Headphones, Headset, Unknown }

enum class StreamKind { Pcm, Dop, NativeDsd, Unknown }

data class StreamFormat(val kind: StreamKind, val rateHz: Int, val bits: Int, val channels: Int) {
    companion object {
        fun from(j: JSONObject) = StreamFormat(enumOr(j.optString("kind"), StreamKind.Unknown), j.getInt("rate_hz"), j.getInt("bits"), j.getInt("channels"))
    }
}

data class AudioCounters(val underruns: Long, val overruns: Long, val feedbackCorrections: Long, val dacFaults: Long) {
    companion object {
        fun from(j: JSONObject) = AudioCounters(j.getLong("underruns"), j.getLong("overruns"), j.getLong("feedback_corrections"), j.getLong("dac_faults"))
    }
}

data class AudioStatus(
    val jack: Jack,
    val usbAudioPresent: Boolean,
    val stream: StreamFormat?,
    val hostVolumeCentiDb: List<Int>,
    val hostMuted: Boolean,
    val eqActive: Int?,
    val counters: AudioCounters,
) {
    companion object {
        fun from(j: JSONObject) = AudioStatus(
            enumOr(j.optString("jack"), Jack.Unknown), j.getBoolean("usb_audio_present"), j.optObject("stream")?.let(StreamFormat::from),
            j.getJSONArray("host_volume_centi_db").ints(), j.getBoolean("host_muted"), j.optIntOrNull("eq_active"),
            AudioCounters.from(j.getJSONObject("counters")),
        )
    }
}

enum class OutputFilter { FastLowLatency, FastPhaseCompensated, SlowLowLatency, SlowPhaseCompensated, NonOversampling }

data class OutputSettings(val filter: OutputFilter, val volumeLimitCentiDb: Int, val balanceCentiDb: Int, val dsdDirect: Boolean) {
    fun toJson(): JSONObject = JSONObject().put("filter", filter.name).put("volume_limit_centi_db", volumeLimitCentiDb)
        .put("balance_centi_db", balanceCentiDb).put("dsd_direct", dsdDirect)

    companion object {
        fun from(j: JSONObject) =
            OutputSettings(enumOf(j.getString("filter")), j.getInt("volume_limit_centi_db"), j.getInt("balance_centi_db"), j.getBoolean("dsd_direct"))
    }
}

enum class BandKind { Peak, LowShelf, HighShelf, LowPass, HighPass }

data class ParametricBand(val kind: BandKind, val frequencyHz: Int, val qMilli: Int, val gainCentiDb: Int) {
    val valid: Boolean
        get() = frequencyHz in MIN_HZ..MAX_HZ && qMilli in MIN_Q_MILLI..MAX_Q_MILLI && kotlin.math.abs(gainCentiDb) <= MAX_GAIN_CENTI_DB

    fun toJson(): JSONObject = JSONObject().put("kind", kind.name).put("frequency_hz", frequencyHz).put("q_milli", qMilli).put("gain_centi_db", gainCentiDb)

    companion object {
        const val MIN_HZ = 10
        const val MAX_HZ = 40_000
        const val MIN_Q_MILLI = 100
        const val MAX_Q_MILLI = 20_000
        const val MAX_GAIN_CENTI_DB = 1_800

        fun from(j: JSONObject) = ParametricBand(enumOf(j.getString("kind")), j.getInt("frequency_hz"), j.getInt("q_milli"), j.getInt("gain_centi_db"))
    }
}

data class EqProfile(val preampCentiDb: Int, val bands: List<ParametricBand>) {
    val valid: Boolean get() = preampCentiDb in MIN_PREAMP_CENTI_DB..0 && bands.size <= MAX_BANDS && bands.all { it.valid }

    fun toJson(): JSONObject = JSONObject().put("preamp_centi_db", preampCentiDb).put("bands", JSONArray(bands.map { it.toJson() }))

    companion object {
        const val SLOTS = 4
        const val MAX_BANDS = 10
        const val MIN_PREAMP_CENTI_DB = -2_400
        val FLAT = EqProfile(0, emptyList())

        fun from(j: JSONObject) = EqProfile(j.getInt("preamp_centi_db"), j.getJSONArray("bands").objects().map(ParametricBand::from))
    }
}

data class MicSettings(val gainCentiDb: Int) {
    fun toJson(): JSONObject = JSONObject().put("gain_centi_db", gainCentiDb)

    companion object {
        fun from(j: JSONObject) = MicSettings(j.getInt("gain_centi_db"))
    }
}

enum class CrossfeedMode { Off, Subtle, Natural, Strong }

data class DspSettings(
    val crossfeed: CrossfeedMode = CrossfeedMode.Off,
    val limiter: Boolean = false,
    val mono: Boolean = false,
    val invertPhase: Boolean = false,
    val loudness: Boolean = false,
    val swapChannels: Boolean = false,
) {
    fun toJson(): JSONObject = JSONObject()
        .put("crossfeed", crossfeed.name)
        .put("limiter", limiter)
        .put("mono", mono)
        .put("invert_phase", invertPhase)
        .put("loudness", loudness)
        .put("swap_channels", swapChannels)

    companion object {
        fun from(j: JSONObject) = DspSettings(
            enumOf(j.getString("crossfeed")),
            j.getBoolean("limiter"),
            j.getBoolean("mono"),
            j.getBoolean("invert_phase"),
            j.optBoolean("loudness", false),
            j.optBoolean("swap_channels", false),
        )
    }
}

enum class ClassHMode { AdaptiveHv, LowPowerLv, FixedHv }

data class DacExtras(
    val highPassFilter: Boolean = true,
    val deEmphasis: Boolean = false,
    val lineOut: Boolean = false,
    val classH: ClassHMode = ClassHMode.AdaptiveHv,
    val autoUacOnJack: Boolean = true,
    val stayAwakeWhileActive: Boolean = false,
) {
    fun toJson(): JSONObject = JSONObject()
        .put("high_pass_filter", highPassFilter)
        .put("de_emphasis", deEmphasis)
        .put("line_out", lineOut)
        .put("class_h", classH.name)
        .put("auto_uac_on_jack", autoUacOnJack)
        .put("stay_awake_while_active", stayAwakeWhileActive)

    companion object {
        fun from(j: JSONObject) = DacExtras(
            j.getBoolean("high_pass_filter"),
            j.getBoolean("de_emphasis"),
            j.getBoolean("line_out"),
            enumOf(j.getString("class_h")),
            j.getBoolean("auto_uac_on_jack"),
            j.optBoolean("stay_awake_while_active", false),
        )
    }
}

data class ImpedanceReport(
    val leftOhms: Int?,
    val rightOhms: Int?,
    val leftAcOhms: Int? = null,
    val rightAcOhms: Int? = null,
    val leftStatus: Int,
    val rightStatus: Int,
    val hpStatus: Int,
    val loadStat: Int,
) {
    companion object {
        fun from(j: JSONObject) = ImpedanceReport(
            j.optIntOrNull("left_ohms"),
            j.optIntOrNull("right_ohms"),
            j.optIntOrNull("left_ac_ohms"),
            j.optIntOrNull("right_ac_ohms"),
            j.getInt("left_status"),
            j.getInt("right_status"),
            j.getInt("hp_status"),
            j.getInt("load_stat"),
        )
    }
}

data class AudioMeters(
    val active: Boolean = false,
    val peakLeftCentiDb: Int = KumiLimits.METER_FLOOR_CENTI_DB,
    val peakRightCentiDb: Int = KumiLimits.METER_FLOOR_CENTI_DB,
    val rmsLeftCentiDb: Int = KumiLimits.METER_FLOOR_CENTI_DB,
    val rmsRightCentiDb: Int = KumiLimits.METER_FLOOR_CENTI_DB,
    val clippedSamples: Long = 0,
    val limiterHits: Long = 0,
) {
    companion object {
        fun from(j: JSONObject) = AudioMeters(
            j.getBoolean("active"),
            j.getInt("peak_left_centi_db"),
            j.getInt("peak_right_centi_db"),
            j.getInt("rms_left_centi_db"),
            j.getInt("rms_right_centi_db"),
            j.getLong("clipped_samples"),
            j.getLong("limiter_hits"),
        )
    }
}

data class TestToneRequest(
    val channel: AudioChannel,
    val frequencyHz: Int,
    val durationMs: Int,
    val invertRight: Boolean = false,
) {
    val valid: Boolean
        get() = frequencyHz == 0 || (
            frequencyHz in KumiLimits.TEST_TONE_MIN_HZ..KumiLimits.TEST_TONE_MAX_HZ &&
                durationMs in KumiLimits.TEST_TONE_MIN_MS..KumiLimits.TEST_TONE_MAX_MS
            )

    fun toJson(): JSONObject = JSONObject()
        .put("channel", channel.name)
        .put("frequency_hz", frequencyHz)
        .put("duration_ms", durationMs)
        .put("invert_right", invertRight)

    companion object {
        fun from(j: JSONObject) = TestToneRequest(
            enumOf(j.getString("channel")),
            j.getInt("frequency_hz"),
            j.getInt("duration_ms"),
            j.getBoolean("invert_right"),
        )
    }
}

data class SystemTelemetry(
    val cpuMhz: Int,
    val dcdcActive: Boolean,
    val dvddMv: Int,
    val analogRailsOn: Boolean,
    val audioPllEnabled: Boolean,
    val wfiPercentTenths: Int,
    val wfiWakeups: Long,
    val jackAdc: Int,
    val uptimeMs: Long,
) {
    companion object {
        fun from(j: JSONObject) = SystemTelemetry(
            j.getInt("cpu_mhz"),
            j.getBoolean("dcdc_active"),
            j.getInt("dvdd_mv"),
            j.getBoolean("analog_rails_on"),
            j.getBoolean("audio_pll_enabled"),
            j.getInt("wfi_percent_tenths"),
            j.getLong("wfi_wakeups"),
            j.getInt("jack_adc"),
            j.getLong("uptime_ms"),
        )
    }
}

enum class CaseHotkey {
    CycleEq, CycleFilter, CycleCrossfeed, ToggleLimiter, ToggleMono, ToggleMute, ToggleLineOut, ToggleLoudness, ToggleCapsWord,
}

enum class MouseAction {
    MoveUp, MoveDown, MoveLeft, MoveRight, LeftClick, RightClick, MiddleClick, WheelUp, WheelDown,
}

data class MacroStep(val usage: Int, val modifiers: Int = 0) {
    val valid: Boolean get() = usage in KumiLimits.KEY_MIN_USAGE..KumiLimits.KEY_MAX_USAGE && modifiers in 0..0xFF

    fun toJson(): JSONObject = JSONObject().put("usage", usage).put("modifiers", modifiers)

    companion object {
        fun from(j: JSONObject) = MacroStep(j.getInt("usage"), j.getInt("modifiers"))
    }
}

data class MacroProgram(val steps: List<MacroStep>) {
    val valid: Boolean get() = steps.size <= KumiLimits.MACRO_MAX_STEPS && steps.all { it.valid }

    fun toJson(): JSONObject = JSONObject().put("steps", JSONArray(steps.map { it.toJson() }))

    companion object {
        val EMPTY = MacroProgram(emptyList())

        fun from(j: JSONObject) = MacroProgram(j.getJSONArray("steps").objects().map(MacroStep::from))
    }
}

data class KeyCombo(val keyA: Int, val keyB: Int, val action: KeyAction) {
    val valid: Boolean
        get() = keyA in KumiLimits.KEY_MIN_USAGE..KumiLimits.COMBO_MAX_USAGE &&
            keyB in KumiLimits.KEY_MIN_USAGE..KumiLimits.COMBO_MAX_USAGE &&
            keyA != keyB &&
            action !is KeyAction.Transparent &&
            action.valid

    fun toJson(): JSONObject = JSONObject().put("key_a", keyA).put("key_b", keyB).put("action", action.toJson())

    companion object {
        fun from(j: JSONObject) = KeyCombo(j.getInt("key_a"), j.getInt("key_b"), KeyAction.from(j.get("action")))
    }
}

data class KeyConfig(val tappingTermMs: Int = 200, val oneShotTimeoutMs: Int = 3_000, val comboTermMs: Int = 50) {
    val valid: Boolean
        get() = tappingTermMs in KumiLimits.KEY_MIN_TAPPING_TERM_MS..KumiLimits.KEY_MAX_TAPPING_TERM_MS &&
            oneShotTimeoutMs in KumiLimits.KEY_MIN_ONE_SHOT_MS..KumiLimits.KEY_MAX_ONE_SHOT_MS &&
            comboTermMs in KumiLimits.KEY_MIN_COMBO_TERM_MS..KumiLimits.KEY_MAX_COMBO_TERM_MS

    fun toJson(): JSONObject = JSONObject()
        .put("tapping_term_ms", tappingTermMs)
        .put("one_shot_timeout_ms", oneShotTimeoutMs)
        .put("combo_term_ms", comboTermMs)

    companion object {
        fun from(j: JSONObject) = KeyConfig(
            j.getInt("tapping_term_ms"),
            j.getInt("one_shot_timeout_ms"),
            j.optInt("combo_term_ms", 50),
        )
    }
}

sealed interface KeyAction {
    data object Transparent : KeyAction
    data object Disabled : KeyAction
    data class Key(val usage: Int) : KeyAction
    data class Modifiers(val mask: Int) : KeyAction
    data class KeyWithModifiers(val usage: Int, val modifiers: Int) : KeyAction
    data class Consumer(val usage: Int) : KeyAction
    data class LayerHold(val layer: Int) : KeyAction
    data class LayerToggle(val layer: Int) : KeyAction
    data class TapHoldMod(val tap: Int, val holdMods: Int) : KeyAction
    data class TapHoldLayer(val tap: Int, val holdLayer: Int) : KeyAction
    data class OneShotMod(val mods: Int) : KeyAction
    data class Macro(val slot: Int) : KeyAction
    data class CaseControl(val hotkey: CaseHotkey) : KeyAction
    data object CapsWord : KeyAction
    data class Mouse(val action: MouseAction) : KeyAction
    data class Unsupported(val raw: String) : KeyAction

    val valid: Boolean
        get() = when (this) {
            Transparent, Disabled, CapsWord, is CaseControl, is Mouse -> true
            is Modifiers -> mask in 0..0xFF
            is Key -> usage in KumiLimits.KEY_MIN_USAGE..KumiLimits.KEY_MAX_USAGE
            is KeyWithModifiers -> usage in KumiLimits.KEY_MIN_USAGE..KumiLimits.KEY_MAX_USAGE && modifiers in 0..0xFF
            is Consumer -> usage in 1..KumiLimits.CONSUMER_MAX_USAGE
            is LayerHold -> layer in 1 until KeymapEntry.LAYERS
            is LayerToggle -> layer in 1 until KeymapEntry.LAYERS
            is TapHoldMod -> tap in KumiLimits.KEY_MIN_USAGE..KumiLimits.KEY_MAX_USAGE && holdMods in 1..0xFF
            is TapHoldLayer -> tap in KumiLimits.KEY_MIN_USAGE..KumiLimits.KEY_MAX_USAGE && holdLayer in 1 until KeymapEntry.LAYERS
            is OneShotMod -> mods in 1..0xFF
            is Macro -> slot in 0 until KumiLimits.MACRO_SLOTS
            is Unsupported -> false
        }

    fun toJson(): Any = when (this) {
        Transparent -> "Transparent"
        Disabled -> "Disabled"
        is Key -> JSONObject().put("Key", usage)
        is Modifiers -> JSONObject().put("Modifiers", mask)
        is KeyWithModifiers -> JSONObject().put("KeyWithModifiers", JSONObject().put("usage", usage).put("modifiers", modifiers))
        is Consumer -> JSONObject().put("Consumer", usage)
        is LayerHold -> JSONObject().put("LayerHold", layer)
        is LayerToggle -> JSONObject().put("LayerToggle", layer)
        is TapHoldMod -> JSONObject().put("TapHoldMod", JSONObject().put("tap", tap).put("hold_mods", holdMods))
        is TapHoldLayer -> JSONObject().put("TapHoldLayer", JSONObject().put("tap", tap).put("hold_layer", holdLayer))
        is OneShotMod -> JSONObject().put("OneShotMod", mods)
        is Macro -> JSONObject().put("Macro", slot)
        is CaseControl -> JSONObject().put("CaseControl", hotkey.name)
        CapsWord -> "CapsWord"
        is Mouse -> JSONObject().put("Mouse", action.name)
        is Unsupported -> JSONTokener(raw).nextValue()
    }

    companion object {
        fun from(value: Any): KeyAction {
            val raw = if (value is String) JSONObject.quote(value) else value.toString()
            if (value is String) {
                return when (value) {
                    "Transparent" -> Transparent
                    "Disabled" -> Disabled
                    "CapsWord" -> CapsWord
                    else -> Unsupported(raw)
                }
            }
            val j = value as? JSONObject ?: return Unsupported(raw)
            val name = j.keys().asSequence().singleOrNull() ?: return Unsupported(raw)
            return runCatching {
                when (name) {
                    "Key" -> Key(j.getInt(name))
                    "Modifiers" -> Modifiers(j.getInt(name))
                    "KeyWithModifiers" -> j.getJSONObject(name).let { KeyWithModifiers(it.getInt("usage"), it.getInt("modifiers")) }
                    "Consumer" -> Consumer(j.getInt(name))
                    "LayerHold" -> LayerHold(j.getInt(name))
                    "LayerToggle" -> LayerToggle(j.getInt(name))
                    "TapHoldMod" -> j.getJSONObject(name).let { TapHoldMod(it.getInt("tap"), it.getInt("hold_mods")) }
                    "TapHoldLayer" -> j.getJSONObject(name).let { TapHoldLayer(it.getInt("tap"), it.getInt("hold_layer")) }
                    "OneShotMod" -> OneShotMod(j.getInt(name))
                    "Macro" -> Macro(j.getInt(name))
                    "CaseControl" -> CaseControl(enumOf(j.getString(name)))
                    "Mouse" -> Mouse(enumOf(j.getString(name)))
                    else -> Unsupported(raw)
                }
            }.getOrElse { Unsupported(raw) }
        }
    }
}

data class KeymapEntry(val layer: Int, val source: Int, val action: KeyAction) {
    fun toJson(): JSONObject = JSONObject().put("layer", layer).put("source", source).put("action", action.toJson())

    companion object {
        const val LAYERS = 4

        fun from(j: JSONObject) = KeymapEntry(j.getInt("layer"), j.getInt("source"), KeyAction.from(j.get("action")))
    }
}

data class KeymapChunk(val entries: List<KeymapEntry>, val more: Boolean) {
    companion object {
        fun from(j: JSONObject) = KeymapChunk(j.getJSONArray("entries").objects().map(KeymapEntry::from), j.getBoolean("more"))
    }
}

data class KeyEvent(val source: Int, val pressed: Boolean, val layer: Int, val action: KeyAction) {
    companion object {
        fun from(j: JSONObject) = KeyEvent(j.getInt("source"), j.getBoolean("pressed"), j.getInt("layer"), KeyAction.from(j.get("action")))
    }
}

data class KeyboardStatus(
    val linkUp: Boolean,
    val frames: Long,
    val badFrames: Long,
    val controllerCommands: Long,
    val activeLayer: Int,
    val capsWord: Boolean = false,
    val hostLeds: Int = 0,
) {
    companion object {
        fun from(j: JSONObject) = KeyboardStatus(
            j.getBoolean("link_up"),
            j.getLong("frames"),
            j.getLong("bad_frames"),
            j.getLong("controller_commands"),
            j.getInt("active_layer"),
            j.optBoolean("caps_word", false),
            j.optInt("host_leds", 0),
        )
    }
}

enum class Slot { A, B, Unknown }

enum class SlotState { Empty, Trial, Confirmed, Invalid, Unknown }

data class SlotInfo(val slot: Slot, val state: SlotState, val version: KumiVersion?, val size: Long) {
    companion object {
        fun from(j: JSONObject) =
            SlotInfo(enumOr(j.optString("slot"), Slot.Unknown), enumOr(j.optString("state"), SlotState.Unknown), j.optObject("version")?.let(KumiVersion::from), j.getLong("size"))
    }
}

data class SlotTable(val booted: Slot?, val slots: List<SlotInfo>) {
    val running: SlotInfo? get() = slots.firstOrNull { it.slot == booted }

    companion object {
        fun from(j: JSONObject) =
            SlotTable(j.optStringOrNull("booted")?.let { enumOr(it, Slot.Unknown) }, j.getJSONArray("slots").objects().map(SlotInfo::from))
    }
}

enum class RebootTarget { Normal, OtherSlot, BootRom }

enum class SettingsScope { All, Audio, Keymap }

enum class CaseError { Unsupported, InvalidArgument, Busy, NotUnlocked, HardwareFault, StorageFull, VerifyFailed, WrongState, Unknown }

enum class LogLevel { Trace, Debug, Info, Warn, Error, Unknown }

data class LogLine(val uptimeMs: Long, val level: LogLevel, val module: String, val text: String) {
    companion object {
        fun from(j: JSONObject) = LogLine(j.getLong("uptime_ms"), enumOr(j.optString("level"), LogLevel.Unknown), j.getString("module"), j.getString("text"))
    }
}

object KumiLimits {
    const val PEEK_MAX_WORDS = 32
    const val BYTE_BLOCK = 64
    const val I2C_MAX_ADDRESS = 0x7F
    const val I2C_BUSES = 2
    const val AUDIO_MIN_LEVEL_CENTI_DBFS = -9_600
    const val AUDIO_MIN_TONE_HZ = 20
    const val AUDIO_MAX_TONE_HZ = 20_000
    const val UNLOCK_MAX_SECONDS = 600
    const val UNLOCK_SECONDS = 300
    const val MIC_MAX_GAIN_CENTI_DB = 3_000
    const val OUTPUT_MAX_BALANCE_CENTI_DB = 600
    const val OUTPUT_MIN_VOLUME_LIMIT_CENTI_DB = -6_000
    const val KEY_MIN_USAGE = 0x04
    const val KEY_MAX_USAGE = 0xDF
    const val COMBO_MAX_USAGE = 0xE7
    const val KEYMAP_MAX = 96
    const val CONSUMER_MAX_USAGE = 0x3FF
    const val EQ_RESPONSE_MAX_POINTS = 2_048
    const val MACRO_SLOTS = 8
    const val MACRO_MAX_STEPS = 16
    const val COMBO_SLOTS = 8
    const val KEY_MIN_TAPPING_TERM_MS = 80
    const val KEY_MAX_TAPPING_TERM_MS = 1_000
    const val KEY_MIN_ONE_SHOT_MS = 250
    const val KEY_MAX_ONE_SHOT_MS = 10_000
    const val KEY_MIN_COMBO_TERM_MS = 15
    const val KEY_MAX_COMBO_TERM_MS = 200
    const val METER_FLOOR_CENTI_DB = -9_600
    const val TEST_TONE_MIN_HZ = 100
    const val TEST_TONE_MAX_HZ = 12_000
    const val TEST_TONE_MIN_MS = 100
    const val TEST_TONE_MAX_MS = 5_000
}

data class WordBlock(val address: Long, val words: List<Long>) {
    companion object {
        fun from(j: JSONObject) = WordBlock(j.getLong("address"), j.getJSONArray("words").let { a -> (0 until a.length()).map(a::getLong) })
    }
}

data class I2cReply(val acked: Boolean, val data: List<Int>) {
    companion object {
        fun from(j: JSONObject) = I2cReply(j.getBoolean("acked"), j.getJSONArray("data").ints())
    }
}

enum class AudioChannel { Left, Right, Both }

enum class AudioBenchState { Idle, Starting, Running, Failed, Unknown }

enum class AudioPhase { Calibrate, Setup, Clock, ResetLow, ResetHigh, Dac, Dma, Unmute, Stop, Unknown }

data class AudioStart(val rateHz: Int, val channel: AudioChannel, val levelCentiDbfs: Int, val toneHz: Int) {
    val valid: Boolean
        get() = levelCentiDbfs in KumiLimits.AUDIO_MIN_LEVEL_CENTI_DBFS..0 &&
            (toneHz == 0 || toneHz in KumiLimits.AUDIO_MIN_TONE_HZ..KumiLimits.AUDIO_MAX_TONE_HZ && 2L * toneHz < rateHz)

    fun toJson(): JSONObject = JSONObject().put("rate_hz", rateHz).put("channel", channel.name).put("level_centi_dbfs", levelCentiDbfs)
        .put("tone_hz", toneHz)

    companion object {
        fun from(j: JSONObject) = AudioStart(j.getInt("rate_hz"), enumOf(j.getString("channel")), j.getInt("level_centi_dbfs"), j.getInt("tone_hz"))
    }
}

sealed interface AudioBenchRequest {
    data object Calibrate : AudioBenchRequest
    data class Start(val start: AudioStart) : AudioBenchRequest
    data object Stop : AudioBenchRequest
    data object Status : AudioBenchRequest

    fun toJson(): Any = when (this) {
        Calibrate -> "Calibrate"
        is Start -> JSONObject().put("Start", start.toJson())
        Stop -> "Stop"
        Status -> "Status"
    }
}

data class AudioCalibration(
    val decodedCpuHz: Long?,
    val frameClockHz: Long?,
    val timersQuiesced: Boolean,
    val fault: String?,
    val trims: List<Int>,
    val windows: List<Int>,
    val counts: Int,
) {
    companion object {
        fun from(j: JSONObject) = AudioCalibration(
            j.optLongOrNull("decoded_cpu_hz"), j.optLongOrNull("frame_clock_hz"), j.optBoolean("timers_quiesced"),
            j.opt("fault").takeUnless { it == null || it == JSONObject.NULL }?.toString(),
            j.optJSONArray("trims")?.ints() ?: emptyList(), j.optJSONArray("windows")?.ints() ?: emptyList(), j.optJSONArray("counts")?.length() ?: 0,
        )
    }
}

data class AudioStream(val start: AudioStart, val ringAddress: Long, val ringBytes: Long, val periods: Long, val startedUs: Long?, val dacs: Int) {
    companion object {
        fun from(j: JSONObject) = AudioStream(
            AudioStart.from(j.getJSONObject("start")), j.getLong("ring_address"), j.getLong("ring_bytes"), j.getLong("periods"),
            j.optLongOrNull("started_us"), j.getInt("dacs"),
        )
    }
}

data class AudioFailure(val phase: AudioPhase, val fault: String) {
    companion object {
        fun from(j: JSONObject) = AudioFailure(enumOr(j.optString("phase"), AudioPhase.Unknown), j.opt("fault")?.toString() ?: "")
    }
}

data class AudioBench(val state: AudioBenchState, val cpuHz: Long?, val calibration: AudioCalibration?, val stream: AudioStream?, val failure: AudioFailure?) {
    companion object {
        fun from(j: JSONObject) = AudioBench(
            enumOr(j.optString("state"), AudioBenchState.Unknown), j.optLongOrNull("cpu_hz"), j.optObject("calibration")?.let(AudioCalibration::from),
            j.optObject("stream")?.let(AudioStream::from), j.optObject("failure")?.let(AudioFailure::from),
        )
    }
}

data class PackageInfo(val version: KumiVersion, val generation: Long, val build: String, val stage0: Int, val image: Int, val sha256: String) {
    companion object {
        fun from(j: JSONObject) = PackageInfo(
            KumiVersion.parse(j.getString("version")) ?: throw IllegalArgumentException("version"),
            j.getLong("generation"), j.getString("build"), j.getInt("stage0"), j.getInt("image"), j.getString("sha256"),
        )
    }
}

data class BackupInfo(val size: Int, val sha256: String, val stock: String) {
    companion object {
        fun from(j: JSONObject) = BackupInfo(j.getInt("size"), j.getString("sha256"), j.getString("stock"))
    }
}

data class ServiceIdentity(val chip: Long, val chipMatches: Boolean, val jedec: String, val protect: Int, val capacity: Long?) {
    companion object {
        fun from(j: JSONObject) = ServiceIdentity(
            j.getLong("chip"), j.getBoolean("chipMatches"), j.getString("jedec"), j.getInt("protect"),
            if (j.has("capacity") && !j.isNull("capacity")) j.getLong("capacity") else null,
        )
    }
}

enum class CrashReason { Panic, Trap, Unknown }

data class CrashReport(
    val bootCount: Long,
    val reason: CrashReason,
    val stage: Int,
    val mcause: Long,
    val mepc: Long,
    val mtval: Long,
    val ra: Long,
    val sp: Long,
    val uptimeMs: Long,
    val message: String,
) {
    companion object {
        fun from(j: JSONObject) = CrashReport(
            j.getLong("boot_count"), enumOr(j.optString("reason"), CrashReason.Unknown), j.getInt("stage"), j.getLong("mcause"), j.getLong("mepc"),
            j.getLong("mtval"), j.getLong("ra"), j.getLong("sp"), j.getLong("uptime_ms"), j.optString("message"),
        )
    }
}

data class Diagnostics(
    val bootCount: Long,
    val safeMode: Boolean,
    val stage: Int,
    val uptimeMs: Long,
    val droppedEvents: Long,
    val watchdogArmed: Boolean,
    val lastCrash: CrashReport?,
) {
    companion object {
        fun from(j: JSONObject) = Diagnostics(
            j.getLong("boot_count"), j.getBoolean("safe_mode"), j.getInt("stage"), j.getLong("uptime_ms"), j.getLong("dropped_events"),
            j.getBoolean("watchdog_armed"), j.optObject("last_crash")?.let(CrashReport::from),
        )
    }
}

object BootStages {
    val NAMES: Map<Int, String> = linkedMapOf(
        0x000 to "unknown", 0x001 to "reset",
        0x011 to "stage0 journal", 0x012 to "stage0 verified", 0x013 to "stage0 loaded", 0x014 to "stage0 jump", 0x015 to "stage0 boot ROM",
        0x016 to "stage0 disarmed",
        0x100 to "firmware entry", 0x101 to "firmware clocks", 0x104 to "firmware UART", 0x105 to "firmware USB", 0x107 to "firmware storage",
        0x1F0 to "firmware running", 0x1F1 to "firmware healthy", 0x200 to "firmware safe mode",
    )

    fun name(code: Int): String = NAMES[code] ?: "stage 0x%03x".format(code)
}

data class EventMask(val keys: Boolean, val log: Boolean, val audio: Boolean, val uart: Boolean) {
    fun toJson(): JSONObject = JSONObject().put("keys", keys).put("log", log).put("audio", audio).put("uart", uart)

    companion object {
        val NAMES = listOf("keys", "log", "audio", "uart")
        val APP = EventMask(keys = true, log = true, audio = true, uart = false)

        fun from(j: JSONObject) = EventMask(j.optBoolean("keys"), j.optBoolean("log"), j.optBoolean("audio"), j.optBoolean("uart"))
    }
}

sealed interface TopicMessage {
    data class Audio(val status: AudioStatus) : TopicMessage
    data class Key(val event: KeyEvent) : TopicMessage
    data class Log(val line: LogLine) : TopicMessage
    data class Uart(val bytes: List<Int>) : TopicMessage

    companion object {
        const val AUDIO = "audio/status/changed"
        const val KEYS = "keys/events"
        const val LOG = "log"
        const val UART = "dev/uart/mirror"
        val PATHS = listOf(AUDIO, KEYS, LOG, UART)

        fun from(j: JSONObject): TopicMessage? {
            val data = j.opt("data") as? JSONObject ?: return null
            return when (j.getString("path")) {
                AUDIO -> Audio(AudioStatus.from(data))
                KEYS -> Key(KeyEvent.from(data))
                LOG -> Log(LogLine.from(data))
                UART -> Uart(data.getJSONArray("bytes").ints())
                else -> null
            }
        }
    }
}

data class BridgeFailure(
    val kind: String,
    val detail: String? = null,
    val step: Int? = null,
    val resume: Int? = null,
    val label: String? = null,
    val bootsBefore: String? = null,
    val bootsAfter: String? = null,
) {
    companion object {
        fun from(j: JSONObject): BridgeFailure {
            val boots = j.optJSONArray("boots")
            return BridgeFailure(
                kind = j.optString("kind", "Unknown"),
                detail = j.optStringOrNull("detail"),
                step = j.optIntOrNull("step"),
                resume = j.optIntOrNull("resume"),
                label = j.optStringOrNull("label"),
                bootsBefore = boots?.optString(0),
                bootsAfter = boots?.optString(1),
            )
        }
    }
}

class BridgeException(val failure: BridgeFailure) : Exception(failure.detail?.let { "${failure.kind}: $it" } ?: failure.kind)

class CaseErrorException(val error: CaseError, val raw: String = error.name) : Exception(raw)

class BridgeReply(val value: Any?, val topics: List<TopicMessage>)

object Replies {
    fun parse(text: String): BridgeReply {
        val root = JSONObject(text)
        root.optJSONObject("err")?.let { throw BridgeException(BridgeFailure.from(it)) }
        if (!root.has("ok")) throw BridgeException(BridgeFailure("Malformed", text.take(200)))
        val topics = root.optJSONArray("topics")?.objects()?.mapNotNull { runCatching { TopicMessage.from(it) }.getOrNull() } ?: emptyList()
        val value = root.get("ok").takeUnless { it == JSONObject.NULL }
        return BridgeReply(value, topics)
    }

    fun caseResult(value: Any?): Any? {
        val j = value as? JSONObject ?: throw BridgeException(BridgeFailure("Malformed", value.toString().take(200)))
        if (j.has("Err")) {
            val raw = j.opt("Err").toString()
            throw CaseErrorException(enumOr(raw, CaseError.Unknown), raw)
        }
        return j.opt("Ok").takeUnless { it == JSONObject.NULL }
    }
}

inline fun <reified E : Enum<E>> enumOf(name: String): E =
    enumValues<E>().firstOrNull { it.name == name } ?: throw BridgeException(BridgeFailure("Malformed", "Unknown ${E::class.simpleName} $name"))

inline fun <reified E : Enum<E>> enumOr(name: String?, fallback: E): E = enumValues<E>().firstOrNull { it.name == name } ?: fallback

fun JSONArray.ints(): List<Int> = (0 until length()).map(::getInt)

fun JSONArray.objects(): List<JSONObject> = (0 until length()).map(::getJSONObject)

fun JSONObject.optObject(name: String): JSONObject? = if (isNull(name)) null else optJSONObject(name)

fun JSONObject.optIntOrNull(name: String): Int? = if (!has(name) || isNull(name)) null else getInt(name)

fun JSONObject.optLongOrNull(name: String): Long? = if (!has(name) || isNull(name)) null else getLong(name)

fun JSONObject.optStringOrNull(name: String): String? = if (!has(name) || isNull(name)) null else getString(name)
