// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import org.json.JSONObject

object Requests {
    fun body(request: Any?): String = when (request) {
        null -> "null"
        is JSONObject -> request.toString()
        is String -> JSONObject.quote(request)
        else -> request.toString()
    }

    fun eqSlot(slot: Int): Int = slot

    fun eqUpdate(slot: Int, profile: EqProfile): JSONObject = JSONObject().put("slot", slot).put("profile", profile.toJson())

    fun eqSelection(slot: Int?): Int? = slot

    fun macroUpdate(slot: Int, program: MacroProgram): JSONObject = JSONObject().put("slot", slot).put("program", program.toJson())

    fun comboUpdate(slot: Int, combo: KeyCombo?): JSONObject = JSONObject().put("slot", slot).put("combo", combo?.toJson() ?: JSONObject.NULL)

    fun keymapPage(layer: Int, start: Int): JSONObject = JSONObject().put("layer", layer).put("start", start)

    fun peek(address: Long, words: Int): JSONObject = JSONObject().put("address", address).put("words", words)

    fun unlock(seconds: Int): JSONObject = JSONObject().put("seconds", seconds)

    fun poke(address: Long, value: Long, mask: Long): JSONObject = JSONObject().put("address", address).put("value", value).put("mask", mask)

    fun i2c(bus: Int, address: Int, write: List<Int>, read: Int): JSONObject =
        JSONObject().put("bus", bus).put("address", address).put("write", org.json.JSONArray(write)).put("read", read)
}

class KumiClient(private val bridge: KumiBridge, private val link: KumiLink, private val onTopic: (TopicMessage) -> Unit = {}) {

    private fun call(path: String, request: Any? = null): Any? {
        val reply = Replies.parse(bridge.call(link, path, Requests.body(request)))
        reply.topics.forEach(onTopic)
        return reply.value
    }

    private fun obj(value: Any?): JSONObject = value as? JSONObject ?: throw BridgeException(BridgeFailure("Malformed", value.toString()))

    private fun ack(path: String, request: Any? = null) {
        Replies.caseResult(call(path, request))
    }

    fun deviceInfo(): DeviceInfo = DeviceInfo.from(obj(call("device/info")))

    fun diagnostics(): Diagnostics = Diagnostics.from(obj(Replies.caseResult(call("system/diagnostics"))))

    fun telemetry(): SystemTelemetry = SystemTelemetry.from(obj(Replies.caseResult(call("system/telemetry"))))

    fun subscribe(events: EventMask): EventMask = EventMask.from(obj(Replies.caseResult(call("events/subscribe", events.toJson()))))

    fun audioStatus(): AudioStatus = AudioStatus.from(obj(call("audio/status")))

    fun output(): OutputSettings = OutputSettings.from(obj(call("audio/output/get")))

    fun setOutput(value: OutputSettings) = ack("audio/output/set", value.toJson())

    fun dsp(): DspSettings = DspSettings.from(obj(call("audio/dsp/get")))

    fun setDsp(value: DspSettings) = ack("audio/dsp/set", value.toJson())

    fun dacExtras(): DacExtras = DacExtras.from(obj(call("audio/dac-extras/get")))

    fun setDacExtras(value: DacExtras) = ack("audio/dac-extras/set", value.toJson())

    fun measureImpedance(): ImpedanceReport = ImpedanceReport.from(obj(Replies.caseResult(call("audio/impedance"))))

    fun audioMeters(): AudioMeters = AudioMeters.from(obj(call("audio/meters")))

    fun playTestTone(request: TestToneRequest) = ack("audio/test-tone", request.toJson())

    fun eq(slot: Int): EqProfile? = Replies.caseResult(call("audio/eq/get", Requests.eqSlot(slot)))?.let { EqProfile.from(obj(it)) }

    fun setEq(slot: Int, profile: EqProfile) = ack("audio/eq/set", Requests.eqUpdate(slot, profile))

    fun selectEq(slot: Int?) = ack("audio/eq/select", Requests.eqSelection(slot))

    fun mic(): MicSettings = MicSettings.from(obj(Replies.caseResult(call("audio/mic/get"))))

    fun setMic(value: MicSettings) = ack("audio/mic/set", value.toJson())

    fun keymap(layer: Int): List<KeymapEntry> {
        val out = mutableListOf<KeymapEntry>()
        var start = 0
        while (start <= 0xFF) {
            val chunk = KeymapChunk.from(obj(Replies.caseResult(call("keys/map/get", Requests.keymapPage(layer, start)))))
            out += chunk.entries
            if (!chunk.more || chunk.entries.isEmpty()) break
            start = chunk.entries.last().source + 1
        }
        return out
    }

    fun setKey(entry: KeymapEntry) = ack("keys/map/set", entry.toJson())

    fun clearKeymap() = ack("keys/map/clear")

    fun macroSlot(slot: Int): MacroProgram? = Replies.caseResult(call("keys/macro/get", slot))?.let { MacroProgram.from(obj(it)) }

    fun setMacro(slot: Int, program: MacroProgram) = ack("keys/macro/set", Requests.macroUpdate(slot, program))

    fun comboSlot(slot: Int): KeyCombo? = Replies.caseResult(call("keys/combo/get", slot))?.let { KeyCombo.from(obj(it)) }

    fun setCombo(slot: Int, combo: KeyCombo?) = ack("keys/combo/set", Requests.comboUpdate(slot, combo))

    fun keyConfig(): KeyConfig = KeyConfig.from(obj(call("keys/config/get")))

    fun setKeyConfig(value: KeyConfig) = ack("keys/config/set", value.toJson())

    fun keyboardStatus(): KeyboardStatus = KeyboardStatus.from(obj(call("keys/status")))

    fun resetSettings(scope: SettingsScope) = ack("settings/reset", scope.name)

    fun slots(): SlotTable = SlotTable.from(obj(call("system/slots")))

    fun reboot(target: RebootTarget) = ack("system/reboot", target.name)

    fun confirmBoot() = ack("system/confirm")

    fun devUnlock(seconds: Int) = ack("dev/unlock", Requests.unlock(seconds))

    fun devPeek(address: Long, words: Int): WordBlock = WordBlock.from(obj(Replies.caseResult(call("dev/peek", Requests.peek(address, words)))))

    fun devPoke(address: Long, value: Long, mask: Long): Long =
        (Replies.caseResult(call("dev/poke", Requests.poke(address, value, mask))) as? Number)?.toLong()
            ?: throw BridgeException(BridgeFailure("Malformed", "dev/poke"))

    fun devI2c(bus: Int, address: Int, write: List<Int>, read: Int): I2cReply =
        I2cReply.from(obj(Replies.caseResult(call("dev/i2c", Requests.i2c(bus, address, write, read)))))

    fun audioBench(request: AudioBenchRequest): AudioBench = AudioBench.from(obj(Replies.caseResult(call("dev/audio", request.toJson()))))

    fun pollTopics(maxFrames: Int = 8): List<TopicMessage> {
        val value = Replies.parse(bridge.pollTopics(link, maxFrames)).value
        val array = value as? org.json.JSONArray ?: return emptyList()
        return array.objects().mapNotNull { runCatching { TopicMessage.from(it) }.getOrNull() }.onEach(onTopic)
    }

    fun update(pkg: ByteArray, key: String, progress: KumiProgress?): SlotInfo =
        SlotInfo.from(obj(Replies.parse(bridge.update(link, pkg, key, progress)).value))

    companion object {
        const val PROTOCOL = 1

        fun speaks(caseProtocol: Int): Boolean = caseProtocol >= PROTOCOL
    }
}

class ServiceSession(private val bridge: KumiBridge, private val link: KumiLink) {
    fun identify(): ServiceIdentity = ServiceIdentity.from(obj(Replies.parse(bridge.identify(link)).value))

    fun backup(path: String, progress: KumiProgress?): BackupInfo = BackupInfo.from(obj(Replies.parse(bridge.backup(link, path, progress)).value))

    fun install(backup: StoredBackup, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?, simulator: Long? = null) {
        val reply = if (simulator != null) {
            bridge.simInstall(simulator, backup.path, backup.sha256, pkg, key, from, progress)
        } else {
            bridge.install(link, backup.path, backup.sha256, pkg, key, from, progress)
        }
        Replies.parse(reply)
    }

    fun restoreStock(backup: StoredBackup, exact: Boolean, from: Int, progress: KumiProgress?) {
        Replies.parse(bridge.restoreStock(link, backup.path, backup.sha256, exact, from, progress))
    }

    fun reboot() {
        Replies.parse(bridge.rebootFromService(link))
    }

    private fun obj(value: Any?): JSONObject = value as? JSONObject ?: throw BridgeException(BridgeFailure("Malformed", value.toString()))
}

data class StoredBackup(val path: String, val sha256: String, val stock: String, val takenAt: Long) {
    fun serialize(): String = JSONObject().put("path", path).put("sha256", sha256).put("stock", stock).put("takenAt", takenAt).toString()

    companion object {
        fun parse(text: String?): StoredBackup? = text?.let {
            runCatching {
                val j = JSONObject(it)
                StoredBackup(j.getString("path"), j.getString("sha256"), j.getString("stock"), j.getLong("takenAt"))
            }.getOrNull()
        }
    }
}
