// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.PermissionDeniedException

@OptIn(ExperimentalCoroutinesApi::class)
class KumiSessionTest {
    companion object {
        const val RAM_DEVICE = """{"protocol":1,"firmware":{"major":0,"minor":1,"patch":0},"build":"ram-dev","chip_id":1224819840,"flash_jedec":[200,101,22],
            "serial":null,"running_from_ram":true,"capabilities":{"audio_out":false,"audio_in":false,"dsd":false,"parametric_eq":false,"keymap":true,
            "layers":true,"battery":false,"persistent_settings":false,"firmware_update":false,"developer":true}}"""
        const val NO_SLOTS = """{"booted":null,"slots":[{"slot":"A","state":"Empty","version":null,"size":0},
            {"slot":"B","state":"Empty","version":null,"size":0}]}"""
    }

    private class Rig(scope: TestScope, kind: CaseKind = CaseKind.KUMI) {
        val port = FakePort(kind)
        val bridge = CaseBridge(port)
        var busy = false
        var connectedCalls = 0
        val session = KumiSession(scope, StandardTestDispatcher(scope.testScheduler), bridge, port, { busy }, { connectedCalls++ })
        val state: KumiState get() = session.state.value
    }

    private suspend fun TestScope.connected(): Rig {
        val rig = Rig(this)
        rig.session.refreshNow()
        assertTrue(rig.state.connected)
        return rig
    }

    @Test
    fun aCaseThatCannotBeOpenedReportsAnErrorInsteadOfCrashing() = runTest {
        val rig = Rig(this)
        rig.port.kumiOpenError = { IllegalStateException("Cannot claim the Kumi control interface") }
        rig.session.refresh()
        advanceUntilIdle()
        assertFalse(rig.state.connected)
        assertEquals("Cannot claim the Kumi control interface", rig.state.error)
        rig.port.kumiOpenError = null
        rig.session.refresh()
        advanceUntilIdle()
        assertTrue(rig.state.connected)
        assertEquals(1, rig.connectedCalls)
    }

    @Test
    fun theUsbPermissionIsAskedOnceAndNotAgainAfterADenial() = runTest {
        val rig = Rig(this)
        val gate = CompletableDeferred<Unit>()
        rig.port.kumiOpenGate = gate
        rig.port.kumiOpenError = { PermissionDeniedException("1209:0001") }
        repeat(3) { rig.session.refresh() }
        advanceUntilIdle()
        assertEquals(1, rig.port.kumiOpens)
        gate.complete(Unit)
        advanceUntilIdle()
        assertTrue(rig.state.permissionDenied)
        repeat(3) { rig.session.refresh() }
        advanceUntilIdle()
        assertEquals(1, rig.port.kumiOpens)
        rig.port.kind = CaseKind.NONE
        rig.session.refreshNow()
        assertFalse(rig.state.permissionDenied)
        rig.port.kind = CaseKind.KUMI
        rig.port.kumiOpenError = null
        rig.session.refreshNow()
        assertEquals(2, rig.port.kumiOpens)
        assertTrue(rig.state.connected)
    }

    @Test
    fun connectingSubscribesToEventsAndReadsDiagnostics() = runTest {
        val rig = connected()
        assertEquals(EventMask.APP, rig.bridge.subscriptions.single())
        assertEquals(EventMask(keys = false, log = true, audio = false, uart = false), rig.state.events)
        assertTrue(rig.state.diagnostics!!.safeMode)
        assertNotNull(rig.state.eqSlots[0])
        assertTrue("empty slots are null, not errors", rig.state.eqSlots.drop(1).all { it == null })
        assertNull(rig.state.error)
    }

    @Test
    fun presenceRequiredOpensPromptAndAutoRetriesOnceConfirmed() = runTest {
        val rig = connected()
        var unlocked = false
        rig.bridge.handlers["keys/map/set"] = {
            if (unlocked) """{"ok":{"Ok":null}}""" else """{"ok":{"Err":"PresenceRequired"}}"""
        }
        rig.bridge.handlers["system/presence"] = {
            if (unlocked) """{"ok":{"tier":1,"remaining_ms":120000,"passthrough":false}}"""
            else """{"ok":{"tier":0,"remaining_ms":0,"passthrough":false}}"""
        }
        rig.session.setKey(KeymapEntry(0, 0x04, KeyAction.Disabled))
        testScheduler.advanceTimeBy(100)
        assertEquals(PresencePrompt(1), rig.state.presencePrompt)
        assertNull(rig.state.error)
        unlocked = true
        advanceUntilIdle()
        assertNull(rig.state.presencePrompt)
        assertNull(rig.state.error)
        assertTrue(rig.state.keymap.any { it.source == 0x04 && it.action == KeyAction.Disabled })
    }

    @Test
    fun anOlderFirmwareWithoutEventsOrDiagnosticsStillConnects() = runTest {
        val rig = Rig(this)
        rig.bridge.handlers["system/diagnostics"] = { """{"ok":{"Err":"Unsupported"}}""" }
        rig.bridge.handlers["events/subscribe"] = { """{"err":{"kind":"UnknownPath"}}""" }
        rig.session.refreshNow()
        assertTrue(rig.state.connected)
        assertNull(rig.state.diagnostics)
        assertNull(rig.state.events)
    }

    @Test
    fun aNewerProtocolKeepsServingVersionOneButAnOlderOneIsNotDriven() = runTest {
        val newer = Rig(this)
        newer.bridge.handlers["device/info"] = { """{"ok":${CaseBridge.DEVICE.replace("\"protocol\":1", "\"protocol\":2")}}""" }
        newer.session.refreshNow()
        assertTrue(newer.state.connected)
        assertEquals(2, newer.state.device?.protocol)

        val older = Rig(this)
        older.bridge.handlers["device/info"] = { """{"ok":${CaseBridge.DEVICE.replace("\"protocol\":1", "\"protocol\":0")}}""" }
        older.session.refreshNow()
        assertFalse(older.state.connected)
        assertTrue(older.state.error!!.contains("protocol 0"))
        assertFalse(older.bridge.calls.contains("audio/output/get"))
    }

    @Test
    fun theRamLoadedRuntimeIsNotAskedForAudioAndIsKnownAsNotInstalled() = runTest {
        val rig = Rig(this)
        rig.bridge.handlers["device/info"] = { """{"ok":$RAM_DEVICE}""" }
        rig.bridge.handlers["system/slots"] = { """{"ok":$NO_SLOTS}""" }
        rig.session.refreshNow()
        assertTrue(rig.state.connected)
        assertTrue(rig.state.ramOnly)
        assertFalse(rig.state.keepsSettings)
        assertNull(rig.state.output)
        assertNull(rig.state.mic)
        assertTrue(rig.state.eqSlots.all { it == null })
        assertTrue(listOf("audio/output/get", "audio/mic/get", "audio/eq/get").none { it in rig.bridge.calls })
        assertEquals(listOf(KeymapEntry(0, 57, KeyAction.Key(41))), rig.state.keymap)
    }

    @Test
    fun anInstalledCaseIsNotRamOnly() = runTest {
        val rig = connected()
        assertFalse(rig.state.ramOnly)
        assertTrue(rig.state.keepsSettings)
    }

    @Test
    fun developerResultsAndRefusalsStayOnTheDeveloperScreen() = runTest {
        val rig = connected()
        rig.bridge.handlers["dev/i2c"] = { """{"ok":{"Ok":{"acked":true,"data":[3,65]}}}""" }
        rig.session.i2c(0, 0x33, listOf(0x01), 2)
        advanceUntilIdle()
        assertEquals(DeveloperResult.I2c(0, 0x33, I2cReply(true, listOf(3, 65))), rig.state.developer)
        rig.bridge.handlers["dev/poke"] = { """{"ok":{"Err":"WrongState"}}""" }
        rig.session.poke(0x2011_0120, 0xA5, 0xFF)
        advanceUntilIdle()
        assertEquals(DeveloperResult.Refused("dev/poke", CaseError.WrongState, "WrongState"), rig.state.developer)
        assertNull("a refusal is not a connection error", rig.state.error)
        rig.bridge.handlers["dev/audio"] = { """{"ok":${Contract.json.getJSONObject("audio_bench_running")}}""" }
        rig.session.audioBench(AudioBenchRequest.Status)
        advanceUntilIdle()
        assertEquals(AudioBenchState.Running, (rig.state.developer as DeveloperResult.Bench).bench.state)
        rig.bridge.handlers["dev/i2c"] = { """{"err":{"kind":"NoReply"}}""" }
        rig.session.i2c(0, 0x31, emptyList(), 0)
        advanceUntilIdle()
        assertEquals("NoReply", rig.state.error)
    }

    @Test
    fun diagnosticsCanBeReadAgain() = runTest {
        val rig = connected()
        rig.bridge.handlers["system/diagnostics"] = { """{"ok":${Contract.json.getJSONObject("diagnostics")}}""" }
        rig.session.refreshDiagnostics()
        advanceUntilIdle()
        assertEquals(CrashReason.Trap, rig.state.diagnostics?.lastCrash?.reason)
        assertEquals(7L, rig.state.diagnostics?.bootCount)
    }

    @Test
    fun aRunningFlowKeepsThePollerAwayFromTheCase() = runTest {
        val rig = Rig(this)
        rig.busy = true
        rig.session.refreshNow()
        assertEquals(CaseKind.KUMI, rig.session.kind.value)
        assertEquals(0, rig.port.kumiOpens)
    }

    @Test
    fun aRefusedOutputChangeShowsWhatTheCaseKept() = runTest {
        val rig = connected()
        rig.bridge.handlers["audio/output/set"] = { """{"ok":{"Err":"Busy"}}""" }
        rig.bridge.handlers["audio/output/get"] = { """{"ok":{"filter":"NonOversampling","volume_limit_centi_db":-600,"balance_centi_db":0,"dsd_direct":false}}""" }
        val wanted = OutputSettings(OutputFilter.SlowLowLatency, -100, 0, true)
        rig.session.setOutput(wanted)
        assertEquals(wanted, rig.state.output)
        advanceUntilIdle()
        assertEquals(OutputFilter.NonOversampling, rig.state.output?.filter)
        assertEquals("Busy", rig.state.error)
    }

    @Test
    fun whenTheCaseCannotBeReReadThePreviousValueReturns() = runTest {
        val rig = connected()
        val before = rig.state.mic
        rig.bridge.handlers["audio/mic/set"] = { """{"err":{"kind":"NoReply"}}""" }
        rig.bridge.handlers["audio/mic/get"] = { """{"err":{"kind":"NoReply"}}""" }
        rig.session.setMic(MicSettings(2_400))
        advanceUntilIdle()
        assertEquals(before, rig.state.mic)
        assertEquals("NoReply", rig.state.error)
    }

    @Test
    fun aRefusedEqEditFallsBackToTheSlotOnTheCase() = runTest {
        val rig = connected()
        rig.bridge.handlers["audio/eq/set"] = { """{"ok":{"Err":"InvalidArgument"}}""" }
        val edited = EqProfile(-600, listOf(ParametricBand(BandKind.Peak, 2_000, 1_000, 600)))
        rig.session.editEq(0, edited)
        rig.session.editEq(0, edited.copy(preampCentiDb = -700))
        assertEquals(-700, rig.state.eqSlots[0]?.preampCentiDb)
        advanceUntilIdle()
        assertEquals(1, rig.bridge.calls.count { it == "audio/eq/set" })
        assertEquals(-300, rig.state.eqSlots[0]?.preampCentiDb)
    }

    @Test
    fun editsToDifferentEqSlotsAreAllSent() = runTest {
        val rig = connected()
        rig.session.editEq(0, EqProfile.FLAT)
        rig.session.editEq(1, EqProfile.FLAT)
        advanceUntilIdle()
        assertEquals(2, rig.bridge.calls.count { it == "audio/eq/set" })
        assertNull(rig.state.error)
    }

    @Test
    fun aRefusedKeyChangeReloadsThatLayer() = runTest {
        val rig = connected()
        assertEquals(1, rig.state.keymap.size)
        rig.bridge.handlers["keys/map/set"] = { """{"ok":{"Err":"StorageFull"}}""" }
        rig.session.setKey(KeymapEntry(0, 0x04, KeyAction.Disabled))
        assertEquals(2, rig.state.keymap.size)
        advanceUntilIdle()
        assertEquals(listOf(KeymapEntry(0, 57, KeyAction.Key(41))), rig.state.keymap)
        assertEquals("StorageFull", rig.state.error)
    }

    @Test
    fun aRefusedSelectionRestoresTheActiveSlot() = runTest {
        val rig = connected()
        rig.bridge.handlers["audio/eq/select"] = { """{"ok":{"Err":"WrongState"}}""" }
        rig.session.selectEq(null)
        advanceUntilIdle()
        assertEquals(0, rig.state.audio?.eqActive)
    }

    @Test
    fun aCaseErrorRecordsItsTypedKindAndClearsWithError() = runTest {
        val rig = connected()
        rig.bridge.handlers["audio/eq/select"] = { """{"ok":{"Err":"Unsupported"}}""" }
        rig.session.selectEq(0)
        advanceUntilIdle()
        assertEquals("Unsupported", rig.state.error)
        assertEquals(CaseError.Unsupported, rig.state.errorKind)
        rig.session.clearError()
        assertNull(rig.state.error)
        assertNull(rig.state.errorKind)
    }

    @Test
    fun aLostLinkDisconnectsButKeepsTheReason() = runTest {
        val rig = connected()
        rig.bridge.handlers["audio/status"] = { """{"err":{"kind":"Link","detail":"send failed"}}""" }
        rig.session.refreshNow()
        assertFalse(rig.state.connected)
        assertNotNull(rig.state.error)
        assertNull(rig.state.device)
    }

    @Test
    fun dspDacExtrasTelemetryImpedanceAndQmkConfigRoundTrip() = runTest {
        val rig = connected()
        assertNotNull(rig.state.dsp)
        assertNotNull(rig.state.dacExtras)
        assertNotNull(rig.state.telemetry)
        assertNotNull(rig.state.keyConfig)

        val nextDsp = DspSettings(CrossfeedMode.Natural, limiter = true, mono = false, invertPhase = true, loudness = true, swapChannels = true)
        rig.session.setDsp(nextDsp)
        advanceUntilIdle()
        assertEquals(nextDsp, rig.state.dsp)

        val nextExtras = DacExtras(highPassFilter = true, deEmphasis = true, lineOut = false, classH = ClassHMode.LowPowerLv, autoUacOnJack = false, stayAwakeWhileActive = true)
        rig.session.setDacExtras(nextExtras)
        advanceUntilIdle()
        assertEquals(nextExtras, rig.state.dacExtras)

        rig.session.measureImpedance()
        advanceUntilIdle()
        assertFalse(rig.state.measuringImpedance)
        assertEquals(49, rig.state.impedance?.leftOhms)
        assertEquals(49, rig.state.impedance?.rightOhms)
        assertEquals(53, rig.state.impedance?.leftAcOhms)

        val nextConfig = KeyConfig(180, 2_500, 40)
        rig.session.setKeyConfig(nextConfig)
        advanceUntilIdle()
        assertEquals(nextConfig, rig.state.keyConfig)

        val macro = MacroProgram(listOf(MacroStep(0x04, 0x02), MacroStep(0x05, 0)))
        rig.session.setMacro(0, macro)
        advanceUntilIdle()
        assertEquals(macro, rig.state.macros[0])

        val combo = KeyCombo(0x0D, 0x0E, KeyAction.Key(0x29))
        rig.session.setCombo(0, combo)
        advanceUntilIdle()
        assertEquals(combo, rig.state.combos[0])
    }

    @Test
    fun pollPreservesErrorBannerUntilClearedOrReplaced() = runTest {
        val rig = connected()
        rig.bridge.handlers["audio/eq/select"] = { """{"ok":{"Err":"Busy"}}""" }
        rig.session.selectEq(0)
        advanceUntilIdle()
        assertEquals("Busy", rig.state.error)
        assertEquals(CaseError.Busy, rig.state.errorKind)
        rig.session.refreshNow()
        assertEquals("Busy", rig.state.error)
        assertEquals(CaseError.Busy, rig.state.errorKind)
        rig.session.clearError()
        assertNull(rig.state.error)
        assertNull(rig.state.errorKind)
    }

    @Test
    fun editEqWithActivateWritesSlotBeforeSelecting() = runTest {
        val rig = connected()
        var slot1Saved = false
        rig.bridge.handlers["audio/eq/set"] = {
            slot1Saved = true
            """{"ok":{"Ok":null}}"""
        }
        rig.bridge.handlers["audio/eq/select"] = {
            if (slot1Saved) """{"ok":{"Ok":null}}""" else """{"ok":{"Err":"WrongState"}}"""
        }
        rig.bridge.calls.clear()
        rig.session.editEq(1, EqProfile.FLAT, immediate = true, activate = true)
        advanceUntilIdle()
        val setIdx = rig.bridge.calls.indexOf("audio/eq/set")
        val selectIdx = rig.bridge.calls.indexOf("audio/eq/select")
        assertTrue("expected set before select: ${rig.bridge.calls}", setIdx in 0 until selectIdx)
        assertNull(rig.state.error)
        assertEquals(EqProfile.FLAT, rig.state.eqSlots[1])
    }

    @Test
    fun usbReenumerationReopensChannelAndReloads() = runTest {
        val rig = connected()
        assertEquals(1, rig.port.kumiOpens)
        rig.port.key = "fake:2"
        rig.session.refreshNow()
        assertEquals(2, rig.port.kumiOpens)
        assertTrue(rig.state.connected)
        assertTrue(rig.port.channels.first().closed)
    }
}
