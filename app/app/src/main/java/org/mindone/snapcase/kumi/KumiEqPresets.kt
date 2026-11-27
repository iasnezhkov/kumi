// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import org.json.JSONArray
import org.json.JSONObject

enum class EqPresetCategory { STANDARD, GENRE }

data class BuiltInEqPreset(
    val id: String,
    val category: EqPresetCategory,
    val bands: List<ParametricBand>,
)

data class CustomEqPreset(
    val name: String,
    val profile: EqProfile,
) {
    fun toJson(): JSONObject = JSONObject()
        .put("name", name)
        .put("profile", profile.toJson())

    companion object {
        fun from(j: JSONObject): CustomEqPreset? = runCatching {
            val name = j.getString("name").trim()
            val profile = EqProfile.from(j.getJSONObject("profile"))
            if (name.isNotEmpty() && profile.valid) CustomEqPreset(name, profile) else null
        }.getOrNull()

        fun parseList(raw: String?): List<CustomEqPreset> {
            if (raw.isNullOrBlank()) return emptyList()
            return runCatching {
                JSONArray(raw).objects().mapNotNull(::from)
            }.getOrDefault(emptyList())
        }

        fun serializeList(list: List<CustomEqPreset>): String =
            JSONArray(list.map { it.toJson() }).toString()
    }
}

object KumiEqPresets {
    const val FLAT = "flat"
    const val HARMAN_OE = "harman_oe"
    const val HARMAN = "harman"
    const val STUDIO = "studio"
    const val DIFFUSE_FIELD = "diffuse_field"
    const val WARM_SMOOTH = "warm_smooth"
    const val LATE_NIGHT = "late_night"
    const val DEEP_BASS = "deep_bass"

    const val EDM = "edm"
    const val ROCK = "rock"
    const val HIPHOP = "hiphop"
    const val JAZZ = "jazz"
    const val CLASSICAL = "classical"
    const val POP = "pop"
    const val ACOUSTIC = "acoustic"
    const val VOICE = "voice"

    val STANDARDS: List<BuiltInEqPreset> = listOf(
        BuiltInEqPreset(FLAT, EqPresetCategory.STANDARD, emptyList()),
        BuiltInEqPreset(
            HARMAN_OE,
            EqPresetCategory.STANDARD,
            listOf(
                ParametricBand(BandKind.LowShelf, 105, 710, 450),
                ParametricBand(BandKind.Peak, 250, 1_100, -150),
                ParametricBand(BandKind.Peak, 3_000, 1_400, 200),
                ParametricBand(BandKind.HighShelf, 10_000, 710, 100),
            ),
        ),
        BuiltInEqPreset(
            HARMAN,
            EqPresetCategory.STANDARD,
            listOf(
                ParametricBand(BandKind.LowShelf, 105, 710, 500),
                ParametricBand(BandKind.Peak, 250, 1_100, -150),
                ParametricBand(BandKind.Peak, 3_000, 1_400, 250),
                ParametricBand(BandKind.HighShelf, 10_000, 710, 150),
            ),
        ),
        BuiltInEqPreset(
            STUDIO,
            EqPresetCategory.STANDARD,
            listOf(
                ParametricBand(BandKind.LowShelf, 65, 710, 150),
                ParametricBand(BandKind.Peak, 3_200, 1_800, -100),
                ParametricBand(BandKind.HighShelf, 12_000, 710, 100),
            ),
        ),
        BuiltInEqPreset(
            DIFFUSE_FIELD,
            EqPresetCategory.STANDARD,
            listOf(
                ParametricBand(BandKind.LowShelf, 80, 710, 100),
                ParametricBand(BandKind.Peak, 2_800, 1_200, 250),
                ParametricBand(BandKind.HighShelf, 8_000, 710, 200),
            ),
        ),
        BuiltInEqPreset(
            WARM_SMOOTH,
            EqPresetCategory.STANDARD,
            listOf(
                ParametricBand(BandKind.LowShelf, 110, 710, 400),
                ParametricBand(BandKind.Peak, 220, 900, 150),
                ParametricBand(BandKind.Peak, 6_500, 2_000, -200),
                ParametricBand(BandKind.HighShelf, 10_000, 710, -250),
            ),
        ),
        BuiltInEqPreset(
            LATE_NIGHT,
            EqPresetCategory.STANDARD,
            listOf(
                ParametricBand(BandKind.LowShelf, 75, 710, 550),
                ParametricBand(BandKind.Peak, 2_500, 1_200, 200),
                ParametricBand(BandKind.HighShelf, 8_000, 710, 350),
            ),
        ),
        BuiltInEqPreset(
            DEEP_BASS,
            EqPresetCategory.STANDARD,
            listOf(
                ParametricBand(BandKind.LowShelf, 85, 710, 600),
                ParametricBand(BandKind.Peak, 260, 1_200, -200),
                ParametricBand(BandKind.Peak, 4_000, 1_500, 150),
                ParametricBand(BandKind.HighShelf, 11_000, 710, 200),
            ),
        ),
    )

    val GENRES: List<BuiltInEqPreset> = listOf(
        BuiltInEqPreset(
            EDM,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.LowShelf, 65, 710, 500),
                ParametricBand(BandKind.Peak, 240, 1_200, -150),
                ParametricBand(BandKind.Peak, 3_500, 1_400, 150),
                ParametricBand(BandKind.HighShelf, 11_000, 710, 250),
            ),
        ),
        BuiltInEqPreset(
            ROCK,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.LowShelf, 90, 710, 300),
                ParametricBand(BandKind.Peak, 350, 1_200, -150),
                ParametricBand(BandKind.Peak, 2_400, 1_200, 250),
                ParametricBand(BandKind.Peak, 5_500, 1_500, 200),
                ParametricBand(BandKind.HighShelf, 10_000, 710, 100),
            ),
        ),
        BuiltInEqPreset(
            HIPHOP,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.LowShelf, 55, 710, 550),
                ParametricBand(BandKind.Peak, 120, 1_000, 200),
                ParametricBand(BandKind.Peak, 300, 1_200, -150),
                ParametricBand(BandKind.Peak, 3_000, 1_300, 150),
                ParametricBand(BandKind.HighShelf, 12_000, 710, 150),
            ),
        ),
        BuiltInEqPreset(
            JAZZ,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.LowShelf, 85, 710, 200),
                ParametricBand(BandKind.Peak, 450, 1_000, 150),
                ParametricBand(BandKind.Peak, 2_200, 1_200, 100),
                ParametricBand(BandKind.HighShelf, 10_000, 710, 200),
            ),
        ),
        BuiltInEqPreset(
            CLASSICAL,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.LowShelf, 50, 710, 150),
                ParametricBand(BandKind.Peak, 280, 1_100, -50),
                ParametricBand(BandKind.Peak, 3_000, 1_400, 100),
                ParametricBand(BandKind.HighShelf, 12_000, 710, 250),
            ),
        ),
        BuiltInEqPreset(
            POP,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.LowShelf, 80, 710, 300),
                ParametricBand(BandKind.Peak, 250, 1_200, -100),
                ParametricBand(BandKind.Peak, 2_800, 1_200, 200),
                ParametricBand(BandKind.HighShelf, 10_000, 710, 200),
            ),
        ),
        BuiltInEqPreset(
            ACOUSTIC,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.LowShelf, 60, 710, 150),
                ParametricBand(BandKind.Peak, 300, 1_100, -100),
                ParametricBand(BandKind.Peak, 1_800, 1_000, 150),
                ParametricBand(BandKind.HighShelf, 10_000, 710, 250),
            ),
        ),
        BuiltInEqPreset(
            VOICE,
            EqPresetCategory.GENRE,
            listOf(
                ParametricBand(BandKind.HighPass, 80, 710, 0),
                ParametricBand(BandKind.Peak, 250, 1_000, -250),
                ParametricBand(BandKind.Peak, 2_800, 1_200, 300),
                ParametricBand(BandKind.Peak, 5_000, 1_500, 200),
                ParametricBand(BandKind.HighShelf, 12_000, 710, -150),
            ),
        ),
    )

    val ALL: List<BuiltInEqPreset> = STANDARDS + GENRES

    val AUTOEQ_BASES: List<Pair<String, List<ParametricBand>>> = listOf(
        "Sennheiser HD 600" to listOf(
            ParametricBand(BandKind.LowShelf, 85, 710, 550),
            ParametricBand(BandKind.Peak, 3_400, 2_200, -250),
            ParametricBand(BandKind.Peak, 5_800, 3_000, 200),
            ParametricBand(BandKind.HighShelf, 10_000, 710, 200),
        ),
        "Sennheiser HD 650" to listOf(
            ParametricBand(BandKind.LowShelf, 80, 710, 500),
            ParametricBand(BandKind.Peak, 200, 800, -150),
            ParametricBand(BandKind.Peak, 3_200, 1_800, 150),
            ParametricBand(BandKind.HighShelf, 9_500, 710, 300),
        ),
        "Beyerdynamic DT 770 Pro" to listOf(
            ParametricBand(BandKind.LowShelf, 70, 710, 200),
            ParametricBand(BandKind.Peak, 210, 1_500, 300),
            ParametricBand(BandKind.Peak, 3_900, 2_500, 250),
            ParametricBand(BandKind.Peak, 8_400, 3_500, -550),
        ),
        "Moondrop Chu II" to listOf(
            ParametricBand(BandKind.LowShelf, 100, 710, 150),
            ParametricBand(BandKind.Peak, 2_600, 1_800, -150),
            ParametricBand(BandKind.Peak, 4_800, 2_600, -200),
            ParametricBand(BandKind.HighShelf, 12_000, 710, 150),
        ),
        "Truthear Zero:RED" to listOf(
            ParametricBand(BandKind.LowShelf, 90, 710, 200),
            ParametricBand(BandKind.Peak, 2_400, 1_500, 150),
            ParametricBand(BandKind.Peak, 6_200, 2_800, -200),
            ParametricBand(BandKind.HighShelf, 11_000, 710, 150),
        ),
        "Koss Porta Pro" to listOf(
            ParametricBand(BandKind.LowShelf, 60, 710, 350),
            ParametricBand(BandKind.Peak, 150, 900, -400),
            ParametricBand(BandKind.Peak, 3_000, 1_300, 350),
            ParametricBand(BandKind.HighShelf, 9_000, 710, 350),
        ),
        "Apple EarPods" to listOf(
            ParametricBand(BandKind.LowShelf, 90, 710, 600),
            ParametricBand(BandKind.Peak, 250, 1_000, -250),
            ParametricBand(BandKind.Peak, 2_500, 1_400, -200),
            ParametricBand(BandKind.HighShelf, 8_000, 710, 450),
        ),
    )

    fun toProfile(preset: BuiltInEqPreset, math: EqMath): EqProfile =
        if (preset.bands.isEmpty()) EqProfile.FLAT else math.withAutoPreamp(EqProfile(0, preset.bands))

    fun toProfile(bands: List<ParametricBand>, math: EqMath): EqProfile =
        if (bands.isEmpty()) EqProfile.FLAT else math.withAutoPreamp(EqProfile(0, bands))
}
