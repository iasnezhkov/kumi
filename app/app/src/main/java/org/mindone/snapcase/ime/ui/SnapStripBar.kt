// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.Locale
import org.mindone.snapcase.R
import org.mindone.snapcase.ime.EmojiCatalog
import org.mindone.snapcase.ime.ImeUiState
import org.mindone.snapcase.ime.LocaleVariations
import org.mindone.snapcase.ime.StickyState
import org.mindone.snapcase.ime.StripDrawer
import org.mindone.snapcase.ime.StripLang

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SnapStripRoot(
    state: ImeUiState,
    emojiCatalog: EmojiCatalog,
    showQuickExtras: Boolean,
    onPickSuggestion: (String) -> Unit,
    onForgetSuggestion: (String) -> Unit,
    onPickVariation: (String) -> Unit,
    onInsertDirect: (String) -> Unit,
    onPickEmoji: (String) -> Unit,
    onToggleDrawer: (StripDrawer) -> Unit,
    onCycleLang: () -> Unit,
    onLongPressLang: () -> Unit,
    onSwitchVirtualKeyboard: () -> Unit,
    onPinClip: (String) -> Unit,
    onDeleteClip: (String) -> Unit,
    onClearClips: () -> Unit,
) {
    val density = LocalDensity.current
    val rawNavBottom = with(density) { WindowInsets.navigationBars.only(WindowInsetsSides.Bottom).getBottom(this).toDp() }
    val bottomClearance = if (rawNavBottom > 0.dp) rawNavBottom.coerceAtLeast(42.dp) else 0.dp

    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        tonalElevation = 2.dp,
        modifier = Modifier
            .fillMaxWidth()
            .wrapContentHeight(),
    ) {
        Column(Modifier.fillMaxWidth().wrapContentHeight()) {
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            Row(
                modifier = Modifier.fillMaxWidth().height(42.dp).padding(horizontal = 6.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                StripBadgeButton(
                    text = buildBadgeLabel(state),
                    active = state.physicalShiftHeld || state.shiftState != StickyState.OFF || state.capsWord || state.kumiLayer > 0,
                    onClick = onCycleLang,
                    onLongClick = onLongPressLang,
                )

                Box(
                    modifier = Modifier.weight(1f).fillMaxHeight(),
                    contentAlignment = Alignment.CenterStart,
                ) {
                    when {
                        !state.hasHardwareKeyboard && state.suggestions.isEmpty() && state.variations.isEmpty() -> {
                            Surface(
                                onClick = onSwitchVirtualKeyboard,
                                color = MaterialTheme.colorScheme.secondaryContainer,
                                contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                                shape = RoundedCornerShape(10.dp),
                                modifier = Modifier.fillMaxWidth().fillMaxHeight(),
                            ) {
                                Box(contentAlignment = Alignment.Center, modifier = Modifier.padding(horizontal = 8.dp)) {
                                    Text(
                                        text = stringResource(R.string.ime_switch_to_virtual),
                                        fontSize = 13.sp,
                                        fontWeight = FontWeight.Medium,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                            }
                        }

                        state.variations.isNotEmpty() -> {
                            Row(
                                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                state.variations.forEach { variant ->
                                    StripActionChip(
                                        text = variant,
                                        highlighted = true,
                                        onClick = { onPickVariation(variant) },
                                    )
                                }
                            }
                        }

                        state.suggestions.isNotEmpty() -> {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                state.suggestions.forEachIndexed { index, word ->
                                    val kbSelected = state.selectedSuggestionIndex == index
                                    val highlight = kbSelected || (state.selectedSuggestionIndex < 0 && index == 0 && state.autoHighlightFirst)
                                    Surface(
                                        color = when {
                                            kbSelected -> MaterialTheme.colorScheme.primaryContainer
                                            highlight -> MaterialTheme.colorScheme.primaryContainer
                                            else -> MaterialTheme.colorScheme.surfaceContainerHigh
                                        },
                                        contentColor = if (highlight) {
                                            MaterialTheme.colorScheme.onPrimaryContainer
                                        } else {
                                            MaterialTheme.colorScheme.onSurface
                                        },
                                        border = when {
                                            kbSelected -> BorderStroke(1.5.dp, MaterialTheme.colorScheme.primary)
                                            state.isNextWordPrediction -> BorderStroke(0.5.dp, MaterialTheme.colorScheme.outlineVariant)
                                            else -> null
                                        },
                                        shape = RoundedCornerShape(10.dp),
                                        modifier = Modifier
                                            .weight(1f)
                                            .fillMaxHeight()
                                            .clip(RoundedCornerShape(10.dp))
                                            .combinedClickable(
                                                onClick = { onPickSuggestion(word) },
                                                onLongClick = { onForgetSuggestion(word) },
                                            ),
                                    ) {
                                        Box(contentAlignment = Alignment.Center, modifier = Modifier.padding(horizontal = 6.dp)) {
                                            Text(
                                                text = word,
                                                maxLines = 1,
                                                overflow = TextOverflow.Ellipsis,
                                                fontSize = 14.sp,
                                                fontWeight = if (highlight) FontWeight.SemiBold else FontWeight.Normal,
                                            )
                                        }
                                    }
                                }
                            }
                        }

                        showQuickExtras && state.lang != StripLang.EN -> {
                            Row(
                                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                LocaleVariations.extrasFor(state.lang).forEach { ch ->
                                    val display = if (state.effectiveUpper && (state.lang == StripLang.RU || state.lang == StripLang.UK)) {
                                        ch.uppercase(Locale.ROOT)
                                    } else {
                                        ch
                                    }
                                    StripActionChip(
                                        text = display,
                                        highlighted = state.effectiveUpper && display != ch,
                                        onClick = { onInsertDirect(display) },
                                    )
                                }
                            }
                        }

                        else -> {
                            Row(
                                modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                                horizontalArrangement = Arrangement.spacedBy(4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                listOf(",", ".", "?", "!", "—", "@").forEach { sym ->
                                    StripActionChip(
                                        text = sym,
                                        highlighted = false,
                                        onClick = { onInsertDirect(sym) },
                                    )
                                }
                            }
                        }
                    }
                }

                StripIconChip(
                    label = "😀",
                    active = state.activeDrawer == StripDrawer.EMOJI,
                    onClick = { onToggleDrawer(StripDrawer.EMOJI) },
                )
                StripIconChip(
                    label = "#+",
                    active = state.activeDrawer == StripDrawer.SYMBOLS,
                    onClick = { onToggleDrawer(StripDrawer.SYMBOLS) },
                )
                StripIconChip(
                    label = "📋",
                    active = state.activeDrawer == StripDrawer.CLIPBOARD,
                    onClick = { onToggleDrawer(StripDrawer.CLIPBOARD) },
                )
            }

            if (state.activeDrawer != StripDrawer.NONE) {
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                ImeDrawerPanel(
                    state = state,
                    emojiCatalog = emojiCatalog,
                    onPickEmoji = onPickEmoji,
                    onInsertText = onInsertDirect,
                    onPinClip = onPinClip,
                    onDeleteClip = onDeleteClip,
                    onClearClips = onClearClips,
                )
            }
            if (bottomClearance > 0.dp) {
                Spacer(Modifier.fillMaxWidth().height(bottomClearance))
            }
        }
    }
}

private fun buildBadgeLabel(state: ImeUiState): String = buildString {
    append(state.lang.label)
    if (state.kumiConnected && state.kumiLayer > 0) {
        append("·L").append(state.kumiLayer + 1)
    }
    when {
        state.capsWord -> append("·CW")
        state.shiftState == StickyState.LOCKED -> append("·⇪")
        state.physicalShiftHeld || state.shiftState == StickyState.ONE_SHOT -> append("·⇧")
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun StripBadgeButton(text: String, active: Boolean, onClick: () -> Unit, onLongClick: () -> Unit) {
    Surface(
        color = if (active) MaterialTheme.colorScheme.tertiaryContainer else MaterialTheme.colorScheme.surfaceContainerHighest,
        contentColor = if (active) MaterialTheme.colorScheme.onTertiaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
        shape = RoundedCornerShape(10.dp),
        modifier = Modifier
            .fillMaxHeight()
            .widthIn(min = 38.dp)
            .clip(RoundedCornerShape(10.dp))
            .combinedClickable(onClick = onClick, onLongClick = onLongClick),
    ) {
        Box(contentAlignment = Alignment.Center, modifier = Modifier.padding(horizontal = 7.dp)) {
            Text(text, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
    }
}

@Composable
private fun StripActionChip(text: String, highlighted: Boolean, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        color = if (highlighted) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor = if (highlighted) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface,
        shape = RoundedCornerShape(9.dp),
        modifier = Modifier.fillMaxHeight().widthIn(min = 32.dp),
    ) {
        Box(contentAlignment = Alignment.Center, modifier = Modifier.padding(horizontal = 8.dp)) {
            Text(text, fontSize = 15.sp, fontWeight = if (highlighted) FontWeight.SemiBold else FontWeight.Medium, maxLines = 1)
        }
    }
}

@Composable
private fun StripIconChip(label: String, active: Boolean, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        color = if (active) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface,
        shape = RoundedCornerShape(10.dp),
        modifier = Modifier.fillMaxHeight().widthIn(min = 34.dp),
    ) {
        Box(contentAlignment = Alignment.Center, modifier = Modifier.padding(horizontal = 6.dp)) {
            Text(label, fontSize = 14.sp, maxLines = 1)
        }
    }
}
