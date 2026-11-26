// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import java.util.Locale
import kotlin.math.abs
import kotlin.math.ceil

enum class EqPreset(val gains: List<Double>, val q: List<Double>, val preamp: Int) {
    FLAT(listOf(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), EqPresets.DEFAULT_Q, 0),
    ROCK(listOf(3.0, 2.0, -1.0, -2.0, 1.0, 2.0, 3.0, 4.0), EqPresets.DEFAULT_Q, 0),
    POP(listOf(1.0, 2.0, 3.0, 2.0, 0.0, -1.0, -1.0, 1.0), EqPresets.DEFAULT_Q, -3),
    JAZZ(listOf(2.0, 1.0, 0.0, 1.0, 2.0, 2.0, 1.0, 0.0), EqPresets.DEFAULT_Q, -4),
    CLASSICAL(listOf(3.0, 2.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0), EqPresets.DEFAULT_Q, -5),
    BASS(listOf(6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0), listOf(1.5, 1.2, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0), -5),
    VOCAL(listOf(-2.0, -1.0, 0.0, 2.0, 4.0, 4.0, 2.0, 1.0), listOf(1.0, 1.0, 1.0, 1.2, 1.5, 1.5, 1.2, 1.0), -5),
    ELECTRONIC(listOf(4.0, 3.0, 1.0, 0.0, -1.0, 1.0, 2.0, 3.0), EqPresets.DEFAULT_Q, -5),
    DANCE(listOf(5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 2.0, 3.0), listOf(1.5, 1.3, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0), -5),
}

data class EqSetting(val gains: List<Double>, val q: List<Double>, val preset: EqPreset?) {
    val preamp: Int get() = preset?.preamp ?: EqPresets.autoPreamp(gains)

    fun withGain(band: Int, gain: Double): EqSetting {
        val next = gains.toMutableList().also { it[band] = EqPresets.clampGain(gain) }
        return EqPresets.setting(next, q)
    }

    fun serialize(): String =
        gains.joinToString(",") { "%.1f".format(Locale.ROOT, it) } + ";" + q.joinToString(",") { "%.2f".format(Locale.ROOT, it) }

    companion object {
        val FLAT = of(EqPreset.FLAT)

        fun of(preset: EqPreset) = EqSetting(preset.gains, preset.q, preset)

        fun parse(text: String?): EqSetting? {
            val parts = text?.split(';') ?: return null
            val gains = numbers(parts[0])?.map(EqPresets::clampGain) ?: return null
            val q = parts.getOrNull(1)?.let(::numbers)?.map { it.coerceIn(EqPresets.MIN_Q, EqPresets.MAX_Q) } ?: EqPresets.DEFAULT_Q
            return EqPresets.setting(gains, q)
        }

        private fun numbers(text: String): List<Double>? {
            val values = text.split(',').mapNotNull { it.trim().toDoubleOrNull() }
            return values.takeIf { it.size == EqPresets.BANDS }
        }
    }
}

object EqPresets {
    const val BANDS = 8
    const val BANK = 0
    const val MAX_GAIN_DB = 12.0
    const val STEP_DB = 0.5
    val FREQUENCIES = listOf(31, 62, 125, 250, 500, 1000, 2000, 4000)
    val DEFAULT_Q = listOf(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0)
    const val MIN_Q = 0.3
    const val MAX_Q = 4.0

    fun setting(gains: List<Double>, q: List<Double>): EqSetting =
        EqSetting(gains, q, match(gains)?.takeIf { preset -> preset.q.zip(q).all { (a, b) -> abs(a - b) < 0.01 } })

    fun clampGain(gain: Double): Double =
        (Math.round(gain / STEP_DB) * STEP_DB).coerceIn(-MAX_GAIN_DB, MAX_GAIN_DB)

    fun match(gains: List<Double>): EqPreset? =
        EqPreset.entries.firstOrNull { preset -> preset.gains.zip(gains).all { (a, b) -> abs(a - b) < 0.01 } }

    fun autoPreamp(gains: List<Double>): Int = -ceil(gains.maxOrNull()?.coerceAtLeast(0.0) ?: 0.0).toInt()

    fun fromDevice(bands: List<EqBand>): EqSetting? {
        if (bands.size != BANDS) return null
        val sorted = bands.sortedBy { it.band }
        val gains = sorted.map { clampGain(it.gainDb) }
        val q = sorted.map { Math.round(it.q * 100) / 100.0 }.map { if (it in MIN_Q..MAX_Q) it else 1.0 }
        return setting(gains, q)
    }

    fun bands(setting: EqSetting, sampleRate: Int = BiquadDesigner.DEFAULT_SAMPLE_RATE): List<EqBand> =
        (0 until BANDS).map { band(setting, it, sampleRate) }

    fun band(setting: EqSetting, index: Int, sampleRate: Int = BiquadDesigner.DEFAULT_SAMPLE_RATE): EqBand {
        val gain = setting.gains[index]
        val q = setting.q[index]
        return EqBand(
            bank = BANK,
            band = index,
            coefficients = BiquadDesigner.coefficients(FilterType.PEAK, FREQUENCIES[index].toDouble(), q, gain, sampleRate),
            frequencyHz = FREQUENCIES[index],
            qRaw = Math.round(q * 256).toInt(),
            gainRaw = Math.round(gain * 256).toInt(),
            filterType = FilterType.PEAK.wire,
            preamp = setting.preamp,
        )
    }
}
