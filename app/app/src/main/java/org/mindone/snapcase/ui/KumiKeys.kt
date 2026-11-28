// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.mindone.snapcase.R
import org.mindone.snapcase.data.AppPreferences
import org.mindone.snapcase.kumi.CaseHotkey
import org.mindone.snapcase.kumi.HidUsages
import org.mindone.snapcase.kumi.KeyAction
import org.mindone.snapcase.kumi.KeyCombo
import org.mindone.snapcase.kumi.KeyConfig
import org.mindone.snapcase.kumi.KeymapEntry
import org.mindone.snapcase.kumi.KumiKeymapPreset
import org.mindone.snapcase.kumi.KumiKeymapPresets
import org.mindone.snapcase.kumi.KumiLimits
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.MacroProgram
import org.mindone.snapcase.kumi.MacroStep
import org.mindone.snapcase.kumi.MouseAction

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun KumiKeysScreen(
    kumi: KumiSession,
    prefs: AppPreferences,
    onBack: () -> Unit = {},
    onOpenAndroidKeys: () -> Unit,
    onOpenIme: () -> Unit,
    onOpenShortcuts: () -> Unit,
    onOpenTester: () -> Unit,
) {
    val state by kumi.state.collectAsStateWithLifecycle()
    val settings by prefs.settings.collectAsStateWithLifecycle()
    var page by rememberSaveable { mutableIntStateOf(0) }
    var layer by rememberSaveable { mutableIntStateOf(0) }
    var adding by remember { mutableStateOf(false) }
    var addingCombo by remember { mutableStateOf(false) }
    var clearing by remember { mutableStateOf(false) }
    var editingMacro by remember { mutableStateOf<Int?>(null) }
    var selectedPreset by remember { mutableStateOf<KumiKeymapPreset?>(null) }
    val entries = state.keymap.filter { it.layer == layer }
    val layers = if (state.device?.capabilities?.layers == false) 1 else KeymapEntry.LAYERS
    val keyConfig = state.keyConfig ?: KeyConfig()
    val freeComboSlot = state.combos.indexOfFirst { it == null }

    val listState = androidx.compose.foundation.lazy.rememberLazyListState()
    androidx.compose.runtime.LaunchedEffect(page) { listState.scrollToItem(0) }

    val shTab = stringResource(R.string.kumi_key_hint_tab)
    val shStrip = stringResource(R.string.kumi_tile_strip)
    val shShortcuts = stringResource(R.string.shortcuts_tile)
    val shShortcutsTitle = stringResource(R.string.shortcuts_title)
    val shAndroidKeys = stringResource(R.string.keys_title)
    val shTester = stringResource(R.string.kb_tester_title)

    val tab0Keys = rememberMnemonicKeys(
        specs = listOf(
            ShortcutSpec("strip", shStrip, KeyEvent.KEYCODE_K),
            ShortcutSpec("shortcuts", shShortcutsTitle, KeyEvent.KEYCODE_S, hintLabel = shShortcuts),
            ShortcutSpec("remap", shAndroidKeys, KeyEvent.KEYCODE_R),
            ShortcutSpec("tester", shTester, KeyEvent.KEYCODE_T),
        ),
    )

    RegisterScreenTabs(count = 3, current = page, onSelect = { page = it })
    RegisterScreenScroll(listState)

    RegisterScreenKeys(
        key = listOf(page, layer, tab0Keys),
        shortcuts = if (page == 0) {
            tab0Keys.pairs() + ("↵" to shTab)
        } else {
            listOf("↵" to shTab, "⌫" to stringResource(R.string.action_back))
        },
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        if (page == 0) {
            when (tab0Keys.idFor(code)) {
                "strip" -> { onOpenIme(); true }
                "shortcuts" -> { onOpenShortcuts(); true }
                "remap" -> { onOpenAndroidKeys(); true }
                "tester" -> { onOpenTester(); true }
                else -> false
            }
        } else {
            false
        }
    }

    Column(Modifier.fillMaxSize()) {
        InlineTabHeader(
            tabs = listOf(
                stringResource(R.string.kumi_keys_tab_strip),
                stringResource(R.string.kumi_keys_tab_remap),
                stringResource(R.string.kumi_keys_tab_more),
            ),
            selectedIndex = page,
            onSelect = { page = it },
            onBack = onBack,
        )
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = 12.dp),
            state = listState,
            contentPadding = PaddingValues(top = 2.dp, bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (state.error != null) {
                item { KumiErrorBanner(state, kumi::clearError) }
            }
            if (state.device != null && !state.keepsSettings) {
                item { Note(stringResource(R.string.kumi_keys_ram_note), MaterialTheme.colorScheme.error) }
            }
            if (state.keyboard?.capsWord == true) {
                item {
                    StatusPill(
                        text = stringResource(R.string.kumi_caps_word_active),
                        containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                    )
                }
            }
            when (page) {
                0 -> {
                    item {
                        Card(
                            Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(16.dp),
                            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                        ) {
                            Column(Modifier.padding(horizontal = 12.dp, vertical = 4.dp)) {
                                NavRow(R.drawable.ic_keyboard, R.string.kumi_tile_strip, stringResource(R.string.ime_settings_subtitle), onOpenIme, keyHint = tab0Keys.label("strip"))
                                NavRow(R.drawable.ic_search, R.string.shortcuts_title, stringResource(R.string.shortcuts_subtitle), onOpenShortcuts, keyHint = tab0Keys.label("shortcuts"))
                            }
                        }
                    }
                    if (state.connected) {
                        item {
                            SectionCard(stringResource(R.string.kumi_keys_presets_title)) {
                                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                    items(KumiKeymapPresets.ALL, key = { it.id }) { preset ->
                                        AssistChip(
                                            onClick = { selectedPreset = preset },
                                            label = { Text(stringResource(preset.nameRes)) },
                                        )
                                    }
                                }
                            }
                        }
                    }
                    item {
                        SectionCard(stringResource(R.string.kumi_keys_tools_title)) {
                            AndroidKeyboardRows(settings.remapTable.entries.size, onOpenAndroidKeys, remapKeyHint = tab0Keys.label("remap"))
                            NavRow(R.drawable.ic_check, R.string.kb_tester_title, stringResource(R.string.kb_tester_text), onOpenTester, keyHint = tab0Keys.label("tester"))
                        }
                    }
                }
                1 -> {
                    if (state.connected) {
                        item {
                            SectionCard(
                                title = stringResource(R.string.kumi_keys_custom_layers_title),
                                trailing = { Note("${state.keymap.size}") },
                            ) {
                                if (layers > 1) {
                                    CompactPageTabs(
                                        labels = (0 until layers).map { "L${it + 1}" },
                                        selectedIndex = layer,
                                        onSelect = { layer = it },
                                    )
                                }
                                if (entries.isEmpty()) {
                                    Note(stringResource(R.string.kumi_keys_layer_empty))
                                } else {
                                    entries.forEach { entry ->
                                        ListItem(
                                            headlineContent = { Text("${HidUsages.key(entry.source)}  →  ${describeAction(entry.action)}") },
                                            trailingContent = {
                                                IconButton(onClick = { kumi.setKey(entry.copy(action = KeyAction.Transparent)) }) {
                                                    Icon(painterResource(R.drawable.ic_delete), contentDescription = stringResource(R.string.action_remove))
                                                }
                                            },
                                            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                        )
                                    }
                                }
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    FilledTonalButton(onClick = { adding = true }, modifier = Modifier.weight(1f)) {
                                        Text(stringResource(R.string.kumi_keys_add))
                                    }
                                    if (state.keymap.isNotEmpty()) {
                                        OutlinedButton(onClick = { clearing = true }) {
                                            Text(stringResource(R.string.action_clear))
                                        }
                                    }
                                }
                            }
                        }
                        item {
                            val activeCombos = state.combos.mapIndexedNotNull { index, combo -> combo?.let { index to it } }
                            SectionCard(
                                title = stringResource(R.string.kumi_keys_combos_title),
                                trailing = { Note("${activeCombos.size}") },
                            ) {
                                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                    item {
                                        val existingJk = state.combos.indexOfFirst {
                                            it != null && ((it.keyA == 0x0D && it.keyB == 0x0E) || (it.keyA == 0x0E && it.keyB == 0x0D))
                                        }
                                        val jkTarget = existingJk.takeIf { it >= 0 } ?: freeComboSlot
                                        AssistChip(
                                            enabled = jkTarget >= 0,
                                            onClick = {
                                                if (jkTarget >= 0) kumi.setCombo(jkTarget, KeyCombo(0x0D, 0x0E, KeyAction.Key(0x29)))
                                            },
                                            label = { Text(stringResource(R.string.kumi_keys_combo_tpl_jk)) },
                                        )
                                    }
                                    item {
                                        val existingDf = state.combos.indexOfFirst {
                                            it != null && ((it.keyA == 0x07 && it.keyB == 0x09) || (it.keyA == 0x09 && it.keyB == 0x07))
                                        }
                                        val dfTarget = existingDf.takeIf { it >= 0 } ?: freeComboSlot
                                        AssistChip(
                                            enabled = dfTarget >= 0,
                                            onClick = {
                                                if (dfTarget >= 0) kumi.setCombo(dfTarget, KeyCombo(0x07, 0x09, KeyAction.CapsWord))
                                            },
                                            label = { Text(stringResource(R.string.kumi_keys_combo_tpl_caps)) },
                                        )
                                    }
                                }
                                if (activeCombos.isEmpty()) {
                                    Note(stringResource(R.string.kumi_keys_combos_empty))
                                } else {
                                    activeCombos.forEach { (slot, combo) ->
                                        ListItem(
                                            headlineContent = {
                                                Text("${HidUsages.key(combo.keyA)} + ${HidUsages.key(combo.keyB)}  →  ${describeAction(combo.action)}")
                                            },
                                            trailingContent = {
                                                IconButton(onClick = { kumi.setCombo(slot, null) }) {
                                                    Icon(painterResource(R.drawable.ic_delete), contentDescription = stringResource(R.string.action_remove))
                                                }
                                            },
                                            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                        )
                                    }
                                }
                                if (freeComboSlot >= 0) {
                                    FilledTonalButton(onClick = { addingCombo = true }, modifier = Modifier.fillMaxWidth()) {
                                        Text(stringResource(R.string.kumi_keys_combo_add))
                                    }
                                }
                            }
                        }
                    } else {
                        item {
                            SectionCard(stringResource(R.string.kumi_case_absent)) {
                                Note(stringResource(R.string.kumi_case_absent_text))
                            }
                        }
                    }
                }
                else -> {
                    if (state.connected) {
                        item {
                            val activeMacros = state.macros.count { it != null && it.steps.isNotEmpty() }
                            SectionCard(
                                title = stringResource(R.string.kumi_keys_macros_title),
                                trailing = { Note("$activeMacros/${KumiLimits.MACRO_SLOTS}") },
                            ) {
                                val rows = (0 until KumiLimits.MACRO_SLOTS).toList().chunked(2)
                                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                                    rows.forEach { pair ->
                                        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                            pair.forEach { slot ->
                                                val program = state.macros.getOrNull(slot)
                                                val count = program?.steps?.size ?: 0
                                                Surface(
                                                    color = if (count > 0) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                                                    contentColor = if (count > 0) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface,
                                                    shape = RoundedCornerShape(10.dp),
                                                    modifier = Modifier
                                                        .weight(1f)
                                                        .height(32.dp)
                                                        .clip(RoundedCornerShape(10.dp))
                                                        .clickable { editingMacro = slot },
                                                ) {
                                                    Row(
                                                        Modifier.fillMaxSize().padding(horizontal = 10.dp),
                                                        verticalAlignment = Alignment.CenterVertically,
                                                        horizontalArrangement = Arrangement.SpaceBetween,
                                                    ) {
                                                        Text(
                                                            stringResource(R.string.kumi_keys_macro_slot, slot + 1),
                                                            style = MaterialTheme.typography.labelLarge,
                                                            fontWeight = FontWeight.SemiBold,
                                                            maxLines = 1,
                                                        )
                                                        Text(
                                                            if (count == 0) stringResource(R.string.kumi_keys_macro_empty)
                                                            else stringResource(R.string.kumi_keys_macro_steps, count),
                                                            style = MaterialTheme.typography.labelSmall,
                                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                                            maxLines = 1,
                                                            overflow = TextOverflow.Ellipsis,
                                                        )
                                                    }
                                                }
                                            }
                                            if (pair.size == 1) Spacer(Modifier.weight(1f))
                                        }
                                    }
                                }
                            }
                        }
                        state.mediaGestures?.let { media ->
                            item {
                                SectionCard(stringResource(R.string.kumi_keys_media_title)) {
                                    ToggleRow(
                                        title = stringResource(R.string.kumi_keys_media_toggle),
                                        subtitle = stringResource(R.string.kumi_keys_media_desc),
                                        checked = media.enabled,
                                        onCheckedChange = { kumi.setMediaGestures(media.copy(enabled = it)) },
                                    )
                                    if (media.enabled) {
                                        Row(
                                            Modifier.fillMaxWidth(),
                                            verticalAlignment = Alignment.CenterVertically,
                                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                                        ) {
                                            Text(
                                                stringResource(R.string.kumi_keys_media_guard),
                                                style = MaterialTheme.typography.labelLarge,
                                                modifier = Modifier.weight(1f),
                                            )
                                            listOf(0, 1_200, 2_500).forEach { guardMs ->
                                                val selected = media.idleGuardMs == guardMs
                                                Surface(
                                                    color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                                                    contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                                                    shape = CircleShape,
                                                    modifier = Modifier
                                                        .height(28.dp)
                                                        .clip(CircleShape)
                                                        .clickable { kumi.setMediaGestures(media.copy(idleGuardMs = guardMs)) },
                                                ) {
                                                    Box(Modifier.padding(horizontal = 10.dp), contentAlignment = Alignment.Center) {
                                                        Text(
                                                            if (guardMs == 0) {
                                                                stringResource(R.string.kumi_crossfeed_off)
                                                            } else {
                                                                stringResource(R.string.kumi_keys_media_guard_sec, guardMs / 1000.0)
                                                            },
                                                            style = MaterialTheme.typography.labelSmall,
                                                            fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                                                        )
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        item {
                            SectionCard(stringResource(R.string.kumi_keys_timing_title)) {
                                MillisSlider(
                                    label = stringResource(R.string.kumi_keys_tapping_term),
                                    value = keyConfig.tappingTermMs,
                                    min = KumiLimits.KEY_MIN_TAPPING_TERM_MS,
                                    max = KumiLimits.KEY_MAX_TAPPING_TERM_MS,
                                    step = 10,
                                ) { kumi.setKeyConfig(keyConfig.copy(tappingTermMs = it)) }
                                MillisSlider(
                                    label = stringResource(R.string.kumi_keys_oneshot_timeout),
                                    value = keyConfig.oneShotTimeoutMs,
                                    min = KumiLimits.KEY_MIN_ONE_SHOT_MS,
                                    max = KumiLimits.KEY_MAX_ONE_SHOT_MS,
                                    step = 250,
                                ) { kumi.setKeyConfig(keyConfig.copy(oneShotTimeoutMs = it)) }
                                MillisSlider(
                                    label = stringResource(R.string.kumi_keys_combo_term),
                                    value = keyConfig.comboTermMs,
                                    min = KumiLimits.KEY_MIN_COMBO_TERM_MS,
                                    max = KumiLimits.KEY_MAX_COMBO_TERM_MS,
                                    step = 5,
                                ) { kumi.setKeyConfig(keyConfig.copy(comboTermMs = it)) }
                            }
                        }
                        if (state.keyEvents.isNotEmpty()) {
                            item {
                                SectionCard(stringResource(R.string.kumi_keys_recent)) {
                                    state.keyEvents.take(6).forEach { event ->
                                        Mono("${if (event.pressed) "▼" else "▲"} ${HidUsages.key(event.source)} · L${event.layer + 1} · ${describeAction(event.action)}")
                                    }
                                }
                            }
                        }
                    } else {
                        item {
                            SectionCard(stringResource(R.string.kumi_case_absent)) {
                                Note(stringResource(R.string.kumi_case_absent_text))
                            }
                        }
                    }
                }
            }
        }
        KeyboardHintStrip(
            hints = if (page == 0) {
                listOf(
                    tab0Keys.label("strip") to shStrip,
                    tab0Keys.label("shortcuts") to shShortcuts,
                    tab0Keys.label("tester") to shTester,
                    "↵" to shTab,
                )
            } else {
                listOf("↵" to shTab, "⌫" to stringResource(R.string.action_back))
            },
        )
    }

    if (adding) {
        AddKeyDialog(layer, layers, onDismiss = { adding = false }) { entry ->
            adding = false
            kumi.setKey(entry)
        }
    }
    if (addingCombo && freeComboSlot >= 0) {
        ComboEditorDialog(
            onDismiss = { addingCombo = false },
            onSave = { combo ->
                addingCombo = false
                kumi.setCombo(freeComboSlot, combo)
            },
        )
    }
    editingMacro?.let { slot ->
        MacroEditorDialog(
            slot = slot,
            initial = state.macros.getOrNull(slot) ?: MacroProgram.EMPTY,
            onDismiss = { editingMacro = null },
            onSave = { program ->
                editingMacro = null
                kumi.setMacro(slot, program)
            },
        )
    }
    if (clearing) {
        ConfirmDialog(
            title = stringResource(R.string.kumi_keys_clear),
            text = stringResource(R.string.kumi_keys_clear_text),
            onConfirm = { kumi.clearKeymap() },
            onDismiss = { clearing = false },
        )
    }
    selectedPreset?.let { preset ->
        AlertDialog(
            onDismissRequest = { selectedPreset = null },
            title = { Text(stringResource(preset.nameRes)) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(stringResource(preset.descRes), style = MaterialTheme.typography.bodyMedium)
                    Note(stringResource(R.string.kumi_preset_summary, preset.entries.size, preset.combos.size))
                }
            },
            confirmButton = {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    TextButton(onClick = {
                        selectedPreset = null
                        kumi.applyKeymapPreset(preset, replaceExisting = true)
                    }) {
                        Text(stringResource(R.string.kumi_preset_replace))
                    }
                    TextButton(onClick = {
                        selectedPreset = null
                        kumi.applyKeymapPreset(preset, replaceExisting = false)
                    }) {
                        Text(stringResource(R.string.kumi_preset_merge))
                    }
                }
            },
            dismissButton = {
                TextButton(onClick = { selectedPreset = null }) { Text(stringResource(R.string.action_cancel)) }
            },
        )
    }
}

@Composable
private fun describeAction(action: KeyAction): String = when (action) {
    KeyAction.Transparent -> stringResource(R.string.kumi_action_desc_default)
    KeyAction.Disabled -> stringResource(R.string.kumi_action_disabled)
    is KeyAction.Key -> HidUsages.key(action.usage)
    is KeyAction.Modifiers -> HidUsages.modifiers(action.mask)
    is KeyAction.KeyWithModifiers -> "${HidUsages.modifiers(action.modifiers)}+${HidUsages.key(action.usage)}"
    is KeyAction.Consumer -> HidUsages.consumer[action.usage] ?: "0x%03X".format(action.usage)
    is KeyAction.LayerHold -> stringResource(R.string.kumi_action_desc_layer_hold, action.layer + 1)
    is KeyAction.LayerToggle -> stringResource(R.string.kumi_action_desc_layer_toggle, action.layer + 1)
    is KeyAction.TapHoldMod -> stringResource(R.string.kumi_action_desc_tap_hold_mod, HidUsages.key(action.tap), HidUsages.modifiers(action.holdMods))
    is KeyAction.TapHoldLayer -> stringResource(R.string.kumi_action_desc_tap_hold_layer, HidUsages.key(action.tap), action.holdLayer + 1)
    is KeyAction.OneShotMod -> stringResource(R.string.kumi_action_desc_one_shot, HidUsages.modifiers(action.mods))
    is KeyAction.Macro -> stringResource(R.string.kumi_action_desc_macro, action.slot + 1)
    is KeyAction.CaseControl -> stringResource(hotkeyTitle(action.hotkey))
    KeyAction.CapsWord -> stringResource(R.string.kumi_action_caps_word)
    is KeyAction.Mouse -> stringResource(mouseActionTitle(action.action))
    is KeyAction.Unsupported -> stringResource(R.string.kumi_action_desc_unsupported)
}

private fun formatMacroSteps(steps: List<MacroStep>): String = steps.joinToString(" ") { step ->
    if (step.modifiers == 0) HidUsages.key(step.usage) else "${HidUsages.modifiers(step.modifiers)}+${HidUsages.key(step.usage)}"
}

private fun hotkeyTitle(hotkey: CaseHotkey): Int = when (hotkey) {
    CaseHotkey.CycleEq -> R.string.kumi_hotkey_cycle_eq
    CaseHotkey.CycleFilter -> R.string.kumi_hotkey_cycle_filter
    CaseHotkey.CycleCrossfeed -> R.string.kumi_hotkey_cycle_crossfeed
    CaseHotkey.ToggleLimiter -> R.string.kumi_hotkey_toggle_limiter
    CaseHotkey.ToggleMono -> R.string.kumi_hotkey_toggle_mono
    CaseHotkey.ToggleMute -> R.string.kumi_hotkey_toggle_mute
    CaseHotkey.ToggleLineOut -> R.string.kumi_hotkey_toggle_line_out
    CaseHotkey.ToggleLoudness -> R.string.kumi_hotkey_toggle_loudness
    CaseHotkey.ToggleCapsWord -> R.string.kumi_hotkey_toggle_caps_word
}

private fun mouseActionTitle(action: MouseAction): Int = when (action) {
    MouseAction.MoveUp -> R.string.kumi_mouse_up
    MouseAction.MoveDown -> R.string.kumi_mouse_down
    MouseAction.MoveLeft -> R.string.kumi_mouse_left
    MouseAction.MoveRight -> R.string.kumi_mouse_right
    MouseAction.LeftClick -> R.string.kumi_mouse_left_click
    MouseAction.RightClick -> R.string.kumi_mouse_right_click
    MouseAction.MiddleClick -> R.string.kumi_mouse_middle_click
    MouseAction.WheelUp -> R.string.kumi_mouse_wheel_up
    MouseAction.WheelDown -> R.string.kumi_mouse_wheel_down
}

private enum class ActionType {
    KEY, CHORD, MEDIA, LAYER_HOLD, LAYER_TOGGLE, TAP_HOLD_MOD, TAP_HOLD_LAYER, ONE_SHOT, MACRO, CASE_HOTKEY, CAPS_WORD, MOUSE, DISABLED,
}

private fun actionTypeName(type: ActionType): Int = when (type) {
    ActionType.KEY -> R.string.kumi_action_key
    ActionType.CHORD -> R.string.kumi_action_chord
    ActionType.MEDIA -> R.string.kumi_action_media
    ActionType.LAYER_HOLD -> R.string.kumi_action_layer_hold
    ActionType.LAYER_TOGGLE -> R.string.kumi_action_layer_toggle
    ActionType.TAP_HOLD_MOD -> R.string.kumi_action_tap_hold_mod
    ActionType.TAP_HOLD_LAYER -> R.string.kumi_action_tap_hold_layer
    ActionType.ONE_SHOT -> R.string.kumi_action_one_shot
    ActionType.MACRO -> R.string.kumi_action_macro
    ActionType.CASE_HOTKEY -> R.string.kumi_action_case_hotkey
    ActionType.CAPS_WORD -> R.string.kumi_action_caps_word
    ActionType.MOUSE -> R.string.kumi_action_mouse
    ActionType.DISABLED -> R.string.kumi_action_disabled
}

@Composable
private fun AddKeyDialog(layer: Int, layers: Int, onDismiss: () -> Unit, onAdd: (KeymapEntry) -> Unit) {
    SuppressScreenKeys()
    val nonModifierKeys = remember { HidUsages.selectable.filter { it.first in KumiLimits.KEY_MIN_USAGE..KumiLimits.KEY_MAX_USAGE } }
    var source by remember { mutableIntStateOf(0x39) }
    var type by remember { mutableStateOf(ActionType.KEY) }
    var target by remember { mutableIntStateOf(0x29) }
    var modifiers by remember { mutableIntStateOf(0x01) }
    var media by remember { mutableIntStateOf(HidUsages.consumer.keys.first()) }
    var macroSlot by remember { mutableIntStateOf(0) }
    var hotkey by remember { mutableStateOf(CaseHotkey.CycleEq) }
    var mouseAction by remember { mutableStateOf(MouseAction.LeftClick) }
    val layerTargets = HidUsages.layerTargets(layer, layers)
    var targetLayer by remember { mutableIntStateOf(layerTargets.firstOrNull() ?: 1) }
    val types = ActionType.entries.filter {
        layerTargets.isNotEmpty() || (it != ActionType.LAYER_HOLD && it != ActionType.LAYER_TOGGLE && it != ActionType.TAP_HOLD_LAYER)
    }
    val tapKey = nonModifierKeys.firstOrNull { it.first == target }?.first ?: nonModifierKeys.first().first

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.kumi_keys_add)) },
        text = {
            Column(Modifier.heightIn(max = 460.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Picker(stringResource(R.string.kumi_keys_source), HidUsages.selectable, source) { source = it }
                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    items(types) { t ->
                        FilterChip(selected = type == t, onClick = { type = t }, label = { Text(stringResource(actionTypeName(t))) })
                    }
                }
                when (type) {
                    ActionType.KEY -> Picker(stringResource(R.string.kumi_keys_target), HidUsages.selectable, target) { target = it }
                    ActionType.CHORD -> {
                        Picker(stringResource(R.string.kumi_keys_target), HidUsages.selectable, target) { target = it }
                        ModifierChipRow(modifiers) { modifiers = it }
                    }
                    ActionType.MEDIA -> Picker(stringResource(R.string.kumi_action_media), HidUsages.consumer.entries.map { it.key to it.value }, media) { media = it }
                    ActionType.LAYER_HOLD, ActionType.LAYER_TOGGLE -> LayerChipRow(layerTargets, targetLayer) { targetLayer = it }
                    ActionType.TAP_HOLD_MOD -> {
                        Picker(stringResource(R.string.kumi_keys_tap_key), nonModifierKeys, tapKey) { target = it }
                        Text(stringResource(R.string.kumi_keys_hold_mod), style = MaterialTheme.typography.labelMedium)
                        ModifierChipRow(modifiers) { modifiers = it }
                    }
                    ActionType.TAP_HOLD_LAYER -> {
                        Picker(stringResource(R.string.kumi_keys_tap_key), nonModifierKeys, tapKey) { target = it }
                        Text(stringResource(R.string.kumi_keys_hold_layer), style = MaterialTheme.typography.labelMedium)
                        LayerChipRow(layerTargets, targetLayer) { targetLayer = it }
                    }
                    ActionType.ONE_SHOT -> {
                        Text(stringResource(R.string.kumi_keys_hold_mod), style = MaterialTheme.typography.labelMedium)
                        ModifierChipRow(modifiers) { modifiers = it }
                    }
                    ActionType.MACRO -> {
                        LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            items((0 until KumiLimits.MACRO_SLOTS).toList()) { slot ->
                                FilterChip(selected = macroSlot == slot, onClick = { macroSlot = slot }, label = { Text("M${slot + 1}") })
                            }
                        }
                    }
                    ActionType.CASE_HOTKEY -> {
                        val options = CaseHotkey.entries.mapIndexed { idx, hk -> idx to stringResource(hotkeyTitle(hk)) }
                        Picker(stringResource(R.string.kumi_keys_hotkey), options, CaseHotkey.entries.indexOf(hotkey)) {
                            hotkey = CaseHotkey.entries[it]
                        }
                    }
                    ActionType.CAPS_WORD -> Note(stringResource(R.string.kumi_action_caps_word_text))
                    ActionType.MOUSE -> {
                        val options = MouseAction.entries.mapIndexed { idx, act -> idx to stringResource(mouseActionTitle(act)) }
                        Picker(stringResource(R.string.kumi_action_mouse), options, MouseAction.entries.indexOf(mouseAction)) {
                            mouseAction = MouseAction.entries[it]
                        }
                    }
                    ActionType.DISABLED -> Note(stringResource(R.string.kumi_action_disabled_text))
                }
            }
        },
        confirmButton = {
            val action = when (type) {
                ActionType.KEY -> HidUsages.keyAction(target)
                ActionType.CHORD -> HidUsages.chord(target, modifiers)
                ActionType.MEDIA -> KeyAction.Consumer(media)
                ActionType.LAYER_HOLD -> KeyAction.LayerHold(targetLayer)
                ActionType.LAYER_TOGGLE -> KeyAction.LayerToggle(targetLayer)
                ActionType.TAP_HOLD_MOD -> KeyAction.TapHoldMod(tapKey, modifiers)
                ActionType.TAP_HOLD_LAYER -> KeyAction.TapHoldLayer(tapKey, targetLayer)
                ActionType.ONE_SHOT -> KeyAction.OneShotMod(modifiers)
                ActionType.MACRO -> KeyAction.Macro(macroSlot)
                ActionType.CASE_HOTKEY -> KeyAction.CaseControl(hotkey)
                ActionType.CAPS_WORD -> KeyAction.CapsWord
                ActionType.MOUSE -> KeyAction.Mouse(mouseAction)
                ActionType.DISABLED -> KeyAction.Disabled
            }
            TextButton(onClick = { onAdd(KeymapEntry(layer, source, action)) }, enabled = action.valid) {
                Text(stringResource(R.string.action_add))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

@Composable
private fun ComboEditorDialog(
    onDismiss: () -> Unit,
    onSave: (KeyCombo) -> Unit,
) {
    SuppressScreenKeys()
    var keyA by remember { mutableIntStateOf(0x0D) }
    var keyB by remember { mutableIntStateOf(0x0E) }
    var type by remember { mutableStateOf(ActionType.KEY) }
    var target by remember { mutableIntStateOf(0x29) }
    var modifiers by remember { mutableIntStateOf(0x01) }
    var media by remember { mutableIntStateOf(HidUsages.consumer.keys.first()) }
    var macroSlot by remember { mutableIntStateOf(0) }
    var hotkey by remember { mutableStateOf(CaseHotkey.CycleEq) }
    var mouseAction by remember { mutableStateOf(MouseAction.LeftClick) }
    val comboTypes = listOf(
        ActionType.KEY, ActionType.CHORD, ActionType.MEDIA, ActionType.CAPS_WORD, ActionType.MOUSE, ActionType.MACRO, ActionType.CASE_HOTKEY,
    )

    val action = when (type) {
        ActionType.KEY -> HidUsages.keyAction(target)
        ActionType.CHORD -> HidUsages.chord(target, modifiers)
        ActionType.MEDIA -> KeyAction.Consumer(media)
        ActionType.CAPS_WORD -> KeyAction.CapsWord
        ActionType.MOUSE -> KeyAction.Mouse(mouseAction)
        ActionType.MACRO -> KeyAction.Macro(macroSlot)
        ActionType.CASE_HOTKEY -> KeyAction.CaseControl(hotkey)
        else -> KeyAction.Disabled
    }
    val combo = KeyCombo(keyA, keyB, action)

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.kumi_keys_combo_add)) },
        text = {
            Column(Modifier.heightIn(max = 460.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Picker(stringResource(R.string.kumi_keys_combo_key_a), HidUsages.selectable, keyA) { keyA = it }
                Picker(stringResource(R.string.kumi_keys_combo_key_b), HidUsages.selectable, keyB) { keyB = it }
                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    items(comboTypes) { t ->
                        FilterChip(selected = type == t, onClick = { type = t }, label = { Text(stringResource(actionTypeName(t))) })
                    }
                }
                when (type) {
                    ActionType.KEY -> Picker(stringResource(R.string.kumi_keys_target), HidUsages.selectable, target) { target = it }
                    ActionType.CHORD -> {
                        Picker(stringResource(R.string.kumi_keys_target), HidUsages.selectable, target) { target = it }
                        ModifierChipRow(modifiers) { modifiers = it }
                    }
                    ActionType.MEDIA -> Picker(stringResource(R.string.kumi_action_media), HidUsages.consumer.entries.map { it.key to it.value }, media) { media = it }
                    ActionType.CAPS_WORD -> Note(stringResource(R.string.kumi_action_caps_word_text))
                    ActionType.MOUSE -> {
                        val options = MouseAction.entries.mapIndexed { idx, act -> idx to stringResource(mouseActionTitle(act)) }
                        Picker(stringResource(R.string.kumi_action_mouse), options, MouseAction.entries.indexOf(mouseAction)) {
                            mouseAction = MouseAction.entries[it]
                        }
                    }
                    ActionType.MACRO -> {
                        LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            items((0 until KumiLimits.MACRO_SLOTS).toList()) { slot ->
                                FilterChip(selected = macroSlot == slot, onClick = { macroSlot = slot }, label = { Text("M${slot + 1}") })
                            }
                        }
                    }
                    ActionType.CASE_HOTKEY -> {
                        val options = CaseHotkey.entries.mapIndexed { idx, hk -> idx to stringResource(hotkeyTitle(hk)) }
                        Picker(stringResource(R.string.kumi_keys_hotkey), options, CaseHotkey.entries.indexOf(hotkey)) {
                            hotkey = CaseHotkey.entries[it]
                        }
                    }
                    else -> Unit
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { onSave(combo) }, enabled = combo.valid) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

@Composable
private fun MacroEditorDialog(
    slot: Int,
    initial: MacroProgram,
    onDismiss: () -> Unit,
    onSave: (MacroProgram) -> Unit,
) {
    SuppressScreenKeys()
    val nonModifierKeys = remember { HidUsages.selectable.filter { it.first in KumiLimits.KEY_MIN_USAGE..KumiLimits.KEY_MAX_USAGE } }
    val steps = remember { mutableStateListOf<MacroStep>().apply { addAll(initial.steps) } }
    var nextKey by remember { mutableIntStateOf(0x04) }
    var nextMods by remember { mutableIntStateOf(0) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.kumi_keys_macro_edit_title, slot + 1)) },
        text = {
            Column(Modifier.heightIn(max = 440.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (steps.isEmpty()) {
                    Note(stringResource(R.string.kumi_keys_macro_empty))
                } else {
                    steps.forEachIndexed { index, step ->
                        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                "${index + 1}. ${if (step.modifiers == 0) HidUsages.key(step.usage) else "${HidUsages.modifiers(step.modifiers)}+${HidUsages.key(step.usage)}"}",
                                style = MaterialTheme.typography.bodyMedium,
                                modifier = Modifier.weight(1f),
                            )
                            IconButton(onClick = { steps.removeAt(index) }) {
                                Icon(painterResource(R.drawable.ic_delete), contentDescription = stringResource(R.string.action_remove))
                            }
                        }
                    }
                }
                if (steps.size < KumiLimits.MACRO_MAX_STEPS) {
                    Picker(stringResource(R.string.kumi_keys_target), nonModifierKeys, nextKey) { nextKey = it }
                    ModifierChipRow(nextMods) { nextMods = it }
                    FilledTonalButton(
                        onClick = { steps.add(MacroStep(nextKey, nextMods)) },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(stringResource(R.string.kumi_keys_macro_add_step))
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { onSave(MacroProgram(steps.toList())) }) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = {
            Row {
                if (steps.isNotEmpty()) {
                    TextButton(onClick = { steps.clear() }) { Text(stringResource(R.string.action_clear)) }
                }
                TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) }
            }
        },
    )
}

@Composable
private fun ModifierChipRow(modifiers: Int, onChange: (Int) -> Unit) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        items(HidUsages.modifierBits) { (bit, name) ->
            FilterChip(
                selected = modifiers and bit != 0,
                onClick = { onChange(modifiers xor bit) },
                label = { Text(name) },
            )
        }
    }
}

@Composable
private fun LayerChipRow(targets: List<Int>, selected: Int, onSelect: (Int) -> Unit) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        items(targets) { l ->
            FilterChip(selected = selected == l, onClick = { onSelect(l) }, label = { Text(stringResource(R.string.kumi_layer, l + 1)) })
        }
    }
}

@Composable
private fun MillisSlider(label: String, value: Int, min: Int, max: Int, step: Int, onCommit: (Int) -> Unit) {
    var dragging by remember { mutableStateOf(false) }
    var current by remember { mutableFloatStateOf(value.toFloat()) }
    if (!dragging && current != value.toFloat()) current = value.toFloat()
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(stringResource(R.string.kumi_unit_ms, current.toInt()), style = MaterialTheme.typography.labelLarge)
        }
        Slider(
            value = current,
            onValueChange = {
                dragging = true
                current = (Math.round(it / step) * step).toFloat()
            },
            onValueChangeFinished = {
                dragging = false
                if (current.toInt() != value) onCommit(current.toInt())
            },
            valueRange = min.toFloat()..max.toFloat(),
            modifier = Modifier.height(24.dp),
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Picker(label: String, options: List<Pair<Int, String>>, selected: Int, onSelect: (Int) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
        OutlinedTextField(
            value = options.firstOrNull { it.first == selected }?.second ?: HidUsages.key(selected),
            onValueChange = {},
            readOnly = true,
            label = { Text(label) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
            modifier = Modifier.menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, true).fillMaxWidth(),
        )
        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (code, name) ->
                DropdownMenuItem(text = { Text(name) }, onClick = {
                    open = false
                    onSelect(code)
                })
            }
        }
    }
}
