// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.mindone.snapcase.R
import org.mindone.snapcase.ime.EmojiCatalog
import org.mindone.snapcase.ime.ImeUiState
import org.mindone.snapcase.ime.LocaleVariations
import org.mindone.snapcase.ime.StripDrawer
import org.mindone.snapcase.ime.StripLang

@Composable
fun ImeDrawerPanel(
    state: ImeUiState,
    emojiCatalog: EmojiCatalog,
    onPickEmoji: (String) -> Unit,
    onInsertText: (String) -> Unit,
    onPinClip: (String) -> Unit,
    onDeleteClip: (String) -> Unit,
    onClearClips: () -> Unit,
) {
    val drawerHeight = when (state.activeDrawer) {
        StripDrawer.SYMBOLS -> 106.dp
        StripDrawer.EMOJI -> 122.dp
        StripDrawer.CLIPBOARD -> 120.dp
        StripDrawer.NONE -> 0.dp
    }
    if (drawerHeight == 0.dp) return
    Box(Modifier.fillMaxWidth().height(drawerHeight).padding(horizontal = 6.dp, vertical = 4.dp)) {
        when (state.activeDrawer) {
            StripDrawer.EMOJI -> EmojiDrawer(state.emojiQuery, emojiCatalog, onPickEmoji)
            StripDrawer.SYMBOLS -> SymbolsDrawer(state, onInsertText)
            StripDrawer.CLIPBOARD -> ClipboardDrawer(state, onInsertText, onPinClip, onDeleteClip, onClearClips)
            StripDrawer.NONE -> Unit
        }
    }
}

@Composable
private fun EmojiDrawer(
    query: String,
    catalog: EmojiCatalog,
    onPickEmoji: (String) -> Unit,
) {
    var category by remember { mutableStateOf("smileys") }
    val recents = remember(query, category) { catalog.recentGlyphs() }
    val items = remember(query, category, recents) {
        when {
            query.isNotBlank() -> catalog.search(query).map { it.glyph }
            category == "recent" -> recents
            else -> catalog.search("", category).map { it.glyph }
        }
    }

    Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            Modifier.fillMaxWidth().height(26.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Box(
                modifier = Modifier
                    .weight(1f)
                    .height(26.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                    .padding(horizontal = 8.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Text(
                    text = if (query.isBlank()) stringResource(R.string.ime_emoji_search_hint) else "🔍 $query",
                    fontSize = 11.sp,
                    color = if (query.isBlank()) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Row(
                modifier = Modifier.horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                EmojiCatalog.CATEGORIES.forEach { (id, icon) ->
                    val selected = query.isBlank() && category == id
                    Box(
                        modifier = Modifier
                            .height(26.dp)
                            .clip(RoundedCornerShape(7.dp))
                            .background(
                                if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainer,
                            )
                            .clickable { category = id }
                            .padding(horizontal = 5.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(icon, fontSize = 14.sp)
                    }
                }
            }
        }
        if (items.isEmpty()) {
            Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
                Text(
                    stringResource(R.string.ime_emoji_empty),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyVerticalGrid(
                columns = GridCells.Fixed(9),
                modifier = Modifier.fillMaxWidth().weight(1f),
                contentPadding = PaddingValues(vertical = 1.dp),
                horizontalArrangement = Arrangement.spacedBy(2.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                items(items) { glyph ->
                    Box(
                        modifier = Modifier
                            .height(29.dp)
                            .clip(RoundedCornerShape(6.dp))
                            .clickable { onPickEmoji(glyph) },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(glyph, fontSize = 19.sp)
                    }
                }
            }
        }
    }
}

@Composable
private fun SymbolsDrawer(state: ImeUiState, onInsertText: (String) -> Unit) {
    val extras = remember(state.lang) { LocaleVariations.extrasFor(state.lang) }
    val allSymbols = remember(extras) { (extras + LocaleVariations.commonSymbols).distinct() }
    val chipBg = MaterialTheme.colorScheme.surfaceContainerHigh
    val chipFg = MaterialTheme.colorScheme.onSurface
    LazyVerticalGrid(
        columns = GridCells.Fixed(9),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(vertical = 1.dp),
        horizontalArrangement = Arrangement.spacedBy(3.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        items(allSymbols) { symbol ->
            val display = if (state.effectiveUpper && (state.lang == StripLang.RU || state.lang == StripLang.UK) && symbol.length == 1 && symbol[0].isLetter()) {
                symbol.uppercase(java.util.Locale.ROOT)
            } else {
                symbol
            }
            Box(
                modifier = Modifier
                    .height(30.dp)
                    .clip(RoundedCornerShape(7.dp))
                    .background(chipBg)
                    .clickable { onInsertText(display) },
                contentAlignment = Alignment.Center,
            ) {
                Text(display, fontSize = 15.sp, fontWeight = FontWeight.Medium, color = chipFg)
            }
        }
    }
}

@Composable
private fun ClipboardDrawer(
    state: ImeUiState,
    onInsertText: (String) -> Unit,
    onPinClip: (String) -> Unit,
    onDeleteClip: (String) -> Unit,
    onClearClips: () -> Unit,
) {
    var tab by remember { mutableIntStateOf(0) }
    Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            Modifier.fillMaxWidth().height(26.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            DrawerTabChip(stringResource(R.string.ime_drawer_clipboard), selected = tab == 0) { tab = 0 }
            DrawerTabChip(stringResource(R.string.ime_drawer_snippets), selected = tab == 1) { tab = 1 }
            Box(Modifier.weight(1f))
            if (tab == 0 && state.clipboardItems.any { !it.pinned }) {
                Box(
                    modifier = Modifier
                        .height(26.dp)
                        .clip(RoundedCornerShape(7.dp))
                        .clickable(onClick = onClearClips)
                        .padding(horizontal = 8.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        stringResource(R.string.action_clear),
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Medium,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            }
        }
        if (tab == 0) {
            if (state.clipboardItems.isEmpty()) {
                Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
                    Text(
                        stringResource(R.string.ime_clipboard_empty),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxWidth().weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    items(state.clipboardItems, key = { it.text }) { item ->
                        Surface(
                            color = MaterialTheme.colorScheme.surfaceContainerHigh,
                            shape = RoundedCornerShape(8.dp),
                            modifier = Modifier.fillMaxWidth().clickable { onInsertText(item.text) },
                        ) {
                            Row(
                                Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 5.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Text(
                                    text = item.text.replace('\n', ' '),
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    style = MaterialTheme.typography.bodySmall,
                                    modifier = Modifier.weight(1f),
                                )
                                Text(
                                    text = if (item.pinned) "📌" else "📍",
                                    fontSize = 13.sp,
                                    modifier = Modifier.clickable { onPinClip(item.text) }.padding(horizontal = 3.dp),
                                )
                                Text(
                                    text = "✕",
                                    fontSize = 13.sp,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.clickable { onDeleteClip(item.text) }.padding(horizontal = 3.dp),
                                )
                            }
                        }
                    }
                }
            }
        } else {
            if (state.snippets.isEmpty()) {
                Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
                    Text(
                        stringResource(R.string.ime_snippets_empty),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(Modifier.fillMaxWidth().weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    items(state.snippets, key = { it.trigger }) { snippet ->
                        Surface(
                            color = MaterialTheme.colorScheme.surfaceContainerHigh,
                            shape = RoundedCornerShape(8.dp),
                            modifier = Modifier.fillMaxWidth().clickable { onInsertText(snippet.expansion) },
                        ) {
                            Row(
                                Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 5.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Text(
                                    text = snippet.trigger,
                                    style = MaterialTheme.typography.labelMedium,
                                    color = MaterialTheme.colorScheme.primary,
                                )
                                Text(
                                    text = "→ ${snippet.expansion}",
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    style = MaterialTheme.typography.bodySmall,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DrawerTabChip(label: String, selected: Boolean, onClick: () -> Unit) {
    Box(
        modifier = Modifier
            .height(26.dp)
            .clip(RoundedCornerShape(7.dp))
            .background(if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            fontSize = 12.sp,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
            color = if (selected) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
