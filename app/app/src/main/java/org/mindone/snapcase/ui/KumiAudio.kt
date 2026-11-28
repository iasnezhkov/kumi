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
import androidx.compose.foundation.lazy.rememberLazyListState
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
import androidx.compose.runtime.LaunchedEffect
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
import org.mindone.snapcase.kumi.AudioSpectrum
import org.mindone.snapcase.kumi.AudioStatus
import org.mindone.snapcase.kumi.CaseError
import org.mindone.snapcase.kumi.ClassHMode
import org.mindone.snapcase.kumi.CrossfeedMode
import org.mindone.snapcase.kumi.DacExtras
import org.mindone.snapcase.kumi.DspExtras
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
    CaseError.PresenceRequired -> stringResource(R.string.kumi_err_presence_required)
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
    onLineOutToggled: (Boolean) -> Unit = {},
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

    val listState = rememberLazyListState()
    LaunchedEffect(page) { listState.scrollToItem(0) }

    val shTab = stringResource(R.string.kumi_key_hint_tab)
    val shEq = stringResource(R.string.eq_title)
    val shAb = stringResource(R.string.kumi_key_hint_eq_ab)
    val shXfeed = stringResource(R.string.kumi_tile_crossfeed)
    val shLoud = stringResource(R.string.kumi_loudness)
    val shLimit = stringResource(R.string.kumi_limiter)
    val shAutoLevel = stringResource(R.string.kumi_auto_level)
    val shClassH = stringResource(R.string.kumi_class_h_label)
    val shLineOut = stringResource(R.string.kumi_line_out)
    val shLineOutBadge = stringResource(R.string.kumi_line_out_badge)
    val shFilter = stringResource(R.string.sound_filter)
    val shMeasure = stringResource(R.string.kumi_impedance_measure)
    val shLeft = stringResource(R.string.kumi_dev_bench_left)
    val shRight = stringResource(R.string.kumi_dev_bench_right)
    val shBoth = stringResource(R.string.kumi_dev_bench_both)
    val shPhase = stringResource(R.string.kumi_test_phase)

    val effectsKeys = rememberMnemonicKeys(
        specs = listOf(
            ShortcutSpec("eq", shEq, KeyEvent.KEYCODE_E),
            ShortcutSpec("xfeed", shXfeed, KeyEvent.KEYCODE_S),
            ShortcutSpec("loud", shLoud, KeyEvent.KEYCODE_D),
            ShortcutSpec("limit", shLimit, KeyEvent.KEYCODE_P),
            ShortcutSpec("auto_level", shAutoLevel, KeyEvent.KEYCODE_A),
        ),
    )
    val dacKeys = rememberMnemonicKeys(
        specs = listOf(
            ShortcutSpec("power", shClassH, KeyEvent.KEYCODE_H),
            ShortcutSpec("line_out", shLineOut, KeyEvent.KEYCODE_L, hintLabel = shLineOutBadge),
            ShortcutSpec("filter", shFilter, KeyEvent.KEYCODE_S),
        ),
    )
    val toolsKeys = rememberMnemonicKeys(
        specs = listOf(
            ShortcutSpec("measure", shMeasure, KeyEvent.KEYCODE_C),
            ShortcutSpec("left", shLeft, KeyEvent.KEYCODE_L),
            ShortcutSpec("right", shRight, KeyEvent.KEYCODE_R),
            ShortcutSpec("both", shBoth, KeyEvent.KEYCODE_O),
            ShortcutSpec("phase", shPhase, KeyEvent.KEYCODE_P),
        ),
    )

    val audioShortcuts = when (page) {
        0 -> listOfNotNull(
            effectsKeys.pair("eq"),
            "␣" to shAb,
            effectsKeys.pair("xfeed"),
            effectsKeys.pair("loud"),
            effectsKeys.pair("limit"),
            effectsKeys.pair("auto_level"),
            "↵" to shTab,
        )
        1 -> dacKeys.pairs() + ("↵" to shTab)
        else -> toolsKeys.pairs() + ("↵" to shTab)
    }

    if (state.connected) {
        RegisterScreenTabs(count = 3, current = page, onSelect = { page = it })
        RegisterScreenScroll(listState)
    }

    RegisterScreenKeys(
        key = listOf(
            page,
            state.connected,
            activeEq,
            lastEqSlot,
            state.eqSlots,
            state.dsp,
            state.dspExtras,
            state.output,
            state.dacExtras,
            state.audio?.stream,
            state.measuringImpedance,
            state.testTonePlaying,
            effectsKeys,
            dacKeys,
            toolsKeys,
        ),
        shortcuts = if (state.connected) audioShortcuts else emptyList(),
    ) { code, repeat ->
        if (repeat || !state.connected) return@RegisterScreenKeys false
        when (page) {
            0 -> {
                if (code == KeyEvent.KEYCODE_SPACE) {
                    return@RegisterScreenKeys if (hasEq) {
                        if (activeEq != null) kumi.selectEq(null)
                        else {
                            val t = lastEqSlot.takeIf { state.eqSlots.getOrNull(it) != null } ?: state.eqSlots.indexOfFirst { it != null }
                            if (t >= 0) kumi.selectEq(t) else onOpenEq(0)
                        }
                        true
                    } else {
                        false
                    }
                }
                when (effectsKeys.idFor(code)) {
                    "eq" -> if (hasEq) { onOpenEq(activeEq ?: lastEqSlot); true } else false
                    "xfeed" -> state.dsp?.let { dsp ->
                        val modes = CrossfeedMode.entries
                        kumi.setDsp(dsp.copy(crossfeed = modes[(modes.indexOf(dsp.crossfeed) + 1) % modes.size]))
                        true
                    } ?: false
                    "loud" -> state.dsp?.let { dsp ->
                        kumi.setDsp(dsp.copy(loudness = !dsp.loudness))
                        true
                    } ?: false
                    "limit" -> state.dsp?.let { dsp ->
                        kumi.setDsp(dsp.copy(limiter = !dsp.limiter))
                        true
                    } ?: false
                    "auto_level" -> state.dspExtras?.let { ex ->
                        kumi.setDspExtras(ex.copy(autoLevel = !ex.autoLevel))
                        true
                    } ?: false
                    else -> false
                }
            }
            1 -> when (dacKeys.idFor(code)) {
                "power" -> state.dacExtras?.let { ex ->
                    if (ex.lineOut) return@let false
                    val modes = ClassHMode.entries
                    kumi.setDacExtras(ex.copy(classH = modes[(modes.indexOf(ex.classH) + 1) % modes.size]))
                    true
                } ?: false
                "line_out" -> state.dacExtras?.let { ex ->
                    if (ex.lineOut) {
                        kumi.setDacExtras(ex.copy(lineOut = false)) {
                            if (!kumi.simulated) onLineOutToggled(false)
                        }
                    } else {
                        confirmLineOut = true
                    }
                    true
                } ?: false
                "filter" -> state.output?.let { out ->
                    val filters = OutputFilter.entries
                    kumi.setOutput(out.copy(filter = filters[(filters.indexOf(out.filter) + 1) % filters.size]))
                    true
                } ?: false
                else -> false
            }
            else -> {
                val streamingNow = state.audio?.stream != null && !kumi.simulated
                when (toolsKeys.idFor(code)) {
                    "measure" -> if (!state.measuringImpedance && !streamingNow && !state.testTonePlaying) {
                        kumi.measureImpedance(); true
                    } else false
                    "left" -> if (!streamingNow && !state.measuringImpedance) {
                        kumi.playTestTone(TestToneRequest(AudioChannel.Left, 1_000, 1_500, false)); true
                    } else false
                    "right" -> if (!streamingNow && !state.measuringImpedance) {
                        kumi.playTestTone(TestToneRequest(AudioChannel.Right, 1_000, 1_500, false)); true
                    } else false
                    "both" -> if (!streamingNow && !state.measuringImpedance) {
                        kumi.playTestTone(TestToneRequest(AudioChannel.Both, 1_000, 1_500, false)); true
                    } else false
                    "phase" -> if (!streamingNow && !state.measuringImpedance) {
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
            state = listState,
            contentPadding = PaddingValues(top = 0.dp, bottom = 4.dp),
            verticalArrangement = Arrangement.spacedBy(5.dp),
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
                            liveSpectrum = kumi.liveSpectrum,
                            autoLevelOn = state.dspExtras?.autoLevel == true,
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
                                curveHeight = 62,
                                liveSpectrum = kumi.liveSpectrum,
                                compact = true,
                                editKeyHint = effectsKeys.label("eq"),
                                onSelect = { kumi.selectEq(it) },
                                onEdit = onOpenEq,
                            )
                        }
                    }
                    if (hasAudioControls) {
                        item {
                            SoundEffectsCard(
                                dsp = state.dsp,
                                dspExtras = state.dspExtras,
                                xfeedKeyHint = effectsKeys.label("xfeed"),
                                loudKeyHint = effectsKeys.label("loud"),
                                limitKeyHint = effectsKeys.label("limit"),
                                autoLevelKeyHint = effectsKeys.label("auto_level"),
                                onChange = { kumi.setDsp(it) },
                                onExtras = { kumi.setDspExtras(it) },
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
                                powerKeyHint = dacKeys.label("power"),
                                lineOutKeyHint = dacKeys.label("line_out"),
                                filterKeyHint = dacKeys.label("filter"),
                                onOutput = { kumi.setOutput(it) },
                                onExtras = { next ->
                                    val wasLineOut = state.dacExtras?.lineOut == true
                                    if (next.lineOut && !wasLineOut) {
                                        confirmLineOut = true
                                    } else {
                                        kumi.setDacExtras(next) {
                                            if (wasLineOut && !next.lineOut && !kumi.simulated) onLineOutToggled(false)
                                        }
                                    }
                                },
                                onMic = { kumi.setMic(it) },
                            )
                        }
                        item {
                            AdvancedSoundCard(
                                dsp = state.dsp,
                                dspExtras = state.dspExtras,
                                output = state.output,
                                extras = state.dacExtras,
                                dsd = caps?.dsd == true || uacSleeping,
                                onDsp = { kumi.setDsp(it) },
                                onDspExtras = { kumi.setDspExtras(it) },
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
                                measureKeyHint = toolsKeys.label("measure"),
                                leftKeyHint = toolsKeys.label("left"),
                                rightKeyHint = toolsKeys.label("right"),
                                bothKeyHint = toolsKeys.label("both"),
                                phaseKeyHint = toolsKeys.label("phase"),
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
                0 -> listOf(
                    effectsKeys.label("eq") to shEq,
                    effectsKeys.label("xfeed") to shXfeed,
                    effectsKeys.label("loud") to shLoud,
                    "↵" to shTab,
                )
                1 -> listOf(
                    dacKeys.label("power") to shClassH,
                    dacKeys.label("line_out") to shLineOutBadge,
                    dacKeys.label("filter") to shFilter,
                    "↵" to shTab,
                )
                else -> listOf(
                    toolsKeys.label("measure") to shMeasure,
                    toolsKeys.label("left") to shLeft,
                    toolsKeys.label("right") to shRight,
                    "↵" to shTab,
                )
            },
        )
    }

    if (confirmLineOut) {
        ConfirmDialog(
            title = stringResource(R.string.kumi_line_out_confirm_title),
            text = stringResource(R.string.kumi_line_out_confirm_text),
            onConfirm = {
                state.dacExtras?.let {
                    kumi.setDacExtras(it.copy(lineOut = true)) {
                        if (!kumi.simulated) onLineOutToggled(true)
                    }
                }
            },
            onDismiss = { confirmLineOut = false },
        )
    }
}

@Composable
private fun NowPlayingAudioCard(
    audio: AudioStatus?,
    liveMeters: StateFlow<AudioMeters?>,
    liveSpectrum: StateFlow<AudioSpectrum?>,
    autoLevelOn: Boolean,
    uacSleeping: Boolean,
    lineOutActive: Boolean,
    dismissedDacFaults: Long,
    onDismissDacFault: () -> Unit,
) {
    val stream = audio?.stream
    val spectrum by liveSpectrum.collectAsStateWithLifecycle()
    val s = spectrum?.takeIf { it.active }
    val lufsPill = if (s != null) {
        val lufs = if (s.lufsShortTermCenti > KumiLimits.LUFS_FLOOR_CENTI + 50) {
            "%.1f LUFS".format(s.lufsShortTermCenti / 100.0)
        } else if (s.lufsMomentaryCenti > KumiLimits.LUFS_FLOOR_CENTI + 50) {
            "%.1f LUFS".format(s.lufsMomentaryCenti / 100.0)
        } else {
            null
        }
        val tpCenti = maxOf(s.truePeakLeftCentiDb, s.truePeakRightCentiDb)
        val tp = if (tpCenti > KumiLimits.METER_FLOOR_CENTI_DB + 100) "%.1f TP".format(tpCenti / 100.0) else null
        val agc = if (autoLevelOn && s.autoLevelGainCentiDb != 0) centiDb(s.autoLevelGainCentiDb) else null
        listOfNotNull(lufs, tp, agc).joinToString(" · ").takeIf { it.isNotEmpty() }
    } else {
        null
    }
    SectionCard(
        title = if (stream == null) stringResource(R.string.kumi_nothing_playing) else streamTitle(stream.kind, stream.rateHz, stream.bits),
        compact = true,
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
        if (lufsPill != null) {
            Text(
                lufsPill,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
            )
        }
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
fun EqQuickCard(
    state: KumiState,
    math: EqMath,
    curveHeight: Int,
    liveSpectrum: StateFlow<AudioSpectrum?>? = null,
    compact: Boolean = false,
    editKeyHint: String = "E",
    onSelect: (Int?) -> Unit,
    onEdit: (Int) -> Unit,
) {
    val active = state.audio?.eqActive
    SectionCard(
        title = stringResource(R.string.eq_title),
        compact = compact,
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
                    KeycapBadge(editKeyHint)
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
                        .height(if (compact) 26.dp else 30.dp)
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
        ResponseCurve(
            profile = shown ?: EqProfile.FLAT,
            math = math,
            modifier = Modifier.fillMaxWidth().height(curveHeight.dp),
            liveSpectrum = liveSpectrum,
            eqBase = state.eqBase,
            toneEq = state.toneEq,
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SoundEffectsCard(
    dsp: DspSettings?,
    dspExtras: DspExtras?,
    xfeedKeyHint: String,
    loudKeyHint: String,
    limitKeyHint: String,
    autoLevelKeyHint: String,
    onChange: (DspSettings) -> Unit,
    onExtras: (DspExtras) -> Unit,
) {
    SectionCard(
        title = stringResource(R.string.kumi_dsp_title),
        compact = true,
        trailing = if (dsp != null) {
            {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    KeycapBadge(xfeedKeyHint)
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
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            CompactEffectToggleChip(
                label = stringResource(R.string.kumi_loudness),
                keyHint = loudKeyHint,
                active = dsp.loudness,
                onClick = { onChange(dsp.copy(loudness = !dsp.loudness)) },
                modifier = Modifier.weight(1f),
            )
            CompactEffectToggleChip(
                label = stringResource(R.string.kumi_limiter),
                keyHint = limitKeyHint,
                active = dsp.limiter,
                onClick = { onChange(dsp.copy(limiter = !dsp.limiter)) },
                modifier = Modifier.weight(1f),
            )
            if (dspExtras != null) {
                CompactEffectToggleChip(
                    label = stringResource(R.string.kumi_auto_level),
                    keyHint = autoLevelKeyHint,
                    active = dspExtras.autoLevel,
                    onClick = { onExtras(dspExtras.copy(autoLevel = !dspExtras.autoLevel)) },
                    modifier = Modifier.weight(1f),
                )
            }
        }
        if (dspExtras != null && dspExtras.autoLevel) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf(-18, -16, -14).forEach { target ->
                    val selected = dspExtras.autoLevelTargetLufs == target
                    Surface(
                        color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                        contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                        shape = CircleShape,
                        modifier = Modifier
                            .weight(1f)
                            .height(26.dp)
                            .clip(CircleShape)
                            .clickable { onExtras(dspExtras.copy(autoLevelTargetLufs = target)) },
                    ) {
                        Box(contentAlignment = Alignment.Center) {
                            Text(
                                stringResource(R.string.kumi_auto_level_target, target),
                                style = MaterialTheme.typography.labelSmall,
                                fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                                maxLines = 1,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun CompactEffectToggleChip(
    label: String,
    keyHint: String,
    active: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        color = if (active) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        contentColor = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
        shape = RoundedCornerShape(12.dp),
        modifier = modifier.height(44.dp).clip(RoundedCornerShape(12.dp)).clickable(onClick = onClick),
    ) {
        Column(
            Modifier.fillMaxSize().padding(horizontal = 8.dp, vertical = 5.dp),
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Text(
                    if (active) stringResource(R.string.value_on).replaceFirstChar { it.uppercase() } else stringResource(R.string.kumi_crossfeed_off),
                    style = MaterialTheme.typography.labelSmall,
                    fontWeight = if (active) FontWeight.Bold else FontWeight.Medium,
                    maxLines = 1,
                )
                KeycapBadge(keyHint, emphasized = active)
            }
            Text(
                label,
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HeadphoneOutputCard(
    output: OutputSettings?,
    extras: DacExtras?,
    mic: MicSettings?,
    powerKeyHint: String,
    lineOutKeyHint: String,
    filterKeyHint: String,
    onOutput: (OutputSettings) -> Unit,
    onExtras: (DacExtras) -> Unit,
    onMic: (MicSettings) -> Unit,
) {
    SectionCard(stringResource(R.string.kumi_dac_title)) {
        if (extras != null) {
            val effectiveClassH = if (extras.lineOut) ClassHMode.FixedHv else extras.classH
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                KeycapBadge(powerKeyHint)
                Text(stringResource(R.string.kumi_class_h_label), style = MaterialTheme.typography.labelLarge)
            }
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                ClassHMode.entries.forEachIndexed { index, mode ->
                    SegmentedButton(
                        selected = effectiveClassH == mode,
                        enabled = !extras.lineOut,
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
                keyHint = lineOutKeyHint,
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
                            KeycapBadge(filterKeyHint)
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
    dspExtras: DspExtras?,
    output: OutputSettings?,
    extras: DacExtras?,
    dsd: Boolean,
    onDsp: (DspSettings) -> Unit,
    onDspExtras: (DspExtras) -> Unit,
    onOutput: (OutputSettings) -> Unit,
    onExtras: (DacExtras) -> Unit,
) {
    ExpandableSection(
        title = stringResource(R.string.kumi_advanced_sound_title),
        subtitle = stringResource(R.string.kumi_advanced_sound_summary),
    ) {
        if (dspExtras != null) {
            FrequencyChoiceRow(
                label = stringResource(R.string.kumi_subsonic_title),
                selectedHz = dspExtras.subsonicHz,
                options = listOf(0, 15, 20, 30, 40),
                onSelect = { onDspExtras(dspExtras.copy(subsonicHz = it)) },
            )
            FrequencyChoiceRow(
                label = stringResource(R.string.kumi_mono_bass_title),
                selectedHz = dspExtras.monoBassHz,
                options = listOf(0, 80, 120, 150, 200),
                onSelect = { onDspExtras(dspExtras.copy(monoBassHz = it)) },
            )
        }
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
private fun FrequencyChoiceRow(
    label: String,
    selectedHz: Int,
    options: List<Int>,
    onSelect: (Int) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
        Text(label, style = MaterialTheme.typography.labelLarge)
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            options.forEach { hz ->
                val selected = selectedHz == hz
                Surface(
                    color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                    contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                    shape = CircleShape,
                    modifier = Modifier
                        .weight(1f)
                        .height(28.dp)
                        .clip(CircleShape)
                        .clickable { onSelect(hz) },
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            if (hz == 0) stringResource(R.string.kumi_crossfeed_off) else stringResource(R.string.kumi_hz_value, hz),
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                            maxLines = 1,
                        )
                    }
                }
            }
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
    measureKeyHint: String,
    leftKeyHint: String,
    rightKeyHint: String,
    bothKeyHint: String,
    phaseKeyHint: String,
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
                KeycapBadge(measureKeyHint)
                Text(stringResource(if (measuring) R.string.kumi_tile_impedance_busy else R.string.kumi_impedance_measure))
            }
        }
        HorizontalDivider()
        Text(stringResource(R.string.kumi_test_tone_title), style = MaterialTheme.typography.titleSmall)
        Note(stringResource(R.string.kumi_test_tone_hint))
        val tones = listOf(
            Triple(leftKeyHint, R.string.kumi_dev_bench_left, TestToneRequest(AudioChannel.Left, 1_000, 1_500, false)),
            Triple(rightKeyHint, R.string.kumi_dev_bench_right, TestToneRequest(AudioChannel.Right, 1_000, 1_500, false)),
            Triple(bothKeyHint, R.string.kumi_dev_bench_both, TestToneRequest(AudioChannel.Both, 1_000, 1_500, false)),
            Triple(phaseKeyHint, R.string.kumi_test_phase, TestToneRequest(AudioChannel.Both, 1_000, 1_500, true)),
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
