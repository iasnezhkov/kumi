// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

enum class FilterType(val wire: Int) {
    DISABLED(0),
    LOW_SHELF(1),
    PEAK(2),
    HIGH_SHELF(3),
    LOW_PASS(4),
    HIGH_PASS(5);

    companion object {
        fun fromWire(value: Int): FilterType? = entries.firstOrNull { it.wire == value }
    }
}

data class EqBand(
    val bank: Int,
    val band: Int,
    val coefficients: List<Int>,
    val frequencyHz: Int,
    val qRaw: Int,
    val gainRaw: Int,
    val filterType: Int,
    val preamp: Int,
    val trailer: Int = 0,
) {
    val q: Double get() = qRaw / 256.0
    val gainDb: Double get() = gainRaw / 256.0

    fun sameFilterAs(other: EqBand): Boolean =
        frequencyHz == other.frequencyHz && qRaw == other.qRaw && gainRaw == other.gainRaw &&
            filterType == other.filterType

    fun sameSettingsAs(other: EqBand): Boolean =
        bank == other.bank && band == other.band && coefficients == other.coefficients &&
            sameFilterAs(other) && preamp == other.preamp
}

object EqCodec {
    const val BANKS = 9
    const val BANDS = 8
    const val WRITE_LENGTH = 33
    const val REPLY_MIN_SIZE = 37
    const val MAX_FILTER_TYPE = 7
    val APPLY_PAYLOAD = byteArrayOf(0x00, 0x00, 0xFF.toByte(), 0xFF.toByte())

    fun encodeRead(bank: Int, band: Int): ByteArray {
        checkSlot(bank, band)
        return SnapProtocol.encodeGet(SnapProtocol.CMD_EQ, header(bank, band))
    }

    fun decodeReply(frame: ByteArray): EqBand {
        val reply = SnapProtocol.decodeReply(frame)
        SnapProtocol.expect(reply, SnapProtocol.GROUP_GET, SnapProtocol.CMD_EQ)
        if (frame.size < REPLY_MIN_SIZE) throw ProtocolException("Incomplete EQ reply")
        return EqBand(
            bank = frame.u8(4),
            band = frame.u16le(5),
            coefficients = List(5) { frame.s32le(8 + 4 * it) },
            frequencyHz = frame.u16le(28),
            qRaw = frame.u16le(30),
            gainRaw = frame.s16le(32),
            filterType = frame.u8(34),
            preamp = frame[35].toInt(),
            trailer = frame.u8(36),
        )
    }

    fun encodeWrite(band: EqBand): ByteArray {
        checkSlot(band.bank, band.band)
        if (band.coefficients.size != 5) throw ProtocolException("Five coefficients are required")
        if (band.frequencyHz !in 1..0xFFFF) throw ProtocolException("Frequency out of range")
        if (band.qRaw !in 0..0xFFFF) throw ProtocolException("Q out of range")
        if (band.gainRaw !in Short.MIN_VALUE..Short.MAX_VALUE) throw ProtocolException("Gain out of range")
        if (band.filterType !in 0..MAX_FILTER_TYPE) throw ProtocolException("Filter type out of range")
        if (band.preamp !in Byte.MIN_VALUE..Byte.MAX_VALUE) throw ProtocolException("Preamp out of range")
        val data = ByteArray(WRITE_LENGTH)
        header(band.bank, band.band).copyInto(data, 0)
        band.coefficients.forEachIndexed { index, value -> data.putU32le(4 + 4 * index, value.toLong()) }
        data.putU16le(24, band.frequencyHz)
        data.putU16le(26, band.qRaw)
        data.putU16le(28, band.gainRaw)
        data[30] = band.filterType.toByte()
        data[31] = band.preamp.toByte()
        data[32] = 0
        return SnapProtocol.encodeSet(SnapProtocol.CMD_EQ, data)
    }

    fun encodeApply(): ByteArray = SnapProtocol.encodeSet(SnapProtocol.CMD_EQ_APPLY, APPLY_PAYLOAD)

    fun checkSlot(bank: Int, band: Int) {
        if (bank !in 0 until BANKS) throw ProtocolException("Bank must be 0..${BANKS - 1}")
        if (band !in 0 until BANDS) throw ProtocolException("Band must be 0..${BANDS - 1}")
    }

    private fun header(bank: Int, band: Int) = byteArrayOf(bank.toByte(), band.toByte(), (band shr 8).toByte(), 0)
}

object BiquadDesigner {
    const val DEFAULT_SAMPLE_RATE = 96_000
    private const val SCALE = 1073741824.0

    fun coefficients(type: FilterType, frequencyHz: Double, q: Double, gainDb: Double, sampleRate: Int): List<Int> {
        if (sampleRate <= 0) throw ProtocolException("Sample rate must be positive")
        if (frequencyHz <= 0.0 || frequencyHz >= sampleRate / 2.0) throw ProtocolException("Frequency must be below Nyquist")
        if (q <= 0.0) throw ProtocolException("Q must be positive")
        val w0 = 2.0 * PI * frequencyHz / sampleRate
        val cosW = cos(w0)
        val alpha = sin(w0) / (2.0 * q)
        val a = 10.0.pow(gainDb / 40.0)
        val sqrtA2Alpha = 2.0 * sqrt(a) * alpha
        val raw: DoubleArray = when (type) {
            FilterType.PEAK -> doubleArrayOf(
                1 + alpha * a, -2 * cosW, 1 - alpha * a,
                1 + alpha / a, -2 * cosW, 1 - alpha / a,
            )
            FilterType.LOW_SHELF -> doubleArrayOf(
                a * ((a + 1) - (a - 1) * cosW + sqrtA2Alpha),
                2 * a * ((a - 1) - (a + 1) * cosW),
                a * ((a + 1) - (a - 1) * cosW - sqrtA2Alpha),
                (a + 1) + (a - 1) * cosW + sqrtA2Alpha,
                -2 * ((a - 1) + (a + 1) * cosW),
                (a + 1) + (a - 1) * cosW - sqrtA2Alpha,
            )
            FilterType.HIGH_SHELF -> doubleArrayOf(
                a * ((a + 1) + (a - 1) * cosW + sqrtA2Alpha),
                -2 * a * ((a - 1) + (a + 1) * cosW),
                a * ((a + 1) + (a - 1) * cosW - sqrtA2Alpha),
                (a + 1) - (a - 1) * cosW + sqrtA2Alpha,
                2 * ((a - 1) - (a + 1) * cosW),
                (a + 1) - (a - 1) * cosW - sqrtA2Alpha,
            )
            FilterType.LOW_PASS -> doubleArrayOf(
                (1 - cosW) / 2, 1 - cosW, (1 - cosW) / 2,
                1 + alpha, -2 * cosW, 1 - alpha,
            )
            FilterType.HIGH_PASS -> doubleArrayOf(
                (1 + cosW) / 2, -(1 + cosW), (1 + cosW) / 2,
                1 + alpha, -2 * cosW, 1 - alpha,
            )
            FilterType.DISABLED -> doubleArrayOf(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
        }
        val a0 = raw[3]
        return listOf(raw[0] / a0, raw[1] / a0, raw[2] / a0, -raw[4] / a0, -raw[5] / a0).map { toQ30(it) }
    }

    private fun toQ30(value: Double): Int {
        val scaled = floor(value * SCALE + 0.5)
        if (scaled < Int.MIN_VALUE.toDouble() || scaled > Int.MAX_VALUE.toDouble()) {
            throw ProtocolException("Coefficient does not fit Q2.30")
        }
        return scaled.toInt()
    }
}
