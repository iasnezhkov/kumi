// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class NativeEqMathTest {
    private val replies = JSONObject(File(System.getProperty("kumi.contract") ?: "../../kumi/crates/kumi-jni/tests/fixtures/contract.json").readText())
        .getJSONObject("bridge_replies")
    private val profile = EqProfile(-600, listOf(ParametricBand(BandKind.Peak, 1_000, 2_000, 600)))

    private class DspBridge(private val response: String, private val preamp: String, override val available: Boolean = true) : FakeBridge() {
        val requests = mutableListOf<List<Any>>()

        override fun eqResponse(profile: String, sampleRate: Int, points: Int, lowHz: Double, highHz: Double): String {
            requests += listOf(profile, sampleRate, points, lowHz, highHz)
            return response
        }

        override fun eqAutoPreamp(profile: String, sampleRate: Int): String {
            requests += listOf(profile, sampleRate)
            return preamp
        }
    }

    @Test
    fun theCurveAndThePreampComeFromTheFirmwareDsp() {
        val bridge = DspBridge(replies.getJSONObject("eq_response").toString(), replies.getJSONObject("eq_auto_preamp").toString())
        val math = NativeEqMath(bridge)
        val curve = math.curve(profile, points = 5)
        assertEquals(5, curve.points.size)
        assertEquals(20.0, curve.frequenciesHz.first(), 1e-9)
        assertEquals(20_000.0, curve.frequenciesHz.last(), 1e-9)
        assertTrue("the curve includes the -6 dB preamp", curve.gainDb.all { it < -4.0 })
        assertEquals(listOf(profile.toJson().toString(), EqMath.SAMPLE_RATE_HZ, 5, EqMath.DISPLAY_LOW_HZ, EqMath.DISPLAY_HIGH_HZ), bridge.requests[0])
        assertEquals(-600, math.autoPreampCentiDb(profile))
        assertEquals(-600, math.withAutoPreamp(profile.copy(preampCentiDb = 0)).preampCentiDb)
        assertEquals(listOf(profile.toJson().toString(), EqMath.SAMPLE_RATE_HZ), bridge.requests[1])
    }

    @Test
    fun pointCountsStayInsideTheBridgeRange() {
        val bridge = DspBridge(replies.getJSONObject("eq_response").toString(), "{}")
        NativeEqMath(bridge).curve(profile, points = 1)
        NativeEqMath(bridge).curve(profile, points = 1_000_000)
        assertEquals(listOf(2, KumiLimits.EQ_RESPONSE_MAX_POINTS), bridge.requests.map { it[2] })
    }

    @Test
    fun aRefusedProfileOrAMissingLibraryLeavesThingsAsTheyAre() {
        val refused = replies.getJSONObject("eq_invalid_profile").toString()
        val math = NativeEqMath(DspBridge(refused, refused))
        assertSame(EqCurve.EMPTY, math.curve(profile))
        assertNull(math.autoPreampCentiDb(profile))
        assertEquals(profile, math.withAutoPreamp(profile))
        val missing = DspBridge(replies.getJSONObject("eq_response").toString(), replies.getJSONObject("eq_auto_preamp").toString(), available = false)
        assertSame(EqCurve.EMPTY, NativeEqMath(missing).curve(profile))
        assertNull(NativeEqMath(missing).autoPreampCentiDb(profile))
        assertTrue(missing.requests.isEmpty())
    }

    @Test
    fun fastEqCurveMatchesFirmwareDspFixtureAndCoversAllBandKinds() {
        val bridge = DspBridge(replies.getJSONObject("eq_response").toString(), replies.getJSONObject("eq_auto_preamp").toString())
        val ref = NativeEqMath(bridge).curve(profile, points = 5)
        ref.points.forEach { (hz, expectedDb) ->
            val actualDb = FastEqCurve.evaluateAt(profile, hz)
            assertEquals("mismatch at $hz Hz", expectedDb, actualDb, 0.02)
        }

        val composite = FloatArray(FastEqCurve.POINTS)
        FastEqCurve.evaluate(EqProfile.FLAT, composite)
        assertTrue(composite.all { kotlin.math.abs(it) < 1e-4f })

        val multi = EqProfile(
            preampCentiDb = -300,
            bands = listOf(
                ParametricBand(BandKind.LowShelf, 100, 710, 600),
                ParametricBand(BandKind.Peak, 1_000, 2_000, 600),
                ParametricBand(BandKind.HighShelf, 8_000, 710, -400),
                ParametricBand(BandKind.LowPass, 15_000, 707, 0),
                ParametricBand(BandKind.HighPass, 40, 707, 0),
            ),
        )
        FastEqCurve.evaluate(multi, composite)
        assertTrue(composite.all { it.isFinite() })
        assertTrue(FastEqCurve.peakDb(multi) > 2.0)

        val single = FloatArray(FastEqCurve.POINTS)
        FastEqCurve.evaluateBand(ParametricBand(BandKind.Peak, 1_000, 1_000, 600), single)
        assertTrue(single.max() > 5.5f)
        assertTrue(kotlin.math.abs(single.first()) < 0.2f)
    }
}
