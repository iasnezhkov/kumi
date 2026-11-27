// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.mindone.snapcase.R
import org.mindone.snapcase.remap.Keys
import org.mindone.snapcase.remap.RemapAccessibilityService
import org.mindone.snapcase.data.AppPreferences

fun keyName(code: Int): String = KeyNames.label(code)

@Composable
fun KeysScreen(prefs: AppPreferences) {
    val settings by prefs.settings.collectAsStateWithLifecycle()
    var adding by remember { mutableStateOf(false) }
    val active = rememberAccessibilityActive()
    val detectedHome by RemapAccessibilityService.detectedHomeFlow.collectAsStateWithLifecycle()

    val shAdd = stringResource(R.string.action_add_mapping)
    val shBack = stringResource(R.string.action_back)
    RegisterScreenKeys(
        key = Unit,
        shortcuts = listOf("A" to shAdd, "⌫" to shBack),
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        when (code) {
            KeyEvent.KEYCODE_A -> { adding = true; true }
            else -> false
        }
    }

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth(),
            contentPadding = PaddingValues(12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (!active) {
                item {
                    AccessibilitySetupCard(active = false, detectedHome = detectedHome)
                }
            }
            item {
                Text(stringResource(R.string.keys_intro), style = MaterialTheme.typography.bodyMedium)
            }
            if (settings.remapTable.entries.isEmpty()) {
                item {
                    Text(
                        stringResource(R.string.keys_empty),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.fillMaxWidth().padding(vertical = 16.dp),
                        textAlign = TextAlign.Center,
                    )
                }
            }
            items(settings.remapTable.entries.toSortedMap().entries.toList(), key = { it.key }) { (from, to) ->
                Card(Modifier.fillMaxWidth()) {
                    ListItem(
                        headlineContent = {
                            Text("${keyName(from)}  →  ${if (to == Keys.UNKNOWN) stringResource(R.string.keys_disabled) else keyName(to)}")
                        },
                        trailingContent = {
                            IconButton(onClick = { prefs.removeRemap(from) }) {
                                Icon(painterResource(R.drawable.ic_delete), contentDescription = stringResource(R.string.action_remove))
                            }
                        },
                    )
                }
            }
            item {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledTonalButton(onClick = { adding = true }) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                            KeycapBadge("A")
                            Text(stringResource(R.string.action_add_mapping))
                        }
                    }
                    if (settings.remapTable.entries.isNotEmpty()) {
                        TextButton(onClick = { prefs.clearRemap() }) { Text(stringResource(R.string.action_clear)) }
                    }
                }
            }
            item {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.keys_only_snap), style = MaterialTheme.typography.bodyLarge)
                        Text(
                            stringResource(R.string.keys_only_snap_text),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Switch(checked = settings.remapOnlySnap, onCheckedChange = { prefs.setRemapOnlySnap(it) })
                }
            }
            item {
                Text(
                    stringResource(R.string.keys_layout_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        KeyboardHintStrip(hints = listOf("A" to shAdd, "⌫" to shBack))
    }

    if (adding) {
        AddMappingDialog(
            onDismiss = { adding = false },
            onAdd = { from, to ->
                adding = false
                prefs.addRemap(from, to)
            },
        )
    }
}

@Composable
private fun AddMappingDialog(onDismiss: () -> Unit, onAdd: (Int, Int) -> Unit) {
    SuppressScreenKeys()
    var source by remember { mutableStateOf<Int?>(null) }
    var target by remember { mutableStateOf<Int?>(null) }
    var query by remember { mutableStateOf("") }
    var showAll by remember { mutableStateOf(false) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    val candidates = remember(query, showAll) {
        val base = if (showAll || query.isNotBlank()) (1..KeyEvent.getMaxKeyCode()).toList() else KeyNames.COMMON_TARGETS
        listOf(Keys.UNKNOWN) + base.filter { query.isBlank() || keyName(it).contains(query.trim(), ignoreCase = true) }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.keys_add_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 56.dp)
                        .border(1.dp, MaterialTheme.colorScheme.outline, MaterialTheme.shapes.medium)
                        .onPreviewKeyEvent { event ->
                            val native = event.nativeKeyEvent
                            if (native.keyCode == KeyEvent.KEYCODE_BACK) return@onPreviewKeyEvent false
                            if (native.action == KeyEvent.ACTION_DOWN) source = native.keyCode
                            true
                        }
                        .focusRequester(focus)
                        .focusable()
                        .clickable { runCatching { focus.requestFocus() } }
                        .padding(12.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        source?.let { stringResource(R.string.keys_source, keyName(it)) } ?: stringResource(R.string.keys_press),
                        style = MaterialTheme.typography.titleMedium,
                        textAlign = TextAlign.Center,
                    )
                }
                if (source != null) {
                    Text(stringResource(R.string.keys_choose_target), style = MaterialTheme.typography.labelLarge)
                    OutlinedTextField(
                        value = query,
                        onValueChange = { query = it },
                        label = { Text(stringResource(R.string.keys_search)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    LazyColumn(Modifier.heightIn(max = 220.dp)) {
                        items(candidates) { code ->
                            ListItem(
                                headlineContent = { Text(if (code == Keys.UNKNOWN) stringResource(R.string.keys_disabled) else keyName(code)) },
                                modifier = Modifier.clickable { target = code },
                                tonalElevation = if (target == code) 6.dp else 0.dp,
                            )
                        }
                        if (!showAll && query.isBlank()) {
                            item {
                                TextButton(onClick = { showAll = true }) { Text(stringResource(R.string.keys_show_all)) }
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            val from = source
            val to = target
            TextButton(onClick = { if (from != null && to != null) onAdd(from, to) }, enabled = from != null && to != null && from != to) {
                Text(stringResource(R.string.action_add))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}
