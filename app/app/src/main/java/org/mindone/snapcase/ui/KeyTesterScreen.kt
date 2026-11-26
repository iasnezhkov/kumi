// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.InputDevice
import android.view.KeyEvent
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import org.mindone.snapcase.R
import org.mindone.snapcase.system.DeviceState

data class KeyRecord(
    val id: Long,
    val action: Int,
    val keyCode: Int,
    val scanCode: Int,
    val metaState: Int,
    val repeat: Int,
    val device: String,
    val fromSnap: Boolean,
)

@Composable
fun KeyTesterScreen(onClose: () -> Unit) {
    val records = remember { mutableStateListOf<KeyRecord>() }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    Scaffold(
        contentWindowInsets = WindowInsets(0, 0, 0, 0),
        topBar = {
            InlineTitleHeader(
                title = stringResource(R.string.kb_tester),
                onBack = onClose,
                trailing = {
                    TextButton(onClick = { records.clear() }) { Text(stringResource(R.string.action_clear)) }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier
                .padding(padding)
                .navigationBarsPadding()
                .fillMaxSize()
                .onPreviewKeyEvent { event ->
                    val native = event.nativeKeyEvent
                    if (native.keyCode == KeyEvent.KEYCODE_BACK) return@onPreviewKeyEvent false
                    val device = InputDevice.getDevice(native.deviceId)
                    records.add(
                        0,
                        KeyRecord(
                            id = System.nanoTime(),
                            action = native.action,
                            keyCode = native.keyCode,
                            scanCode = native.scanCode,
                            metaState = native.metaState,
                            repeat = native.repeatCount,
                            device = device?.name ?: "#${native.deviceId}",
                            fromSnap = DeviceState.isCaseKeyboard(device),
                        ),
                    )
                    if (records.size > 200) records.removeAt(records.lastIndex)
                    true
                }
                .focusRequester(focus)
                .focusable(),
        ) {
            Text(
                stringResource(R.string.kb_tester_hint),
                Modifier.padding(16.dp),
                style = MaterialTheme.typography.bodyMedium,
            )
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                items(records.filter { it.action == KeyEvent.ACTION_DOWN && it.repeat == 0 }, key = { it.id }) { record ->
                    Column {
                        Text(keyName(record.keyCode), style = MaterialTheme.typography.titleMedium)
                        Note(stringResource(if (record.fromSnap) R.string.tester_from_case else R.string.tester_from_other))
                    }
                }
            }
        }
    }
}
