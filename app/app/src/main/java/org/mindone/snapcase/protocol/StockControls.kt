// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

enum class DacFilter(val wire: Int) {
    FAST_LOW_LATENCY(0),
    FAST_PHASE_COMPENSATED(1),
    SLOW_LOW_LATENCY(2),
    SLOW_PHASE_COMPENSATED(3),
    NON_OVERSAMPLING(4);

    companion object {
        fun fromWire(wire: Int): DacFilter? = entries.firstOrNull { it.wire == wire }
    }
}

enum class UsbAudioProfile { DUPLEX, PLAYBACK_ONLY, UNKNOWN }

enum class StreamMode { PCM, DOP, NATIVE_DSD, UNKNOWN }

data class CaseAudioState(
    val usbConfigRaw: Int,
    val dacFilterRaw: Int,
    val eqProfileRaw: Int,
    val modeRaw: Long,
    val gateRaw: Long,
    val dsdModeRaw: Int,
    val sampleRateHz: Long,
    val volumeLeftRaw: Int,
    val volumeRightRaw: Int,
    val outputTrimRaw: Int,
    val muteRaw: Int,
    val pcmWidthRaw: Long = 0,
    val measuredRateRaw: Long = 0,
    val captureTrimRaw: Int = 0,
) {
    val dacFilter: DacFilter? get() = DacFilter.fromWire(dacFilterRaw)
    val usbClass: Int get() = if (usbConfigRaw == 1) 1 else 2
    val eqEnabled: Boolean get() = eqProfileRaw != 0
    val usbProfile: UsbAudioProfile get() = when (modeRaw) {
        1L, 2L -> UsbAudioProfile.DUPLEX
        3L, 4L -> UsbAudioProfile.PLAYBACK_ONLY
        else -> UsbAudioProfile.UNKNOWN
    }
    val streamMode: StreamMode get() = when (dsdModeRaw) {
        0 -> StreamMode.PCM
        1 -> StreamMode.DOP
        2 -> StreamMode.NATIVE_DSD
        else -> StreamMode.UNKNOWN
    }
    val pcmBits: Int? get() = pcmWidthRaw.toInt().takeIf { it == 16 || it == 24 || it == 32 }
}

object StockState {
    const val USB_CONFIG = 0x0011287DL
    const val DAC_FILTER = 0x00112881L
    const val EQ_PROFILE = 0x00112886L
    const val MODE = 0x001128B4L
    const val GATE = 0x001128B8L
    const val DSD_MODE = 0x00112980L
    const val SAMPLE_RATE = 0x0011319CL
    const val VOLUME_LEFT = 0x001131CEL
    const val VOLUME_RIGHT = 0x001131D0L
    const val OUTPUT_TRIM = 0x001131D2L
    const val MUTE = 0x001131DCL
    const val PCM_WIDTH = 0x001131A0L
    const val MEASURED_RATE = 0x001131A4L
    const val CAPTURE_TRIM = 0x001131D6L

    val TWEAK_ALLOWLIST = setOf(DAC_FILTER)

    private val byteAddresses: List<Long> = listOf(
        USB_CONFIG, DAC_FILTER, EQ_PROFILE, DSD_MODE, MUTE,
        MODE, MODE + 1, MODE + 2, MODE + 3,
        GATE, GATE + 1, GATE + 2, GATE + 3,
        SAMPLE_RATE, SAMPLE_RATE + 1, SAMPLE_RATE + 2, SAMPLE_RATE + 3,
        VOLUME_LEFT, VOLUME_LEFT + 1, VOLUME_RIGHT, VOLUME_RIGHT + 1,
        OUTPUT_TRIM, OUTPUT_TRIM + 1,
        PCM_WIDTH, PCM_WIDTH + 1, PCM_WIDTH + 2, PCM_WIDTH + 3,
        MEASURED_RATE, MEASURED_RATE + 1, MEASURED_RATE + 2, MEASURED_RATE + 3,
        CAPTURE_TRIM, CAPTURE_TRIM + 1,
    )

    fun wordAddresses(): Set<Long> = byteAddresses.map { it and 3L.inv() }.toSortedSet()

    fun decode(image: WordImage) = CaseAudioState(
        usbConfigRaw = image.u8(USB_CONFIG),
        dacFilterRaw = image.u8(DAC_FILTER),
        eqProfileRaw = image.u8(EQ_PROFILE),
        modeRaw = image.u32(MODE),
        gateRaw = image.u32(GATE),
        dsdModeRaw = image.u8(DSD_MODE),
        sampleRateHz = image.u32(SAMPLE_RATE),
        volumeLeftRaw = image.s16(VOLUME_LEFT),
        volumeRightRaw = image.s16(VOLUME_RIGHT),
        outputTrimRaw = image.s16(OUTPUT_TRIM),
        muteRaw = image.u8(MUTE),
        pcmWidthRaw = image.u32(PCM_WIDTH),
        measuredRateRaw = image.u32(MEASURED_RATE),
        captureTrimRaw = image.s16(CAPTURE_TRIM),
    )
}
