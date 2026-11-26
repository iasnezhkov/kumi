// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent

object KeyNames {
    private val names = mapOf(
        KeyEvent.KEYCODE_CTRL_LEFT to "Ctrl",
        KeyEvent.KEYCODE_CTRL_RIGHT to "Right Ctrl",
        KeyEvent.KEYCODE_ALT_LEFT to "Alt",
        KeyEvent.KEYCODE_ALT_RIGHT to "Right Alt",
        KeyEvent.KEYCODE_SHIFT_LEFT to "Shift",
        KeyEvent.KEYCODE_SHIFT_RIGHT to "Right Shift",
        KeyEvent.KEYCODE_META_LEFT to "Meta",
        KeyEvent.KEYCODE_META_RIGHT to "Right Meta",
        KeyEvent.KEYCODE_CAPS_LOCK to "Caps Lock",
        KeyEvent.KEYCODE_FUNCTION to "Fn",
        KeyEvent.KEYCODE_ESCAPE to "Esc",
        KeyEvent.KEYCODE_TAB to "Tab",
        KeyEvent.KEYCODE_ENTER to "Enter",
        KeyEvent.KEYCODE_SPACE to "Space",
        KeyEvent.KEYCODE_DEL to "Backspace",
        KeyEvent.KEYCODE_FORWARD_DEL to "Delete",
        KeyEvent.KEYCODE_INSERT to "Insert",
        KeyEvent.KEYCODE_MOVE_HOME to "Home",
        KeyEvent.KEYCODE_MOVE_END to "End",
        KeyEvent.KEYCODE_PAGE_UP to "Page Up",
        KeyEvent.KEYCODE_PAGE_DOWN to "Page Down",
        KeyEvent.KEYCODE_DPAD_UP to "↑",
        KeyEvent.KEYCODE_DPAD_DOWN to "↓",
        KeyEvent.KEYCODE_DPAD_LEFT to "←",
        KeyEvent.KEYCODE_DPAD_RIGHT to "→",
        KeyEvent.KEYCODE_GRAVE to "`",
        KeyEvent.KEYCODE_MINUS to "-",
        KeyEvent.KEYCODE_EQUALS to "=",
        KeyEvent.KEYCODE_LEFT_BRACKET to "[",
        KeyEvent.KEYCODE_RIGHT_BRACKET to "]",
        KeyEvent.KEYCODE_BACKSLASH to "\\",
        KeyEvent.KEYCODE_SEMICOLON to ";",
        KeyEvent.KEYCODE_APOSTROPHE to "'",
        KeyEvent.KEYCODE_COMMA to ",",
        KeyEvent.KEYCODE_PERIOD to ".",
        KeyEvent.KEYCODE_SLASH to "/",
        KeyEvent.KEYCODE_VOLUME_UP to "Vol +",
        KeyEvent.KEYCODE_VOLUME_DOWN to "Vol −",
        KeyEvent.KEYCODE_VOLUME_MUTE to "Mute",
        KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE to "Play/Pause",
        KeyEvent.KEYCODE_MEDIA_NEXT to "Next",
        KeyEvent.KEYCODE_MEDIA_PREVIOUS to "Previous",
        KeyEvent.KEYCODE_VOICE_ASSIST to "Voice",
        KeyEvent.KEYCODE_HOME to "Home screen",
        KeyEvent.KEYCODE_BACK to "Back",
        KeyEvent.KEYCODE_APP_SWITCH to "Recent apps",
        KeyEvent.KEYCODE_SEARCH to "Search",
        KeyEvent.KEYCODE_LANGUAGE_SWITCH to "Language",
        KeyEvent.KEYCODE_SYSRQ to "Print Screen",
        KeyEvent.KEYCODE_BRIGHTNESS_UP to "Brightness +",
        KeyEvent.KEYCODE_BRIGHTNESS_DOWN to "Brightness −",
    )

    val COMMON_TARGETS = listOf(
        KeyEvent.KEYCODE_CTRL_LEFT, KeyEvent.KEYCODE_ALT_LEFT, KeyEvent.KEYCODE_META_LEFT, KeyEvent.KEYCODE_SHIFT_LEFT,
        KeyEvent.KEYCODE_ESCAPE, KeyEvent.KEYCODE_TAB, KeyEvent.KEYCODE_DEL, KeyEvent.KEYCODE_FORWARD_DEL,
        KeyEvent.KEYCODE_MOVE_HOME, KeyEvent.KEYCODE_MOVE_END, KeyEvent.KEYCODE_PAGE_UP, KeyEvent.KEYCODE_PAGE_DOWN,
        KeyEvent.KEYCODE_LANGUAGE_SWITCH, KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE, KeyEvent.KEYCODE_MEDIA_NEXT,
        KeyEvent.KEYCODE_MEDIA_PREVIOUS, KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN, KeyEvent.KEYCODE_BACK,
        KeyEvent.KEYCODE_HOME, KeyEvent.KEYCODE_APP_SWITCH, KeyEvent.KEYCODE_SEARCH,
    )

    fun label(code: Int): String = names[code] ?: KeyEvent.keyCodeToString(code).removePrefix("KEYCODE_")
        .split('_').joinToString(" ") { part -> part.lowercase().replaceFirstChar { it.uppercase() } }
        .let { if (it.length == 1) it.uppercase() else it }
}
