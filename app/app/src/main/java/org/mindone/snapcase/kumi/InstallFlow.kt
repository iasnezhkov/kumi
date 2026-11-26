// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONObject
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.CasePort
import org.mindone.snapcase.transport.ReplugException
import org.mindone.snapcase.transport.ServiceMode

enum class FlowKind { INSTALL, UPDATE, RESTORE, RETURN }

enum class FlowStep { PREPARING, ENTERING_SERVICE, BACKING_UP, WRITING, RESTARTING, CONFIRMING, DONE, FAILED }

enum class FlowNotice {
    INSTALL_NOT_ENABLED,
    BACKUP_STALE;

    companion object {
        fun of(failure: BridgeFailure?): FlowNotice? = when {
            failure == null -> null
            failure.kind.startsWith("GateMissing") -> INSTALL_NOT_ENABLED
            failure.kind == "BackupStale" -> BACKUP_STALE
            else -> null
        }
    }
}

data class FlowState(
    val kind: FlowKind,
    val step: FlowStep,
    val progress: Float? = null,
    val label: String? = null,
    val target: KumiVersion? = null,
    val failure: BridgeFailure? = null,
    val error: String? = null,
    val resumable: Boolean = false,
    val replug: Boolean = false,
    val notice: FlowNotice? = null,
    val failedAt: FlowStep? = null,
) {
    val busy: Boolean get() = step != FlowStep.DONE && step != FlowStep.FAILED
}

interface FlowStore {
    fun stockBackup(): StoredBackup?
    suspend fun saveStockBackup(backup: StoredBackup)
    suspend fun setKumiSeen(seen: Boolean)
}

interface KumiAccess {
    suspend fun client(): KumiClient?
    fun disconnect()
    suspend fun reload()
}

class BackupStore(private val dir: File, private val now: () -> Long = System::currentTimeMillis) {
    fun newPath(demo: Boolean): String {
        dir.mkdirs()
        val stamp = STAMP.format(Instant.ofEpochMilli(now()))
        var file = File(dir, "${if (demo) DEMO_PREFIX else ""}$PREFIX$stamp$SUFFIX")
        var n = 1
        while (file.exists()) file = File(dir, "${if (demo) DEMO_PREFIX else ""}$PREFIX$stamp-${n++}$SUFFIX")
        return file.path
    }

    fun discard(path: String) {
        File(path).delete()
        File(path + PARTIAL).delete()
    }

    fun prune(keep: Collection<String>, demo: Boolean) {
        val kept = keep.map { File(it).canonicalFile }.toSet()
        dir.listFiles()?.filter { owned(it.name) && it.name.startsWith(DEMO_PREFIX) == demo && it.canonicalFile !in kept }?.forEach { it.delete() }
    }

    fun files(): List<File> = dir.listFiles()?.filter { owned(it.name) }?.sortedBy { it.name } ?: emptyList()

    private fun owned(name: String): Boolean =
        (name.startsWith(PREFIX) || name.startsWith(DEMO_PREFIX + PREFIX)) && (name.endsWith(SUFFIX) || name.endsWith(SUFFIX + PARTIAL))

    companion object {
        private const val PREFIX = "case-flash-"
        private const val DEMO_PREFIX = "demo-"
        private const val SUFFIX = ".bin"
        private const val PARTIAL = ".partial"
        private val STAMP = DateTimeFormatter.ofPattern("yyyyMMdd-HHmmss").withZone(ZoneOffset.UTC)
    }
}

class InstallFlow(
    private val scope: CoroutineScope,
    private val dispatcher: CoroutineDispatcher,
    private val bridge: KumiBridge,
    private val port: CasePort,
    private val service: ServiceMode,
    private val kumi: KumiAccess,
    private val store: FlowStore,
    private val backups: BackupStore,
    private val publicKey: () -> String,
    private val onFinished: (FlowKind) -> Unit = {},
) {
    private class Pending(
        val kind: FlowKind,
        var lease: ServiceMode.Lease,
        var session: ServiceSession,
        val backup: StoredBackup,
        val pkg: ByteArray?,
        val exact: Boolean,
    ) {
        var from: Int? = null
        var written = false
        val resumable: Boolean get() = written || from != null
    }

    private val _state = MutableStateFlow<FlowState?>(null)
    val state: StateFlow<FlowState?> = _state.asStateFlow()

    @Volatile private var pending: Pending? = null

    val busy: Boolean get() = _state.value?.busy == true

    private val _missingGate = MutableStateFlow<String?>(null)
    val missingGate: StateFlow<String?> = _missingGate.asStateFlow()

    fun refreshGates(): String? {
        val missing = if (port.simulated || !bridge.available) {
            null
        } else {
            runCatching { gateOf(Replies.parse(bridge.installGates()).value) }.getOrElse { UNKNOWN_GATE }
        }
        _missingGate.value = missing
        return missing
    }

    fun install(pkg: FirmwarePackage) {
        if (busy) return
        val missing = refreshGates()
        if (missing != null) {
            pending?.let { stale -> scope.launch(dispatcher) { stale.lease.close() } }
            pending = null
            _state.value = FlowState(
                FlowKind.INSTALL, FlowStep.FAILED, target = pkg.version, failure = BridgeFailure("GateMissing($missing)"),
                error = "Installing Kumi needs the $missing hardware check", notice = FlowNotice.INSTALL_NOT_ENABLED,
            )
            return
        }
        start(FlowKind.INSTALL, pkg.version) { doInstall(pkg.bytes()) }
    }

    fun update(pkg: FirmwarePackage) = start(FlowKind.UPDATE, pkg.version) { doUpdate(pkg) }

    fun restoreStock(exact: Boolean) = start(FlowKind.RESTORE, null) { doRestore(exact) }

    fun returnToNormal() = start(FlowKind.RETURN, null) { doReturn() }

    fun resume() {
        val p = pending ?: return
        if (busy || !p.resumable) return
        _state.update { FlowState(p.kind, FlowStep.ENTERING_SERVICE, target = it?.target) }
        scope.launch(dispatcher) {
            runCatching {
                p.lease.close()
                p.lease = service.enter(SERVICE_WAIT_MS)
                p.session = ServiceSession(bridge, p.lease)
                writeAndRestart()
            }.onFailure { failed(p.kind, it) }
        }
    }

    fun close() {
        if (busy) return
        val p = pending
        pending = null
        _state.value = null
        if (p != null) scope.launch(dispatcher) { p.lease.close() }
    }

    fun closeNow() {
        val p = pending
        pending = null
        _state.value = null
        p?.let { runCatching { it.lease.close() } }
    }

    private fun start(kind: FlowKind, target: KumiVersion?, block: suspend () -> Unit) {
        if (busy) return
        val stale = pending
        pending = null
        _state.value = FlowState(kind, FlowStep.PREPARING, target = target)
        scope.launch(dispatcher) {
            stale?.lease?.close()
            runCatching { block() }.onFailure { failed(kind, it) }
        }
    }

    private fun step(kind: FlowKind, step: FlowStep, progress: Float? = null) {
        _state.update { FlowState(kind, step, progress, null, it?.target) }
    }

    private fun progress(kind: FlowKind, step: FlowStep): KumiProgress {
        var last = -1
        return KumiProgress { done, total, label ->
            val permille = if (total > 0) (done * 1000 / total).toInt() else 0
            if (permille != last) {
                last = permille
                _state.update { it?.copy(kind = kind, step = step, progress = permille / 1000f, label = label) }
            }
        }
    }

    private fun failed(kind: FlowKind, error: Throwable) {
        val failure = (error as? BridgeException)?.failure
        val p = pending
        if (p != null && !p.written && failure?.resume != null) p.from = failure.resume
        val resumable = p?.resumable == true
        if (p != null && !resumable) {
            pending = null
            p.lease.close()
        }
        _state.update {
            FlowState(
                kind, FlowStep.FAILED, target = it?.target, failure = failure, error = error.message ?: error.toString(),
                resumable = resumable, replug = error is ReplugException, notice = FlowNotice.of(failure),
                failedAt = it?.step?.takeIf { s -> s != FlowStep.FAILED && s != FlowStep.DONE },
            )
        }
    }

    private fun checkIdentity(identity: ServiceIdentity) {
        if (!identity.chipMatches) throw BridgeException(BridgeFailure("Chip", "0x%08x".format(identity.chip)))
        if (!identity.jedec.lowercase().startsWith(FLASH_MANUFACTURER) || identity.capacity != FLASH_BYTES) {
            throw BridgeException(BridgeFailure("Flash", identity.jedec))
        }
    }

    private fun takeBackup(kind: FlowKind, session: ServiceSession): StoredBackup {
        step(kind, FlowStep.BACKING_UP, 0f)
        val path = backups.newPath(port.simulated)
        val info = try {
            session.backup(path, progress(kind, FlowStep.BACKING_UP))
        } catch (error: Exception) {
            backups.discard(path)
            throw error
        }
        return StoredBackup(path, info.sha256, info.stock, System.currentTimeMillis())
    }

    private suspend fun keepBackup(backup: StoredBackup) {
        val stored = store.stockBackup()?.takeIf { File(it.path).isFile }
        val reference = if (port.simulated) null else stored ?: backup.takeIf { it.stock == STOCK_HEAD }
        if (reference != null && reference != stored) store.saveStockBackup(reference)
        backups.prune(listOfNotNull(stored?.path, reference?.path, backup.path), demo = port.simulated)
    }

    private suspend fun leaveRamKumi() {
        val client = kumi.client() ?: throw IllegalStateException("The case is not connected")
        if (client.slots().booted != null) throw IllegalStateException("Kumi is already installed on this case; update it instead")
        client.reboot(RebootTarget.BootRom)
        kumi.disconnect()
    }

    private suspend fun doInstall(pkg: ByteArray) {
        step(FlowKind.INSTALL, FlowStep.ENTERING_SERVICE)
        val start = port.kind()
        if (start == CaseKind.KUMI) leaveRamKumi()
        val fromStock = start == CaseKind.STOCK || start == CaseKind.KUMI
        val lease = service.enter(SERVICE_WAIT_MS, sendEntry = start == CaseKind.STOCK)
        try {
            val session = ServiceSession(bridge, lease)
            checkIdentity(session.identify())
            val backup = takeBackup(FlowKind.INSTALL, session)
            keepBackup(backup)
            if (backup.stock != STOCK_HEAD) throw BridgeException(BridgeFailure("NotStock"))
            pending = Pending(FlowKind.INSTALL, lease, session, backup, pkg, exact = false)
        } catch (error: Throwable) {
            lease.close()
            throw error
        }
        try {
            writeAndRestart()
        } catch (refused: BridgeException) {
            val p = pending
            if (p != null && !p.written && p.from == null && refused.failure.step == null) {
                pending = null
                if (FlowNotice.of(refused.failure) == FlowNotice.INSTALL_NOT_ENABLED) refreshGates()
                leaveUntouched(p, fromStock)
            }
            throw refused
        }
    }

    private suspend fun leaveUntouched(p: Pending, fromStock: Boolean) {
        if (!fromStock) {
            p.lease.close()
            return
        }
        step(FlowKind.INSTALL, FlowStep.RESTARTING)
        runCatching { p.session.reboot() }
        p.lease.close()
        service.awaitRuntime(RESTART_WAIT_MS)
    }

    private suspend fun doRestore(exact: Boolean) {
        step(FlowKind.RESTORE, FlowStep.ENTERING_SERVICE)
        if (port.kind() == CaseKind.KUMI) {
            val client = kumi.client() ?: throw IllegalStateException("The case is not connected")
            client.reboot(RebootTarget.BootRom)
            kumi.disconnect()
        }
        val lease = service.enter(SERVICE_WAIT_MS, sendEntry = false)
        val stored: StoredBackup?
        try {
            val session = ServiceSession(bridge, lease)
            checkIdentity(session.identify())
            stored = if (port.simulated) null else store.stockBackup()?.takeIf { File(it.path).isFile }
            val first = stored ?: takeBackup(FlowKind.RESTORE, session).also { keepBackup(it) }
            pending = Pending(FlowKind.RESTORE, lease, session, first, null, exact = exact && first.stock == STOCK_HEAD)
        } catch (error: Throwable) {
            lease.close()
            throw error
        }
        try {
            writeAndRestart()
        } catch (refused: BridgeException) {
            val p = pending
            if (stored == null || p == null || p.written || p.from != null || refused.failure.kind !in STORED_BACKUP_REFUSALS) throw refused
            val fresh = takeBackup(FlowKind.RESTORE, p.session)
            keepBackup(fresh)
            pending = Pending(FlowKind.RESTORE, p.lease, p.session, fresh, null, exact = exact && fresh.stock == STOCK_HEAD)
            writeAndRestart()
        }
    }

    private suspend fun writeAndRestart() {
        val p = pending ?: return
        if (!p.written) {
            step(p.kind, FlowStep.WRITING, 0f)
            val from = p.from ?: 0
            when (p.kind) {
                FlowKind.INSTALL -> p.session.install(p.backup, requireNotNull(p.pkg), publicKey(), from, progress(p.kind, FlowStep.WRITING), port.simulator)
                FlowKind.RESTORE -> p.session.restoreStock(p.backup, p.exact, from, progress(p.kind, FlowStep.WRITING))
                FlowKind.UPDATE, FlowKind.RETURN -> throw IllegalStateException("${p.kind} does not write through service mode")
            }
            p.written = true
        }
        step(p.kind, FlowStep.RESTARTING)
        val rebootError = runCatching { p.session.reboot() }.exceptionOrNull()
        if (rebootError == null) {
            pending = null
            p.lease.close()
        }
        val expected = if (p.kind == FlowKind.INSTALL) CaseKind.KUMI else CaseKind.STOCK
        val back = service.awaitRuntime(if (rebootError == null) RESTART_WAIT_MS else REBOOT_GRACE_MS)
        if (back == null) throw rebootError ?: ReplugException("The case did not come back after the restart")
        if (rebootError != null) {
            pending = null
            p.lease.close()
        }
        if (back != expected) throw IllegalStateException("The case came back with the ${back.name.lowercase()} firmware")
        arrived(back)
        step(p.kind, FlowStep.DONE, 1f)
        onFinished(p.kind)
    }

    private suspend fun arrived(kind: CaseKind) {
        store.setKumiSeen(kind == CaseKind.KUMI)
        if (kind == CaseKind.KUMI) kumi.reload()
    }

    private suspend fun doUpdate(pkg: FirmwarePackage) {
        step(FlowKind.UPDATE, FlowStep.WRITING, 0f)
        val client = kumi.client() ?: throw IllegalStateException("The case is not connected")
        if (client.slots().running?.state != SlotState.Confirmed) {
            throw IllegalStateException("The running firmware is not confirmed yet; confirm or restart it before updating")
        }
        val written = client.update(pkg.bytes(), publicKey(), progress(FlowKind.UPDATE, FlowStep.WRITING))
        step(FlowKind.UPDATE, FlowStep.RESTARTING)
        client.reboot(RebootTarget.Normal)
        kumi.disconnect()
        if (!port.simulated) service.waitWhile(CaseKind.KUMI, REENUMERATE_MS)
        if (!service.waitFor(CaseKind.KUMI, RESTART_WAIT_MS)) throw ReplugException("The case did not come back after the update")
        step(FlowKind.UPDATE, FlowStep.CONFIRMING)
        val fresh = kumi.client() ?: throw IllegalStateException("The case did not reconnect")
        val running = fresh.slots().running
        val device = fresh.deviceInfo()
        val state = running?.state.takeIf { running?.slot == written.slot && device.firmware == pkg.version }
        if (state != SlotState.Trial && state != SlotState.Confirmed) {
            throw IllegalStateException("The case still runs ${device.firmware}; it kept the previous firmware")
        }
        if (state == SlotState.Trial) fresh.confirmBoot()
        kumi.reload()
        step(FlowKind.UPDATE, FlowStep.DONE, 1f)
        onFinished(FlowKind.UPDATE)
    }

    private suspend fun doReturn() {
        step(FlowKind.RETURN, FlowStep.RESTARTING)
        val lease = service.enter(0, sendEntry = false)
        val rebootError = try {
            runCatching { ServiceSession(bridge, lease).reboot() }.exceptionOrNull()
        } finally {
            lease.close()
        }
        val back = service.awaitRuntime(if (rebootError == null) RESTART_WAIT_MS else REBOOT_GRACE_MS)
            ?: throw rebootError ?: ReplugException("The case did not leave service mode")
        arrived(back)
        step(FlowKind.RETURN, FlowStep.DONE, 1f)
        onFinished(FlowKind.RETURN)
    }

    companion object {
        const val SERVICE_WAIT_MS = 15_000L
        const val RESTART_WAIT_MS = 20_000L
        const val REENUMERATE_MS = 5_000L
        const val REBOOT_GRACE_MS = 8_000L
        const val FLASH_BYTES = 4L * 1024 * 1024

        const val UNKNOWN_GATE = "Unknown"

        fun gateOf(reply: Any?): String? {
            val value = reply as? JSONObject ?: return UNKNOWN_GATE
            if (!value.has("missing")) return UNKNOWN_GATE
            return if (value.isNull("missing")) null else value.getString("missing")
        }
        private const val FLASH_MANUFACTURER = "c8"
        private const val STOCK_HEAD = "Head"
        private val STORED_BACKUP_REFUSALS = setOf("BackupMissing", "BackupChanged", "BackupSize", "StockMismatch", "NoStockInBackup")
    }
}
