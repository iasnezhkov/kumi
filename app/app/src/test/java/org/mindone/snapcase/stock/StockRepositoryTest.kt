// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.stock

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.data.AppSettings
import org.mindone.snapcase.protocol.DacFilter
import org.mindone.snapcase.protocol.DemoTransport
import org.mindone.snapcase.protocol.EqPreset
import org.mindone.snapcase.protocol.EqPresets
import org.mindone.snapcase.protocol.EqSetting
import org.mindone.snapcase.protocol.Transport
import org.mindone.snapcase.system.HidrawNode
import org.mindone.snapcase.system.TypecInfo
import org.mindone.snapcase.system.UsbDeviceInfo
import org.mindone.snapcase.system.UsbPresence

@OptIn(ExperimentalCoroutinesApi::class)
class StockRepositoryTest {
    private class Platform : StockPlatform {
        val transport = DemoTransport()
        override fun findNode() = HidrawNode("hidraw0", "/dev/hidraw0", null, null)
        override fun usbInfo(): UsbDeviceInfo? = null
        override fun typec(): List<TypecInfo> = emptyList()
        override fun presence() = UsbPresence(normal = true)
        override fun open(node: HidrawNode): Transport = transport
    }

    private class Store : SoundStore {
        var eq: EqSetting? = null
        var filter: Int? = null
        var mic: Double? = null
        override suspend fun saveEq(value: EqSetting) {
            eq = value
        }
        override suspend fun saveDacFilter(wire: Int) {
            filter = wire
        }
        override suspend fun saveMicGain(db: Double) {
            mic = db
        }
        override suspend fun setExperimentalWrites(value: Boolean) = Unit
    }

    private class Rig(scope: TestScope) {
        val platform = Platform()
        val store = Store()
        val messages = mutableListOf<UserMessage>()
        private val dispatcher = StandardTestDispatcher(scope.testScheduler)
        private val owner = CoroutineScope(dispatcher + SupervisorJob())
        val stock = StockRepository(
            owner, dispatcher, dispatcher, platform, store,
            MutableStateFlow(AppSettings(experimentalWrites = true, restoreOnConnect = false)),
        )
        val sound: StockSound get() = stock.sound.value

        init {
            owner.launch { stock.messages.collect { messages += it } }
        }
    }

    private suspend fun TestScope.connected(): Rig {
        val rig = Rig(this)
        rig.stock.reconnect()
        advanceUntilIdle()
        assertEquals("V0.1", rig.stock.connection.value.version)
        assertTrue(rig.sound.loaded)
        return rig
    }

    @Test
    fun theTechnicalAndTheHomeDacFilterAreOneState() = runTest {
        val rig = connected()
        rig.stock.setDacFilter(DacFilter.SLOW_LOW_LATENCY)
        advanceUntilIdle()
        assertEquals(DacFilter.SLOW_LOW_LATENCY, rig.sound.dacFilter)
        assertTrue(rig.sound.lastWrite!!.startsWith("DAC filter ack=true readBack=true"))
        assertNull("the technical screen does not change the saved sound settings", rig.store.filter)
        rig.stock.chooseDacFilter(DacFilter.NON_OVERSAMPLING)
        advanceUntilIdle()
        assertEquals(DacFilter.NON_OVERSAMPLING.wire, rig.sound.audio?.dacFilterRaw)
        assertEquals(DacFilter.NON_OVERSAMPLING.wire, rig.store.filter)
    }

    @Test
    fun aRawEqWriteToBankZeroShowsOnTheHomeEqualizer() = runTest {
        val rig = connected()
        val band = rig.sound.bands.first { it.band == 3 }
        val louder = rig.stock.buildBand(band, band.frequencyHz, band.q, 6.0, band.filterType, band.preamp)
        rig.stock.writeBand(louder)
        advanceUntilIdle()
        assertEquals(6.0, rig.sound.eq!!.gains[3], 0.01)
        rig.stock.applyPreset(EqPreset.ROCK)
        advanceUntilIdle()
        assertEquals(EqPreset.ROCK, rig.sound.eq?.preset)
        assertEquals(EqPreset.ROCK.gains[0], rig.sound.bands.first { it.band == 0 }.gainDb, 0.01)
        assertEquals(EqPreset.ROCK, rig.store.eq?.preset)
    }

    @Test
    fun theMicrophoneGainIsSharedBetweenScreens() = runTest {
        val rig = connected()
        rig.stock.chooseMicGain(3.0)
        advanceUntilIdle()
        assertEquals(3.0, rig.sound.micGainDb!!, 0.01)
        rig.stock.setRecordGain(-1.5)
        advanceUntilIdle()
        assertEquals(-1.5 * 256, rig.sound.recordGainRaw!!.toDouble(), 0.5)
    }

    @Test
    fun aWriteTheCaseMissesRollsBackAndSaysSo() = runTest {
        val rig = connected()
        val before = rig.sound.eq
        rig.platform.transport.asleep = true
        rig.stock.setEqGain(2, 9.0)
        assertEquals(9.0, rig.sound.eq!!.gains[2], 0.01)
        advanceUntilIdle()
        assertEquals(before, rig.sound.eq)
        assertNull(rig.store.eq)
        assertEquals(UserMessage.CASE_NOT_ANSWERING, rig.messages.first())
    }

    @Test
    fun selectingABankReadsItWithoutLosingBankZero() = runTest {
        val rig = connected()
        val home = rig.sound.eq
        rig.stock.selectBank(2)
        advanceUntilIdle()
        assertEquals(2, rig.sound.bank)
        assertTrue(rig.sound.bands.all { it.bank == 2 })
        assertEquals(EqPresets.BANDS, rig.sound.bands.size)
        assertEquals(home, rig.sound.eq)
    }
}
