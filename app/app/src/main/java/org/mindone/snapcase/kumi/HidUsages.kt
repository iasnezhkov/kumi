// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

object HidUsages {
    private val keys: Map<Int, String> = buildMap {
        ('A'..'Z').forEachIndexed { i, c -> put(0x04 + i, c.toString()) }
        "1234567890".forEachIndexed { i, c -> put(0x1E + i, c.toString()) }
        putAll(
            mapOf(
                0x28 to "Enter", 0x29 to "Esc", 0x2A to "Backspace", 0x2B to "Tab", 0x2C to "Space", 0x2D to "-", 0x2E to "=",
                0x2F to "[", 0x30 to "]", 0x31 to "\\", 0x33 to ";", 0x34 to "'", 0x35 to "`", 0x36 to ",", 0x37 to ".", 0x38 to "/",
                0x39 to "Caps Lock", 0x46 to "Print Screen", 0x47 to "Scroll Lock", 0x48 to "Pause", 0x49 to "Insert", 0x4A to "Home",
                0x4B to "Page Up", 0x4C to "Delete", 0x4D to "End", 0x4E to "Page Down", 0x4F to "→", 0x50 to "←", 0x51 to "↓", 0x52 to "↑",
                0x65 to "Menu", 0xE0 to "Left Ctrl", 0xE1 to "Left Shift", 0xE2 to "Left Alt", 0xE3 to "Left Meta",
                0xE4 to "Right Ctrl", 0xE5 to "Right Shift", 0xE6 to "Right Alt", 0xE7 to "Right Meta",
            ),
        )
        (1..12).forEach { put(0x39 + it, "F$it") }
    }

    val consumer: Map<Int, String> = linkedMapOf(
        0xE9 to "Volume up", 0xEA to "Volume down", 0xE2 to "Mute", 0xCD to "Play/Pause", 0xB5 to "Next track",
        0xB6 to "Previous track", 0xB7 to "Stop", 0x6F to "Brightness up", 0x70 to "Brightness down",
        0x0CF to "Voice", 0x223 to "Home", 0x224 to "Back", 0x225 to "Forward", 0x221 to "Search",
    )

    val modifierBits: List<Pair<Int, String>> = listOf(
        0x01 to "Ctrl", 0x02 to "Shift", 0x04 to "Alt", 0x08 to "Meta", 0x10 to "Right Ctrl", 0x20 to "Right Shift", 0x40 to "Right Alt", 0x80 to "Right Meta",
    )

    val selectable: List<Pair<Int, String>> = keys.entries.sortedBy { it.key }.map { it.key to it.value }

    fun key(usage: Int): String = keys[usage] ?: "0x%02X".format(usage)

    private fun modifierBit(usage: Int): Int? = if (usage in 0xE0..0xE7) 1 shl (usage - 0xE0) else null

    fun keyAction(usage: Int): KeyAction = modifierBit(usage)?.let { KeyAction.Modifiers(it) } ?: KeyAction.Key(usage)

    fun chord(usage: Int, modifiers: Int): KeyAction {
        if (modifiers == 0) return keyAction(usage)
        return modifierBit(usage)?.let { KeyAction.Modifiers(modifiers or it) } ?: KeyAction.KeyWithModifiers(usage, modifiers)
    }

    fun layerTargets(current: Int, layers: Int): List<Int> = (1 until layers).filter { it != current }

    fun modifiers(mask: Int): String = modifierBits.filter { mask and it.first != 0 }.joinToString("+") { it.second }.ifEmpty { "—" }

    fun hotkeyName(hotkey: CaseHotkey): String = when (hotkey) {
        CaseHotkey.CycleEq -> "Cycle EQ"
        CaseHotkey.CycleFilter -> "Cycle Filter"
        CaseHotkey.CycleCrossfeed -> "Cycle Crossfeed"
        CaseHotkey.ToggleLimiter -> "Toggle Limiter"
        CaseHotkey.ToggleMono -> "Toggle Mono"
        CaseHotkey.ToggleMute -> "Toggle Mute"
        CaseHotkey.ToggleLineOut -> "Toggle Line-Out"
        CaseHotkey.ToggleLoudness -> "Toggle Loudness"
        CaseHotkey.ToggleCapsWord -> "Toggle Caps Word"
    }

    fun mouseName(action: MouseAction): String = when (action) {
        MouseAction.MoveUp -> "Mouse Up"
        MouseAction.MoveDown -> "Mouse Down"
        MouseAction.MoveLeft -> "Mouse Left"
        MouseAction.MoveRight -> "Mouse Right"
        MouseAction.LeftClick -> "Left Click"
        MouseAction.RightClick -> "Right Click"
        MouseAction.MiddleClick -> "Middle Click"
        MouseAction.WheelUp -> "Wheel Up"
        MouseAction.WheelDown -> "Wheel Down"
    }

    fun describe(action: KeyAction): String = when (action) {
        KeyAction.Transparent -> "Default"
        KeyAction.Disabled -> "Disabled"
        is KeyAction.Key -> key(action.usage)
        is KeyAction.Modifiers -> modifiers(action.mask)
        is KeyAction.KeyWithModifiers -> "${modifiers(action.modifiers)}+${key(action.usage)}"
        is KeyAction.Consumer -> consumer[action.usage] ?: "Media 0x%03X".format(action.usage)
        is KeyAction.LayerHold -> "Hold layer ${action.layer + 1}"
        is KeyAction.LayerToggle -> "Toggle layer ${action.layer + 1}"
        is KeyAction.TapHoldMod -> "Tap ${key(action.tap)} / Hold ${modifiers(action.holdMods)}"
        is KeyAction.TapHoldLayer -> "Tap ${key(action.tap)} / Hold L${action.holdLayer + 1}"
        is KeyAction.OneShotMod -> "One-Shot ${modifiers(action.mods)}"
        is KeyAction.Macro -> "Macro ${action.slot + 1}"
        is KeyAction.CaseControl -> hotkeyName(action.hotkey)
        KeyAction.CapsWord -> "Caps Word"
        is KeyAction.Mouse -> mouseName(action.action)
        is KeyAction.Unsupported -> "Unsupported action"
    }
}
