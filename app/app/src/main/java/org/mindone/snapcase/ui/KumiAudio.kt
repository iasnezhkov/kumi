// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.view.KeyEvent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.flow.StateFlow
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.AudioChannel
import org.mindone.snapcase.kumi.AudioMeters
import org.mindone.snapcase.kumi.AudioStatus
import org.mindone.snapcase.kumi.CaseError
import org.mindone.snapcase.kumi.ClassHMode
import org.mindone.snapcase.kumi.CrossfeedMode
import org.mindone.snapcase.kumi.DacExtras
import org.mindone.snapcase.kumi.DspSettings
import org.mindone.snapcase.kumi.EqMath
import org.mindone.snapcase.kumi.EqProfile
import org.mindone.snapcase.kumi.ImpedanceReport
import org.mindone.snapcase.kumi.Jack
import org.mindone.snapcase.kumi.KumiLimits
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.KumiState
import org.mindone.snapcase.kumi.MicSettings
import org.mindone.snapcase.kumi.OutputFilter
import org.mindone.snapcase.kumi.OutputSettings
import org.mindone.snapcase.kumi.StreamKind
import org.mindone.snapcase.kumi.TestToneRequest

fun KumiState.isUacSleeping(): Boolean =
    (device?.capabilities?.audioOut == false || audio?.usbAudioPresent == false) && dacExtras?.autoUacOnJack == true

@Composable
fun kumiFriendlyError(state: KumiState): String = when (state.errorKind) {
    CaseError.Unsupported -> stringResource(
        if (state.isUacSleeping()) R.string.kumi_err_audio_unplugged else R.string.kumi_err_unsupported,
    )
    CaseError.Busy -> stringResource(R.string.kumi_err_busy)
    CaseError.WrongState -> stringResource(R.string.kumi_err_wrong_state)
    CaseError.NotUnlocked -> stringResource(R.string.kumi_err_not_unlocked)
    CaseError.StorageFull -> stringResource(R.string.kumi_err_storage_full)
    CaseError.HardwareFault -> stringResource(R.string.kumi_err_hardware_fault)
    CaseError.InvalidArgument -> stringResource(R.string.kumi_dev_refused_argument)
    CaseError.VerifyFailed -> stringResource(R.string.kumi_err_verify_failed)
    else -> when (state.errorBridgeKind) {
        "NoReply" -> stringResource(R.string.kumi_err_no_reply)
        "Transport", "Link" -> stringResource(R.string.kumi_err_usb_link)
        "Decode" -> stringResource(R.string.kumi_err_protocol)
        else -> {
            val raw = state.error.orEmpty()
            when {
                raw.contains("not connected", ignoreCase = true) -> stringResource(R.string.kumi_err_not_connected)
                raw.contains("timeout", ignoreCase = true) -> stringResource(R.string.kumi_err_no_reply)
                raw.isBlank() -> stringResource(R.string.kumi_err_generic)
                else -> stringResource(R.string.kumi_err_generic)
            }
        }
    }
}

@Composable
fun friendlyNetworkError(raw: String): String = when {
    raw.contains("UnknownHost", ignoreCase = true) ||
        raw.contains("Unable to resolve host", ignoreCase = true) ||
        raw.contains("ConnectException", ignoreCase = true) ||
        raw.contains("Network is unreachable", ignoreCase = true) -> stringResource(R.string.kumi_err_network_offline)
    raw.contains("Timeout", ignoreCase = true) -> stringResource(R.string.kumi_err_network_timeout)
    raw.contains("HTTP 403", ignoreCase = true) || raw.contains("429") -> stringResource(R.string.kumi_err_network_rate_limit)
    else -> stringResource(R.string.kumi_err_network_generic)
}

@Composable
fun KumiErrorBanner(state: KumiState, onDismiss: () -> Unit) {
    if (state.error == null) return
    DismissibleErrorBanner(kumiFriendlyError(state), onDismiss = onDismiss)
}

@Composable
fun KumiAudioScreen(
    kumi: KumiSession,
    math: EqMath,
    onBack: () -> Unit = {},
    onOpenEq: (Int) -> Unit,
) {
    val state by kumi.state.collectAsStateWithLifecycle()
    val caps = state.device?.capabilities
    var page by rememberSaveable { mutableIntStateOf(0) }
    var lastEqSlot by rememberSaveable { mutableIntStateOf(0) }
    var confirmLineOut by remember { mutableStateOf(false) }
    var dismissedDacFaults by rememberSaveable { mutableLongStateOf(0L) }
    val hasAudioControls = caps?.audioOut == true || state.output != null || state.dsp != null || state.dacExtras != null
    val hasEq = caps?.parametricEq == true || state.eqSlots.any { it != null } || state.output != null
    val uacSleeping = state.isUacSleeping()
    val activeEq = state.audio?.eqActive
    if (activeEq != null) lastEqSlot = activeEq

    val shTab = stringResource(R.string.kumi_key_hint_tab)
    val shEq = stringResource(R.string.eq_title)
    val shAb = stringResource(R.string.kumi_key_hint_eq_ab)
    val shXfeed = stringResource(R.string.kumi_tile_crossfeed)
    val shLoud = stringResource(R.string.kumi_loudness)
    val shLimit = stringResource(R.string.kumi_limiter)
    val shClassH = stringResource(R.string.kumi_class_h_label)
    val shLineOut = stringResource(R.string.kumi_line_out)
    val shFilter = stringResource(R.string.sound_filter)
    val shMeasure = stringResource(R.string.kumi_impedance_measure)

    val audioShortcuts = when (page) {
        0 -> listOf("E" to shEq, "␣" to shAb, "X" to shXfeed, "L" to shLoud, "M" to shLimit, "Tab" to shTab)
        1 -> listOf("C" to shClassH, "O" to shLineOut, "F" to shFilter, "Tab" to shTab)
        else -> listOf("M" to shMeasure, "Q / W / E / R" to stringResource(R.string.kumi_test_tone_title), "Tab" to shTab)
    }

    RegisterScreenKeys(
        key = listOf(
            page,
            state.connected,
            activeEq,
            lastEqSlot,
            state.eqSlots,
            state.dsp,
            state.output,
            state.dacExtras,
            state.audio?.stream,
            state.measuringImpedance,
            state.testTonePlaying,
        ),
        shortcuts = if (state.connected) audioShortcuts else emptyList(),
    ) { code, repeat ->
        if (repeat || !state.connected) return@RegisterScreenKeys false
        if (code == KeyEvent.KEYCODE_TAB) {
            page = (page + 1) % 3
            return@RegisterScreenKeys true
        }
        when (page) {
            0 -> when (code) {
                KeyEvent.KEYCODE_E -> if (hasEq) { onOpenEq(activeEq ?: lastEqSlot); true } else false
                KeyEvent.KEYCODE_SPACE -> if (hasEq) {
                    if (activeEq != null) kumi.selectEq(null)
                    else {
                        val t = lastEqSlot.takeIf { state.eqSlots.getOrNull(it) != null } ?: state.eqSlots.indexOfFirst { it != null }
                        if (t >= 0) kumi.selectEq(t) else onOpenEq(0)
                    }
                    true
                } else false
                KeyEvent.KEYCODE_X -> state.dsp?.let { dsp ->
                    val modes = CrossfeedMode.entries
                    kumi.setDsp(dsp.copy(crossfeed = modes[(modes.indexOf(dsp.crossfeed) + 1) % modes.size]))
                    true
                } ?: false
                KeyEvent.KEYCODE_L -> state.dsp?.let { dsp ->
                    kumi.setDsp(dsp.copy(loudness = !dsp.loudness))
                    true
                } ?: false
                KeyEvent.KEYCODE_M -> state.dsp?.let { dsp ->
                    kumi.setDsp(dsp.copy(limiter = !dsp.limiter))
                    true
                } ?: false
                else -> false
            }
            1 -> when (code) {
                KeyEvent.KEYCODE_C -> state.dacExtras?.let { ex ->
                    val modes = ClassHMode.entries
                    kumi.setDacExtras(ex.copy(classH = modes[(modes.indexOf(ex.classH) + 1) % modes.size]))
                    true
                } ?: false
                KeyEvent.KEYCODE_O -> state.dacExtras?.let { ex ->
                    if (ex.lineOut) kumi.setDacExtras(ex.copy(lineOut = false)) else confirmLineOut = true
                    true
                } ?: false
                KeyEvent.KEYCODE_F -> state.output?.let { out ->
                    val filters = OutputFilter.entries
                    kumi.setOutput(out.copy(filter = filters[(filters.indexOf(out.filter) + 1) % filters.size]))
                    true
                } ?: false
                else -> false
            }
            else -> {
                val streamingNow = state.audio?.stream != null && !kumi.simulated
                when (code) {
                    KeyEvent.KEYCODE_M -> if (!state.measuringImpedance && !streamingNow && !state.testTonePlaying) {
                        kumi.measureImpedance(); true
                    } else false
                    KeyEvent.KEYCODE_Q -> if (!streamingNow && !state.measuringImpedance) {
                        kumi.playTestTone(TestToneRequest(AudioChannel.Left, 1_000, 1_500, false)); true
                    } else false
                    KeyEvent.KEYCODE_W -> if (!streamingNow && !state.measuringImpedance) {
                        kumi.playTestTone(TestToneRequest(AudioChannel.Right, 1_000, 1_500, false)); true
                    } else false
                    KeyEvent.KEYCODE_E -> if (!streamingNow && !state.measuringImpedance) {
                        kumi.playTestTone(TestToneRequest(AudioChannel.Both, 1_000, 1_500, false)); true
                    } else false
                    KeyEvent.KEYCODE_R -> if (!streamingNow && !state.measuringImpedance) {
                        kumi.playTestTone(TestToneRequest(AudioChannel.Both, 1_000, 1_500, true)); true
                    } else false
                    else -> false
                }
            }
        }
    }

    if (!state.connected) {
        Column(Modifier.fillMaxSize()) {
            InlineTitleHeader(stringResource(R.string.kumi_audio_title), onBack = onBack)
            Column(Modifier.padding(horizontal = 10.dp, vertical = 4.dp)) {
                SectionCard(stringResource(R.string.kumi_case_absent)) {
                    Note(stringResource(R.string.kumi_case_absent_text))
                }
            }
        }
        return
    }

    Column(Modifier.fillMaxSize()) {
        InlineTabHeader(
            tabs = listOf(
                stringResource(R.string.kumi_audio_tab_effects),
                stringResource(R.string.kumi_audio_tab_dac),
                stringResource(R.string.kumi_audio_tab_tools),
            ),
            selectedIndex = page,
            onSelect = { page = it },
            onBack = onBack,
        )
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth().padding(horizontal = 12.dp),
            contentPadding = PaddingValues(top = 2.dp, bottom = 6.dp),
            verticalArrangement = Arrangement.spacedBy(7.dp),
        ) {
            if (state.error != null) {
                item { KumiErrorBanner(state, kumi::clearError) }
            }
            when (page) {
                0 -> {
                    item {
                        NowPlayingAudioCard(
                            audio = state.audio,
                            liveMeters = kumi.liveMeters,
                            uacSleeping = uacSleeping,
                            lineOutActive = state.dacExtras?.lineOut == true,
                            dismissedDacFaults = dismissedDacFaults,
                            onDismissDacFault = { dismissedDacFaults = state.audio?.counters?.dacFaults ?: dismissedDacFaults },
                        )
                    }
                    if (hasEq) {
                        item {
                            EqQuickCard(
                                state = state,
                                math = math,
                                curveHeight = 28,
                                onSelect = { kumi.selectEq(it) },
                                onEdit = onOpenEq,
                            )
                        }
                    }
                    if (hasAudioControls) {
                        item {
                            SoundEffectsCard(
                                dsp = state.dsp,
                                onChange = { kumi.setDsp(it) },
                            )
                        }
                    }
                }
                1 -> {
                    if (hasAudioControls) {
                        item {
                            HeadphoneOutputCard(
                                output = state.output,
                                extras = state.dacExtras,
                                mic = state.mic.takeIf { caps?.audioIn == true },
                                onOutput = { kumi.setOutput(it) },
                                onExtras = { next ->
                                    val wasLineOut = state.dacExtras?.lineOut == true
                                    if (next.lineOut && !wasLineOut) confirmLineOut = true else kumi.setDacExtras(next)
                                },
                                onMic = { kumi.setMic(it) },
                            )
                        }
                        item {
                            AdvancedSoundCard(
                                dsp = state.dsp,
                                output = state.output,
                                extras = state.dacExtras,
                                dsd = caps?.dsd == true || uacSleeping,
                                onDsp = { kumi.setDsp(it) },
                                onOutput = { kumi.setOutput(it) },
                                onExtras = { kumi.setDacExtras(it) },
                            )
                        }
                    }
                }
                else -> {
                    if (hasAudioControls) {
                        item {
                            HeadphoneCheckCard(
                                impedance = state.impedance,
                                measuring = state.measuringImpedance,
                                streaming = state.audio?.stream != null && !kumi.simulated,
                                tonePlaying = state.testTonePlaying,
                                jackEmpty = state.audio?.jack == Jack.Empty,
                                onMeasure = { kumi.measureImpedance() },
                                onTone = { kumi.playTestTone(it) },
                            )
                        }
                    }
                }
            }
        }
        KeyboardHintStrip(
            hints = when (page) {
                0 -> listOf("E" to shEq, "X" to shXfeed, "L" to shLoud, "Tab" to shTab)
                1 -> listOf("C" to shClassH, "O" to stringResource(R.string.kumi_line_out_badge), "F" to shFilter, "Tab" to shTab)
                else -> listOf("M" to shMeasure, "Q–R" to stringResource(R.string.kumi_test_tone_title))
            },
        )
    }

    if (confirmLineOut) {
        ConfirmDialog(
            title = stringResource(R.string.kumi_line_out_confirm_title),
            text = stringResource(R.string.kumi_line_out_confirm_text),
            onConfirm = { state.dacExtras?.let { kumi.setDacExtras(it.copy(lineOut = true)) } },
            onDismiss = { confirmLineOut = false },
        )
    }
}

@Composable
private fun NowPlayingAudioCard(
    audio: AudioStatus?,
    liveMeters: StateFlow<AudioMeters?>,
    uacSleeping: Boolean,
    lineOutActive: Boolean,
    dismissedDacFaults: Long,
    onDismissDacFault: () -> Unit,
) {
    val stream = audio?.stream
    SectionCard(
        title = if (stream == null) stringResource(R.string.kumi_nothing_playing) else streamTitle(stream.kind, stream.rateHz, stream.bits),
        trailing = {
            Row(horizontalArrangement = Arrangement.spacedBy(5.dp), verticalAlignment = Alignment.CenterVertically) {
                if (lineOutActive) {
                    StatusPill(
                        text = stringResource(R.string.kumi_line_out_badge),
                        containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
                    )
                }
                if (audio != null) {
                    StatusPill(
                        text = stringResource(jackName(audio.jack)),
                        containerColor = if (audio.jack == Jack.Empty) MaterialTheme.colorScheme.surfaceContainerHighest else MaterialTheme.colorScheme.secondaryContainer,
                        contentColor = if (audio.jack == Jack.Empty) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                }
            }
        },
    ) {
        if (uacSleeping && stream == null) {
            Note(stringResource(R.string.kumi_audio_unplugged_hint))
        }
        LiveVuMeter(liveMeters = liveMeters, streaming = stream != null, showCounters = false)
        if (audio != null && audio.hostMuted) {
            StatusPill(
                stringResource(R.string.kumi_muted),
                containerColor = MaterialTheme.colorScheme.tertiaryContainer,
                contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
            )
        }
        val faults = audio?.counters?.dacFaults ?: 0L
        if (faults > dismissedDacFaults) {
            DismissibleErrorBanner(
                message = stringResource(R.string.kumi_audio_dac_glitch),
                onDismiss = onDismissDacFault,
            )
        }
    }
}

@Composable
fun EqQuickCard(state: KumiState, math: EqMath, curveHeight: Int, onSelect: (Int?) -> Unit, onEdit: (Int) -> Unit) {
    val active = state.audio?.eqActive
    SectionCard(
        title = stringResource(R.string.eq_title),
        trailing = {
            Surface(
                color = MaterialTheme.colorScheme.surfaceContainerHighest,
                contentColor = MaterialTheme.colorScheme.primary,
                shape = CircleShape,
                modifier = Modifier.clip(CircleShape).clickable { onEdit(active ?: 0) },
            ) {
                Row(
                    Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    Text(stringResource(R.string.action_edit), style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold)
                    KeycapBadge("E")
                }
            }
        },
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            val pills = listOf(null to stringResource(R.string.kumi_eq_off)) + (0 until EqProfile.SLOTS).map { slot ->
                val filled = state.eqSlots.getOrNull(slot) != null
                val shortName = stringResource(R.string.kumi_eq_slot_short, slot + 1)
                slot to if (filled) shortName else "+ $shortName"
            }
            pills.forEach { (slot, label) ->
                val selected = active == slot
                val filled = slot == null || state.eqSlots.getOrNull(slot) != null
                Surface(
                    color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                    contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                    shape = CircleShape,
                    modifier = Modifier
                        .weight(1f)
                        .height(30.dp)
                        .clip(CircleShape)
                        .clickable { if (slot == null || filled) onSelect(slot) else onEdit(slot) },
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            label,
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                            maxLines = 1,
                        )
                    }
                }
            }
        }
        val shown = active?.let { state.eqSlots.getOrNull(it) }
        ResponseCurve(shown ?: EqProfile.FLAT, math, Modifier.fillMaxWidth().height(curveHeight.dp))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SoundEffectsCard(dsp: DspSettings?, onChange: (DspSettings) -> Unit) {
    SectionCard(
        title = stringResource(R.string.kumi_dsp_title),
        trailing = if (dsp != null) {
            {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    KeycapBadge("X")
                    Text(
                        stringResource(R.string.kumi_tile_crossfeed),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        } else null,
    ) {
        if (dsp == null) {
            Note(stringResource(R.string.value_unavailable))
            return@SectionCard
        }
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            CrossfeedMode.entries.forEachIndexed { index, mode ->
                SegmentedButton(
                    selected = dsp.crossfeed == mode,
                    onClick = { if (dsp.crossfeed != mode) onChange(dsp.copy(crossfeed = mode)) },
                    shape = SegmentedButtonDefaults.itemShape(index, CrossfeedMode.entries.size),
                    icon = {},
                ) {
                    Text(stringResource(crossfeedShortName(mode)), style = MaterialTheme.typography.labelSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        ToggleRow(
            title = stringResource(R.string.kumi_loudness),
            checked = dsp.loudness,
            onCheckedChange = { onChange(dsp.copy(loudness = it)) },
            keyHint = "L",
        )
        ToggleRow(
            title = stringResource(R.string.kumi_limiter),
            checked = dsp.limiter,
            onCheckedChange = { onChange(dsp.copy(limiter = it)) },
            keyHint = "M",
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HeadphoneOutputCard(
    output: OutputSettings?,
    extras: DacExtras?,
    mic: MicSettings?,
    onOutput: (OutputSettings) -> Unit,
    onExtras: (DacExtras) -> Unit,
    onMic: (MicSettings) -> Unit,
) {
    SectionCard(stringResource(R.string.kumi_dac_title)) {
        if (extras != null) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                KeycapBadge("C")
                Text(stringResource(R.string.kumi_class_h_label), style = MaterialTheme.typography.labelLarge)
            }
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                ClassHMode.entries.forEachIndexed { index, mode ->
                    SegmentedButton(
                        selected = extras.classH == mode,
                        onClick = { if (extras.classH != mode) onExtras(extras.copy(classH = mode)) },
                        shape = SegmentedButtonDefaults.itemShape(index, ClassHMode.entries.size),
                        icon = {},
                    ) {
                        Text(stringResource(classHName(mode)), style = MaterialTheme.typography.labelSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            ToggleRow(
                title = stringResource(R.string.kumi_line_out),
                subtitle = stringResource(R.string.kumi_line_out_text),
                checked = extras.lineOut,
                onCheckedChange = { onExtras(extras.copy(lineOut = it)) },
                keyHint = "O",
            )
        }
        if (output != null) {
            var open by remember { mutableStateOf(false) }
            ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
                OutlinedTextField(
                    value = stringResource(filterTitle(output.filter.stock())),
                    onValueChange = {},
                    readOnly = true,
                    label = { Text(stringResource(R.string.sound_filter)) },
                    trailingIcon = {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                            KeycapBadge("F")
                            ExposedDropdownMenuDefaults.TrailingIcon(expanded = open)
                        }
                    },
                    modifier = Modifier.menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, true).fillMaxWidth(),
                )
                ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                    OutputFilter.entries.forEach { filter ->
                        DropdownMenuItem(
                            text = {
                                Column {
                                    Text(stringResource(filterTitle(filter.stock())))
                                    Text(stringResource(filterText(filter.stock())), style = MaterialTheme.typography.bodySmall)
                                }
                            },
                            onClick = {
                                open = false
                                if (filter != output.filter) onOutput(output.copy(filter = filter))
                            },
                        )
                    }
                }
            }
            CentiDbSlider(
                stringResource(R.string.kumi_volume_limit),
                output.volumeLimitCentiDb,
                KumiLimits.OUTPUT_MIN_VOLUME_LIMIT_CENTI_DB,
                0,
                50,
            ) { onOutput(output.copy(volumeLimitCentiDb = it)) }
        }
        if (mic != null) {
            HorizontalDivider()
            CentiDbSlider(stringResource(R.string.sound_mic), mic.gainCentiDb, 0, KumiLimits.MIC_MAX_GAIN_CENTI_DB, 50) { onMic(MicSettings(it)) }
        }
    }
}

@Composable
private fun AdvancedSoundCard(
    dsp: DspSettings?,
    output: OutputSettings?,
    extras: DacExtras?,
    dsd: Boolean,
    onDsp: (DspSettings) -> Unit,
    onOutput: (OutputSettings) -> Unit,
    onExtras: (DacExtras) -> Unit,
) {
    ExpandableSection(
        title = stringResource(R.string.kumi_advanced_sound_title),
        subtitle = stringResource(R.string.kumi_advanced_sound_summary),
    ) {
        if (output != null) {
            CentiDbSlider(
                stringResource(R.string.kumi_balance),
                output.balanceCentiDb,
                -KumiLimits.OUTPUT_MAX_BALANCE_CENTI_DB,
                KumiLimits.OUTPUT_MAX_BALANCE_CENTI_DB,
                50,
            ) { onOutput(output.copy(balanceCentiDb = it)) }
        }
        if (dsp != null) {
            ToggleRow(
                title = stringResource(R.string.kumi_mono),
                subtitle = stringResource(R.string.kumi_mono_text),
                checked = dsp.mono,
                onCheckedChange = { onDsp(dsp.copy(mono = it)) },
            )
            ToggleRow(
                title = stringResource(R.string.kumi_swap_channels),
                subtitle = stringResource(R.string.kumi_swap_channels_text),
                checked = dsp.swapChannels,
                onCheckedChange = { onDsp(dsp.copy(swapChannels = it)) },
            )
            ToggleRow(
                title = stringResource(R.string.kumi_invert_phase),
                subtitle = stringResource(R.string.kumi_invert_phase_text),
                checked = dsp.invertPhase,
                onCheckedChange = { onDsp(dsp.copy(invertPhase = it)) },
            )
        }
        if (output != null && dsd) {
            ToggleRow(
                title = stringResource(R.string.kumi_dsd_direct),
                subtitle = stringResource(R.string.kumi_dsd_direct_text),
                checked = output.dsdDirect,
                onCheckedChange = { onOutput(output.copy(dsdDirect = it)) },
            )
        }
        if (extras != null) {
            ToggleRow(
                title = stringResource(R.string.kumi_hpf),
                subtitle = stringResource(R.string.kumi_hpf_text),
                checked = extras.highPassFilter,
                onCheckedChange = { onExtras(extras.copy(highPassFilter = it)) },
            )
            ToggleRow(
                title = stringResource(R.string.kumi_de_emphasis),
                subtitle = stringResource(R.string.kumi_de_emphasis_text),
                checked = extras.deEmphasis,
                onCheckedChange = { onExtras(extras.copy(deEmphasis = it)) },
            )
        }
    }
}

@Composable
fun PowerSleepCard(
    extras: DacExtras?,
    onExtras: (DacExtras) -> Unit,
) {
    if (extras == null) return
    SectionCard(stringResource(R.string.kumi_power_sleep_title)) {
        ToggleRow(
            title = stringResource(R.string.kumi_stay_awake),
            subtitle = stringResource(R.string.kumi_stay_awake_text),
            checked = extras.stayAwakeWhileActive,
            onCheckedChange = { onExtras(extras.copy(stayAwakeWhileActive = it)) },
        )
        ToggleRow(
            title = stringResource(R.string.kumi_auto_uac),
            subtitle = stringResource(R.string.kumi_auto_uac_text),
            checked = extras.autoUacOnJack,
            onCheckedChange = { onExtras(extras.copy(autoUacOnJack = it)) },
        )
    }
}

@Composable
private fun HeadphoneCheckCard(
    impedance: ImpedanceReport?,
    measuring: Boolean,
    streaming: Boolean,
    tonePlaying: Boolean,
    jackEmpty: Boolean,
    onMeasure: () -> Unit,
    onTone: (TestToneRequest) -> Unit,
) {
    SectionCard(
        title = stringResource(R.string.kumi_hp_diag_title),
    ) {
        if (jackEmpty) {
            Note(stringResource(R.string.kumi_tools_unplugged_hint))
        }
        if (measuring) {
            LinearProgressIndicator(Modifier.fillMaxWidth())
        }
        if (impedance != null) {
            Text(impedanceSummary(impedance), style = MaterialTheme.typography.titleLarge)
            if (impedance.leftAcOhms != null || impedance.rightAcOhms != null) {
                Note(impedanceAcSummary(impedance))
            }
            impedanceAdvice(impedance)?.let { Note(stringResource(it)) }
        } else {
            Note(stringResource(R.string.kumi_impedance_hint))
        }
        FilledTonalButton(
            onClick = onMeasure,
            enabled = !measuring && !streaming && !tonePlaying,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                KeycapBadge("M")
                Text(stringResource(if (measuring) R.string.kumi_tile_impedance_busy else R.string.kumi_impedance_measure))
            }
        }
        HorizontalDivider()
        Text(stringResource(R.string.kumi_test_tone_title), style = MaterialTheme.typography.titleSmall)
        Note(stringResource(R.string.kumi_test_tone_hint))
        val tones = listOf(
            Triple("Q", R.string.kumi_dev_bench_left, TestToneRequest(AudioChannel.Left, 1_000, 1_500, false)),
            Triple("W", R.string.kumi_dev_bench_right, TestToneRequest(AudioChannel.Right, 1_000, 1_500, false)),
            Triple("E", R.string.kumi_dev_bench_both, TestToneRequest(AudioChannel.Both, 1_000, 1_500, false)),
            Triple("R", R.string.kumi_test_phase, TestToneRequest(AudioChannel.Both, 1_000, 1_500, true)),
        )
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            tones.chunked(2).forEach { pair ->
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    pair.forEach { (key, labelRes, req) ->
                        OutlinedButton(
                            onClick = { onTone(req) },
                            enabled = !streaming && !measuring,
                            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 8.dp),
                            modifier = Modifier.weight(1f),
                        ) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                                KeycapBadge(key)
                                Text(stringResource(labelRes), style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    }
                }
            }
        }
        if (tonePlaying) {
            FilledTonalButton(
                onClick = { onTone(TestToneRequest(AudioChannel.Both, 0, 0, false)) },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.kumi_dev_bench_stop))
            }
        }
    }
}

@Composable
fun streamTitle(kind: StreamKind, rate: Int, bits: Int): String {
    val khz = if (rate % 1000 == 0) (rate / 1000).toString() else "%.1f".format(rate / 1000.0)
    return when (kind) {
        StreamKind.Pcm -> stringResource(R.string.kumi_stream_pcm, khz, bits)
        StreamKind.Dop -> stringResource(R.string.kumi_stream_dop, dsdName(if (rate >= 1_000_000) rate else rate * 16))
        StreamKind.NativeDsd -> stringResource(R.string.kumi_stream_dsd, dsdName(if (rate >= 1_000_000) rate else rate * 32))
        StreamKind.Unknown -> stringResource(R.string.kumi_stream_unknown, khz)
    }
}

private fun dsdName(bitRate: Int): String {
    val base = if (bitRate % 48_000 == 0 && bitRate % 44_100 != 0) 48_000 else 44_100
    return "DSD" + (bitRate / base).coerceAtLeast(1).toString()
}

fun jackName(jack: Jack): Int = when (jack) {
    Jack.Empty -> R.string.kumi_jack_empty
    Jack.Headphones -> R.string.kumi_jack_headphones
    Jack.Headset -> R.string.kumi_jack_headset
    Jack.Unknown -> R.string.kumi_jack_unknown
}

fun jackShortName(jack: Jack): Int = when (jack) {
    Jack.Empty -> R.string.kumi_jack_short_empty
    Jack.Headphones -> R.string.kumi_jack_short_headphones
    Jack.Headset -> R.string.kumi_jack_short_headset
    Jack.Unknown -> R.string.kumi_jack_short_unknown
}

fun crossfeedShortName(mode: CrossfeedMode): Int = when (mode) {
    CrossfeedMode.Off -> R.string.kumi_crossfeed_off
    CrossfeedMode.Subtle -> R.string.kumi_crossfeed_subtle
    CrossfeedMode.Natural -> R.string.kumi_crossfeed_natural
    CrossfeedMode.Strong -> R.string.kumi_crossfeed_strong
}

fun classHName(mode: ClassHMode): Int = when (mode) {
    ClassHMode.AdaptiveHv -> R.string.kumi_class_h_adaptive
    ClassHMode.LowPowerLv -> R.string.kumi_class_h_low_power
    ClassHMode.FixedHv -> R.string.kumi_class_h_fixed
}

@Composable
fun impedanceSummary(report: ImpedanceReport): String {
    val left = report.leftOhms?.let { stringResource(R.string.kumi_impedance_ohm, it) } ?: stringResource(R.string.kumi_impedance_open)
    val right = report.rightOhms?.let { stringResource(R.string.kumi_impedance_ohm, it) } ?: stringResource(R.string.kumi_impedance_open)
    return stringResource(R.string.kumi_impedance_result, left, right)
}

@Composable
fun impedanceAcSummary(report: ImpedanceReport): String {
    val left = report.leftAcOhms?.let { stringResource(R.string.kumi_impedance_ohm, it) } ?: stringResource(R.string.kumi_impedance_open)
    val right = report.rightAcOhms?.let { stringResource(R.string.kumi_impedance_ohm, it) } ?: stringResource(R.string.kumi_impedance_open)
    return stringResource(R.string.kumi_impedance_ac_result, left, right)
}

fun impedanceAdvice(report: ImpedanceReport): Int? {
    val ohms = listOfNotNull(report.leftOhms, report.rightOhms).maxOrNull() ?: return null
    return when {
        ohms <= 32 -> R.string.kumi_impedance_rec_iem
        ohms <= 150 -> R.string.kumi_impedance_rec_mid
        else -> R.string.kumi_impedance_rec_high
    }
}
