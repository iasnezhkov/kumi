// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.BootStages
import org.mindone.snapcase.kumi.InstallFlow
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.SlotInfo
import org.mindone.snapcase.kumi.SlotState
import org.mindone.snapcase.kumi.SystemTelemetry

@Composable
fun KumiDetailsScreen(
    kumi: KumiSession,
    flows: InstallFlow,
    onOpenDeveloper: () -> Unit,
) {
    val state by kumi.state.collectAsStateWithLifecycle()
    val flowState by flows.state.collectAsStateWithLifecycle()
    val busy = flowState?.busy == true
    val runningSlot = state.slots?.running
    val otherConfirmed = state.slots?.slots?.firstOrNull { it.slot != state.slots?.booted && it.state == SlotState.Confirmed }
    var confirmSwitch by remember { mutableStateOf<SlotInfo?>(null) }

    val shRefresh = stringResource(R.string.action_refresh)
    val shDev = stringResource(R.string.kumi_dev_title)
    val shBack = stringResource(R.string.action_back)
    val canDev = state.connected && state.device?.capabilities?.developer == true

    RegisterScreenKeys(
        key = listOf(state.connected, canDev),
        shortcuts = buildList {
            if (state.connected) add("R" to shRefresh)
            if (canDev) add("D" to shDev)
            add("⌫" to shBack)
        },
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        when (code) {
            KeyEvent.KEYCODE_R -> if (state.connected) {
                kumi.refreshTelemetry()
                kumi.refreshDiagnostics()
                true
            } else false
            KeyEvent.KEYCODE_D -> if (canDev) {
                onOpenDeveloper()
                true
            } else false
            else -> false
        }
    }

    LaunchedEffect(state.connected) {
        if (state.connected) kumi.refreshTelemetry()
    }

    val listState = androidx.compose.foundation.lazy.rememberLazyListState()
    RegisterScreenScroll(listState)

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth(),
            state = listState,
            contentPadding = PaddingValues(start = 12.dp, end = 12.dp, top = 4.dp, bottom = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
        if (!state.connected) {
            item {
                SectionCard(stringResource(R.string.kumi_case_absent)) {
                    Note(stringResource(R.string.kumi_case_absent_text))
                }
            }
            return@LazyColumn
        }
        if (state.error != null) {
            item { KumiErrorBanner(state, kumi::clearError) }
        }
        state.telemetry?.let { telemetry ->
            item {
                VitalsCard(
                    telemetry = telemetry,
                    build = state.device?.build,
                    lab = state.device?.capabilities?.lab == true,
                    serial = state.device?.serialText,
                    onRefresh = { kumi.refreshTelemetry() },
                )
            }
        }
        state.slots?.let { table ->
            item {
                SectionCard(stringResource(R.string.kumi_slots_title)) {
                    table.slots.forEach { slot ->
                        InfoRow(
                            stringResource(R.string.kumi_slot, slot.slot.name),
                            listOfNotNull(slot.version?.toString(), stringResource(slotStateName(slot.state))).joinToString(" · "),
                        )
                    }
                    if (!state.ramOnly && (runningSlot?.state == SlotState.Trial || otherConfirmed != null)) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (runningSlot?.state == SlotState.Trial) {
                                FilledTonalButton(onClick = { kumi.confirmBoot() }, enabled = !busy) {
                                    Text(stringResource(R.string.kumi_slot_confirm))
                                }
                            }
                            if (otherConfirmed != null) {
                                OutlinedButton(onClick = { confirmSwitch = otherConfirmed }, enabled = !busy) {
                                    Text(stringResource(R.string.kumi_slot_switch, otherConfirmed.slot.name))
                                }
                            }
                        }
                    }
                }
            }
        }
        state.diagnostics?.let { diag ->
            item {
                SectionCard(
                    title = stringResource(R.string.kumi_dev_diagnostics),
                    trailing = { TextButton(onClick = { kumi.refreshDiagnostics() }) { Text(stringResource(R.string.action_refresh)) } },
                ) {
                    InfoRow(stringResource(R.string.kumi_diag_boot_count), diag.bootCount.toString())
                    InfoRow(stringResource(R.string.kumi_diag_stage), BootStages.name(diag.stage))
                    InfoRow(stringResource(R.string.kumi_diag_watchdog), stringResource(if (diag.watchdogArmed) R.string.value_on else R.string.value_off))
                    state.audio?.counters?.let { c ->
                        InfoRow(
                            stringResource(R.string.kumi_diag_audio_counters),
                            stringResource(R.string.kumi_counters, c.underruns, c.overruns, c.dacFaults),
                        )
                    }
                    diag.lastCrash?.let { crash ->
                        Note(stringResource(R.string.kumi_crash_note, crash.reason.name, BootStages.name(crash.stage), crash.bootCount), MaterialTheme.colorScheme.error)
                    } ?: Note(stringResource(R.string.kumi_dev_no_crash))
                }
            }
        }
        if (state.device?.capabilities?.developer == true) {
            item {
                Card(
                    Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(16.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
                ) {
                    Box(Modifier.padding(horizontal = 12.dp, vertical = 4.dp)) {
                        NavRow(R.drawable.ic_settings, R.string.kumi_dev_title, stringResource(R.string.kumi_dev_text), onOpenDeveloper, keyHint = "D")
                    }
                }
            }
        }
        }
        KeyboardHintStrip(
            hints = buildList {
                if (state.connected) add("R" to shRefresh)
                if (canDev) add("D" to shDev)
                add("⌫" to shBack)
            },
        )
    }

    confirmSwitch?.let { target ->
        ConfirmDialog(
            title = stringResource(R.string.kumi_slot_switch_title, target.slot.name),
            text = stringResource(R.string.kumi_slot_switch_text, target.slot.name, target.version?.toString() ?: "?"),
            onConfirm = { kumi.switchSlot() },
            onDismiss = { confirmSwitch = null },
        )
    }
}

@Composable
private fun VitalsCard(telemetry: SystemTelemetry, build: String?, lab: Boolean, serial: String?, onRefresh: () -> Unit) {
    val powerMode = stringResource(if (telemetry.dcdcActive) R.string.kumi_power_dcdc else R.string.kumi_power_ldo)
    val edition = stringResource(if (lab) R.string.kumi_vitals_edition_lab else R.string.kumi_vitals_edition_everyday)
    SectionCard(
        title = stringResource(R.string.kumi_vitals_title),
        trailing = { TextButton(onClick = onRefresh) { Text(stringResource(R.string.action_refresh)) } },
    ) {
        InfoRow(stringResource(R.string.kumi_vitals_edition), build?.let { "$edition ($it)" } ?: edition)
        InfoRow(stringResource(R.string.kumi_vitals_cpu), stringResource(R.string.kumi_vitals_mhz, telemetry.cpuMhz))
        InfoRow(stringResource(R.string.kumi_vitals_power), stringResource(R.string.kumi_vitals_power_val, powerMode, telemetry.dvddMv))
        InfoRow(
            stringResource(R.string.kumi_vitals_wfi),
            stringResource(R.string.kumi_vitals_wfi_val, telemetry.wfiPercentTenths / 10.0, telemetry.wfiWakeups),
        )
        InfoRow(stringResource(R.string.kumi_vitals_rails), stringResource(if (telemetry.analogRailsOn) R.string.value_on else R.string.value_off))
        InfoRow(stringResource(R.string.kumi_vitals_pll), stringResource(if (telemetry.audioPllEnabled) R.string.value_on else R.string.value_off))
        InfoRow(stringResource(R.string.kumi_vitals_jack_adc), telemetry.jackAdc.toString())
        InfoRow(stringResource(R.string.kumi_vitals_uptime), formatLocalizedUptime(telemetry.uptimeMs))
        serial?.takeIf { it.isNotBlank() }?.let { InfoRow(stringResource(R.string.kumi_vitals_serial), it) }
        Note(stringResource(R.string.kumi_vitals_power_note))
    }
}

@Composable
private fun formatLocalizedUptime(ms: Long): String {
    val totalSeconds = (ms / 1000L).coerceAtLeast(0L)
    val hours = totalSeconds / 3600L
    val minutes = (totalSeconds % 3600L) / 60L
    val seconds = totalSeconds % 60L
    return when {
        hours > 0 -> stringResource(R.string.kumi_uptime_hms, hours, minutes, seconds)
        minutes > 0 -> stringResource(R.string.kumi_uptime_ms, minutes, seconds)
        else -> stringResource(R.string.kumi_uptime_s, seconds)
    }
}
