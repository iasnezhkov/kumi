// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class EqPresetsTest {

    @Test
    fun everyPresetAndExtremeFitsTheCaseFormat() {
        EqPreset.entries.forEach { preset ->
            val bands = EqPresets.bands(EqSetting.of(preset))
            assertEquals(EqPresets.BANDS, bands.size)
            bands.forEach { EqCodec.encodeWrite(it) }
        }
        for (gain in listOf(-12.0, 12.0)) {
            EqPresets.bands(EqSetting(List(EqPresets.BANDS) { gain }, EqPresets.DEFAULT_Q, null)).forEach { EqCodec.encodeWrite(it) }
        }
    }

    @Test
    fun bandsCarryVendorFrequenciesQAndPreamp() {
        val bands = EqPresets.bands(EqSetting.of(EqPreset.BASS))
        assertEquals(listOf(31, 62, 125, 250, 500, 1000, 2000, 4000), bands.map { it.frequencyHz })
        assertEquals(384, bands[0].qRaw)
        assertEquals(1536, bands[0].gainRaw)
        assertTrue(bands.all { it.preamp == -5 && it.filterType == FilterType.PEAK.wire && it.bank == 0 })
    }

    @Test
    fun flatPresetIsUnity() {
        EqPresets.bands(EqSetting.FLAT).forEach { band ->
            val c = band.coefficients
            assertEquals(1 shl 30, c[0])
            assertTrue(Math.abs(c[1] + c[3]) <= 1)
            assertTrue(Math.abs(c[2] + c[4]) <= 1)
        }
    }

    @Test
    fun matchingAndCustomSettings() {
        assertEquals(EqPreset.ROCK, EqPresets.match(EqPreset.ROCK.gains))
        val custom = EqSetting.of(EqPreset.ROCK).withGain(2, 5.3)
        assertNull(custom.preset)
        assertEquals(5.5, custom.gains[2], 0.0)
        assertEquals(-6, custom.preamp)
        assertEquals(12.0, EqPresets.clampGain(30.0), 0.0)
        assertEquals(-12.0, EqPresets.clampGain(-30.0), 0.0)
    }

    @Test
    fun serializeRoundTrip() {
        val setting = EqSetting.of(EqPreset.VOCAL).withGain(7, -3.5)
        assertEquals(setting, EqSetting.parse(setting.serialize()))
        assertNull(EqSetting.parse("1,2,3"))
        assertNull(EqSetting.parse(null))
        assertEquals(EqPreset.JAZZ, EqSetting.parse(EqSetting.of(EqPreset.JAZZ).serialize())?.preset)
        assertEquals(EqPreset.ROCK, EqSetting.parse("3,2,-1,-2,1,2,3,4")?.preset)
    }

    @Test
    fun editingAPresetKeepsItsQ() {
        val dance = EqSetting.of(EqPreset.DANCE)
        val edited = dance.withGain(7, 2.5)
        assertNull(edited.preset)
        assertEquals(EqPreset.DANCE.q, edited.q)
        assertEquals(EqPreset.DANCE, edited.withGain(7, 3.0).preset)
        assertNull(EqPresets.setting(EqPreset.BASS.gains, EqPresets.DEFAULT_Q).preset)
    }

    @Test
    fun readsSettingFromDeviceBands() {
        val case = SnapCase(DemoTransport()) { true }
        val setting = EqPresets.fromDevice(case.readEqBank(0))!!
        assertEquals(listOf(0.0, 0.0, 0.0, 4.0, 4.0, 4.0, 0.0, 2.0), setting.gains)
        assertEquals(0.71, setting.q[0], 0.0)
        EqPresets.bands(EqSetting.of(EqPreset.POP)).forEach { assertEquals(true, case.writeEqBand(it).readBackMatches) }
        assertEquals(true, case.applyEq().readBackMatches)
        assertEquals(EqPreset.POP, EqPresets.fromDevice(case.readEqBank(0))?.preset)
    }
}
