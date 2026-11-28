// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.app.Application
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.File
import java.util.concurrent.Executors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import org.mindone.snapcase.BuildConfig
import org.mindone.snapcase.data.AppPreferences
import org.mindone.snapcase.data.SettingsRepository
import org.mindone.snapcase.kumi.AssetPackages
import org.mindone.snapcase.kumi.BackupStore
import org.mindone.snapcase.kumi.EqMath
import org.mindone.snapcase.kumi.FirmwarePackage
import org.mindone.snapcase.kumi.FlowStore
import org.mindone.snapcase.kumi.HomeKind
import org.mindone.snapcase.kumi.InstallFlow
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.NativeBridge
import org.mindone.snapcase.kumi.NativeDemoPackages
import org.mindone.snapcase.kumi.NativeEqMath
import org.mindone.snapcase.kumi.ReleaseCatalog
import org.mindone.snapcase.kumi.ReleaseSettings
import org.mindone.snapcase.kumi.Routing
import org.mindone.snapcase.kumi.SimulatedCase
import org.mindone.snapcase.kumi.SimulatedCasePort
import org.mindone.snapcase.kumi.StoredBackup
import org.mindone.snapcase.stock.AndroidStockPlatform
import org.mindone.snapcase.stock.StockRepository
import org.mindone.snapcase.system.DeviceState
import org.mindone.snapcase.system.PhoneBattery
import org.mindone.snapcase.system.SnapInputDevice
import org.mindone.snapcase.system.UsbAudioInfo
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.NoCasePort
import org.mindone.snapcase.transport.ServiceMode
import org.mindone.snapcase.transport.SwitchableCasePort
import org.mindone.snapcase.transport.UsbCasePort

class PhoneState(
    val battery: StateFlow<PhoneBattery?>,
    val inputs: StateFlow<List<SnapInputDevice>>,
    val usbAudio: StateFlow<List<UsbAudioInfo>>,
)

class AppViewModel(application: Application) : AndroidViewModel(application) {
    private val caseExecutor = Executors.newSingleThreadExecutor { Thread(it, "snap-case").apply { isDaemon = true } }
    private val caseDispatcher = caseExecutor.asCoroutineDispatcher()

    val prefs = AppPreferences(SettingsRepository.from(application), viewModelScope)

    val eqMath: EqMath = NativeEqMath(NativeBridge)

    val stock = StockRepository(viewModelScope, caseDispatcher, Dispatchers.IO, AndroidStockPlatform(application), prefs, prefs.settings)

    private val usbPort = UsbCasePort(application, stock::releaseForServiceMode)
    private val casePort = SwitchableCasePort(usbPort)
    private var simulated: SimulatedCase? = null
    private var appliedDemo: Boolean = false
    @Volatile private var flashFocusRequest: AudioFocusRequest? = null

    val kumi: KumiSession = KumiSession(viewModelScope, caseDispatcher, NativeBridge, casePort, flowBusy = { flows.busy }, onConnected = {
        if (!prefs.settings.value.kumiSeen) prefs.setKumiSeen(true)
    })

    val catalog = ReleaseCatalog(
        viewModelScope, Dispatchers.IO, NativeBridge, File(application.cacheDir, "kumi"), AssetPackages(application), NativeDemoPackages,
        BuildConfig.KUMI_RELEASE_KEY, BuildConfig.KUMI_RELEASES_REPO,
    ) { prefs.settings.value.let { ReleaseSettings(it.checkReleases, it.includePrereleases, it.releaseRepository) } }

    val flows: InstallFlow = InstallFlow(
        viewModelScope, caseDispatcher, NativeBridge, casePort, ServiceMode(casePort), kumi,
        object : FlowStore {
            override fun stockBackup(): StoredBackup? = StoredBackup.parse(prefs.settings.value.stockBackup)
            override suspend fun saveStockBackup(backup: StoredBackup) = prefs.setStockBackup(backup.serialize())
            override suspend fun setKumiSeen(seen: Boolean) = prefs.setKumiSeen(seen)
        },
        BackupStore(File(application.filesDir, "kumi")),
        publicKey = { catalog.publicKey },
        onFinished = { reloadCatalog() },
        onFlashAudioFocus = ::setFlashAudioFocus,
    )

    val installBlocked: StateFlow<Boolean> = flows.missingGate.map { it != null }.stateIn(viewModelScope, SharingStarted.Eagerly, false)

    val kumiStreaming: StateFlow<Boolean> = kumi.state.map { it.audio?.stream != null }.distinctUntilChanged()
        .stateIn(viewModelScope, SharingStarted.Eagerly, false)

    val home: StateFlow<HomeKind> = combine(kumi.kind, prefs.settings) { kind, settings -> Routing.home(kind, settings.kumiSeen) }
        .stateIn(viewModelScope, SharingStarted.Eagerly, HomeKind.STOCK)

    val legacyVisible: StateFlow<Boolean> = combine(kumi.kind, prefs.settings) { kind, settings ->
        Routing.legacyVisible(kind, settings.kumiSeen, settings.showLegacy)
    }.stateIn(viewModelScope, SharingStarted.Eagerly, true)

    val installOffer: StateFlow<FirmwarePackage?> = combine(kumi.kind, catalog.state, prefs.settings, installBlocked) { kind, catalog, settings, blocked ->
        catalog.newest?.takeIf { !blocked && Routing.offerInstall(kind, it.version, settings.offerDismissed) }
    }.stateIn(viewModelScope, SharingStarted.Eagerly, null)

    val phone = PhoneState(
        DeviceState.phoneBattery(application).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), null),
        DeviceState.snapInputDevices(application).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptyList()),
        DeviceState.usbAudioDevices(application).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptyList()),
    )

    init {
        org.mindone.snapcase.kumi.SnapLiveMirror.bindContext(application)
        org.mindone.snapcase.kumi.SnapLiveMirror.bindSession(kumi)
        viewModelScope.launch {
            prefs.rawSettings.map { it.demoMode }.distinctUntilChanged().collect { enabled ->
                if (enabled != appliedDemo) {
                    appliedDemo = enabled
                    if (enabled || usbPort.kind() != CaseKind.KUMI) {
                        stock.reconnect()
                    }
                    applyDemo(enabled)
                } else {
                    viewModelScope.launch(caseDispatcher) {
                        if (!flows.busy) {
                            flows.refreshGates()
                            reloadCatalog()
                        }
                    }
                }
            }
        }
        viewModelScope.launch {
            prefs.rawSettings.map { it.launcherBridgeEnabled }.distinctUntilChanged().collect {
                org.mindone.snapcase.kumi.SnapLiveMirror.setControlsAllowed(application, it)
            }
        }
        viewModelScope.launch {
            catalog.state.map { it.appUpdate(BuildConfig.VERSION_NAME) != null }.distinctUntilChanged().collect {
                org.mindone.snapcase.kumi.SnapLiveMirror.setUpdateAvailable(it)
            }
        }
        viewModelScope.launch {
            prefs.rawSettings
                .map { Triple(it.checkReleases, it.includePrereleases, it.releaseRepository) }
                .distinctUntilChanged()
                .drop(1)
                .filter { it.first }
                .collect { catalog.checkReleases() }
        }
    }

    fun refresh() {
        kumi.refresh()
        if (usbPort.kind() != CaseKind.KUMI && home.value != HomeKind.KUMI) {
            stock.refreshStatus()
        }
    }

    suspend fun refreshNow() {
        kumi.refreshOnDispatcher()
        if (kumi.kind.value != CaseKind.KUMI && home.value != HomeKind.KUMI) {
            stock.refreshStatus()
        }
    }

    private fun applyDemo(enabled: Boolean) {
        viewModelScope.launch(caseDispatcher) {
            if (flows.busy) return@launch
            flows.close()
            kumi.disconnect()
            simulated?.close()
            simulated = null
            casePort.target = if (enabled) SimulatedCase.create()?.also { simulated = it }?.let(::SimulatedCasePort) ?: NoCasePort else usbPort
            flows.refreshGates()
            kumi.refreshNow()
            reloadCatalog()
        }
    }

    private fun reloadCatalog() {
        catalog.reload(prefs.settings.value.demoMode, kumi.kind.value == CaseKind.KUMI)
    }

    fun applyLineOutVolume(enabled: Boolean) {
        val app = getApplication<Application>()
        val am = app.getSystemService(AudioManager::class.java) ?: return
        val store = app.getSharedPreferences(LINE_OUT_PREFS, Context.MODE_PRIVATE)
        val usbAttached = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).any {
            it.type == AudioDeviceInfo.TYPE_USB_HEADSET || it.type == AudioDeviceInfo.TYPE_USB_DEVICE
        }
        val maxVol = am.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
        if (enabled) {
            if (!usbAttached || maxVol <= 0) return
            val cur = am.getStreamVolume(AudioManager.STREAM_MUSIC)
            if (!store.contains(KEY_PRE_LINE_OUT_VOL) && cur < maxVol) {
                store.edit().putInt(KEY_PRE_LINE_OUT_VOL, cur).apply()
            }
            runCatching { am.setStreamVolume(AudioManager.STREAM_MUSIC, maxVol, AudioManager.FLAG_SHOW_UI) }
        } else {
            if (!store.contains(KEY_PRE_LINE_OUT_VOL)) return
            val saved = store.getInt(KEY_PRE_LINE_OUT_VOL, -1)
            store.edit().remove(KEY_PRE_LINE_OUT_VOL).apply()
            if (usbAttached && saved in 0..maxVol) {
                runCatching { am.setStreamVolume(AudioManager.STREAM_MUSIC, saved, AudioManager.FLAG_SHOW_UI) }
            }
        }
    }

    private fun setFlashAudioFocus(active: Boolean) {
        val am = getApplication<Application>().getSystemService(AudioManager::class.java) ?: return
        if (active) {
            if (flashFocusRequest != null) return
            val req = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build(),
                )
                .setOnAudioFocusChangeListener {}
                .build()
            flashFocusRequest = req
            runCatching { am.requestAudioFocus(req) }
        } else {
            val req = flashFocusRequest ?: return
            flashFocusRequest = null
            runCatching { am.abandonAudioFocusRequest(req) }
        }
    }

    override fun onCleared() {
        setFlashAudioFocus(false)
        org.mindone.snapcase.kumi.SnapLiveMirror.bindSession(null)
        caseExecutor.execute {
            stock.close()
            flows.closeNow()
            kumi.close()
            simulated?.close()
            simulated = null
        }
        caseDispatcher.close()
    }

    private companion object {
        private const val LINE_OUT_PREFS = "kumi_line_out_vol"
        private const val KEY_PRE_LINE_OUT_VOL = "pre_music_vol"
    }
}
