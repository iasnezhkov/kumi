// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import androidx.annotation.StringRes
import org.mindone.snapcase.R

data class KumiKeymapPreset(
    val id: String,
    @StringRes val nameRes: Int,
    @StringRes val descRes: Int,
    val entries: List<KeymapEntry>,
    val combos: List<KeyCombo> = emptyList(),
    val config: KeyConfig? = null,
) {
    val valid: Boolean
        get() = entries.size <= KumiLimits.KEYMAP_MAX &&
            combos.size <= KumiLimits.COMBO_SLOTS &&
            entries.all { it.layer in 0 until KeymapEntry.LAYERS && it.source in KumiLimits.KEY_MIN_USAGE..KumiLimits.COMBO_MAX_USAGE && it.action.valid } &&
            combos.all { it.valid } &&
            (config == null || config.valid)
}

object KumiKeymapPresets {
    private const val MOD_CTRL = 0x01
    private const val MOD_SHIFT = 0x02
    private const val MOD_ALT = 0x04
    private const val MOD_META = 0x08

    val ALL: List<KumiKeymapPreset> = listOf(
        KumiKeymapPreset(
            id = "clicks_nav_ijkl",
            nameRes = R.string.kumi_preset_clicks_ijkl_name,
            descRes = R.string.kumi_preset_clicks_ijkl_desc,
            entries = listOf(
                KeymapEntry(0, 0x2C, KeyAction.TapHoldLayer(0x2C, 1)),
                KeymapEntry(1, 0x0C, KeyAction.Key(0x52)),
                KeymapEntry(1, 0x0D, KeyAction.Key(0x50)),
                KeymapEntry(1, 0x0E, KeyAction.Key(0x51)),
                KeymapEntry(1, 0x0F, KeyAction.Key(0x4F)),
                KeymapEntry(1, 0x18, KeyAction.Key(0x4A)),
                KeymapEntry(1, 0x12, KeyAction.Key(0x4D)),
                KeymapEntry(1, 0x0B, KeyAction.Key(0x4B)),
                KeymapEntry(1, 0x11, KeyAction.Key(0x4E)),
                KeymapEntry(1, 0x2A, KeyAction.Key(0x4C)),
                KeymapEntry(1, 0x10, KeyAction.Key(0x29)),
                KeymapEntry(1, 0x1A, KeyAction.Mouse(MouseAction.WheelUp)),
                KeymapEntry(1, 0x16, KeyAction.Mouse(MouseAction.WheelDown)),
            ),
            combos = listOf(
                KeyCombo(0x0D, 0x0E, KeyAction.Key(0x29)),
                KeyCombo(0x07, 0x09, KeyAction.CapsWord),
            ),
            config = KeyConfig(tappingTermMs = 190, oneShotTimeoutMs = 3_000, comboTermMs = 45),
        ),
        KumiKeymapPreset(
            id = "clicks_nav_wasd",
            nameRes = R.string.kumi_preset_clicks_wasd_name,
            descRes = R.string.kumi_preset_clicks_wasd_desc,
            entries = listOf(
                KeymapEntry(0, 0x2C, KeyAction.TapHoldLayer(0x2C, 1)),
                KeymapEntry(1, 0x1A, KeyAction.Key(0x52)),
                KeymapEntry(1, 0x04, KeyAction.Key(0x50)),
                KeymapEntry(1, 0x16, KeyAction.Key(0x51)),
                KeymapEntry(1, 0x07, KeyAction.Key(0x4F)),
                KeymapEntry(1, 0x14, KeyAction.Key(0x4B)),
                KeymapEntry(1, 0x08, KeyAction.Key(0x4E)),
                KeymapEntry(1, 0x15, KeyAction.Mouse(MouseAction.WheelUp)),
                KeymapEntry(1, 0x09, KeyAction.Mouse(MouseAction.WheelDown)),
                KeymapEntry(1, 0x0C, KeyAction.Mouse(MouseAction.MoveUp)),
                KeymapEntry(1, 0x0D, KeyAction.Mouse(MouseAction.MoveLeft)),
                KeymapEntry(1, 0x0E, KeyAction.Mouse(MouseAction.MoveDown)),
                KeymapEntry(1, 0x0F, KeyAction.Mouse(MouseAction.MoveRight)),
                KeymapEntry(1, 0x28, KeyAction.Mouse(MouseAction.LeftClick)),
                KeymapEntry(1, 0x2A, KeyAction.Mouse(MouseAction.RightClick)),
            ),
            combos = listOf(
                KeyCombo(0x07, 0x09, KeyAction.CapsWord),
            ),
        ),
        KumiKeymapPreset(
            id = "clicks_android_os",
            nameRes = R.string.kumi_preset_android_os_name,
            descRes = R.string.kumi_preset_android_os_desc,
            entries = listOf(
                KeymapEntry(0, 0xE2, KeyAction.TapHoldLayer(0x29, 2)),
                KeymapEntry(2, 0x0B, KeyAction.KeyWithModifiers(0x0B, MOD_META)),
                KeymapEntry(2, 0x05, KeyAction.Consumer(0x224)),
                KeymapEntry(2, 0x11, KeyAction.KeyWithModifiers(0x11, MOD_META)),
                KeymapEntry(2, 0x16, KeyAction.KeyWithModifiers(0x16, MOD_META)),
                KeymapEntry(2, 0x0A, KeyAction.KeyWithModifiers(0x0A, MOD_META)),
                KeymapEntry(2, 0x19, KeyAction.Consumer(0x0CF)),
                KeymapEntry(2, 0x13, KeyAction.Consumer(0xCD)),
                KeymapEntry(2, 0x10, KeyAction.Consumer(0xE2)),
                KeymapEntry(2, 0x18, KeyAction.Consumer(0xE9)),
                KeymapEntry(2, 0x07, KeyAction.Consumer(0xEA)),
                KeymapEntry(2, 0x17, KeyAction.KeyWithModifiers(0x2B, MOD_ALT)),
                KeymapEntry(2, 0x06, KeyAction.KeyWithModifiers(0x06, MOD_CTRL)),
                KeymapEntry(2, 0x1B, KeyAction.KeyWithModifiers(0x1B, MOD_CTRL)),
                KeymapEntry(2, 0x1D, KeyAction.KeyWithModifiers(0x1D, MOD_CTRL)),
            ),
        ),
        KumiKeymapPreset(
            id = "one_hand_sticky",
            nameRes = R.string.kumi_preset_one_hand_name,
            descRes = R.string.kumi_preset_one_hand_desc,
            entries = listOf(
                KeymapEntry(0, 0xE1, KeyAction.OneShotMod(MOD_SHIFT)),
                KeymapEntry(0, 0xE2, KeyAction.OneShotMod(MOD_ALT)),
                KeymapEntry(0, 0x2C, KeyAction.TapHoldLayer(0x2C, 1)),
            ),
            combos = listOf(
                KeyCombo(0x09, 0x0D, KeyAction.CapsWord),
            ),
            config = KeyConfig(tappingTermMs = 190, oneShotTimeoutMs = 3_000, comboTermMs = 50),
        ),
        KumiKeymapPreset(
            id = "ru_36key_hardware",
            nameRes = R.string.kumi_preset_ru_36key_name,
            descRes = R.string.kumi_preset_ru_36key_desc,
            entries = listOf(
                KeymapEntry(0, 0x2C, KeyAction.TapHoldLayer(0x2C, 1)),
                KeymapEntry(1, 0x17, KeyAction.Key(0x35)),
                KeymapEntry(1, 0x13, KeyAction.Key(0x2F)),
                KeymapEntry(1, 0x12, KeyAction.Key(0x30)),
                KeymapEntry(1, 0x0F, KeyAction.Key(0x33)),
                KeymapEntry(1, 0x0E, KeyAction.Key(0x34)),
                KeymapEntry(1, 0x10, KeyAction.Key(0x36)),
                KeymapEntry(1, 0x37, KeyAction.Key(0x37)),
            ),
            combos = listOf(
                KeyCombo(0x08, 0x17, KeyAction.Key(0x35)),
                KeyCombo(0x12, 0x13, KeyAction.Key(0x2F)),
                KeyCombo(0x0E, 0x0F, KeyAction.Key(0x33)),
                KeyCombo(0x11, 0x10, KeyAction.Key(0x30)),
            ),
        ),
    )

    fun byId(id: String): KumiKeymapPreset? = ALL.firstOrNull { it.id == id }
}
