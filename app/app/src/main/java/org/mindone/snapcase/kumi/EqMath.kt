// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.util.concurrent.ConcurrentHashMap
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.log10
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.math.tan
import org.json.JSONArray
import org.json.JSONObject

data class EqCurve(val frequenciesHz: List<Double>, val gainDb: List<Double>) {
    val points: List<Pair<Double, Double>> get() = frequenciesHz.zip(gainDb)

    companion object {
        val EMPTY = EqCurve(emptyList(), emptyList())
    }
}

interface EqMath {
    fun curve(profile: EqProfile, points: Int = CURVE_POINTS, lowHz: Double = DISPLAY_LOW_HZ, highHz: Double = DISPLAY_HIGH_HZ): EqCurve

    fun autoPreampCentiDb(profile: EqProfile): Int?

    fun withAutoPreamp(profile: EqProfile): EqProfile = autoPreampCentiDb(profile)?.let { profile.copy(preampCentiDb = it) } ?: profile

    companion object {
        const val CURVE_POINTS = 96
        const val DISPLAY_LOW_HZ = 20.0
        const val DISPLAY_HIGH_HZ = 20_000.0
        const val SAMPLE_RATE_HZ = 48_000
    }
}

class NativeEqMath(private val bridge: KumiBridge, private val sampleRateHz: Int = EqMath.SAMPLE_RATE_HZ) : EqMath {
    private val curveCache = ConcurrentHashMap<EqProfile, EqCurve>()

    private fun reply(call: () -> String): JSONObject? =
        if (!bridge.available) null else runCatching { Replies.parse(call()).value as? JSONObject }.getOrNull()

    override fun curve(profile: EqProfile, points: Int, lowHz: Double, highHz: Double): EqCurve {
        val useCache = points == EqMath.CURVE_POINTS && lowHz == EqMath.DISPLAY_LOW_HZ && highHz == EqMath.DISPLAY_HIGH_HZ
        if (useCache) curveCache[profile]?.let { return it }
        val count = points.coerceIn(2, KumiLimits.EQ_RESPONSE_MAX_POINTS)
        val value = reply { bridge.eqResponse(profile.toJson().toString(), sampleRateHz, count, lowHz, highHz) } ?: return EqCurve.EMPTY
        val result = runCatching { EqCurve(value.getJSONArray("frequencies_hz").doubles(), value.getJSONArray("gain_db").doubles()) }
            .getOrNull()?.takeIf { it.frequenciesHz.size == it.gainDb.size } ?: EqCurve.EMPTY
        if (useCache && result.frequenciesHz.isNotEmpty()) {
            if (curveCache.size >= 32) curveCache.clear()
            curveCache[profile] = result
        }
        return result
    }

    override fun autoPreampCentiDb(profile: EqProfile): Int? =
        reply { bridge.eqAutoPreamp(profile.toJson().toString(), sampleRateHz) }?.let { if (it.has("preamp_centi_db")) it.getInt("preamp_centi_db") else null }

    private fun JSONArray.doubles(): List<Double> = (0 until length()).map(::getDouble)
}

object FastEqCurve {
    const val POINTS = EqMath.CURVE_POINTS
    private const val SAMPLE_RATE = EqMath.SAMPLE_RATE_HZ.toDouble()
    private const val LOW_HZ = EqMath.DISPLAY_LOW_HZ
    private const val HIGH_HZ = EqMath.DISPLAY_HIGH_HZ
    private const val NYQUIST_LIMIT = SAMPLE_RATE * 0.49

    val frequenciesHz = DoubleArray(POINTS) { i ->
        LOW_HZ * (HIGH_HZ / LOW_HZ).pow(i.toDouble() / (POINTS - 1).toDouble())
    }

    private val cos1 = DoubleArray(POINTS) { i -> cos(2.0 * PI * frequenciesHz[i] / SAMPLE_RATE) }
    private val cos2 = DoubleArray(POINTS) { i -> cos(4.0 * PI * frequenciesHz[i] / SAMPLE_RATE) }
    private val sin1 = DoubleArray(POINTS) { i -> sin(2.0 * PI * frequenciesHz[i] / SAMPLE_RATE) }
    private val sin2 = DoubleArray(POINTS) { i -> sin(4.0 * PI * frequenciesHz[i] / SAMPLE_RATE) }

    private class Coeffs(
        val b0: Double,
        val b1: Double,
        val b2: Double,
        val a0: Double,
        val a1: Double,
        val a2: Double,
    )

    private fun design(band: ParametricBand): Coeffs? {
        val f0 = band.frequencyHz.toDouble()
        val q = band.qMilli / 1000.0
        val gainDb = band.gainCentiDb / 100.0
        if (f0 <= 0.0 || f0 >= NYQUIST_LIMIT || q < 0.1 || q > 20.0 || kotlin.math.abs(gainDb) > 18.0) return null
        if (band.gainCentiDb == 0 && (band.kind == BandKind.Peak || band.kind == BandKind.LowShelf || band.kind == BandKind.HighShelf)) {
            return null
        }
        val a = 10.0.pow(gainDb / 40.0)
        val warped = tan(PI * f0 / SAMPLE_RATE)
        val gRaw: Double
        val kRaw: Double
        val m0Raw: Double
        val m1Raw: Double
        val m2Raw: Double
        when (band.kind) {
            BandKind.LowPass -> {
                gRaw = warped; kRaw = 1.0 / q; m0Raw = 0.0; m1Raw = 0.0; m2Raw = 1.0
            }
            BandKind.HighPass -> {
                gRaw = warped; kRaw = 1.0 / q; m0Raw = 1.0; m1Raw = -1.0 / q; m2Raw = -1.0
            }
            BandKind.Peak -> {
                val k = 1.0 / (q * a)
                gRaw = warped; kRaw = k; m0Raw = 1.0; m1Raw = k * (a * a - 1.0); m2Raw = 0.0
            }
            BandKind.LowShelf -> {
                gRaw = warped / sqrt(a); kRaw = 1.0 / q; m0Raw = 1.0; m1Raw = (a - 1.0) / q; m2Raw = a * a - 1.0
            }
            BandKind.HighShelf -> {
                gRaw = warped * sqrt(a); kRaw = 1.0 / q; m0Raw = a * a; m1Raw = (1.0 - a) * a / q; m2Raw = 1.0 - a * a
            }
        }
        val g = gRaw.toFloat().toDouble()
        val k = kRaw.toFloat().toDouble()
        val m0 = m0Raw.toFloat().toDouble()
        val m1 = m1Raw.toFloat().toDouble()
        val m2 = m2Raw.toFloat().toDouble()
        if (g <= 0.0 || k <= 0.0 || !g.isFinite() || !k.isFinite()) return null
        val gg = g * g
        val n2 = m0
        val n1 = m0 * k + m1
        val n0 = m0 + m2
        return Coeffs(
            b0 = n2 + n1 * g + n0 * gg,
            b1 = 2.0 * (n0 * gg - n2),
            b2 = n2 - n1 * g + n0 * gg,
            a0 = 1.0 + k * g + gg,
            a1 = 2.0 * (gg - 1.0),
            a2 = 1.0 - k * g + gg,
        )
    }

    private fun sectionPower(c: Coeffs, c1: Double, c2: Double, s1: Double, s2: Double): Double {
        val bRe = c.b0 + c.b1 * c1 + c.b2 * c2
        val bIm = c.b1 * s1 + c.b2 * s2
        val aRe = c.a0 + c.a1 * c1 + c.a2 * c2
        val aIm = c.a1 * s1 + c.a2 * s2
        return (bRe * bRe + bIm * bIm) / (aRe * aRe + aIm * aIm)
    }

    fun evaluate(profile: EqProfile, outDb: FloatArray) {
        evaluateComposite(profile, null, null, outDb)
    }

    fun evaluateComposite(
        profile: EqProfile,
        base: EqBaseSettings?,
        tone: ToneEqSettings?,
        outDb: FloatArray,
    ) {
        val baseActive = base != null && base.enabled && base.profile.bands.isNotEmpty()
        val toneBands = tone?.toBands().orEmpty()
        val preampDb = (
            profile.preampCentiDb +
                (if (baseActive) base.profile.preampCentiDb else 0) +
                (tone?.autoPreampCentiDb() ?: 0)
            ) / 100.0
        val sections = ArrayList<Coeffs>(profile.bands.size + (if (baseActive) base.profile.bands.size else 0) + toneBands.size)
        if (baseActive) {
            for (band in base.profile.bands) {
                design(band)?.let(sections::add)
            }
        }
        for (band in profile.bands) {
            design(band)?.let(sections::add)
        }
        for (band in toneBands) {
            design(band)?.let(sections::add)
        }
        if (sections.isEmpty()) {
            val flat = preampDb.toFloat()
            for (i in 0 until POINTS) outDb[i] = flat
            return
        }
        val count = sections.size
        for (i in 0 until POINTS) {
            val c1 = cos1[i]
            val c2 = cos2[i]
            val s1 = sin1[i]
            val s2 = sin2[i]
            var power = 1.0
            for (j in 0 until count) {
                power *= sectionPower(sections[j], c1, c2, s1, s2)
            }
            val db = preampDb + 10.0 * log10(if (power.isFinite()) power.coerceAtLeast(1e-12) else 1e-12)
            outDb[i] = (if (db.isFinite()) db else -120.0).toFloat()
        }
    }

    fun evaluateBand(band: ParametricBand, outDb: FloatArray) {
        val section = design(band)
        if (section == null) {
            for (i in 0 until POINTS) outDb[i] = 0f
            return
        }
        for (i in 0 until POINTS) {
            val power = sectionPower(section, cos1[i], cos2[i], sin1[i], sin2[i])
            val db = 10.0 * log10(if (power.isFinite()) power.coerceAtLeast(1e-12) else 1e-12)
            outDb[i] = (if (db.isFinite()) db else -120.0).toFloat()
        }
    }

    fun evaluateAt(profile: EqProfile, frequencyHz: Double): Double {
        val w = 2.0 * PI * frequencyHz / SAMPLE_RATE
        val c1 = cos(w)
        val c2 = cos(2.0 * w)
        val s1 = sin(w)
        val s2 = sin(2.0 * w)
        var power = 1.0
        for (band in profile.bands) {
            val s = design(band) ?: continue
            power *= sectionPower(s, c1, c2, s1, s2)
        }
        val db = profile.preampCentiDb / 100.0 + 10.0 * log10(if (power.isFinite()) power.coerceAtLeast(1e-12) else 1e-12)
        return if (db.isFinite()) db else -120.0
    }

    fun peakDb(profile: EqProfile): Double {
        val buf = FloatArray(POINTS)
        evaluate(profile, buf)
        var best = -120.0
        for (v in buf) {
            if (v > best) best = v.toDouble()
        }
        return best
    }
}
