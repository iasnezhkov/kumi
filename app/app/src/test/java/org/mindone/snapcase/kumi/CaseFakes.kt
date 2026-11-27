// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import java.security.MessageDigest
import kotlinx.coroutines.CompletableDeferred
import org.json.JSONObject
import org.mindone.snapcase.transport.CaseChannel
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.CasePort

class FakeChannel : CaseChannel {
    var closed = false
    override fun send(data: ByteArray) = !closed
    override fun receive(): ByteArray? = null
    override fun close() {
        closed = true
    }
}

class FakePort(var kind: CaseKind, override val simulated: Boolean = false, override val simulator: Long? = null) : CasePort {
    var entries = 0
    var kumiOpens = 0
    var serviceOpens = 0
    var kumiOpenGate: CompletableDeferred<Unit>? = null
    var kumiOpenError: (() -> Exception)? = null
    var entryLands: CaseKind? = CaseKind.SERVICE
    var key: String? = "fake:1"
    val channels = mutableListOf<FakeChannel>()

    override fun kind(): CaseKind = kind

    override fun deviceKey(): String? = if (kind == CaseKind.NONE) null else key

    override suspend fun sendServiceEntry() {
        entries++
        entryLands?.let { kind = it }
    }

    override suspend fun openService(): CaseChannel {
        serviceOpens++
        check(kind == CaseKind.SERVICE) { "not in service mode" }
        return FakeChannel().also { channels += it }
    }

    override suspend fun openKumi(): CaseChannel {
        kumiOpens++
        kumiOpenGate?.await()
        kumiOpenError?.let { throw it() }
        check(kind == CaseKind.KUMI) { "no Kumi case" }
        return FakeChannel().also { channels += it }
    }
}

object Contract {
    val json: JSONObject by lazy {
        JSONObject(File(System.getProperty("kumi.contract") ?: "../../kumi/crates/kumi-jni/tests/fixtures/contract.json").readText())
    }

    fun reply(name: String): String = json.getJSONObject("bridge_replies").getJSONObject(name).toString()
}

fun sha256(data: ByteArray): String = MessageDigest.getInstance("SHA-256").digest(data).joinToString("") { "%02x".format(it) }

open class CaseBridge(private val port: FakePort) : FakeBridge() {
    val calls = mutableListOf<String>()
    var identity = """{"chip":1224819840,"chipMatches":true,"jedec":"c86516","protect":0,"capacity":4194304}"""
    var stock = "Head"
    var installReplies = ArrayDeque<String>()
    var restoreReplies = ArrayDeque<String>()
    var rebootLands: CaseKind? = CaseKind.KUMI
    val installs = mutableListOf<Pair<String, Int>>()
    val simInstalls = mutableListOf<Long>()
    var gates = Contract.reply("install_gates_complete")
    var gateQueries = 0
    val restores = mutableListOf<Triple<String, Boolean, Int>>()
    val handlers = mutableMapOf<String, (String) -> String>()
    val subscriptions = mutableListOf<EventMask>()

    override fun installGates(): String {
        gateQueries++
        return gates
    }

    override fun identify(link: KumiLink): String {
        calls += "identify"
        return """{"ok":$identity}"""
    }

    override fun backup(link: KumiLink, path: String, progress: KumiProgress?): String {
        calls += "backup"
        val data = ByteArray(4096) { (it * 7).toByte() } + path.toByteArray()
        File(path).writeBytes(data)
        progress?.progress(1, 2, "backup")
        return """{"ok":{"size":${data.size},"sha256":"${sha256(data)}","stock":"$stock"}}"""
    }

    override fun install(link: KumiLink, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?): String {
        calls += "install"
        installs += backup to from
        return installReplies.removeFirstOrNull() ?: """{"ok":{"steps":10,"boots":"Kumi"}}"""
    }

    override fun simInstall(simulator: Long, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?): String {
        simInstalls += simulator
        return install(FakeChannel(), backup, backupSha, pkg, key, from, progress)
    }

    override fun restoreStock(link: KumiLink, backup: String, backupSha: String, exact: Boolean, from: Int, progress: KumiProgress?): String {
        calls += "restore"
        restores += Triple(backup, exact, from)
        return restoreReplies.removeFirstOrNull() ?: """{"ok":{"steps":4,"boots":"Stock"}}"""
    }

    override fun rebootFromService(link: KumiLink): String {
        calls += "rebootFromService"
        rebootLands?.let { port.kind = it }
        return """{"ok":null}"""
    }

    override fun call(link: KumiLink, path: String, request: String): String {
        calls += path
        if (path == "events/subscribe") subscriptions += EventMask.from(JSONObject(request))
        handlers[path]?.let { return it(request) }
        return when (path) {
            "device/info" -> """{"ok":$DEVICE}"""
            "system/slots" -> """{"ok":$SLOTS}"""
            "audio/output/get" -> """{"ok":$OUTPUT}"""
            "audio/dsp/get" -> """{"ok":$DSP}"""
            "audio/dac-extras/get" -> """{"ok":$DAC_EXTRAS}"""
            "audio/meters" -> """{"ok":$METERS}"""
            "audio/spectrum" -> """{"ok":$SPECTRUM}"""
            "audio/dsp-extras/get" -> """{"ok":$DSP_EXTRAS}"""
            "audio/eq/base/get" -> """{"ok":$EQ_BASE}"""
            "audio/eq/tone/get" -> """{"ok":$TONE_EQ}"""
            "audio/impedance" -> """{"ok":{"Ok":$IMPEDANCE}}"""
            "audio/mic/get" -> """{"ok":{"Ok":{"gain_centi_db":1200}}}"""
            "audio/eq/get" -> if (request == "0") """{"ok":{"Ok":$EQ}}""" else """{"ok":{"Ok":null}}"""
            "system/diagnostics" -> """{"ok":{"Ok":{"boot_count":3,"safe_mode":true,"stage":512,"uptime_ms":10,"dropped_events":0,"watchdog_armed":true,"last_crash":null}}}"""
            "system/telemetry" -> """{"ok":{"Ok":$TELEMETRY}}"""
            "system/presence" -> """{"ok":$PRESENCE}"""
            "events/subscribe" -> """{"ok":{"Ok":{"keys":false,"log":true,"audio":false,"uart":false}}}"""
            "keys/map/get" -> if (JSONObject(request).getInt("layer") == 0) """{"ok":{"Ok":$KEYMAP}}""" else """{"ok":{"Ok":{"entries":[],"more":false}}}"""
            "keys/config/get" -> """{"ok":{"tapping_term_ms":200,"one_shot_timeout_ms":3000,"combo_term_ms":50}}"""
            "keys/media/get" -> """{"ok":$MEDIA_GESTURES}"""
            "keys/macro/get" -> """{"ok":{"Ok":null}}"""
            "keys/combo/get" -> """{"ok":{"Ok":null}}"""
            "audio/status" -> """{"ok":$AUDIO}"""
            "keys/status" -> """{"ok":{"link_up":true,"frames":1,"bad_frames":0,"controller_commands":0,"active_layer":0,"caps_word":false,"host_leds":0}}"""
            else -> """{"ok":{"Ok":null}}"""
        }
    }

    companion object {
        const val DEVICE = """{"protocol":1,"firmware":{"major":0,"minor":1,"patch":0},"build":"t","chip_id":1224819840,"flash_jedec":[200,101,22],
            "serial":[83,78],"running_from_ram":false,"capabilities":{"audio_out":true,"audio_in":true,"dsd":true,"parametric_eq":true,"keymap":true,
            "layers":true,"battery":false,"persistent_settings":true,"firmware_update":true,"developer":true}}"""
        const val SLOTS = """{"booted":"A","slots":[{"slot":"A","state":"Confirmed","version":{"major":0,"minor":1,"patch":0},"size":1},
            {"slot":"B","state":"Empty","version":null,"size":0}]}"""
        const val OUTPUT = """{"filter":"FastLowLatency","volume_limit_centi_db":-300,"balance_centi_db":0,"dsd_direct":false}"""
        const val DSP = """{"crossfeed":"Off","limiter":false,"mono":false,"invert_phase":false,"loudness":false,"swap_channels":false}"""
        const val DAC_EXTRAS = """{"high_pass_filter":true,"de_emphasis":false,"line_out":false,"class_h":"AdaptiveHv","auto_uac_on_jack":true}"""
        const val METERS = """{"active":false,"peak_left_centi_db":-9600,"peak_right_centi_db":-9600,"rms_left_centi_db":-9600,"rms_right_centi_db":-9600,"clipped_samples":0,"limiter_hits":0}"""
        const val SPECTRUM = """{"active":false,"bands_db_x2":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"true_peak_left_centi_db":-9600,"true_peak_right_centi_db":-9600,"lufs_momentary_centi":-7000,"lufs_short_term_centi":-7000,"auto_level_gain_centi_db":0}"""
        const val DSP_EXTRAS = """{"auto_level":false,"auto_level_target_lufs":-16,"subsonic_hz":0,"mono_bass_hz":0}"""
        const val EQ_BASE = """{"enabled":false,"profile":{"preamp_centi_db":0,"bands":[]}}"""
        const val TONE_EQ = """{"enabled":false,"sub_bass_centi_db":0,"bass_centi_db":0,"mid_centi_db":0,"presence_centi_db":0,"treble_centi_db":0}"""
        const val MEDIA_GESTURES = """{"enabled":false,"window_ms":320,"idle_guard_ms":1200,"double_shift_usage":182,"double_backspace_usage":181,"double_space_usage":205}"""
        const val IMPEDANCE = """{"left_ohms":49,"right_ohms":49,"left_ac_ohms":53,"right_ac_ohms":53,"left_status":13,"right_status":13,"hp_status":2,"load_stat":0}"""
        const val TELEMETRY = """{"cpu_mhz":90,"dcdc_active":true,"dvdd_mv":1400,"analog_rails_on":false,"audio_pll_enabled":false,"wfi_percent_tenths":998,"wfi_wakeups":50,"jack_adc":202,"uptime_ms":1000}"""
        const val PRESENCE = """{"tier":1,"remaining_ms":120000,"passthrough":false}"""
        const val EQ = """{"preamp_centi_db":-300,"bands":[{"kind":"Peak","frequency_hz":1000,"q_milli":1000,"gain_centi_db":300}]}"""
        const val KEYMAP = """{"entries":[{"layer":0,"source":57,"action":{"Key":41}}],"more":false}"""
        const val AUDIO = """{"jack":"Headphones","usb_audio_present":true,"stream":null,"host_volume_centi_db":[0,0],"host_muted":false,
            "eq_active":0,"counters":{"underruns":0,"overruns":0,"feedback_corrections":0,"dac_faults":0}}"""
    }
}
