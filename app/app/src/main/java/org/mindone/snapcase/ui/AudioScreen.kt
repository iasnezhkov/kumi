// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.mindone.snapcase.R
import org.mindone.snapcase.protocol.DacFilter
import org.mindone.snapcase.protocol.EqBand
import org.mindone.snapcase.protocol.EqCodec
import org.mindone.snapcase.protocol.FilterType
import org.mindone.snapcase.protocol.SnapProtocol
import org.mindone.snapcase.protocol.StreamMode
import org.mindone.snapcase.protocol.UsbAudioProfile
import org.mindone.snapcase.stock.TransportKind
import org.mindone.snapcase.stock.StockRepository
import org.mindone.snapcase.data.AppPreferences

@Composable
fun AudioScreen(stock: StockRepository, prefs: AppPreferences) {
    val audio by stock.sound.collectAsStateWithLifecycle()
    val connection by stock.connection.collectAsStateWithLifecycle()
    val settings by prefs.settings.collectAsStateWithLifecycle()
    var editing by remember { mutableStateOf<EqBand?>(null) }
    var pendingWrite by remember { mutableStateOf<EqBand?>(null) }
    var confirmApply by remember { mutableStateOf(false) }
    var pendingFilter by remember { mutableStateOf<DacFilter?>(null) }
    var editingGain by remember { mutableStateOf(false) }
    var pendingGain by remember { mutableStateOf<Double?>(null) }
    val connected = connection.kind != TransportKind.NONE

    LaunchedEffect(connection.kind) {
        if (connected && audio.bands.isEmpty()) stock.load()
    }

    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (!connected) item { Note(stringResource(R.string.audio_not_connected)) }
        if (audio.busy) item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
        audio.error?.let { item { Note(it, MaterialTheme.colorScheme.error) } }
        audio.lastWrite?.let { item { Note(stringResource(R.string.audio_last_write, it)) } }
        item {
            SectionCard(stringResource(R.string.audio_gains)) {
                InfoRow(stringResource(R.string.audio_record_gain), gainText(audio.recordGainRaw), Evidence.UNVERIFIED)
                InfoRow(stringResource(R.string.audio_global_gain), gainText(audio.globalGainRaw), Evidence.UNVERIFIED)
                Note(stringResource(R.string.audio_gains_note))
                if (settings.experimentalWrites) {
                    EvidenceTag(Evidence.EXPERIMENTAL)
                    Note(stringResource(R.string.audio_record_gain_note))
                    TextButton(onClick = { editingGain = true }, enabled = connected && !audio.busy) {
                        Text(stringResource(R.string.action_set_record_gain))
                    }
                }
            }
        }
        item {
            SectionCard(stringResource(R.string.audio_eq)) {
                Note(stringResource(R.string.audio_eq_note))
                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    items((0 until EqCodec.BANKS).toList()) { bank ->
                        FilterChip(
                            selected = bank == audio.bank,
                            onClick = { stock.selectBank(bank) },
                            enabled = connected && !audio.busy,
                            label = { Text(stringResource(R.string.audio_bank_n, bank)) },
                        )
                    }
                }
                FilledTonalButton(onClick = { stock.load() }, enabled = connected && !audio.busy) {
                    Text(stringResource(R.string.action_read))
                }
            }
        }
        item {
            SectionCard(stringResource(R.string.audio_state_title)) {
                Note(stringResource(R.string.audio_state_note))
                FilledTonalButton(onClick = { stock.load() }, enabled = connected && !audio.busy) {
                    Text(stringResource(R.string.action_read))
                }
                audio.audio?.let { s ->
                    InfoRow(stringResource(R.string.audio_state_usb), "UAC${s.usbClass}", Evidence.MEASURED)
                    InfoRow(stringResource(R.string.audio_state_rate), "${s.sampleRateHz} Hz", Evidence.MEASURED)
                    InfoRow(stringResource(R.string.audio_state_bits), s.pcmBits?.let { "$it bit" } ?: stringResource(R.string.audio_state_idle, s.pcmWidthRaw), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_measured_rate), if (s.measuredRateRaw != 0L) s.measuredRateRaw.toString() else stringResource(R.string.audio_state_idle, 0L), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_stream), streamText(s.streamMode, s.dsdModeRaw), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_profile), profileText(s.usbProfile, s.modeRaw), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_capture_trim), "%.1f dB".format(s.captureTrimRaw / 256.0), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_filter), dacFilterName(s.dacFilterRaw), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_eq), stringResource(if (s.eqEnabled) R.string.value_on else R.string.value_off) + " (${s.eqProfileRaw})", Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_mute), if (s.muteRaw != 0) stringResource(R.string.value_on) else stringResource(R.string.value_off), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_volume), "%.1f / %.1f dB".format(s.volumeLeftRaw / 256.0, s.volumeRightRaw / 256.0), Evidence.UNVERIFIED)
                    InfoRow(stringResource(R.string.audio_state_trim), "%.1f dB".format(s.outputTrimRaw / 256.0), Evidence.UNVERIFIED)
                }
                if (settings.experimentalWrites) {
                    EvidenceTag(Evidence.EXPERIMENTAL)
                    Note(stringResource(R.string.audio_filter_note))
                    LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        items(DacFilter.entries.toList()) { filter ->
                            FilterChip(
                                selected = audio.audio?.dacFilterRaw == filter.wire,
                                onClick = { pendingFilter = filter },
                                enabled = connected && !audio.busy,
                                label = { Text(dacFilterName(filter.wire)) },
                            )
                        }
                    }
                }
            }
        }
        items(audio.bands, key = { "${it.bank}-${it.band}" }) { band ->
            BandCard(band, settings.experimentalWrites && connected && !audio.busy) { editing = band }
        }
        item {
            SectionCard(stringResource(R.string.audio_experimental)) {
                if (!settings.experimentalWrites) {
                    Note(stringResource(R.string.audio_writes_off))
                } else {
                    EvidenceTag(Evidence.EXPERIMENTAL)
                    Note(stringResource(R.string.audio_apply_note))
                    OutlinedButton(onClick = { confirmApply = true }, enabled = connected && !audio.busy) {
                        Text(stringResource(R.string.action_apply_eq))
                    }
                }
            }
        }
    }

    editing?.let { band ->
        BandEditor(
            band = band,
            build = { f, q, g, t, p -> stock.buildBand(band, f, q, g, t, p) },
            onDismiss = { editing = null },
            onSubmit = {
                editing = null
                pendingWrite = it
            },
        )
    }
    pendingWrite?.let { band ->
        ConfirmDialog(
            title = stringResource(R.string.confirm_write_title),
            text = stringResource(R.string.confirm_write_eq, band.bank, band.band, band.frequencyHz, band.q, band.gainDb, band.filterType, band.preamp),
            onConfirm = { stock.writeBand(band) },
            onDismiss = { pendingWrite = null },
        )
    }
    if (confirmApply) {
        ConfirmDialog(
            title = stringResource(R.string.confirm_write_title),
            text = stringResource(R.string.confirm_apply),
            onConfirm = { stock.applyEq() },
            onDismiss = { confirmApply = false },
        )
    }
    if (editingGain) {
        RecordGainDialog(
            initialDb = (audio.recordGainRaw ?: 0) / 256.0,
            onDismiss = { editingGain = false },
            onSubmit = {
                editingGain = false
                pendingGain = it
            },
        )
    }
    pendingGain?.let { db ->
        ConfirmDialog(
            title = stringResource(R.string.confirm_write_title),
            text = stringResource(R.string.confirm_record_gain, db),
            onConfirm = { stock.setRecordGain(db) },
            onDismiss = { pendingGain = null },
        )
    }
    pendingFilter?.let { filter ->
        ConfirmDialog(
            title = stringResource(R.string.confirm_write_title),
            text = stringResource(R.string.confirm_filter, dacFilterName(filter.wire)),
            onConfirm = { stock.setDacFilter(filter) },
            onDismiss = { pendingFilter = null },
        )
    }
}

@Composable
private fun RecordGainDialog(initialDb: Double, onDismiss: () -> Unit, onSubmit: (Double) -> Unit) {
    val step = SnapProtocol.RECORD_GAIN_STEP_RAW / 256.0
    val min = SnapProtocol.RECORD_GAIN_MIN_DB
    val max = SnapProtocol.RECORD_GAIN_MAX_DB
    var value by remember { mutableFloatStateOf((Math.round(initialDb / step) * step).coerceIn(min, max).toFloat()) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.audio_record_gain)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                EvidenceTag(Evidence.EXPERIMENTAL)
                Text("%+.1f dB".format(value), style = MaterialTheme.typography.headlineSmall)
                Slider(
                    value = value,
                    onValueChange = { value = (Math.round(it / step) * step).toFloat() },
                    valueRange = min.toFloat()..max.toFloat(),
                    steps = ((max - min) / step).toInt() - 1,
                )
                Note(stringResource(R.string.audio_record_gain_dialog_note))
            }
        },
        confirmButton = { TextButton(onClick = { onSubmit(value.toDouble()) }) { Text(stringResource(R.string.action_write)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

@Composable
private fun streamText(mode: StreamMode, raw: Int): String = when (mode) {
    StreamMode.PCM -> "PCM"
    StreamMode.DOP -> "DoP"
    StreamMode.NATIVE_DSD -> "DSD (alt 4)"
    StreamMode.UNKNOWN -> raw.toString()
} + " ($raw)"

@Composable
private fun profileText(profile: UsbAudioProfile, raw: Long): String = when (profile) {
    UsbAudioProfile.DUPLEX -> stringResource(R.string.audio_profile_duplex)
    UsbAudioProfile.PLAYBACK_ONLY -> stringResource(R.string.audio_profile_playback)
    UsbAudioProfile.UNKNOWN -> stringResource(R.string.audio_profile_unknown)
} + " ($raw)"

@Composable
private fun dacFilterName(wire: Int): String = when (DacFilter.fromWire(wire)) {
    DacFilter.FAST_LOW_LATENCY -> stringResource(R.string.filter_fast_low)
    DacFilter.FAST_PHASE_COMPENSATED -> stringResource(R.string.filter_fast_phase)
    DacFilter.SLOW_LOW_LATENCY -> stringResource(R.string.filter_slow_low)
    DacFilter.SLOW_PHASE_COMPENSATED -> stringResource(R.string.filter_slow_phase)
    DacFilter.NON_OVERSAMPLING -> stringResource(R.string.filter_nos)
    null -> stringResource(R.string.filter_unknown, wire)
}

@Composable
private fun gainText(raw: Int?): String? = raw?.let { stringResource(R.string.audio_gain_value, it, it / 256.0) }

@Composable
private fun filterName(type: Int): String = when (FilterType.fromWire(type)) {
    FilterType.DISABLED -> stringResource(R.string.filter_disabled)
    FilterType.LOW_SHELF -> stringResource(R.string.filter_low_shelf)
    FilterType.PEAK -> stringResource(R.string.filter_peak)
    FilterType.HIGH_SHELF -> stringResource(R.string.filter_high_shelf)
    FilterType.LOW_PASS -> stringResource(R.string.filter_low_pass)
    FilterType.HIGH_PASS -> stringResource(R.string.filter_high_pass)
    null -> stringResource(R.string.filter_unknown, type)
}

@Composable
private fun BandCard(band: EqBand, editable: Boolean, onEdit: () -> Unit) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(stringResource(R.string.audio_band_title, band.band, band.frequencyHz), style = MaterialTheme.typography.titleSmall)
            InfoRow(stringResource(R.string.audio_band_q), stringResource(R.string.audio_q_value, band.qRaw, band.q), Evidence.UNVERIFIED)
            InfoRow(stringResource(R.string.audio_band_gain), stringResource(R.string.audio_gain_value, band.gainRaw, band.gainDb), Evidence.UNVERIFIED)
            InfoRow(stringResource(R.string.audio_band_type), "${band.filterType} · ${filterName(band.filterType)}", Evidence.UNVERIFIED)
            InfoRow(stringResource(R.string.audio_band_preamp), band.preamp.toString(), Evidence.UNVERIFIED)
            Mono(band.coefficients.joinToString(" "))
            if (editable) {
                TextButton(onClick = onEdit) { Text(stringResource(R.string.action_edit)) }
            }
        }
    }
}

@Composable
private fun BandEditor(
    band: EqBand,
    build: (Int, Double, Double, Int, Int) -> EqBand,
    onDismiss: () -> Unit,
    onSubmit: (EqBand) -> Unit,
) {
    var frequency by remember { mutableStateOf(band.frequencyHz.toString()) }
    var q by remember { mutableStateOf(band.q.toString()) }
    var gain by remember { mutableStateOf(band.gainDb.toString()) }
    var preamp by remember { mutableStateOf(band.preamp.toString()) }
    var type by remember { mutableIntStateOf(band.filterType) }
    val result = runCatching {
        build(frequency.toInt(), q.toDouble(), gain.toDouble(), type, preamp.toInt())
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.audio_edit_title, band.bank, band.band)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                EvidenceTag(Evidence.EXPERIMENTAL)
                NumberField(stringResource(R.string.audio_field_frequency), frequency) { frequency = it }
                NumberField(stringResource(R.string.audio_field_q), q) { q = it }
                NumberField(stringResource(R.string.audio_field_gain), gain) { gain = it }
                NumberField(stringResource(R.string.audio_field_preamp), preamp) { preamp = it }
                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    items(FilterType.entries.toList()) { filter ->
                        FilterChip(selected = type == filter.wire, onClick = { type = filter.wire }, label = { Text(filterName(filter.wire)) })
                    }
                }
                Note(stringResource(R.string.audio_coefficients_note))
                result.exceptionOrNull()?.let { Note(it.message ?: it.toString(), MaterialTheme.colorScheme.error) }
                result.getOrNull()?.let { Mono(it.coefficients.joinToString(" ")) }
            }
        },
        confirmButton = {
            TextButton(onClick = { result.getOrNull()?.let(onSubmit) }, enabled = result.isSuccess) {
                Text(stringResource(R.string.action_write))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

@Composable
private fun NumberField(label: String, value: String, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = value,
        onValueChange = { text -> onChange(text.filter { it.isDigit() || it == '.' || it == '-' }) },
        label = { Text(label) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
        modifier = Modifier.fillMaxWidth(),
    )
}
