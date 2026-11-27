// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.withContext
import org.mindone.snapcase.transport.CaseChannel
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.CasePort
import org.mindone.snapcase.transport.PermissionDeniedException

data class PresencePrompt(val tier: Int)

data class KumiState(
    val connected: Boolean = false,
    val device: DeviceInfo? = null,
    val audio: AudioStatus? = null,
    val output: OutputSettings? = null,
    val dsp: DspSettings? = null,
    val dspExtras: DspExtras? = null,
    val dacExtras: DacExtras? = null,
    val meters: AudioMeters? = null,
    val eqBase: EqBaseSettings? = null,
    val toneEq: ToneEqSettings? = null,
    val impedance: ImpedanceReport? = null,
    val measuringImpedance: Boolean = false,
    val testTonePlaying: Boolean = false,
    val mic: MicSettings? = null,
    val eqSlots: List<EqProfile?> = List(EqProfile.SLOTS) { null },
    val keymap: List<KeymapEntry> = emptyList(),
    val keyConfig: KeyConfig? = null,
    val mediaGestures: MediaGestures? = null,
    val macros: List<MacroProgram?> = List(KumiLimits.MACRO_SLOTS) { null },
    val combos: List<KeyCombo?> = List(KumiLimits.COMBO_SLOTS) { null },
    val keyboard: KeyboardStatus? = null,
    val slots: SlotTable? = null,
    val diagnostics: Diagnostics? = null,
    val telemetry: SystemTelemetry? = null,
    val events: EventMask? = null,
    val log: List<LogLine> = emptyList(),
    val keyEvents: List<KeyEvent> = emptyList(),
    val loading: Boolean = false,
    val error: String? = null,
    val errorKind: CaseError? = null,
    val errorBridgeKind: String? = null,
    val permissionDenied: Boolean = false,
    val presencePrompt: PresencePrompt? = null,
    val unlockedUntil: Long = 0,
    val peek: WordBlock? = null,
    val developer: DeveloperResult? = null,
) {
    val ramOnly: Boolean
        get() = device?.capabilities?.let { !it.firmwareUpdate && !it.persistentSettings } == true && slots?.booted == null

    val keepsSettings: Boolean get() = device?.capabilities?.persistentSettings == true
}

sealed interface DeveloperResult {
    data class Poked(val address: Long, val value: Long) : DeveloperResult
    data class I2c(val bus: Int, val address: Int, val reply: I2cReply) : DeveloperResult
    data class Bench(val bench: AudioBench) : DeveloperResult
    data class Refused(val action: String, val error: CaseError, val raw: String) : DeveloperResult
}

class KumiSession(
    private val scope: CoroutineScope,
    private val dispatcher: CoroutineDispatcher,
    private val bridge: KumiBridge,
    private val port: CasePort,
    private val flowBusy: () -> Boolean,
    private val onConnected: suspend () -> Unit = {},
    private val now: () -> Long = System::currentTimeMillis,
) : KumiAccess {
    private val _kind = MutableStateFlow(CaseKind.NONE)
    val kind: StateFlow<CaseKind> = _kind.asStateFlow()

    private val _state = MutableStateFlow(KumiState())
    val state: StateFlow<KumiState> = _state.asStateFlow()

    private val _liveMeters = MutableStateFlow<AudioMeters?>(null)
    val liveMeters: StateFlow<AudioMeters?> = _liveMeters.asStateFlow()

    private val _liveSpectrum = MutableStateFlow<AudioSpectrum?>(null)
    val liveSpectrum: StateFlow<AudioSpectrum?> = _liveSpectrum.asStateFlow()

    @Volatile private var spectrumSupported: Boolean = false

    private class EqSnapshot(val profile: EqProfile?)

    @Volatile private var client: KumiClient? = null
    @Volatile private var channel: CaseChannel? = null
    @Volatile private var connectedDeviceKey: String? = null
    private val connecting = Mutex()
    private val eqJobs = ConcurrentHashMap<Int, Job>()
    private val eqBefore = ConcurrentHashMap<Int, EqSnapshot>()
    @Volatile private var toneJob: Job? = null
    @Volatile private var presenceJob: Job? = null

    val simulated: Boolean get() = port.simulated

    val isLiveConnected: Boolean get() = client != null && _state.value.connected && !flowBusy()

    fun dismissPresencePrompt() {
        presenceJob?.cancel()
        presenceJob = null
        _state.update { it.copy(presencePrompt = null) }
    }

    private fun requestPresence(tier: Int, retry: () -> Unit) {
        presenceJob?.cancel()
        _state.update { it.copy(loading = false, error = null, errorKind = null, errorBridgeKind = null, presencePrompt = PresencePrompt(tier)) }
        presenceJob = scope.launch(dispatcher) {
            repeat(PRESENCE_POLL_ATTEMPTS) {
                delay(PRESENCE_POLL_MS)
                val c = client ?: run {
                    _state.update { it.copy(presencePrompt = null) }
                    return@launch
                }
                val status = runCatching { c.presence() }.getOrNull()
                if (status != null && status.tier >= tier) {
                    _state.update { it.copy(presencePrompt = null) }
                    presenceJob = null
                    retry()
                    return@launch
                }
            }
            _state.update { it.copy(presencePrompt = null) }
            presenceJob = null
        }
    }

    fun setKeyEventsSubscribed(enabled: Boolean): Job = scope.launch(dispatcher) {
        val c = client ?: return@launch
        val mask = EventMask(keys = enabled, log = true, audio = false, uart = false)
        val result = runCatching { c.subscribe(mask) }
        result.onSuccess { granted -> _state.update { it.copy(events = granted) } }
        result.onFailure { error ->
            if (enabled && error is CaseErrorException && error.error == CaseError.PresenceRequired) {
                requestPresence(1) { setKeyEventsSubscribed(true) }
            }
        }
    }

    fun enqueueBridgeCommand(command: String, value: Int): Boolean {
        if (!isLiveConnected) return false
        val st = _state.value
        when (command) {
            "toggle_eq" -> {
                val active = st.audio?.eqActive
                if (active != null) {
                    selectEq(null)
                } else {
                    val slot = st.eqSlots.indexOfFirst { it != null && it.bands.isNotEmpty() }.takeIf { it >= 0 } ?: 0
                    selectEq(slot)
                }
            }
            "select_eq" -> selectEq(value.takeIf { it in 0 until EqProfile.SLOTS })
            "cycle_crossfeed" -> {
                val cur = st.dsp ?: DspSettings()
                val modes = CrossfeedMode.entries
                val next = modes[(modes.indexOf(cur.crossfeed) + 1) % modes.size]
                setDsp(cur.copy(crossfeed = next))
            }
            "toggle_stay_awake" -> {
                val cur = st.dacExtras ?: DacExtras()
                setDacExtras(cur.copy(stayAwakeWhileActive = !cur.stayAwakeWhileActive))
            }
            "refresh" -> refresh()
            else -> return false
        }
        return true
    }

    fun refresh(): Job = scope.launch(dispatcher) { refreshNow() }

    suspend fun refreshOnDispatcher() = withContext(dispatcher) { refreshNow() }

    suspend fun refreshNow() {
        val kind = port.kind()
        _kind.value = kind
        if (flowBusy()) return
        if (kind != CaseKind.KUMI) {
            if (client != null || _state.value.connected) disconnect()
            if (_state.value.permissionDenied) _state.update { it.copy(permissionDenied = false, error = null, errorKind = null, errorBridgeKind = null) }
            return
        }
        val activeKey = port.deviceKey()
        if (client != null && connectedDeviceKey != null && activeKey != null && activeKey != connectedDeviceKey) {
            disconnect()
        }
        if (_state.value.permissionDenied) return
        val current = client ?: connect() ?: return
        if (!_state.value.connected || _state.value.device == null) loadAll(current) else poll(current)
    }

    fun retry() {
        _state.update { it.copy(permissionDenied = false, error = null, errorKind = null, errorBridgeKind = null) }
        refresh()
    }

    override suspend fun client(): KumiClient? = client ?: connect(wait = true)

    override suspend fun reload() {
        client()?.let { loadAll(it) }
    }

    override fun disconnect() {
        eqJobs.values.forEach { it.cancel() }
        eqJobs.clear()
        eqBefore.clear()
        toneJob?.cancel()
        toneJob = null
        presenceJob?.cancel()
        presenceJob = null
        client = null
        connectedDeviceKey = null
        channel?.let { runCatching { it.close() } }
        channel = null
        runCatching { bridge.resetSession() }
        SnapLiveMirror.clear()
        spectrumSupported = false
        _liveMeters.value = null
        _liveSpectrum.value = null
        _state.update {
            KumiState(
                log = it.log,
                permissionDenied = it.permissionDenied,
                error = it.error.takeIf { _ -> it.permissionDenied },
            )
        }
    }

    private suspend fun connect(wait: Boolean = false): KumiClient? {
        if (wait) connecting.lock() else if (!connecting.tryLock()) return null
        try {
            client?.let { return it }
            val key = port.deviceKey()
            val opened = try {
                port.openKumi()
            } catch (denied: PermissionDeniedException) {
                _state.update { it.copy(loading = false, permissionDenied = true, error = denied.message, errorKind = null) }
                return null
            } catch (error: Exception) {
                fail(error)
                return null
            }
            channel = opened
            connectedDeviceKey = key
            return KumiClient(bridge, opened, ::onTopic).also { client = it }
        } finally {
            connecting.unlock()
        }
    }

    private fun onTopic(message: TopicMessage) {
        when (message) {
            is TopicMessage.Audio -> _state.update { it.copy(audio = message.status) }
            is TopicMessage.Key -> _state.update { it.copy(keyEvents = (listOf(message.event) + it.keyEvents).take(KEY_EVENTS)) }
            is TopicMessage.Log -> _state.update { it.copy(log = (it.log + message.line).takeLast(LOG_LINES)) }
            is TopicMessage.Uart -> Unit
        }
    }

    private suspend fun loadAll(c: KumiClient) {
        _state.update { it.copy(loading = true, error = null, errorKind = null) }
        runCatching {
            val device = c.deviceInfo()
            if (!KumiClient.speaks(device.protocol)) {
                throw IllegalStateException("The case speaks protocol ${device.protocol}; this app needs protocol ${KumiClient.PROTOCOL} or newer")
            }
            val caps = device.capabilities
            val readAudio = caps.audioOut || caps.persistentSettings
            val readEq = caps.parametricEq || caps.persistentSettings
            val slots = runCatching { c.slots() }.getOrNull()
            val output = if (readAudio) runCatching { c.output() }.getOrNull() else null
            val dsp = if (readAudio) runCatching { c.dsp() }.getOrNull() else null
            val dspExtras = if (readAudio) runCatching { c.dspExtras() }.getOrNull() else null
            val dacExtras = if (readAudio) runCatching { c.dacExtras() }.getOrNull() else null
            val mic = if (caps.audioIn) runCatching { c.mic() }.getOrNull() else null
            val eq = if (readEq) (0 until EqProfile.SLOTS).map { runCatching { c.eq(it) }.getOrNull() } else List(EqProfile.SLOTS) { null }
            val eqBase = if (readEq) runCatching { c.eqBase() }.getOrNull() else null
            val toneEq = if (readEq) runCatching { c.toneEq() }.getOrNull() else null
            val keymap = if (caps.keymap) (0 until KeymapEntry.LAYERS).flatMap { runCatching { c.keymap(it) }.getOrDefault(emptyList()) } else emptyList()
            val keyConfig = if (caps.keymap) runCatching { c.keyConfig() }.getOrNull() else null
            val mediaGestures = if (caps.keymap) runCatching { c.mediaGestures() }.getOrNull() else null
            val macros = if (caps.keymap) (0 until KumiLimits.MACRO_SLOTS).map { runCatching { c.macroSlot(it) }.getOrNull() } else List(KumiLimits.MACRO_SLOTS) { null }
            val combos = if (caps.keymap) (0 until KumiLimits.COMBO_SLOTS).map { runCatching { c.comboSlot(it) }.getOrNull() } else List(KumiLimits.COMBO_SLOTS) { null }
            val events = runCatching { c.subscribe(EventMask.APP) }.getOrNull()
            val audio = runCatching { c.audioStatus() }.getOrNull()
            val meters = if (caps.audioOut && audio?.stream != null) runCatching { c.audioMeters() }.getOrNull() else null
            val initialSpectrum = if (caps.audioOut) runCatching { c.audioSpectrum() }.getOrNull() else null
            spectrumSupported = initialSpectrum != null
            val keyboard = runCatching { c.keyboardStatus() }.getOrNull()
            val diagnostics = runCatching { c.diagnostics() }.getOrNull()
            val telemetry = runCatching { c.telemetry() }.getOrNull()
            _liveMeters.value = meters
            _liveSpectrum.value = initialSpectrum?.takeIf { audio?.stream != null }
            _state.update {
                it.copy(
                    connected = true, device = device, slots = slots, output = output, dsp = dsp, dspExtras = dspExtras,
                    dacExtras = dacExtras, meters = meters, mic = mic, eqSlots = eq, eqBase = eqBase, toneEq = toneEq,
                    keymap = keymap, keyConfig = keyConfig, mediaGestures = mediaGestures, macros = macros, combos = combos,
                    audio = audio, keyboard = keyboard, diagnostics = diagnostics, telemetry = telemetry, events = events,
                    loading = false,
                )
            }
            SnapLiveMirror.update(_state.value)
            onConnected()
        }.onFailure(::fail)
    }

    private fun poll(c: KumiClient) {
        runCatching {
            val audio = c.audioStatus()
            val keyboard = c.keyboardStatus()
            if (audio.stream == null) {
                _liveMeters.value = null
                _liveSpectrum.value = null
            }
            val subscribed = _state.value.events
            if (subscribed != null && (subscribed.keys || subscribed.uart)) {
                c.pollTopics()
            }
            val prev = _state.value
            val clearTransient = prev.error != null && prev.errorKind == null
            if (prev.audio != audio || prev.keyboard != keyboard || clearTransient) {
                _state.update {
                    it.copy(
                        audio = audio,
                        keyboard = keyboard,
                        error = if (clearTransient) null else it.error,
                        errorBridgeKind = if (clearTransient) null else it.errorBridgeKind,
                    )
                }
                SnapLiveMirror.update(_state.value)
            }
        }.onFailure(::fail)
    }

    fun pollMeters(): Job = scope.launch(dispatcher) { pollMetersNow() }

    suspend fun pollMetersNow() = withContext(dispatcher) {
        val c = client ?: return@withContext
        if (_state.value.audio?.stream == null) return@withContext
        runCatching { c.audioMeters() }.getOrNull()?.let { m -> _liveMeters.value = m }
        if (spectrumSupported) {
            runCatching { c.audioSpectrum() }.getOrNull()?.let { s -> _liveSpectrum.value = s }
        }
    }

    private fun fail(error: Throwable) {
        val bridgeKind = if (error is BridgeException) {
            if (error.failure.kind == "Link") {
                disconnect()
            } else if (error.failure.kind == "NoReply" || error.failure.kind == "Transport") {
                runCatching { bridge.resetSession() }
            }
            error.failure.kind
        } else {
            null
        }
        val kind = (error as? CaseErrorException)?.error
        _state.update {
            it.copy(
                loading = false,
                error = error.message ?: error.toString(),
                errorKind = kind,
                errorBridgeKind = bridgeKind,
            )
        }
    }

    private fun change(
        optimistic: (KumiState) -> KumiState,
        send: (KumiClient) -> Unit,
        retry: (() -> Unit)? = null,
        onSuccess: () -> Unit = {},
        recover: (KumiClient?) -> (KumiState) -> KumiState,
    ): Job {
        _state.update { optimistic(it).copy(error = null, errorKind = null, errorBridgeKind = null) }
        SnapLiveMirror.update(_state.value, _kind.value)
        return scope.launch(dispatcher) { perform(send, retry, onSuccess, recover) }
    }

    private fun perform(
        send: (KumiClient) -> Unit,
        retry: (() -> Unit)? = null,
        onSuccess: () -> Unit = {},
        recover: (KumiClient?) -> (KumiState) -> KumiState,
    ) {
        val c = client
        val error = if (c == null) IllegalStateException("The case is not connected") else runCatching { send(c) }.exceptionOrNull()
        if (error != null) {
            _state.update(recover(c))
            SnapLiveMirror.update(_state.value, _kind.value)
            if (retry != null && error is CaseErrorException && error.error == CaseError.PresenceRequired) {
                requestPresence(1, retry)
            } else {
                fail(error)
            }
        } else {
            SnapLiveMirror.update(_state.value, _kind.value)
            onSuccess()
        }
    }

    private fun <T> reread(c: KumiClient?, read: (KumiClient) -> T, fallback: T): T =
        c?.let { runCatching { read(it) }.getOrNull() } ?: fallback

    fun setOutput(value: OutputSettings): Job {
        val before = _state.value.output
        return change({ it.copy(output = value) }, { it.setOutput(value) }, retry = { setOutput(value) }) { c ->
            val output = reread(c, KumiClient::output, before)
            return@change { s -> s.copy(output = output) }
        }
    }

    fun setDsp(value: DspSettings): Job {
        val before = _state.value.dsp
        return change({ it.copy(dsp = value) }, { it.setDsp(value) }) { c ->
            val dsp = reread(c, KumiClient::dsp, before)
            return@change { s -> s.copy(dsp = dsp) }
        }
    }

    fun setDspExtras(value: DspExtras): Job {
        val before = _state.value.dspExtras
        return change({ it.copy(dspExtras = value) }, { it.setDspExtras(value) }) { c ->
            val extras = reread(c, KumiClient::dspExtras, before)
            return@change { s -> s.copy(dspExtras = extras) }
        }
    }

    fun setEqBase(value: EqBaseSettings): Job {
        val before = _state.value.eqBase
        return change({ it.copy(eqBase = value) }, { it.setEqBase(value) }) { c ->
            val base = reread(c, KumiClient::eqBase, before)
            return@change { s -> s.copy(eqBase = base) }
        }
    }

    fun setToneEq(value: ToneEqSettings): Job {
        val before = _state.value.toneEq
        return change({ it.copy(toneEq = value) }, { it.setToneEq(value) }) { c ->
            val tone = reread(c, KumiClient::toneEq, before)
            return@change { s -> s.copy(toneEq = tone) }
        }
    }

    fun setMediaGestures(value: MediaGestures): Job {
        val before = _state.value.mediaGestures
        return change({ it.copy(mediaGestures = value) }, { it.setMediaGestures(value) }, retry = { setMediaGestures(value) }) { c ->
            val gestures = reread(c, KumiClient::mediaGestures, before)
            return@change { s -> s.copy(mediaGestures = gestures) }
        }
    }

    fun setDacExtras(value: DacExtras, onSuccess: () -> Unit = {}): Job {
        val before = _state.value.dacExtras
        return change(
            { it.copy(dacExtras = value) },
            { it.setDacExtras(value) },
            retry = { setDacExtras(value, onSuccess) },
            onSuccess = onSuccess,
        ) { c ->
            val extras = reread(c, KumiClient::dacExtras, before)
            return@change { s -> s.copy(dacExtras = extras) }
        }
    }

    fun measureImpedance(): Job = scope.launch(dispatcher) {
        val c = client ?: return@launch
        _state.update { it.copy(measuringImpedance = true, error = null, errorKind = null) }
        runCatching { c.measureImpedance() }
            .onSuccess { report -> _state.update { it.copy(impedance = report, measuringImpedance = false) } }
            .onFailure { error ->
                _state.update { it.copy(measuringImpedance = false) }
                fail(error)
            }
    }

    fun playTestTone(request: TestToneRequest): Job {
        toneJob?.cancel()
        return scope.launch(dispatcher) {
            val c = client ?: return@launch
            val playing = request.frequencyHz > 0
            _state.update { it.copy(testTonePlaying = playing, error = null, errorKind = null) }
            runCatching { c.playTestTone(request) }
                .onSuccess {
                    if (playing) {
                        delay(request.durationMs.toLong() + 250L)
                        _state.update { it.copy(testTonePlaying = false) }
                    }
                }
                .onFailure { error ->
                    _state.update { it.copy(testTonePlaying = false) }
                    if (error is CaseErrorException && error.error == CaseError.PresenceRequired) {
                        requestPresence(1) { playTestTone(request) }
                    } else {
                        fail(error)
                    }
                }
        }.also { toneJob = it }
    }

    fun setKeyConfig(value: KeyConfig): Job {
        val before = _state.value.keyConfig
        return change({ it.copy(keyConfig = value) }, { it.setKeyConfig(value) }, retry = { setKeyConfig(value) }) { c ->
            val config = reread(c, KumiClient::keyConfig, before)
            return@change { s -> s.copy(keyConfig = config) }
        }
    }

    fun setMacro(slot: Int, program: MacroProgram): Job {
        val before = _state.value.macros
        val stored = program.takeIf { it.steps.isNotEmpty() }
        return change(
            { s -> s.copy(macros = s.macros.mapIndexed { i, p -> if (i == slot) stored else p }) },
            { it.setMacro(slot, program) },
            retry = { setMacro(slot, program) },
        ) { c ->
            val restored = reread(c, { it.macroSlot(slot) }, before.getOrNull(slot))
            return@change { s -> s.copy(macros = s.macros.mapIndexed { i, p -> if (i == slot) restored else p }) }
        }
    }

    fun setCombo(slot: Int, combo: KeyCombo?): Job {
        val before = _state.value.combos
        return change(
            { s -> s.copy(combos = s.combos.mapIndexed { i, item -> if (i == slot) combo else item }) },
            { it.setCombo(slot, combo) },
            retry = { setCombo(slot, combo) },
        ) { c ->
            val restored = reread(c, { it.comboSlot(slot) }, before.getOrNull(slot))
            return@change { s -> s.copy(combos = s.combos.mapIndexed { i, item -> if (i == slot) restored else item }) }
        }
    }

    fun confirmBoot(): Job = command {
        it.confirmBoot()
        val slots = it.slots()
        _state.update { s -> s.copy(slots = slots) }
    }

    fun switchSlot(): Job = command {
        it.reboot(RebootTarget.OtherSlot)
        disconnect()
    }

    fun refreshTelemetry(): Job = command {
        val telemetry = it.telemetry()
        _state.update { s -> s.copy(telemetry = telemetry) }
    }

    fun setMic(value: MicSettings): Job {
        val before = _state.value.mic
        return change({ it.copy(mic = value) }, { it.setMic(value) }) { c ->
            val mic = reread(c, KumiClient::mic, before)
            return@change { s -> s.copy(mic = mic) }
        }
    }

    fun editEq(slot: Int, profile: EqProfile, immediate: Boolean = false, activate: Boolean = false): Job {
        eqBefore.putIfAbsent(slot, EqSnapshot(_state.value.eqSlots.getOrNull(slot)))
        val audioBefore = if (activate) _state.value.audio else null
        _state.update { s ->
            s.copy(
                eqSlots = s.eqSlots.replaced(slot, profile),
                audio = if (activate) s.audio?.copy(eqActive = slot) else s.audio,
                error = null,
                errorKind = null,
            )
        }
        eqJobs[slot]?.cancel()
        return scope.launch(dispatcher) {
            if (!immediate && !activate) delay(EQ_SETTLE_MS)
            val before = eqBefore.remove(slot)?.profile
            perform({ c ->
                c.setEq(slot, profile)
                if (activate) {
                    c.selectEq(slot)
                    val audio = c.audioStatus()
                    _state.update { s -> s.copy(audio = audio) }
                }
            }) { c ->
                val restored = reread(c, { it.eq(slot) }, before)
                val restoredAudio = if (activate) reread(c, KumiClient::audioStatus, audioBefore) else null
                return@perform { s ->
                    s.copy(
                        eqSlots = s.eqSlots.replaced(slot, restored),
                        audio = if (activate) restoredAudio else s.audio,
                    )
                }
            }
        }.also { eqJobs[slot] = it }
    }

    fun selectEq(slot: Int?): Job {
        val before = _state.value.audio
        return change({ s -> s.copy(audio = s.audio?.copy(eqActive = slot)) }, { c ->
            c.selectEq(slot)
            val audio = c.audioStatus()
            _state.update { s -> s.copy(audio = audio) }
        }) { c ->
            val audio = reread(c, KumiClient::audioStatus, before)
            return@change { s -> s.copy(audio = audio) }
        }
    }

    fun setKey(entry: KeymapEntry): Job {
        val before = _state.value.keymap
        return change({ s -> s.copy(keymap = s.keymap.withEntry(entry)) }, { it.setKey(entry) }, retry = { setKey(entry) }) { c ->
            val layer = c?.let { runCatching { it.keymap(entry.layer) }.getOrNull() }
            return@change { s ->
                s.copy(keymap = if (layer != null) (s.keymap.filterNot { it.layer == entry.layer } + layer).sortedWith(KEY_ORDER) else before)
            }
        }
    }

    fun clearKeymap(): Job {
        val before = _state.value.keymap
        return change({ it.copy(keymap = emptyList()) }, { it.clearKeymap() }, retry = { clearKeymap() }) { c ->
            val keymap = reread(c, { client -> (0 until KeymapEntry.LAYERS).flatMap { client.keymap(it) } }, before)
            return@change { s -> s.copy(keymap = keymap) }
        }
    }

    fun applyKeymapPreset(preset: KumiKeymapPreset, replaceExisting: Boolean): Job = scope.launch(dispatcher) {
        val c = client ?: return@launch
        if (!preset.valid) return@launch
        _state.update { it.copy(loading = true, error = null, errorKind = null) }
        runCatching {
            if (replaceExisting) {
                c.clearKeymap()
                for (slot in 0 until KumiLimits.COMBO_SLOTS) {
                    if (_state.value.combos.getOrNull(slot) != null) c.setCombo(slot, null)
                }
            }
            preset.entries.forEach { c.setKey(it) }
            val workingCombos = if (replaceExisting) {
                MutableList<KeyCombo?>(KumiLimits.COMBO_SLOTS) { null }
            } else {
                _state.value.combos.toMutableList()
            }
            preset.combos.forEach { combo ->
                val target = workingCombos.indexOfFirst {
                    it != null && ((it.keyA == combo.keyA && it.keyB == combo.keyB) || (it.keyA == combo.keyB && it.keyB == combo.keyA))
                }.takeIf { it >= 0 } ?: workingCombos.indexOfFirst { it == null }
                if (target in 0 until KumiLimits.COMBO_SLOTS) {
                    c.setCombo(target, combo)
                    workingCombos[target] = combo
                }
            }
            preset.config?.let { c.setKeyConfig(it) }
            val keymap = (0 until KeymapEntry.LAYERS).flatMap { runCatching { c.keymap(it) }.getOrDefault(emptyList()) }
            val combos = (0 until KumiLimits.COMBO_SLOTS).map { runCatching { c.comboSlot(it) }.getOrNull() }
            val config = runCatching { c.keyConfig() }.getOrNull()
            _state.update {
                it.copy(
                    keymap = keymap,
                    combos = combos,
                    keyConfig = config ?: it.keyConfig,
                    loading = false,
                )
            }
        }.onFailure { error ->
            if (error is CaseErrorException && error.error == CaseError.PresenceRequired) {
                requestPresence(1) { applyKeymapPreset(preset, replaceExisting) }
            } else {
                fail(error)
            }
        }
    }

    private fun command(action: (KumiClient) -> Unit): Job = scope.launch(dispatcher) {
        val c = client ?: return@launch
        runCatching { action(c) }.onFailure(::fail)
    }

    fun resetSettings(settingsScope: SettingsScope): Job = scope.launch(dispatcher) {
        val c = client ?: return@launch
        runCatching {
            c.resetSettings(settingsScope)
            loadAll(c)
        }.onFailure { error ->
            if (error is CaseErrorException && error.error == CaseError.PresenceRequired) {
                requestPresence(1) { resetSettings(settingsScope) }
            } else {
                fail(error)
            }
        }
    }

    fun unlockDeveloper(seconds: Int): Job = command {
        it.devUnlock(seconds)
        _state.update { s -> s.copy(unlockedUntil = now() + seconds * 1000L) }
    }

    fun peek(address: Long, words: Int): Job = command {
        val block = it.devPeek(address, words)
        _state.update { s -> s.copy(peek = block) }
    }

    fun refreshDiagnostics(): Job = command {
        val diagnostics = it.diagnostics()
        _state.update { s -> s.copy(diagnostics = diagnostics) }
    }

    fun poke(address: Long, value: Long, mask: Long): Job = developer("dev/poke") { DeveloperResult.Poked(address, it.devPoke(address, value, mask)) }

    fun i2c(bus: Int, address: Int, write: List<Int>, read: Int): Job =
        developer("dev/i2c") { DeveloperResult.I2c(bus, address, it.devI2c(bus, address, write, read)) }

    fun audioBench(request: AudioBenchRequest): Job = developer("dev/audio") { DeveloperResult.Bench(it.audioBench(request)) }

    fun restartCase(): Job = command {
        it.reboot(RebootTarget.Normal)
        disconnect()
    }

    private fun developer(action: String, run: (KumiClient) -> DeveloperResult): Job = scope.launch(dispatcher) {
        val c = client ?: return@launch
        try {
            val result = run(c)
            _state.update { s -> s.copy(developer = result) }
        } catch (refused: CaseErrorException) {
            _state.update { s -> s.copy(developer = DeveloperResult.Refused(action, refused.error, refused.raw)) }
        } catch (error: Exception) {
            fail(error)
        }
    }

    fun clearError() = _state.update { it.copy(error = null, errorKind = null, errorBridgeKind = null) }

    fun close() {
        disconnect()
    }

    companion object {
        const val EQ_SETTLE_MS = 250L
        const val PRESENCE_POLL_MS = 350L
        private const val PRESENCE_POLL_ATTEMPTS = 180
        private const val KEY_EVENTS = 32
        private const val LOG_LINES = 200
        private val KEY_ORDER = compareBy<KeymapEntry>({ it.layer }, { it.source })

        private fun List<EqProfile?>.replaced(slot: Int, profile: EqProfile?): List<EqProfile?> =
            mapIndexed { i, p -> if (i == slot) profile else p }

        private fun List<KeymapEntry>.withEntry(entry: KeymapEntry): List<KeymapEntry> {
            val rest = filterNot { it.layer == entry.layer && it.source == entry.source }
            return if (entry.action == KeyAction.Transparent) rest else (rest + entry).sortedWith(KEY_ORDER)
        }
    }
}
