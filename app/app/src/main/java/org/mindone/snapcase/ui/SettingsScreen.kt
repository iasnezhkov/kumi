// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.mindone.snapcase.BuildConfig
import org.mindone.snapcase.R
import org.mindone.snapcase.stock.StockRepository
import org.mindone.snapcase.data.AppPreferences

val EQ_SAMPLE_RATES = listOf(44_100, 48_000, 96_000)

@Composable
fun SettingsScreen(stock: StockRepository, prefs: AppPreferences) {
    val settings by prefs.settings.collectAsStateWithLifecycle()
    val connection by stock.connection.collectAsStateWithLifecycle()
    var confirmWrites by remember { mutableStateOf(false) }

    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            SectionCard(stringResource(R.string.settings_writes)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.settings_writes_switch), Modifier.weight(1f))
                    Switch(
                        checked = settings.experimentalWrites,
                        onCheckedChange = { enable -> if (enable) confirmWrites = true else stock.setExperimentalWrites(false) },
                    )
                }
                Note(stringResource(R.string.settings_writes_note))
            }
        }
        item {
            SectionCard(stringResource(R.string.settings_demo)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.settings_demo_switch), Modifier.weight(1f))
                    Switch(checked = settings.demoMode, onCheckedChange = { prefs.setDemoMode(it) })
                }
                Note(stringResource(R.string.settings_demo_note))
            }
        }
        item {
            SectionCard(stringResource(R.string.settings_eq_rate)) {
                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    items(EQ_SAMPLE_RATES) { rate ->
                        FilterChip(
                            selected = settings.eqSampleRate == rate,
                            onClick = { prefs.setEqSampleRate(rate) },
                            label = { Text("$rate Hz") },
                        )
                    }
                }
                Note(stringResource(R.string.settings_eq_rate_note))
            }
        }
        item {
            SectionCard(stringResource(R.string.settings_about)) {
                InfoRow(stringResource(R.string.settings_version), "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})")
                InfoRow(stringResource(R.string.settings_build), stringResource(if (BuildConfig.SYSTEM_BUILD) R.string.value_system_build else R.string.value_standalone_build))
                InfoRow(stringResource(R.string.settings_transport), connection.kind.name)
                Note(stringResource(R.string.settings_about_note))
            }
        }
    }

    if (confirmWrites) {
        ConfirmDialog(
            title = stringResource(R.string.settings_writes),
            text = stringResource(R.string.confirm_enable_writes),
            onConfirm = { stock.setExperimentalWrites(true) },
            onDismiss = { confirmWrites = false },
        )
    }
}
