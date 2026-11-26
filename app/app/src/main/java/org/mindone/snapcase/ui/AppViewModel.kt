// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.app.Application
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
        viewModelScope.launch {
            prefs.rawSettings.map { it.demoMode }.distinctUntilChanged().collect {
                stock.reconnect()
                applyDemo(it)
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
        stock.refreshStatus()
        kumi.refresh()
    }

    suspend fun refreshNow() {
        if (kumi.kind.value != CaseKind.KUMI || home.value != HomeKind.KUMI) {
            stock.refreshStatus()
        }
        kumi.refreshOnDispatcher()
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

    override fun onCleared() {
        caseExecutor.execute {
            stock.close()
            flows.closeNow()
            kumi.close()
            simulated?.close()
            simulated = null
        }
        caseDispatcher.close()
    }
}
