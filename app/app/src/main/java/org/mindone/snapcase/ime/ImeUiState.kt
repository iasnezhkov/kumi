// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

enum class StripDrawer { NONE, EMOJI, SYMBOLS, CLIPBOARD }

enum class StickyState { OFF, ONE_SHOT, LOCKED }

data class ImeUiState(
    val visible: Boolean = true,
    val lang: StripLang = StripLang.EN,
    val composing: String = "",
    val suggestions: List<String> = emptyList(),
    val selectedSuggestionIndex: Int = -1,
    val isNextWordPrediction: Boolean = false,
    val autoHighlightFirst: Boolean = false,
    val variations: List<String> = emptyList(),
    val activeDrawer: StripDrawer = StripDrawer.NONE,
    val emojiQuery: String = "",
    val shiftState: StickyState = StickyState.OFF,
    val physicalShiftHeld: Boolean = false,
    val effectiveUpper: Boolean = false,
    val altState: StickyState = StickyState.OFF,
    val capsWord: Boolean = false,
    val kumiConnected: Boolean = false,
    val kumiLayer: Int = 0,
    val hasHardwareKeyboard: Boolean = true,
    val undoAvailable: Boolean = false,
    val isPasswordField: Boolean = false,
    val clipboardItems: List<ClipboardItem> = emptyList(),
    val snippets: List<TextSnippet> = emptyList(),
)
