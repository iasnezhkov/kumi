// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.os.BatteryManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.mindone.snapcase.R
import org.mindone.snapcase.protocol.EqCodec
import org.mindone.snapcase.protocol.LiveState
import org.mindone.snapcase.system.Sysfs
import org.mindone.snapcase.stock.TransportKind
import org.mindone.snapcase.stock.StockRepository
import org.mindone.snapcase.stock.LiveUi

@Composable
fun StatusScreen(stock: StockRepository, phone: PhoneState, onDemo: () -> Unit) {
    val connection by stock.connection.collectAsStateWithLifecycle()
    val status by stock.status.collectAsStateWithLifecycle()
    val battery by phone.battery.collectAsStateWithLifecycle()
    val inputs by phone.inputs.collectAsStateWithLifecycle()
    val live by stock.live.collectAsStateWithLifecycle()
    val usbAudio by phone.usbAudio.collectAsStateWithLifecycle()

    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            SectionCard(stringResource(R.string.status_connection)) {
                val stateText = when (connection.kind) {
                    TransportKind.HIDRAW -> stringResource(R.string.status_connected_hidraw, connection.node?.devPath ?: "")
                    TransportKind.DEMO -> stringResource(R.string.status_connected_demo)
                    TransportKind.NONE -> if (connection.node != null) {
                        stringResource(R.string.status_open_failed, connection.node?.devPath ?: "", connection.openError ?: "")
                    } else {
                        stringResource(R.string.status_not_connected)
                    }
                }
                Text(stateText, style = MaterialTheme.typography.bodyLarge)
                if (status.presence.service) Note(stringResource(R.string.status_in_service), MaterialTheme.colorScheme.error)
                InfoRow(stringResource(R.string.status_firmware), connection.version, Evidence.MEASURED)
                connection.probeError?.let { Note(stringResource(R.string.status_probe_failed, it), MaterialTheme.colorScheme.error) }
                InfoRow(
                    stringResource(R.string.status_flash_magic),
                    connection.flashMagic?.let { stringResource(if (it) R.string.value_ok else R.string.value_mismatch) },
                )
                Note(stringResource(R.string.status_probe_note))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledTonalButton(onClick = { stock.probe() }, enabled = connection.kind != TransportKind.NONE && !connection.probing) {
                        Text(stringResource(R.string.action_probe))
                    }
                    if (connection.kind == TransportKind.NONE) {
                        Button(onClick = onDemo) { Text(stringResource(R.string.action_use_demo)) }
                    }
                }
            }
        }
        item { LiveStateCard(connection.kind != TransportKind.NONE, live, onBank = stock::setLiveBank, onRead = { stock.readLiveState() }) }
        item {
            SectionCard(stringResource(R.string.status_usb)) {
                val usb = status.usb
                if (usb == null) {
                    Note(stringResource(R.string.status_usb_missing))
                } else {
                    InfoRow(stringResource(R.string.status_usb_sysfs), usb.sysfsName)
                    InfoRow(stringResource(R.string.status_usb_product), listOfNotNull(usb.attributes["manufacturer"], usb.attributes["product"]).joinToString(" "))
                    InfoRow(stringResource(R.string.status_usb_speed), usb.speedMbps?.let { stringResource(R.string.unit_mbps, it) })
                    InfoRow(stringResource(R.string.status_usb_version), usb.usbVersion)
                    InfoRow(stringResource(R.string.status_usb_bcd), usb.attributes["bcdDevice"])
                    InfoRow(stringResource(R.string.status_usb_max_power), usb.attributes["bMaxPower"])
                    InfoRow(
                        stringResource(R.string.status_usb_power_source),
                        Sysfs.selfPowered(usb.attributes["bmAttributes"])?.let {
                            stringResource(if (it) R.string.value_self_powered else R.string.value_bus_powered)
                        },
                    )
                    InfoRow(stringResource(R.string.status_usb_address), "${usb.attributes["busnum"] ?: "?"}-${usb.attributes["devnum"] ?: "?"}")
                    InfoRow(stringResource(R.string.status_usb_runtime), usb.attributes["power/runtime_status"])
                }
            }
        }
        item {
            SectionCard(stringResource(R.string.status_usb_audio)) {
                Note(stringResource(R.string.status_usb_audio_note))
                if (usbAudio.isEmpty()) Note(stringResource(R.string.status_usb_audio_missing))
                usbAudio.forEach { device ->
                    Text(
                        "${device.productName} · " + stringResource(if (device.isSink) R.string.status_usb_audio_out else R.string.status_usb_audio_in),
                        style = MaterialTheme.typography.labelLarge,
                    )
                    InfoRow(stringResource(R.string.status_usb_audio_rates), device.sampleRates.takeIf { it.isNotEmpty() }?.joinToString(" ") { formatRate(it) } ?: stringResource(R.string.status_usb_audio_any))
                    InfoRow(stringResource(R.string.status_usb_audio_channels), device.channelCounts.takeIf { it.isNotEmpty() }?.joinToString(" ") ?: stringResource(R.string.status_usb_audio_any))
                    InfoRow(stringResource(R.string.status_usb_audio_encodings), device.encodings.joinToString(" ").ifEmpty { null })
                    if (device.isSink) {
                        InfoRow(
                            stringResource(R.string.status_usb_audio_bit_perfect),
                            device.bitPerfect?.let { stringResource(if (it) R.string.value_yes else R.string.value_no) },
                        )
                    }
                }
            }
        }
        item {
            SectionCard(stringResource(R.string.status_power)) {
                Note(stringResource(R.string.status_no_battery_note))
                if (status.typec.isEmpty()) Note(stringResource(R.string.status_typec_missing))
                status.typec.forEach { port ->
                    InfoRow(stringResource(R.string.status_typec_port), port.port)
                    InfoRow(stringResource(R.string.status_typec_power_role), port.powerRole)
                    InfoRow(stringResource(R.string.status_typec_data_role), port.dataRole)
                    InfoRow(stringResource(R.string.status_typec_op_mode), port.powerOperationMode)
                    InfoRow(
                        stringResource(R.string.status_typec_partner),
                        stringResource(if (port.partnerPresent) R.string.value_yes else R.string.value_no),
                    )
                    if (port.powerRole == "source") Note(stringResource(R.string.status_phone_is_source))
                    if (port.powerRole == "sink") Note(stringResource(R.string.status_phone_is_sink))
                }
            }
        }
        item {
            SectionCard(stringResource(R.string.status_phone_battery)) {
                val b = battery
                if (b == null) {
                    Note(stringResource(R.string.value_unavailable))
                } else {
                    InfoRow(stringResource(R.string.status_battery_level), b.percent?.let { "$it %" })
                    InfoRow(stringResource(R.string.status_battery_state), stringResource(batteryStatusText(b.status)))
                    InfoRow(stringResource(R.string.status_battery_plugged), stringResource(pluggedText(b.plugged)))
                    InfoRow(stringResource(R.string.status_battery_voltage), b.voltageMv?.let { "$it mV" })
                    InfoRow(stringResource(R.string.status_battery_temperature), b.temperatureDeciC?.let { "%.1f °C".format(it / 10.0) })
                    InfoRow(stringResource(R.string.status_battery_current), b.currentNowUa?.let { "${it / 1000} mA" })
                }
            }
        }
        item {
            SectionCard(stringResource(R.string.status_input_device)) {
                if (inputs.isEmpty()) Note(stringResource(R.string.status_input_missing))
                inputs.forEach { device ->
                    InfoRow(stringResource(R.string.status_input_name), "${device.name} (#${device.id})")
                    InfoRow(
                        stringResource(R.string.status_input_external),
                        stringResource(if (device.isExternal) R.string.value_yes else R.string.value_no),
                    )
                    InfoRow(stringResource(R.string.status_input_language), device.languageTag)
                    InfoRow(stringResource(R.string.status_input_layout_type), device.layoutType)
                    InfoRow(
                        stringResource(R.string.status_input_battery),
                        if (device.batteryPresent) "${((device.batteryCapacity ?: 0f) * 100).toInt()} %" else stringResource(R.string.value_not_reported),
                    )
                }
            }
        }
    }
}

@Composable
private fun LiveStateCard(connected: Boolean, live: LiveUi, onBank: (Int) -> Unit, onRead: () -> Unit) {
    SectionCard(stringResource(R.string.live_title)) {
        Note(stringResource(R.string.live_note))
        LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            items((0 until EqCodec.BANKS).toList()) { index ->
                FilterChip(
                    selected = index == live.bank,
                    onClick = { onBank(index) },
                    label = { Text(stringResource(R.string.audio_bank_n, index)) },
                )
            }
        }
        FilledTonalButton(onClick = onRead, enabled = connected && !live.busy) {
            Text(stringResource(R.string.action_read_live))
        }
        live.error?.let { Note(it, MaterialTheme.colorScheme.error) }
        live.state?.let { LiveStateContent(it) }
    }
}

@Composable
private fun LiveStateContent(state: LiveState) {
    if (state.looksLikeEcho) Note(stringResource(R.string.live_echo_warning), MaterialTheme.colorScheme.error)
    InfoRow(stringResource(R.string.live_volume), state.volumeRaw.toString(), Evidence.MEASURED)
    InfoRow(stringResource(R.string.live_volume_flag), state.volumeFlag.toString(), Evidence.UNVERIFIED)
    InfoRow(stringResource(R.string.live_getter02), state.getter02Shadow.toString(), Evidence.UNVERIFIED)
    InfoRow(stringResource(R.string.live_getter03), state.getter03Shadow.toString(), Evidence.UNVERIFIED)
    InfoRow(
        stringResource(R.string.live_record_marker),
        when (state.recordMarker) {
            1 -> stringResource(R.string.live_marker_defaults)
            2 -> stringResource(R.string.live_marker_override)
            else -> state.recordMarker.toString()
        },
        Evidence.UNVERIFIED,
    )
    Text(stringResource(R.string.live_tables, state.bank), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 4.dp))
    Mono("f  " + state.frequencies.joinToString(" "))
    Mono("Q  " + state.qRaw.joinToString(" "))
    Mono("G  " + state.gainRaw.joinToString(" "))
    Note(stringResource(R.string.live_tables_note))
}

private fun formatRate(hz: Int): String = if (hz % 1000 == 0) "${hz / 1000}k" else "%.1fk".format(hz / 1000.0)

private fun batteryStatusText(status: Int): Int = when (status) {
    BatteryManager.BATTERY_STATUS_CHARGING -> R.string.battery_charging
    BatteryManager.BATTERY_STATUS_DISCHARGING -> R.string.battery_discharging
    BatteryManager.BATTERY_STATUS_FULL -> R.string.battery_full
    BatteryManager.BATTERY_STATUS_NOT_CHARGING -> R.string.battery_not_charging
    else -> R.string.battery_unknown
}

private fun pluggedText(plugged: Int): Int = when (plugged) {
    BatteryManager.BATTERY_PLUGGED_AC -> R.string.plugged_ac
    BatteryManager.BATTERY_PLUGGED_USB -> R.string.plugged_usb
    BatteryManager.BATTERY_PLUGGED_WIRELESS -> R.string.plugged_wireless
    BatteryManager.BATTERY_PLUGGED_DOCK -> R.string.plugged_dock
    0 -> R.string.plugged_none
    else -> R.string.plugged_other
}
