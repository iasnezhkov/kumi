// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import java.nio.file.Files
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.ServiceBusyException
import org.mindone.snapcase.transport.ServiceMode

@OptIn(ExperimentalCoroutinesApi::class)
class InstallFlowTest {
    private class Store : FlowStore {
        var backup: StoredBackup? = null
        var kumiSeen: Boolean? = null
        override fun stockBackup(): StoredBackup? = backup
        override suspend fun saveStockBackup(backup: StoredBackup) {
            this.backup = backup
        }
        override suspend fun setKumiSeen(seen: Boolean) {
            kumiSeen = seen
        }
    }

    private class Access(private val bridge: KumiBridge, private val port: FakePort) : KumiAccess {
        var reloads = 0
        var disconnects = 0
        private var client: KumiClient? = null
        override suspend fun client(): KumiClient? = client ?: if (port.kind == CaseKind.KUMI) KumiClient(bridge, FakeChannel()).also { client = it } else null
        override fun disconnect() {
            disconnects++
            client = null
        }
        override suspend fun reload() {
            reloads++
        }
    }

    private class Rig(scope: TestScope, kind: CaseKind, simulated: Boolean = false) {
        val dir: File = Files.createTempDirectory("kumi-flow").toFile()
        val port = FakePort(kind, simulated, if (simulated) 42L else null)
        val bridge = CaseBridge(port)
        val service = ServiceMode(port)
        val store = Store()
        val access = Access(bridge, port)
        val finished = mutableListOf<FlowKind>()
        val backups = BackupStore(dir)
        val flow = InstallFlow(
            scope, StandardTestDispatcher(scope.testScheduler), bridge, port, service, access, store, backups,
            publicKey = { "ab".repeat(32) }, onFinished = { finished += it },
        )
        val pkg = File(dir, "kumi-0.1.0.kumi").apply { writeBytes(byteArrayOf(1, 2, 3)) }
            .let { FirmwarePackage(FirmwareSource.BUNDLED, it.name, PackageInfo(KumiVersion(0, 1, 0), 1, "t", 1, 1, "00"), it) }
        val state: FlowState get() = requireNotNull(flow.state.value)
    }

    @Test
    fun installFromStockEntersServiceBacksUpWritesAndComesBackAsKumi() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertEquals(1, rig.port.entries)
        assertEquals(listOf("identify", "backup", "install", "rebootFromService"), rig.bridge.calls)
        assertEquals(0, rig.bridge.installs.single().second)
        assertEquals(rig.bridge.installs.single().first, rig.store.backup?.path)
        assertEquals(true, rig.store.kumiSeen)
        assertEquals(1, rig.access.reloads)
        assertEquals(listOf(FlowKind.INSTALL), rig.finished)
        assertFalse(rig.service.held)
        assertTrue(rig.port.channels.all { it.closed })
    }

    @Test
    fun aRamLoadedKumiIsInstalledThroughTheBootRomWithoutTheStockEntry() = runTest {
        val rig = Rig(this, CaseKind.KUMI)
        rig.bridge.handlers["system/slots"] = { """{"ok":${KumiSessionTest.NO_SLOTS}}""" }
        rig.bridge.handlers["system/reboot"] = { request ->
            assertEquals("\"BootRom\"", request)
            rig.port.kind = CaseKind.SERVICE
            """{"ok":{"Ok":null}}"""
        }
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertEquals("the stock 4B 01 FF entry is never sent to Kumi", 0, rig.port.entries)
        assertEquals(listOf("system/slots", "system/reboot", "identify", "backup", "install", "rebootFromService"), rig.bridge.calls)
        assertEquals(1, rig.access.disconnects)
        assertEquals(CaseKind.KUMI, rig.port.kind)
    }

    @Test
    fun anInstalledKumiIsNotInstalledAgain() = runTest {
        val rig = Rig(this, CaseKind.KUMI)
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.FAILED, rig.state.step)
        assertTrue(rig.state.error!!.contains("already installed"))
        assertFalse(rig.bridge.calls.contains("system/reboot"))
        assertFalse(rig.service.held)
    }

    @Test
    fun aMissingHardwareGateStopsTheInstallBeforeServiceMode() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.gates = Contract.reply("install_gates_missing")
        assertEquals("G2bWritePath", rig.flow.refreshGates())
        assertEquals("G2bWritePath", rig.flow.missingGate.value)
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.FAILED, rig.state.step)
        assertEquals(FlowNotice.INSTALL_NOT_ENABLED, rig.state.notice)
        assertFalse("nothing to retry", rig.state.resumable)
        assertEquals("no service mode, no backup, no other bridge call", emptyList<String>(), rig.bridge.calls)
        assertEquals(0, rig.port.entries)
        assertFalse(rig.service.held)
        assertTrue(rig.backups.files().isEmpty())

        rig.bridge.gates = Contract.reply("install_gates_complete")
        rig.flow.close()
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertNull(rig.flow.missingGate.value)
    }

    @Test
    fun anUnreadableGateAnswerCountsAsMissing() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.gates = """{"ok":{}}"""
        assertEquals(InstallFlow.UNKNOWN_GATE, rig.flow.refreshGates())
        rig.bridge.gates = """{"err":{"kind":"Panic"}}"""
        assertEquals(InstallFlow.UNKNOWN_GATE, rig.flow.refreshGates())
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowNotice.INSTALL_NOT_ENABLED, rig.state.notice)
        assertEquals(0, rig.port.entries)
    }

    @Test
    fun aGateMissingReplyFromInstallStillLeavesTheCaseOnStock() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.rebootLands = CaseKind.STOCK
        rig.bridge.installReplies += """{"err":{"kind":"GateMissing(G2bWritePath)"}}"""
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.FAILED, rig.state.step)
        assertEquals(FlowNotice.INSTALL_NOT_ENABLED, rig.state.notice)
        assertFalse(rig.state.resumable)
        assertEquals(listOf("identify", "backup", "install", "rebootFromService"), rig.bridge.calls)
        assertEquals(CaseKind.STOCK, rig.port.kind)
        assertFalse(rig.service.held)
        assertTrue("the stock backup is still kept", File(rig.store.backup!!.path).isFile)
        assertEquals("the gates are asked again after the refusal", 2, rig.bridge.gateQueries)
    }

    @Test
    fun aStaleBackupAsksForAFreshOneWithoutBlockingInstalls() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.rebootLands = CaseKind.STOCK
        rig.bridge.installReplies += """{"err":{"kind":"BackupStale"}}"""
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowNotice.BACKUP_STALE, rig.state.notice)
        assertFalse(rig.state.resumable)
        assertEquals(CaseKind.STOCK, rig.port.kind)
        assertNull(rig.flow.missingGate.value)
        rig.flow.close()
        rig.bridge.rebootLands = CaseKind.KUMI
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertEquals(2, rig.bridge.calls.count { it == "backup" })
    }

    @Test
    fun theDemoInstallsThroughTheSimulatorEntry() = runTest {
        val rig = Rig(this, CaseKind.STOCK, simulated = true)
        rig.bridge.gates = Contract.reply("install_gates_missing")
        assertNull(rig.flow.refreshGates())
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertEquals(listOf(42L), rig.bridge.simInstalls)
        assertEquals("the demo never asks for the real gates", 0, rig.bridge.gateQueries)
    }

    @Test
    fun aWriteThatStopsResumesFromTheBridgeResumePoint() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.installReplies += """{"err":{"kind":"Link","step":9,"resume":7,"label":"slot A","boots":["Stock","Service"]}}"""
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.FAILED, rig.state.step)
        assertTrue(rig.state.resumable)
        assertEquals("Service", rig.state.failure?.bootsAfter)
        assertTrue(rig.service.held)
        rig.flow.resume()
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertEquals(listOf(0, 7), rig.bridge.installs.map { it.second })
        assertEquals(1, rig.bridge.calls.count { it == "backup" })
        assertFalse(rig.service.held)
    }

    @Test
    fun aFailedRestartOnlyRetriesTheRestart() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.rebootLands = null
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.FAILED, rig.state.step)
        assertTrue("a case that never shows up asks for a replug", rig.state.replug)
        assertFalse("the restart was sent, so nothing is left to resume", rig.state.resumable)
        assertNull(rig.store.kumiSeen)
        assertTrue(rig.finished.isEmpty())
    }

    @Test
    fun startingANewFlowDropsTheOldResumePoint() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.installReplies += """{"err":{"kind":"VerifyFailed","step":3,"resume":2}}"""
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertTrue(rig.state.resumable)
        rig.bridge.restoreReplies += """{"err":{"kind":"StockMismatch"}}"""
        rig.flow.restoreStock(exact = true)
        advanceUntilIdle()
        assertEquals(FlowKind.RESTORE, rig.state.kind)
        assertEquals(FlowStep.FAILED, rig.state.step)
        assertFalse(rig.state.resumable)
        rig.flow.resume()
        advanceUntilIdle()
        assertEquals(1, rig.bridge.installs.size)
        assertFalse(rig.service.held)
    }

    @Test
    fun onlyTheNewestBackupAndTheStockReferenceAreKept() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        val reference = File(rig.dir, "case-flash-20260101-000000.bin").apply { writeBytes(byteArrayOf(9)) }
        rig.store.backup = StoredBackup(reference.path, sha256(byteArrayOf(9)), "Head", 1)
        File(rig.dir, "case-flash-20260102-000000.bin").writeBytes(byteArrayOf(1))
        File(rig.dir, "case-flash-20260103-000000.bin.partial").writeBytes(byteArrayOf(2))
        File(rig.dir, "demo-case-flash-20260104-000000.bin").writeBytes(byteArrayOf(3))
        File(rig.dir, "notes.txt").writeText("keep")
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        val newest = File(rig.bridge.installs.single().first)
        assertEquals(setOf(reference.name, newest.name, "demo-case-flash-20260104-000000.bin", "notes.txt", "kumi-0.1.0.kumi"), rig.dir.list()!!.toSet())
        assertEquals(reference.path, rig.store.backup?.path)
    }

    @Test
    fun aDemoRunNeverTouchesTheRealBackups() = runTest {
        val rig = Rig(this, CaseKind.STOCK, simulated = true)
        val reference = File(rig.dir, "case-flash-20260101-000000.bin").apply { writeBytes(byteArrayOf(9)) }
        rig.store.backup = StoredBackup(reference.path, sha256(byteArrayOf(9)), "Head", 1)
        val older = File(rig.dir, "case-flash-20260102-000000.bin").apply { writeBytes(byteArrayOf(1)) }
        File(rig.dir, "demo-case-flash-20260103-000000.bin").writeBytes(byteArrayOf(3))
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        val demo = File(rig.bridge.installs.single().first)
        assertTrue(demo.name.startsWith("demo-"))
        assertEquals(setOf(reference.name, older.name, demo.name, "kumi-0.1.0.kumi"), rig.dir.list()!!.toSet())
        assertEquals(reference.path, rig.store.backup?.path)
    }

    @Test
    fun aLostRestartReplyIsFineWhenTheCaseComesBack() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        val bridge = object : CaseBridge(rig.port) {
            override fun rebootFromService(link: KumiLink): String {
                rig.port.kind = CaseKind.KUMI
                return """{"err":{"kind":"Link","detail":"timeout"}}"""
            }
        }
        val flow = InstallFlow(this, StandardTestDispatcher(testScheduler), bridge, rig.port, rig.service, rig.access, rig.store, rig.backups, { "" })
        flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, flow.state.value?.step)
        assertFalse(rig.service.held)
    }

    @Test
    fun aRestartThatFailsWithoutTheCaseReturningResumesWithTheRestartOnly() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        var attempts = 0
        val bridge = object : CaseBridge(rig.port) {
            override fun rebootFromService(link: KumiLink): String {
                attempts++
                if (attempts == 1) return """{"err":{"kind":"Link","detail":"send failed"}}"""
                rig.port.kind = CaseKind.KUMI
                return """{"ok":null}"""
            }
        }
        val flow = InstallFlow(this, StandardTestDispatcher(testScheduler), bridge, rig.port, rig.service, rig.access, rig.store, rig.backups, { "" })
        flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowStep.FAILED, flow.state.value?.step)
        assertTrue(flow.state.value!!.resumable)
        flow.resume()
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, flow.state.value?.step)
        assertEquals(1, bridge.installs.size)
        assertEquals(2, attempts)
        assertFalse(rig.service.held)
    }

    @Test
    fun aFailedBackupLeavesNoFileBehind() = runTest {
        val rig = Rig(this, CaseKind.SERVICE)
        val flow = InstallFlow(this, StandardTestDispatcher(testScheduler), object : CaseBridge(rig.port) {
            override fun backup(link: KumiLink, path: String, progress: KumiProgress?): String {
                File(path).writeBytes(byteArrayOf(1))
                File("$path.partial").writeBytes(byteArrayOf(2))
                return """{"err":{"kind":"ReadsDiffer"}}"""
            }
        }, rig.port, rig.service, rig.access, rig.store, rig.backups, { "" })
        flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals("ReadsDiffer", flow.state.value?.failure?.kind)
        assertTrue(rig.backups.files().isEmpty())
        assertFalse(rig.service.held)
    }

    @Test
    fun anUnexpectedFlashIsRefusedBeforeAnyRead() = runTest {
        val rig = Rig(this, CaseKind.SERVICE)
        rig.bridge.identity = """{"chip":1224819840,"chipMatches":true,"jedec":"ef4016","protect":0,"capacity":4194304}"""
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals("Flash", rig.state.failure?.kind)
        assertEquals(listOf("identify"), rig.bridge.calls)
        assertEquals(0, rig.port.entries)
    }

    @Test
    fun serviceModeHasOneOwnerWhileAFlowRuns() = runTest {
        val rig = Rig(this, CaseKind.STOCK)
        rig.bridge.installReplies += """{"err":{"kind":"Link","step":1,"resume":0}}"""
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertTrue(rig.service.held)
        val refused = runCatching { rig.service.enter(0) }.exceptionOrNull()
        assertTrue(refused is ServiceBusyException)
        rig.flow.close()
        advanceUntilIdle()
        assertFalse(rig.service.held)
        assertNull(rig.flow.state.value)
    }

    @Test
    fun restoreFromKumiRebootsIntoTheBootRomAndUsesTheStoredBackup() = runTest {
        val rig = Rig(this, CaseKind.KUMI)
        val stored = File(rig.dir, "case-flash-20260101-000000.bin").apply { writeBytes(byteArrayOf(5)) }
        rig.store.backup = StoredBackup(stored.path, sha256(byteArrayOf(5)), "Head", 1)
        rig.bridge.handlers["system/reboot"] = { request ->
            assertEquals("\"BootRom\"", request)
            rig.port.kind = CaseKind.SERVICE
            """{"ok":{"Ok":null}}"""
        }
        rig.bridge.rebootLands = CaseKind.STOCK
        rig.flow.restoreStock(exact = true)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertEquals(Triple(stored.path, true, 0), rig.bridge.restores.single())
        assertEquals(0, rig.port.entries)
        assertEquals(1, rig.access.disconnects)
        assertEquals(false, rig.store.kumiSeen)
        assertFalse(rig.bridge.calls.contains("backup"))
    }

    @Test
    fun aStoredBackupTheCaseRejectsIsReplacedByAFreshOne() = runTest {
        val rig = Rig(this, CaseKind.SERVICE)
        val stored = File(rig.dir, "case-flash-20260101-000000.bin").apply { writeBytes(byteArrayOf(5)) }
        rig.store.backup = StoredBackup(stored.path, sha256(byteArrayOf(5)), "Head", 1)
        rig.bridge.stock = "Copy"
        rig.bridge.restoreReplies += """{"err":{"kind":"StockMismatch"}}"""
        rig.bridge.rebootLands = CaseKind.STOCK
        rig.flow.restoreStock(exact = true)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, rig.state.step)
        assertEquals(2, rig.bridge.restores.size)
        assertEquals(stored.path, rig.bridge.restores[0].first)
        assertFalse("a copy-only backup restores the stock copy, not bit for bit", rig.bridge.restores[1].second)
    }

    @Test
    fun anUpdateRestartsIntoTheTrialSlotAndConfirmsIt() = runTest {
        val rig = Rig(this, CaseKind.KUMI, simulated = true)
        var trial = false
        val target = FirmwarePackage(FirmwareSource.RELEASE, "kumi-0.2.0.kumi", PackageInfo(KumiVersion(0, 2, 0), 2, "t", 1, 1, "00"), rig.pkg.file)
        val bridge = object : CaseBridge(rig.port) {
            override fun update(link: KumiLink, pkg: ByteArray, key: String, progress: KumiProgress?): String =
                """{"ok":{"slot":"B","state":"Trial","version":{"major":0,"minor":2,"patch":0},"size":1}}"""
        }
        bridge.handlers["system/reboot"] = { trial = true; """{"ok":{"Ok":null}}""" }
        bridge.handlers["system/slots"] = {
            if (trial) """{"ok":{"booted":"B","slots":[{"slot":"A","state":"Confirmed","version":null,"size":1},{"slot":"B","state":"Trial","version":null,"size":1}]}}"""
            else """{"ok":${CaseBridge.SLOTS}}"""
        }
        bridge.handlers["device/info"] = { """{"ok":${CaseBridge.DEVICE.replace("\"minor\":1", if (trial) "\"minor\":2" else "\"minor\":1")}}""" }
        val access = Access(bridge, rig.port)
        val flow = InstallFlow(this, StandardTestDispatcher(testScheduler), bridge, rig.port, rig.service, access, rig.store, rig.backups, { "" })
        flow.update(target)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, flow.state.value?.step)
        assertTrue(bridge.calls.contains("system/confirm"))
        assertEquals(1, access.reloads)
    }

    @Test
    fun anUpdateTheCaseAlreadyConfirmedItselfIsDone() = runTest {
        val rig = Rig(this, CaseKind.KUMI, simulated = true)
        var restarted = false
        val target = FirmwarePackage(FirmwareSource.RELEASE, "kumi-0.2.0.kumi", PackageInfo(KumiVersion(0, 2, 0), 2, "t", 1, 1, "00"), rig.pkg.file)
        val bridge = object : CaseBridge(rig.port) {
            override fun update(link: KumiLink, pkg: ByteArray, key: String, progress: KumiProgress?): String =
                """{"ok":{"slot":"B","state":"Trial","version":{"major":0,"minor":2,"patch":0},"size":1}}"""
        }
        bridge.handlers["system/reboot"] = { restarted = true; """{"ok":{"Ok":null}}""" }
        bridge.handlers["system/slots"] = {
            if (restarted) """{"ok":{"booted":"B","slots":[{"slot":"A","state":"Confirmed","version":null,"size":1},{"slot":"B","state":"Confirmed","version":null,"size":1}]}}"""
            else """{"ok":${CaseBridge.SLOTS}}"""
        }
        bridge.handlers["device/info"] = { """{"ok":${CaseBridge.DEVICE.replace("\"minor\":1", if (restarted) "\"minor\":2" else "\"minor\":1")}}""" }
        val access = Access(bridge, rig.port)
        val flow = InstallFlow(this, StandardTestDispatcher(testScheduler), bridge, rig.port, rig.service, access, rig.store, rig.backups, { "" })
        flow.update(target)
        advanceUntilIdle()
        assertEquals(FlowStep.DONE, flow.state.value?.step)
        assertFalse("no confirm for a slot that confirmed itself", bridge.calls.contains("system/confirm"))
    }

    @Test
    fun returnToNormalIsRefusedWhileAFlowIsBusyAndWaitsWithATimeout() = runTest {
        val rig = Rig(this, CaseKind.SERVICE)
        rig.bridge.rebootLands = null
        rig.flow.returnToNormal()
        rig.flow.install(rig.pkg)
        advanceUntilIdle()
        assertEquals(FlowKind.RETURN, rig.state.kind)
        assertEquals(FlowStep.FAILED, rig.state.step)
        assertTrue(rig.state.replug)
        assertEquals(listOf("rebootFromService"), rig.bridge.calls)
        assertFalse(rig.service.held)
    }
}
