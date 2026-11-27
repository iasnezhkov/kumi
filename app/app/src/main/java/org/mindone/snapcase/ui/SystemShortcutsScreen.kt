// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.view.KeyEvent
import android.view.accessibility.AccessibilityManager
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
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
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.mindone.snapcase.R
import org.mindone.snapcase.data.AppPreferences
import org.mindone.snapcase.ime.LaunchableApp
import org.mindone.snapcase.ime.QuickLauncher
import org.mindone.snapcase.ime.QuickSearchActivity
import org.mindone.snapcase.ime.SpeedKeyBinding
import org.mindone.snapcase.remap.RemapAccessibilityService

@Composable
fun rememberAccessibilityActive(): Boolean {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val liveActive by RemapAccessibilityService.activeFlow.collectAsStateWithLifecycle()
    var systemEnabled by remember { mutableStateOf(isKumiAccessibilityEnabled(context)) }

    DisposableEffect(lifecycleOwner, context) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                systemEnabled = isKumiAccessibilityEnabled(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    return liveActive || systemEnabled
}

private fun isKumiAccessibilityEnabled(context: Context): Boolean {
    if (RemapAccessibilityService.active) return true
    val am = context.getSystemService(AccessibilityManager::class.java) ?: return false
    val enabled = am.getEnabledAccessibilityServiceList(android.accessibilityservice.AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
    return enabled.any { it.resolveInfo?.serviceInfo?.packageName == context.packageName }
}

@Composable
fun AccessibilitySetupCard(
    active: Boolean,
    detectedHome: String? = null,
) {
    val context = LocalContext.current
    if (active) {
        Surface(
            Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(14.dp),
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ) {
            Row(
                Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
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
                    Text(
                        stringResource(R.string.shortcuts_service_active_title),
                        style = MaterialTheme.typography.titleSmall,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        if (!detectedHome.isNullOrBlank()) {
                            stringResource(R.string.shortcuts_service_active_launcher, detectedHome)
                        } else {
                            stringResource(R.string.shortcuts_service_active_text)
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.85f),
                    )
                }
            }
        }
        return
    }

    val container = MaterialTheme.colorScheme.secondaryContainer
    val content = MaterialTheme.colorScheme.onSecondaryContainer

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(containerColor = container, contentColor = content),
    ) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    stringResource(R.string.shortcuts_setup_title),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.weight(1f),
                )
                StatusPill(
                    text = stringResource(R.string.shortcuts_setup_badge),
                    containerColor = MaterialTheme.colorScheme.primary,
                    contentColor = MaterialTheme.colorScheme.onPrimary,
                )
            }
            SetupStepItem(
                number = "1",
                title = stringResource(R.string.shortcuts_step1_title),
                subtitle = stringResource(R.string.shortcuts_step1_hint),
                contentColor = content,
            )
            OutlinedButton(
                onClick = {
                    val intent = Intent(
                        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                        Uri.fromParts("package", context.packageName, null),
                    )
                    context.startActivity(intent)
                },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.shortcuts_step1_cta))
            }
            HorizontalDivider(color = content.copy(alpha = 0.14f))
            SetupStepItem(
                number = "2",
                title = stringResource(R.string.shortcuts_step2_title),
                subtitle = stringResource(R.string.shortcuts_step2_hint),
                contentColor = content,
            )
            Button(
                onClick = { context.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)) },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.action_open_accessibility))
            }
        }
    }
}

@Composable
private fun SetupStepItem(
    number: String,
    title: String,
    subtitle: String,
    contentColor: Color,
) {
    Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(
            Modifier
                .padding(top = 2.dp)
                .size(22.dp)
                .clip(CircleShape)
                .background(MaterialTheme.colorScheme.primary),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                number,
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onPrimary,
            )
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = contentColor)
            Text(subtitle, style = MaterialTheme.typography.bodySmall, color = contentColor.copy(alpha = 0.85f))
        }
    }
}

@Composable
fun SystemShortcutsScreen(
    prefs: AppPreferences,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val settings by prefs.settings.collectAsStateWithLifecycle()
    val serviceActive = rememberAccessibilityActive()
    val detectedHome by RemapAccessibilityService.detectedHomeFlow.collectAsStateWithLifecycle()
    var page by rememberSaveable { mutableIntStateOf(0) }
    var addingSpeedKey by remember { mutableStateOf(false) }
    var managingExcluded by remember { mutableStateOf(false) }

    val shTab = stringResource(R.string.kumi_key_hint_tab)
    val shSearch = stringResource(R.string.home_search_try)
    val shSearchToggle = stringResource(R.string.home_search_enable)
    val shBack = stringResource(R.string.shortcuts_back_toggle)
    val shMedia = stringResource(R.string.shortcuts_media_toggle)
    val shSpeed = stringResource(R.string.ime_speed_key_add_title)

    val listState = rememberLazyListState()
    LaunchedEffect(page) { listState.scrollToItem(0) }
    RegisterScreenTabs(count = 2, current = page, onSelect = { page = it })
    RegisterScreenScroll(listState)

    RegisterScreenKeys(
        key = listOf(page, settings.homeTypeToSearch, settings.backspaceAsBack, settings.mediaDoubleTap, settings.speedKeysEnabled),
        shortcuts = if (page == 0) {
            listOf(
                "Q" to shSearchToggle,
                "W" to shSearch,
                "E" to shBack,
                "M" to shMedia,
                "↵" to shTab,
                "⌫" to stringResource(R.string.action_back),
            )
        } else {
            listOf(
                "R" to shSpeed,
                "↵" to shTab,
                "⌫" to stringResource(R.string.action_back),
            )
        },
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        if (page == 0) {
            when (code) {
                KeyEvent.KEYCODE_Q -> { prefs.setHomeTypeToSearch(!settings.homeTypeToSearch); true }
                KeyEvent.KEYCODE_W -> { QuickSearchActivity.launch(context, null); true }
                KeyEvent.KEYCODE_E -> { prefs.setBackspaceAsBack(!settings.backspaceAsBack); true }
                KeyEvent.KEYCODE_M -> { prefs.setMediaDoubleTap(!settings.mediaDoubleTap); true }
                else -> false
            }
        } else {
            when (code) {
                KeyEvent.KEYCODE_R -> { addingSpeedKey = true; true }
                else -> false
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        InlineTabHeader(
            tabs = listOf(
                stringResource(R.string.shortcuts_tile),
                stringResource(R.string.shortcuts_tab_speed_keys),
            ),
            selectedIndex = page,
            onSelect = { page = it },
            onBack = onBack,
        )
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = 12.dp),
            state = listState,
            contentPadding = PaddingValues(top = 0.dp, bottom = 6.dp),
            verticalArrangement = Arrangement.spacedBy(5.dp),
        ) {
            if (page == 0 || !serviceActive) {
                item {
                    AccessibilitySetupCard(
                        active = serviceActive,
                        detectedHome = detectedHome,
                    )
                }
            }
            if (page == 0) {
                item {
                    SectionCard(
                        title = stringResource(R.string.home_search_title),
                        compact = true,
                        trailing = {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                KeycapBadge("Q")
                                Switch(
                                    checked = settings.homeTypeToSearch,
                                    onCheckedChange = prefs::setHomeTypeToSearch,
                                )
                            }
                        },
                    ) {
                        Note(stringResource(R.string.home_search_desc))
                        OutlinedButton(
                            onClick = { QuickSearchActivity.launch(context, null) },
                            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 0.dp),
                            modifier = Modifier.fillMaxWidth().height(34.dp),
                        ) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                KeycapBadge("W")
                                Text(stringResource(R.string.home_search_try), style = MaterialTheme.typography.labelLarge)
                            }
                        }
                    }
                }
                item {
                    SectionCard(
                        title = stringResource(R.string.shortcuts_back_title),
                        compact = true,
                        trailing = {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                KeycapBadge("E")
                                Switch(
                                    checked = settings.backspaceAsBack,
                                    onCheckedChange = prefs::setBackspaceAsBack,
                                )
                            }
                        },
                    ) {
                        Note(stringResource(R.string.shortcuts_back_desc))
                        OutlinedButton(
                            onClick = { managingExcluded = true },
                            contentPadding = PaddingValues(horizontal = 12.dp, vertical = 0.dp),
                            modifier = Modifier.fillMaxWidth().height(34.dp),
                        ) {
                            Text(
                                stringResource(R.string.shortcuts_back_excluded_btn, settings.backspaceBackExcluded.size),
                                style = MaterialTheme.typography.labelLarge,
                            )
                        }
                    }
                }
                item {
                    SectionCard(
                        title = stringResource(R.string.shortcuts_media_title),
                        compact = true,
                        trailing = {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                KeycapBadge("M")
                                Switch(
                                    checked = settings.mediaDoubleTap,
                                    onCheckedChange = prefs::setMediaDoubleTap,
                                )
                            }
                        },
                    ) {
                        Note(stringResource(R.string.shortcuts_media_desc))
                    }
                }
            } else {
                item {
                    SectionCard(
                        title = stringResource(R.string.ime_speed_keys_title),
                        trailing = { Note(stringResource(R.string.ime_speed_keys_summary, settings.speedKeys.size)) },
                    ) {
                        Note(stringResource(R.string.ime_speed_keys_desc))
                        ToggleRow(
                            title = stringResource(R.string.ime_speed_keys_enable),
                            checked = settings.speedKeysEnabled,
                            onCheckedChange = prefs::setSpeedKeysEnabled,
                        )
                        if (settings.speedKeys.isEmpty()) {
                            Note(stringResource(R.string.ime_speed_keys_empty))
                        } else {
                            settings.speedKeys.sortedBy { it.keyCode }.forEach { binding ->
                                ListItem(
                                    headlineContent = { Text("${QuickLauncher.keyLetter(binding.keyCode)}  →  ${binding.label}") },
                                    supportingContent = { Text(binding.packageName) },
                                    trailingContent = {
                                        IconButton(onClick = { prefs.removeSpeedKey(binding.keyCode) }) {
                                            Icon(painterResource(R.drawable.ic_delete), contentDescription = stringResource(R.string.action_remove))
                                        }
                                    },
                                    colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                )
                            }
                        }
                        FilledTonalButton(onClick = { addingSpeedKey = true }, modifier = Modifier.fillMaxWidth()) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                KeycapBadge("R")
                                Text(stringResource(R.string.ime_speed_key_add_title))
                            }
                        }
                    }
                }
            }
        }
        KeyboardHintStrip(
            hints = if (page == 0) {
                listOf(
                    "W" to shSearch,
                    "E" to stringResource(R.string.shortcuts_back_title),
                    "↵" to shTab,
                )
            } else {
                listOf(
                    "R" to shSpeed,
                    "↵" to shTab,
                    "⌫" to stringResource(R.string.action_back),
                )
            },
        )
    }

    if (addingSpeedKey) {
        AddSpeedKeyDialog(
            onDismiss = { addingSpeedKey = false },
            onAdd = { binding ->
                addingSpeedKey = false
                prefs.setSpeedKey(binding)
            },
        )
    }

    if (managingExcluded) {
        ExcludedAppsDialog(
            excluded = settings.backspaceBackExcluded,
            onToggle = { pkg, shouldExclude ->
                if (shouldExclude) prefs.addBackspaceBackExcluded(pkg) else prefs.removeBackspaceBackExcluded(pkg)
            },
            onDismiss = { managingExcluded = false },
        )
    }
}

@Composable
private fun ExcludedAppsDialog(
    excluded: Set<String>,
    onToggle: (String, Boolean) -> Unit,
    onDismiss: () -> Unit,
) {
    SuppressScreenKeys()
    val context = LocalContext.current
    var apps by remember { mutableStateOf<List<LaunchableApp>>(emptyList()) }
    var query by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        apps = withContext(Dispatchers.IO) { QuickLauncher.listLaunchableApps(context).distinctBy { it.packageName } }
    }
    val filtered = remember(apps, query, excluded) {
        val q = query.trim()
        val list = if (q.isEmpty()) apps else apps.filter { it.label.contains(q, ignoreCase = true) || it.packageName.contains(q, ignoreCase = true) }
        list.sortedWith(compareByDescending<LaunchableApp> { it.packageName in excluded }.thenBy { it.label.lowercase() })
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.shortcuts_back_excluded_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Note(stringResource(R.string.shortcuts_back_excluded_hint))
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    label = { Text(stringResource(R.string.keys_search)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                LazyColumn(Modifier.heightIn(max = 260.dp)) {
                    items(filtered, key = { it.packageName }) { app ->
                        val isExcluded = app.packageName in excluded
                        ListItem(
                            headlineContent = { Text(app.label) },
                            supportingContent = { Text(app.packageName) },
                            trailingContent = {
                                if (isExcluded) {
                                    Icon(
                                        painterResource(R.drawable.ic_check),
                                        contentDescription = null,
                                        tint = MaterialTheme.colorScheme.primary,
                                    )
                                }
                            },
                            modifier = Modifier.clickable { onToggle(app.packageName, !isExcluded) },
                        )
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_close)) }
        },
    )
}

@Composable
private fun AddSpeedKeyDialog(onDismiss: () -> Unit, onAdd: (SpeedKeyBinding) -> Unit) {
    SuppressScreenKeys()
    val context = LocalContext.current
    var apps by remember { mutableStateOf<List<LaunchableApp>>(emptyList()) }
    var selectedKey by remember { mutableIntStateOf(KeyEvent.KEYCODE_A) }
    var selectedApp by remember { mutableStateOf<LaunchableApp?>(null) }
    var query by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        apps = withContext(Dispatchers.IO) { QuickLauncher.listLaunchableApps(context).distinctBy { it.packageName } }
    }
    val filtered = remember(apps, query) {
        val q = query.trim()
        if (q.isEmpty()) apps else apps.filter { it.label.contains(q, ignoreCase = true) || it.packageName.contains(q, ignoreCase = true) }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.ime_speed_key_add_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    items((KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z).toList()) { code ->
                        FilterChip(
                            selected = selectedKey == code,
                            onClick = { selectedKey = code },
                            label = { Text(QuickLauncher.keyLetter(code)) },
                        )
                    }
                }
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    label = { Text(stringResource(R.string.keys_search)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                LazyColumn(Modifier.heightIn(max = 220.dp)) {
                    items(filtered, key = { it.packageName }) { app ->
                        ListItem(
                            headlineContent = { Text(app.label) },
                            supportingContent = { Text(app.packageName) },
                            tonalElevation = if (selectedApp?.packageName == app.packageName) 6.dp else 0.dp,
                            modifier = Modifier.clickable { selectedApp = app },
                        )
                    }
                }
            }
        },
        confirmButton = {
            val app = selectedApp
            TextButton(
                onClick = { if (app != null) onAdd(SpeedKeyBinding(selectedKey, app.packageName, app.label)) },
                enabled = app != null,
            ) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}
