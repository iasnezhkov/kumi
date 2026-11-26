// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.stock

import android.util.Log
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.mindone.snapcase.data.AppSettings
import org.mindone.snapcase.protocol.BiquadDesigner
import org.mindone.snapcase.protocol.CaseAsleepException
import org.mindone.snapcase.protocol.CaseAudioState
import org.mindone.snapcase.protocol.CaseException
import org.mindone.snapcase.protocol.DacFilter
import org.mindone.snapcase.protocol.DemoTransport
import org.mindone.snapcase.protocol.EqBand
import org.mindone.snapcase.protocol.EqPreset
import org.mindone.snapcase.protocol.EqPresets
import org.mindone.snapcase.protocol.EqSetting
import org.mindone.snapcase.protocol.FilterType
import org.mindone.snapcase.protocol.LiveState
import org.mindone.snapcase.protocol.SnapCase
import org.mindone.snapcase.protocol.SnapProtocol
import org.mindone.snapcase.protocol.Transport
import org.mindone.snapcase.protocol.WriteOutcome
import org.mindone.snapcase.system.HidrawNode
import org.mindone.snapcase.system.TypecInfo
import org.mindone.snapcase.system.UsbDeviceInfo
import org.mindone.snapcase.system.UsbPresence

enum class TransportKind { NONE, HIDRAW, DEMO }

data class ConnectionState(
    val kind: TransportKind = TransportKind.NONE,
    val node: HidrawNode? = null,
    val openError: String? = null,
    val version: String? = null,
    val probeError: String? = null,
    val probing: Boolean = false,
    val flashMagic: Boolean? = null,
)

data class StatusState(
    val usb: UsbDeviceInfo? = null,
    val typec: List<TypecInfo> = emptyList(),
    val updatedAt: Long = 0,
    val presence: UsbPresence = UsbPresence(),
)

enum class CaseLink { CHECKING, ABSENT, NO_ACCESS, NOT_RESPONDING, CONNECTING, CONNECTED, SERVICE_MODE, DEMO }

enum class UserMessage { CASE_NOT_ANSWERING, WRITE_FAILED, RESTORED, LOAD_FAILED }

data class StockSound(
    val loaded: Boolean = false,
    val eq: EqSetting? = null,
    val bank: Int = EqPresets.BANK,
    val bands: List<EqBand> = emptyList(),
    val audio: CaseAudioState? = null,
    val recordGainRaw: Int? = null,
    val globalGainRaw: Int? = null,
    val busy: Boolean = false,
    val error: String? = null,
    val lastWrite: String? = null,
) {
    val dacFilter: DacFilter? get() = audio?.dacFilter
    val micGainDb: Double? get() = recordGainRaw?.let { it / 256.0 }
    val sampleRateHz: Long? get() = audio?.sampleRateHz?.takeIf { it > 0 }
    val bits: Int? get() = audio?.pcmBits
}

data class LiveUi(
    val bank: Int = EqPresets.BANK,
    val state: LiveState? = null,
    val busy: Boolean = false,
    val error: String? = null,
    val readAt: Long = 0,
)

interface StockPlatform {
    fun findNode(): HidrawNode?
    fun usbInfo(): UsbDeviceInfo?
    fun typec(): List<TypecInfo>
    fun presence(): UsbPresence
    fun open(node: HidrawNode): Transport
}

interface SoundStore {
    suspend fun saveEq(value: EqSetting)
    suspend fun saveDacFilter(wire: Int)
    suspend fun saveMicGain(db: Double)
    suspend fun setExperimentalWrites(value: Boolean)
}

class StockRepository(
    private val scope: CoroutineScope,
    private val case: CoroutineDispatcher,
    private val io: CoroutineDispatcher,
    private val platform: StockPlatform,
    private val store: SoundStore,
    private val settings: StateFlow<AppSettings>,
    private val now: () -> Long = System::currentTimeMillis,
) {
    private val _connection = MutableStateFlow(ConnectionState())
    val connection: StateFlow<ConnectionState> = _connection.asStateFlow()

    private val _status = MutableStateFlow(StatusState())
    val status: StateFlow<StatusState> = _status.asStateFlow()

    private val _sound = MutableStateFlow(StockSound())
    val sound: StateFlow<StockSound> = _sound.asStateFlow()

    private val _live = MutableStateFlow(LiveUi())
    val live: StateFlow<LiveUi> = _live.asStateFlow()

    private val _messages = MutableSharedFlow<UserMessage>(extraBufferCapacity = 4)
    val messages: SharedFlow<UserMessage> = _messages.asSharedFlow()

    val link: StateFlow<CaseLink> = combine(settings, _connection, _status) { settings, connection, status ->
        when {
            settings.demoMode -> CaseLink.DEMO
            status.updatedAt == 0L -> CaseLink.CHECKING
            status.presence.service -> CaseLink.SERVICE_MODE
            connection.kind == TransportKind.HIDRAW && connection.version != null -> CaseLink.CONNECTED
            connection.kind == TransportKind.HIDRAW && connection.probeError != null -> CaseLink.NOT_RESPONDING
            connection.kind == TransportKind.HIDRAW -> CaseLink.CONNECTING
            status.presence.normal -> CaseLink.NO_ACCESS
            else -> CaseLink.ABSENT
        }
    }.stateIn(scope, SharingStarted.Eagerly, CaseLink.CHECKING)

    @Volatile private var transport: Transport? = null
    @Volatile private var client: SnapCase? = null
    @Volatile private var boundNodeKey: String? = null
    @Volatile private var writesAccepted = false
    private var restoredForKey: String? = null

    private fun nodeKey(node: HidrawNode?, usb: UsbDeviceInfo?): String? = node?.let { "${it.devPath}@${usb?.attributes?.get("devnum")}" }

    fun refreshStatus(): Job = scope.launch {
        val probe = withContext(io) { listOf(platform.findNode(), platform.usbInfo(), platform.typec(), platform.presence()) }
        val node = probe[0] as HidrawNode?
        val usb = probe[1] as UsbDeviceInfo?
        @Suppress("UNCHECKED_CAST")
        _status.value = StatusState(usb, probe[2] as List<TypecInfo>, now(), probe[3] as UsbPresence)
        if (!settings.value.demoMode && nodeKey(node, usb) != boundNodeKey) reconnect()
    }

    fun reconnect(): Job = scope.launch(case) {
        release()
        if (settings.value.demoMode) {
            bind(DemoTransport(), TransportKind.DEMO, null)
            return@launch
        }
        val node = platform.findNode()
        boundNodeKey = nodeKey(node, platform.usbInfo())
        if (node == null) {
            Log.i(TAG, "no hidraw node for the case")
            _sound.value = StockSound()
            _live.value = LiveUi()
            return@launch
        }
        runCatching { platform.open(node) }.fold(
            onSuccess = { bind(it, TransportKind.HIDRAW, node) },
            onFailure = {
                Log.w(TAG, "cannot open ${node.devPath}", it)
                _connection.value = ConnectionState(TransportKind.NONE, node, it.message ?: it.toString())
            },
        )
    }

    private fun release() {
        transport?.close()
        transport = null
        client = null
        _connection.value = ConnectionState(TransportKind.NONE)
    }

    private fun bind(newTransport: Transport, kind: TransportKind, node: HidrawNode?) {
        transport = newTransport
        client = SnapCase(newTransport) { settings.value.experimentalWrites || writesAccepted }
        _connection.value = ConnectionState(kind, node)
        _sound.value = StockSound()
        _live.value = LiveUi()
        probe()
    }

    suspend fun releaseForServiceMode(): Boolean = withContext(case) {
        val current = client
        val bound = current != null && _connection.value.kind == TransportKind.HIDRAW
        if (bound) current.enterServiceMode()
        release()
        boundNodeKey = null
        bound
    }

    fun probe(): Job {
        val current = client ?: return Job().apply { complete() }
        return scope.launch {
            _connection.update { it.copy(probing = true) }
            val result = withContext(case) { runCatching { current.version() to runCatching { current.flashMagicMatches() }.getOrNull() } }
            if (client !== current) return@launch
            _connection.update { state ->
                result.fold(
                    onSuccess = { state.copy(version = it.first, flashMagic = it.second, probeError = null, probing = false) },
                    onFailure = { state.copy(version = null, probeError = it.message ?: it.toString(), probing = false) },
                )
            }
            result.exceptionOrNull()?.let { Log.w(TAG, "case did not answer the version probe", it) }
            if (result.isSuccess) load(restore = true)
        }
    }

    private fun readSound(c: SnapCase, base: StockSound): StockSound {
        val bank0 = c.readEqBank(EqPresets.BANK)
        val bands = if (base.bank == EqPresets.BANK) bank0 else runCatching { c.readEqBank(base.bank) }.getOrDefault(base.bands)
        return base.copy(
            loaded = true,
            eq = EqPresets.fromDevice(bank0),
            bands = bands,
            audio = runCatching { c.readAudioState() }.getOrNull(),
            recordGainRaw = runCatching { c.recordGainRaw() }.getOrNull(),
            globalGainRaw = runCatching { c.globalGainRaw() }.getOrNull(),
            busy = false,
            error = null,
        )
    }

    fun load(restore: Boolean = false): Job {
        val current = client ?: return Job().apply { complete() }
        val key = "${_connection.value.kind}:$boundNodeKey:${transport?.description}"
        return scope.launch {
            _sound.update { it.copy(busy = true) }
            val result = withContext(case) { runCatching { readSound(current, _sound.value) } }
            if (client !== current) return@launch
            result.onSuccess { _sound.value = it }
            result.onFailure {
                Log.w(TAG, "reading sound settings failed", it)
                _sound.update { state -> state.copy(busy = false, error = it.message ?: it.toString()) }
                _messages.tryEmit(UserMessage.LOAD_FAILED)
            }
            val saved = settings.value
            if (restore && result.isSuccess && saved.restoreOnConnect && saved.experimentalWrites && restoredForKey != key) {
                restoredForKey = key
                restoreSaved(result.getOrThrow(), saved)
            }
        }
    }

    private fun restoreSaved(current: StockSound, saved: AppSettings) {
        val eq = saved.savedEq?.takeIf { it.gains != current.eq?.gains }
        val filter = saved.savedDacFilter?.let(DacFilter::fromWire)?.takeIf { it != current.dacFilter }
        val mic = saved.savedMicGainDb?.takeIf { db -> current.micGainDb.let { it == null || Math.abs(db - it) > 0.01 } }
        if (eq == null && filter == null && mic == null) return
        write(success = UserMessage.RESTORED) { case ->
            eq?.let { writeBank(case, it) }
            filter?.let { case.writeDacFilter(it).confirm() }
            mic?.let { case.writeRecordGain(it).confirm() }
            emptyList()
        }
    }

    private fun write(
        label: String? = null,
        success: UserMessage? = null,
        optimistic: (StockSound) -> StockSound = { it },
        persist: suspend () -> Unit = {},
        action: (SnapCase) -> List<WriteOutcome>,
    ): Job {
        val current = client ?: return Job().apply { complete() }
        val before = _sound.value
        _sound.value = optimistic(before).copy(busy = true, error = null)
        return scope.launch {
            val result = withContext(case) {
                runCatching { action(current) }.map { outcomes -> outcomes to runCatching { readSound(current, _sound.value) }.getOrNull() }
            }
            if (client !== current) return@launch
            result.onSuccess { (outcomes, fresh) ->
                val shown = fresh ?: _sound.value.copy(busy = false)
                _sound.value = shown.copy(lastWrite = label?.let { describe(it, outcomes) } ?: shown.lastWrite)
                persist()
                success?.let(_messages::tryEmit)
            }
            result.onFailure { error ->
                Log.w(TAG, "write failed", error)
                _sound.value = before.copy(busy = false, error = error.message ?: error.toString())
                load()
                _messages.tryEmit(if (error is CaseAsleepException) UserMessage.CASE_NOT_ANSWERING else UserMessage.WRITE_FAILED)
            }
        }
    }

    private fun describe(label: String, outcomes: List<WriteOutcome>): String =
        "$label " + outcomes.joinToString(" ") { "ack=${it.acknowledged} readBack=${it.readBackMatches ?: "n/a"}" }

    private fun writeBank(case: SnapCase, setting: EqSetting) {
        EqPresets.bands(setting, settings.value.eqSampleRate).forEach { case.writeEqBand(it).confirm() }
        case.applyEq().confirm()
    }

    private fun WriteOutcome.confirm() {
        if (readBackMatches != true) throw CaseException("The case did not confirm the write")
    }

    fun applyPreset(preset: EqPreset): Job {
        val next = EqSetting.of(preset)
        return write(optimistic = { it.copy(eq = next) }, persist = { store.saveEq(next) }) { case ->
            writeBank(case, next)
            emptyList()
        }
    }

    fun setEqGain(band: Int, gain: Double): Job {
        val previous = _sound.value.eq
        val next = (previous ?: EqSetting.FLAT).withGain(band, gain)
        return write(optimistic = { it.copy(eq = next) }, persist = { store.saveEq(next) }) { case ->
            if (previous == null || next.preamp != previous.preamp || next.q != previous.q) {
                writeBank(case, next)
            } else {
                case.writeEqBand(EqPresets.band(next, band, settings.value.eqSampleRate)).confirm()
                case.applyEq().confirm()
            }
            emptyList()
        }
    }

    fun chooseDacFilter(filter: DacFilter): Job =
        write(optimistic = { it.copy(audio = it.audio?.copy(dacFilterRaw = filter.wire)) }, persist = { store.saveDacFilter(filter.wire) }) {
            it.writeDacFilter(filter).confirm()
            emptyList()
        }

    fun chooseMicGain(db: Double): Job {
        val raw = SnapProtocol.quantizeRecordGain(SnapProtocol.recordGainRaw(db))
        val rounded = raw / 256.0
        return write(optimistic = { it.copy(recordGainRaw = raw) }, persist = { store.saveMicGain(rounded) }) {
            it.writeRecordGain(rounded).confirm()
            emptyList()
        }
    }

    fun setDacFilter(filter: DacFilter): Job = write("DAC filter") { listOf(it.writeDacFilter(filter)) }

    fun setRecordGain(db: Double): Job = write("SET 0x02 %.1f dB".format(db)) { listOf(it.writeRecordGain(db)) }

    fun writeBand(band: EqBand): Job = write("EQ ${band.bank}/${band.band}") { listOf(it.writeEqBand(band)) }

    fun applyEq(): Job = write("0x0A") { listOf(it.applyEq()) }

    fun selectBank(bank: Int): Job {
        _sound.update { it.copy(bank = bank) }
        return load()
    }

    fun buildBand(original: EqBand, frequencyHz: Int, q: Double, gainDb: Double, type: Int, preamp: Int): EqBand {
        val draft = original.copy(
            frequencyHz = frequencyHz,
            qRaw = Math.round(q * 256).toInt(),
            gainRaw = Math.round(gainDb * 256).toInt(),
            filterType = type,
            preamp = preamp,
        )
        if (draft.sameFilterAs(original)) return draft
        val filter = FilterType.fromWire(type) ?: return draft
        return draft.copy(coefficients = BiquadDesigner.coefficients(filter, frequencyHz.toDouble(), draft.q, draft.gainDb, settings.value.eqSampleRate))
    }

    fun setLiveBank(bank: Int) = _live.update { it.copy(bank = bank) }

    fun readLiveState(): Job {
        val current = client ?: return Job().apply { complete() }
        val bank = _live.value.bank
        return scope.launch {
            _live.update { it.copy(busy = true, error = null) }
            val result = withContext(case) { runCatching { current.readLiveState(bank) } }
            _live.update { state ->
                result.fold(
                    onSuccess = { LiveUi(bank, it, false, null, now()) },
                    onFailure = { state.copy(busy = false, error = it.message ?: it.toString()) },
                )
            }
        }
    }

    fun acceptWrites(): Job {
        writesAccepted = true
        return scope.launch { store.setExperimentalWrites(true) }
    }

    fun setExperimentalWrites(value: Boolean): Job {
        writesAccepted = value
        return scope.launch { store.setExperimentalWrites(value) }
    }

    fun close() {
        transport?.close()
        transport = null
        client = null
    }

    companion object {
        private const val TAG = "SnapCase"
    }
}
