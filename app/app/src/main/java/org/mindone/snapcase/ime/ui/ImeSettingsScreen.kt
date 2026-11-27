// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.ui

import android.content.Context
import android.content.Intent
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.VibratorManager
import android.provider.Settings
import android.view.KeyEvent
import android.view.inputmethod.InputMethodManager
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.mindone.snapcase.R
import org.mindone.snapcase.data.AppPreferences
import org.mindone.snapcase.ime.HapticMode
import org.mindone.snapcase.ime.LongPressBehavior
import org.mindone.snapcase.ime.StripLang
import org.mindone.snapcase.ime.dict.DictionaryPackManager
import org.mindone.snapcase.ime.dict.LanguagePackInfo
import org.mindone.snapcase.ui.CompactPageTabs
import org.mindone.snapcase.ui.DismissibleErrorBanner
import org.mindone.snapcase.ui.InlineTabHeader
import org.mindone.snapcase.ui.KeyboardHintStrip
import org.mindone.snapcase.ui.KeycapBadge
import org.mindone.snapcase.ui.NavRow
import org.mindone.snapcase.ui.Note
import org.mindone.snapcase.ui.RegisterScreenKeys
import org.mindone.snapcase.ui.SectionCard
import org.mindone.snapcase.ui.StatusPill
import org.mindone.snapcase.ui.SuppressScreenKeys
import org.mindone.snapcase.ui.ToggleRow
import org.mindone.snapcase.ui.friendlyNetworkError

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ImeSettingsScreen(
    prefs: AppPreferences,
    onBack: () -> Unit = {},
    onOpenShortcuts: () -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val settings by prefs.settings.collectAsStateWithLifecycle()
    val imm = remember { context.getSystemService(InputMethodManager::class.java) }
    val vibrator = remember { context.getSystemService(VibratorManager::class.java)?.defaultVibrator }
    val dictManager = remember { DictionaryPackManager.shared(context.applicationContext) }
    val lifecycleOwner = LocalLifecycleOwner.current
    val windowFocused = LocalWindowInfo.current.isWindowFocused

    var page by rememberSaveable { mutableIntStateOf(0) }
    var installedDicts by remember { mutableStateOf<Set<String>>(emptySet()) }
    var downloadedDicts by remember { mutableStateOf<Set<String>>(emptySet()) }
    val downloading = remember { mutableStateMapOf<String, Boolean>() }
    var dictError by remember { mutableStateOf<String?>(null) }
    var addingSnippet by remember { mutableStateOf(false) }
    var waitingForEnable by rememberSaveable { mutableStateOf(false) }
    var autoOpenPicker by remember { mutableStateOf(false) }
    var imeEnabled by remember { mutableStateOf(isKumiImeEnabled(context, imm)) }
    var imeSelected by remember { mutableStateOf(isKumiImeSelected(context, imm)) }

    val pulseHaptic: (Int) -> Unit = { ms ->
        if (vibrator != null && vibrator.hasVibrator()) {
            val effect = VibrationEffect.createOneShot(ms.coerceIn(4, 15).toLong(), VibrationEffect.DEFAULT_AMPLITUDE)
            val attrs = VibrationAttributes.Builder().setUsage(VibrationAttributes.USAGE_TOUCH).build()
            runCatching { vibrator.vibrate(effect, attrs) }
        }
    }

    val refreshImeStatus: () -> Unit = {
        val enabled = isKumiImeEnabled(context, imm)
        val selected = enabled && isKumiImeSelected(context, imm)
        imeEnabled = enabled
        imeSelected = selected
        if (waitingForEnable && enabled) {
            waitingForEnable = false
            if (!selected) autoOpenPicker = true
        }
    }

    DisposableEffect(lifecycleOwner, imm, context) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) refreshImeStatus()
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    LaunchedEffect(windowFocused) {
        if (windowFocused) refreshImeStatus()
    }

    LaunchedEffect(autoOpenPicker) {
        if (autoOpenPicker) {
            autoOpenPicker = false
            delay(250L)
            imm?.showInputMethodPicker()
        }
    }

    LaunchedEffect(Unit) {
        val (inst, dl) = withContext(Dispatchers.IO) {
            dictManager.installedCodes() to DictionaryPackManager.CATALOG.filter { dictManager.isDownloaded(it.code) }.map { it.code }.toSet()
        }
        installedDicts = inst
        downloadedDicts = dl
    }

    val startDownload: (LanguagePackInfo) -> Unit = { pack ->
        if (downloading[pack.code] != true) {
            dictError = null
            downloading[pack.code] = true
            scope.launch {
                val res = dictManager.downloadPack(pack)
                downloading.remove(pack.code)
                val (inst, dl) = withContext(Dispatchers.IO) {
                    dictManager.installedCodes() to DictionaryPackManager.CATALOG.filter { dictManager.isDownloaded(it.code) }.map { it.code }.toSet()
                }
                installedDicts = inst
                downloadedDicts = dl
                res.onFailure { err -> dictError = err.message ?: err.toString() }
            }
        }
    }

    val shTab = stringResource(R.string.kumi_key_hint_tab)
    val shHaptic = stringResource(R.string.ime_haptic_label)
    val shLong = stringResource(R.string.ime_long_press_label)
    val shBack = stringResource(R.string.action_back)

    RegisterScreenKeys(
        key = listOf(page, settings.imeHapticMode, settings.imeHapticDurationMs, settings.imeLongPressBehavior),
        shortcuts = when (page) {
            0 -> listOf("H" to shHaptic, "L" to shLong, "Tab" to shTab, "⌫" to shBack)
            2 -> listOf("A" to stringResource(R.string.ime_snippets_add), "F" to stringResource(R.string.shortcuts_tile), "Tab" to shTab, "⌫" to shBack)
            else -> listOf("Tab" to shTab, "⌫" to shBack)
        },
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        if (code == KeyEvent.KEYCODE_TAB) {
            page = (page + 1) % 3
            return@RegisterScreenKeys true
        }
        when (page) {
            0 -> when (code) {
                KeyEvent.KEYCODE_H -> {
                    val modes = HapticMode.entries
                    val next = modes[(modes.indexOf(settings.imeHapticMode) + 1) % modes.size]
                    prefs.setImeHapticMode(next)
                    if (next != HapticMode.OFF) pulseHaptic(settings.imeHapticDurationMs)
                    true
                }
                KeyEvent.KEYCODE_L -> {
                    val behaviors = LongPressBehavior.entries
                    prefs.setImeLongPressBehavior(behaviors[(behaviors.indexOf(settings.imeLongPressBehavior) + 1) % behaviors.size])
                    true
                }
                else -> false
            }
            2 -> when (code) {
                KeyEvent.KEYCODE_A -> { addingSnippet = true; true }
                KeyEvent.KEYCODE_F -> { onOpenShortcuts(); true }
                else -> false
            }
            else -> false
        }
    }

    Column(Modifier.fillMaxSize()) {
        InlineTabHeader(
            tabs = listOf(
                stringResource(R.string.ime_tab_typing),
                stringResource(R.string.ime_tab_langs),
                stringResource(R.string.ime_tab_snippets),
            ),
            selectedIndex = page,
            onSelect = { page = it },
            onBack = onBack,
        )
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = 12.dp),
            contentPadding = PaddingValues(top = 2.dp, bottom = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            when (page) {
                0 -> {
                    item {
                        ImeSetupCard(
                            enabled = imeEnabled,
                            selected = imeSelected,
                            onOpenSettings = {
                                waitingForEnable = true
                                context.startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
                            },
                            onChooseKeyboard = { imm?.showInputMethodPicker() },
                        )
                    }
                    item {
                        SectionCard(
                            title = stringResource(R.string.ime_haptic_label),
                            trailing = { KeycapBadge("H") },
                        ) {
                            val haptics = HapticMode.entries
                            CompactPageTabs(
                                labels = haptics.map { stringResource(hapticModeTitle(it)) },
                                selectedIndex = haptics.indexOf(settings.imeHapticMode).coerceAtLeast(0),
                                onSelect = {
                                    val chosen = haptics[it]
                                    prefs.setImeHapticMode(chosen)
                                    if (chosen != HapticMode.OFF) pulseHaptic(settings.imeHapticDurationMs)
                                },
                            )
                            if (settings.imeHapticMode != HapticMode.OFF) {
                                Row(
                                    Modifier.fillMaxWidth(),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    Note(stringResource(R.string.ime_haptic_strength), modifier = Modifier.weight(1f))
                                    listOf(5, 8, 12).forEach { ms ->
                                        FilterChip(
                                            selected = settings.imeHapticDurationMs == ms || (ms == 5 && settings.imeHapticDurationMs == 6),
                                            onClick = {
                                                prefs.setImeHapticDurationMs(ms)
                                                pulseHaptic(ms)
                                            },
                                            label = { Text(stringResource(R.string.kumi_unit_ms, ms)) },
                                        )
                                    }
                                }
                                if (settings.imeHapticMode == HapticMode.EVENTS) {
                                    Note(stringResource(R.string.ime_haptic_events_hint))
                                }
                            }
                        }
                    }
                    item {
                        SectionCard(
                            title = stringResource(R.string.ime_long_press_label),
                            trailing = { KeycapBadge("L") },
                        ) {
                            val behaviors = LongPressBehavior.entries
                            CompactPageTabs(
                                labels = behaviors.map { stringResource(longPressTitle(it)) },
                                selectedIndex = behaviors.indexOf(settings.imeLongPressBehavior).coerceAtLeast(0),
                                onSelect = { prefs.setImeLongPressBehavior(behaviors[it]) },
                            )
                            if (settings.imeLongPressBehavior == LongPressBehavior.ALT_SYMBOL) {
                                Note(stringResource(R.string.ime_long_press_alt_hint))
                            }
                        }
                    }
                    item {
                        SectionCard(stringResource(R.string.ime_typing_title)) {
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_strip),
                                checked = settings.imeStripEnabled,
                                onCheckedChange = prefs::setImeStripEnabled,
                            )
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_typing_only),
                                checked = settings.imeShowOnTypingOnly,
                                onCheckedChange = prefs::setImeShowOnTypingOnly,
                            )
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_autocorrect),
                                checked = settings.imeAutoCorrect,
                                onCheckedChange = prefs::setImeAutoCorrect,
                            )
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_autocap),
                                checked = settings.imeAutoCapitalize,
                                onCheckedChange = prefs::setImeAutoCapitalize,
                            )
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_double_space),
                                checked = settings.imeDoubleSpacePeriod,
                                onCheckedChange = prefs::setImeDoubleSpacePeriod,
                            )
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_sticky_shift),
                                checked = settings.imeStickyShift,
                                onCheckedChange = prefs::setImeStickyShift,
                            )
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_quick_extras),
                                checked = settings.imeShowQuickExtras,
                                onCheckedChange = prefs::setImeShowQuickExtras,
                            )
                        }
                    }
                }
                1 -> {
                    item {
                        SectionCard(stringResource(R.string.ime_langs_title)) {
                            Note(stringResource(R.string.ime_langs_hint))
                            FlowRow(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                                verticalArrangement = Arrangement.spacedBy(2.dp),
                            ) {
                                StripLang.entries.forEach { lang ->
                                    val enabled = settings.imeEnabledLangs.any { it.equals(lang.code, ignoreCase = true) }
                                    val pack = DictionaryPackManager.CATALOG.firstOrNull { it.code == lang.code }
                                    val needsDict = pack != null && lang.code !in installedDicts
                                    val busy = downloading[lang.code] == true
                                    FilterChip(
                                        selected = enabled,
                                        onClick = {
                                            val next = if (enabled) {
                                                settings.imeEnabledLangs.filterNot { it.equals(lang.code, ignoreCase = true) }
                                            } else {
                                                settings.imeEnabledLangs + lang.code
                                            }
                                            if (next.isNotEmpty()) prefs.setImeEnabledLangs(next)
                                            if (!enabled && needsDict) startDownload(pack)
                                        },
                                        label = {
                                            Text(
                                                when {
                                                    busy -> "${lang.label} …"
                                                    needsDict -> "${lang.label} ↓"
                                                    else -> lang.label
                                                },
                                            )
                                        },
                                    )
                                }
                            }
                            if (StripLang.entries.any { it.code !in installedDicts }) {
                                Note(stringResource(R.string.ime_langs_download_hint))
                            }
                            ToggleRow(
                                title = stringResource(R.string.ime_toggle_shift_space),
                                subtitle = stringResource(R.string.ime_toggle_shift_space_desc),
                                checked = settings.imeShiftSpaceSwitch,
                                onCheckedChange = prefs::setImeShiftSpaceSwitch,
                            )
                        }
                    }
                    item {
                        SectionCard(
                            title = stringResource(R.string.ime_dicts_title),
                            trailing = { Note(stringResource(R.string.ime_dicts_summary, installedDicts.size)) },
                        ) {
                            dictError?.let { err ->
                                DismissibleErrorBanner(friendlyNetworkError(err), onDismiss = { dictError = null })
                            }
                            DictionaryPackManager.CATALOG.forEach { pack ->
                                val busy = downloading[pack.code] == true
                                val downloaded = pack.code in downloadedDicts
                                val available = pack.code in installedDicts
                                DictPackRow(
                                    pack = pack,
                                    available = available,
                                    downloaded = downloaded,
                                    busy = busy,
                                    onDownload = { startDownload(pack) },
                                    onRemove = {
                                        scope.launch {
                                            val (inst, dl) = withContext(Dispatchers.IO) {
                                                dictManager.removeDownloaded(pack.code)
                                                dictManager.installedCodes() to DictionaryPackManager.CATALOG.filter { dictManager.isDownloaded(it.code) }.map { it.code }.toSet()
                                            }
                                            installedDicts = inst
                                            downloadedDicts = dl
                                        }
                                    },
                                )
                            }
                        }
                    }
                }
                else -> {
                    item {
                        Card(
                            Modifier.fillMaxWidth(),
                            shape = RoundedCornerShape(16.dp),
                            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                        ) {
                            Column(Modifier.padding(horizontal = 12.dp, vertical = 4.dp)) {
                                NavRow(
                                    R.drawable.ic_search,
                                    R.string.shortcuts_title,
                                    stringResource(R.string.shortcuts_subtitle),
                                    onOpenShortcuts,
                                    keyHint = "F",
                                )
                            }
                        }
                    }
                    item {
                        SectionCard(
                            title = stringResource(R.string.ime_snippets_title),
                            trailing = { Note("${settings.imeSnippets.size}") },
                        ) {
                            settings.imeSnippets.forEach { snippet ->
                                ListItem(
                                    headlineContent = { Text("${snippet.trigger}  →  ${snippet.expansion}") },
                                    trailingContent = {
                                        IconButton(onClick = { prefs.removeImeSnippet(snippet.trigger) }) {
                                            Icon(painterResource(R.drawable.ic_delete), contentDescription = stringResource(R.string.action_remove))
                                        }
                                    },
                                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                )
                            }
                            FilledTonalButton(onClick = { addingSnippet = true }, modifier = Modifier.fillMaxWidth()) {
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                    KeycapBadge("A")
                                    Text(stringResource(R.string.ime_snippets_add))
                                }
                            }
                        }
                    }
                }
            }
        }
        KeyboardHintStrip(
            hints = when (page) {
                0 -> listOf("H" to shHaptic, "L" to shLong, "Tab" to shTab, "⌫" to shBack)
                2 -> listOf("A" to stringResource(R.string.ime_snippets_add), "Tab" to shTab, "⌫" to shBack)
                else -> listOf("Tab" to shTab, "⌫" to shBack)
            },
        )
    }

    if (addingSnippet) {
        AddSnippetDialog(
            onDismiss = { addingSnippet = false },
            onAdd = { trigger, expansion ->
                addingSnippet = false
                prefs.addImeSnippet(trigger, expansion)
            },
        )
    }
}

@Composable
private fun ImeSetupCard(
    enabled: Boolean,
    selected: Boolean,
    onOpenSettings: () -> Unit,
    onChooseKeyboard: () -> Unit,
) {
    if (enabled && selected) {
        Surface(
            Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ) {
            Row(
                Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Icon(
                    painterResource(R.drawable.ic_check),
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(18.dp),
                )
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.ime_setup_title), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                    Text(
                        stringResource(R.string.ime_status_active),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.85f),
                    )
                }
                TextButton(onClick = onChooseKeyboard) {
                    Text(stringResource(R.string.ime_switch_ime))
                }
            }
        }
        return
    }

    val container = if (enabled) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.secondaryContainer
    val content = if (enabled) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSecondaryContainer
    val stepPill = stringResource(if (enabled) R.string.ime_step_2_of_2 else R.string.ime_step_1_of_2)

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(containerColor = container, contentColor = content),
    ) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    stringResource(R.string.ime_setup_title),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
                StatusPill(
                    text = stepPill,
                    containerColor = MaterialTheme.colorScheme.primary,
                    contentColor = MaterialTheme.colorScheme.onPrimary,
                )
            }
            ImeStepRow(
                number = "1",
                title = stringResource(if (enabled) R.string.ime_step1_done else R.string.ime_step1_title),
                subtitle = if (!enabled) stringResource(R.string.ime_step1_hint) else null,
                done = enabled,
                active = !enabled,
                contentColor = content,
            )
            if (!enabled) {
                Button(onClick = onOpenSettings, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.ime_step1_cta))
                }
            }
            HorizontalDivider(color = content.copy(alpha = 0.14f))
            ImeStepRow(
                number = "2",
                title = stringResource(R.string.ime_step2_title),
                subtitle = stringResource(if (enabled) R.string.ime_step2_hint else R.string.ime_step2_locked),
                done = false,
                active = enabled,
                contentColor = content,
            )
            if (enabled) {
                Button(onClick = onChooseKeyboard, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.ime_step2_cta))
                }
            }
        }
    }
}

@Composable
private fun ImeStepRow(
    number: String,
    title: String,
    subtitle: String?,
    done: Boolean,
    active: Boolean,
    contentColor: Color,
) {
    Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(
            Modifier
                .padding(top = 2.dp)
                .size(22.dp)
                .clip(CircleShape)
                .background(
                    when {
                        done || active -> MaterialTheme.colorScheme.primary
                        else -> contentColor.copy(alpha = 0.16f)
                    },
                ),
            contentAlignment = Alignment.Center,
        ) {
            if (done) {
                Icon(
                    painterResource(R.drawable.ic_check),
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onPrimary,
                    modifier = Modifier.size(14.dp),
                )
            } else {
                Text(
                    number,
                    style = MaterialTheme.typography.labelSmall,
                    fontWeight = FontWeight.Bold,
                    color = if (active) MaterialTheme.colorScheme.onPrimary else contentColor.copy(alpha = 0.65f),
                )
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                title,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = if (active) FontWeight.SemiBold else FontWeight.Medium,
                color = if (active || done) contentColor else contentColor.copy(alpha = 0.6f),
            )
            if (subtitle != null) {
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = contentColor.copy(alpha = if (active) 0.85f else 0.55f),
                )
            }
        }
    }
}

private fun isKumiImeEnabled(context: Context, imm: InputMethodManager?): Boolean =
    imm?.enabledInputMethodList?.any { it.packageName == context.packageName } == true

private fun isKumiImeSelected(context: Context, imm: InputMethodManager?): Boolean {
    val current = imm?.currentInputMethodInfo
    if (current != null) return current.packageName == context.packageName
    val def = runCatching {
        Settings.Secure.getString(context.contentResolver, Settings.Secure.DEFAULT_INPUT_METHOD)
    }.getOrNull()
    return def?.startsWith("${context.packageName}/") == true
}

@Composable
private fun DictPackRow(
    pack: LanguagePackInfo,
    available: Boolean,
    downloaded: Boolean,
    busy: Boolean,
    onDownload: () -> Unit,
    onRemove: () -> Unit,
) {
    ListItem(
        headlineContent = { Text("${pack.nativeName} (${pack.code.uppercase()})") },
        supportingContent = {
            Text(
                when {
                    downloaded -> stringResource(R.string.ime_dict_downloaded)
                    pack.bundled -> stringResource(R.string.ime_dict_bundled)
                    else -> stringResource(R.string.ime_dict_available)
                },
            )
        },
        trailingContent = {
            when {
                busy -> CircularProgressIndicator(modifier = Modifier.heightIn(max = 22.dp))
                downloaded -> OutlinedButton(onClick = onRemove) { Text(stringResource(R.string.action_remove)) }
                pack.bundled -> TextButton(onClick = onDownload) { Text(stringResource(R.string.ime_dict_upgrade)) }
                !available -> FilledTonalButton(onClick = onDownload) { Text(stringResource(R.string.ime_dict_download)) }
            }
        },
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
    )
}

@Composable
private fun AddSnippetDialog(onDismiss: () -> Unit, onAdd: (String, String) -> Unit) {
    SuppressScreenKeys()
    var trigger by remember { mutableStateOf("") }
    var expansion by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.ime_snippets_add)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = trigger,
                    onValueChange = { trigger = it },
                    label = { Text(stringResource(R.string.ime_snippet_trigger)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = expansion,
                    onValueChange = { expansion = it },
                    label = { Text(stringResource(R.string.ime_snippet_expansion)) },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onAdd(trigger, expansion) },
                enabled = trigger.isNotBlank() && expansion.isNotBlank(),
            ) {
                Text(stringResource(R.string.action_add))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

private fun longPressTitle(behavior: LongPressBehavior): Int = when (behavior) {
    LongPressBehavior.ALT_SYMBOL -> R.string.ime_long_press_alt_symbol
    LongPressBehavior.VARIATIONS -> R.string.ime_long_press_variations
    LongPressBehavior.UPPERCASE -> R.string.ime_long_press_upper
    LongPressBehavior.OFF -> R.string.value_off
}

private fun hapticModeTitle(mode: HapticMode): Int = when (mode) {
    HapticMode.OFF -> R.string.ime_haptic_off
    HapticMode.EVENTS -> R.string.ime_haptic_events
    HapticMode.ALL_KEYS -> R.string.ime_haptic_all
}
