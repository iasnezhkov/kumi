// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.os.SystemClock
import android.view.KeyEvent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.InputChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.math.exp
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.StateFlow
import org.mindone.snapcase.R
import org.mindone.snapcase.data.AppPreferences
import org.mindone.snapcase.kumi.AudioSpectrum
import org.mindone.snapcase.kumi.BandKind
import org.mindone.snapcase.kumi.BuiltInEqPreset
import org.mindone.snapcase.kumi.CustomEqPreset
import org.mindone.snapcase.kumi.EqBaseSettings
import org.mindone.snapcase.kumi.EqMath
import org.mindone.snapcase.kumi.EqProfile
import org.mindone.snapcase.kumi.FastEqCurve
import org.mindone.snapcase.kumi.KumiEqPresets
import org.mindone.snapcase.kumi.KumiLimits
import org.mindone.snapcase.kumi.KumiSession
import org.mindone.snapcase.kumi.ParametricBand
import org.mindone.snapcase.kumi.ToneEqSettings

private const val EQ_MIN_HZ = 20.0
private const val EQ_MAX_HZ = 20_000.0
private val EQ_LOG_SPAN = ln(EQ_MAX_HZ / EQ_MIN_HZ)
private const val EQ_GAIN_RANGE_DB = 18.0
private val EQ_GRID_DB = doubleArrayOf(-12.0, -6.0, 0.0, 6.0, 12.0)
private val EQ_GRID_HZ = doubleArrayOf(50.0, 100.0, 250.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0)
private const val LIVE_DRAG_COMMIT_MS = 85L

@Composable
fun KumiEqScreen(
    kumi: KumiSession,
    prefs: AppPreferences,
    math: EqMath,
    initialSlot: Int,
    onBack: () -> Unit = {},
) {
    val state by kumi.state.collectAsStateWithLifecycle()
    val settings by prefs.settings.collectAsStateWithLifecycle()
    var slot by rememberSaveable { mutableIntStateOf(initialSlot.coerceIn(0, EqProfile.SLOTS - 1)) }
    var page by rememberSaveable { mutableIntStateOf(0) }
    var presetCategory by rememberSaveable { mutableIntStateOf(0) }
    var selectedBand by rememberSaveable { mutableIntStateOf(0) }
    var selectedBuiltInId by rememberSaveable { mutableStateOf<String?>(null) }
    var savingCustom by remember { mutableStateOf(false) }
    var confirmReset by remember { mutableStateOf(false) }
    var deletingPreset by remember { mutableStateOf<CustomEqPreset?>(null) }

    val profile = state.eqSlots.getOrNull(slot) ?: EqProfile.FLAT
    val active = state.audio?.eqActive == slot
    val apply: (EqProfile, Boolean) -> Unit = { next, immediate ->
        if (next.valid) {
            kumi.editEq(slot, next, immediate = immediate, activate = !active && next.bands.isNotEmpty())
        }
    }
    val toggleEqBypass: () -> Unit = {
        if (state.audio?.eqActive != null) {
            kumi.selectEq(null)
        } else {
            val target = slot.takeIf { state.eqSlots.getOrNull(it) != null }
                ?: state.eqSlots.indexOfFirst { it != null }
            if (target >= 0) kumi.selectEq(target)
        }
    }

    if (profile.bands.isNotEmpty() && selectedBand !in profile.bands.indices) {
        selectedBand = (profile.bands.size - 1).coerceAtLeast(0)
    }

    val shTab = stringResource(R.string.kumi_key_hint_tab)
    val shAb = stringResource(R.string.kumi_key_hint_eq_ab)
    val shPreset = stringResource(R.string.kumi_eq_tab_presets)
    val shGroup = stringResource(R.string.kumi_key_hint_group)
    val shSave = stringResource(R.string.action_save)
    val shBand = stringResource(R.string.kumi_key_hint_band)
    val shGain = stringResource(R.string.kumi_key_hint_gain)
    val shFreq = stringResource(R.string.kumi_key_hint_freq)
    val shQ = stringResource(R.string.kumi_eq_q)
    val shType = stringResource(R.string.kumi_key_hint_type)
    val shZero = stringResource(R.string.kumi_key_hint_zero)
    val shSlot = stringResource(R.string.kumi_eq_tab_slots)
    val shPreamp = stringResource(R.string.kumi_preamp)
    val shAuto = stringResource(R.string.kumi_preamp_auto)

    val scrollState = rememberScrollState()
    LaunchedEffect(page) { scrollState.scrollTo(0) }

    val pageShortcuts = when (page) {
        0 -> listOf("Q–R / A–F" to shPreset, "Z" to shGroup, "C" to shSave, "␣" to shAb, "↵" to shTab)
        1 -> listOf("Q–P" to shBand, "H / J" to shGain, "A / D" to shFreq, "K / L" to shQ, "F" to shType, "Z" to shZero, "␣" to shAb, "↵" to shTab)
        else -> listOf("Q–R" to shSlot, "H / J" to shPreamp, "A" to shAuto, "S" to shSave, "␣" to shAb, "↵" to shTab)
    }

    RegisterScreenTabs(count = 3, current = page, onSelect = { page = it })
    RegisterScreenScroll(scrollState)

    RegisterScreenKeys(
        key = listOf(page, presetCategory, slot, selectedBand, profile, active, state.eqSlots, state.audio?.eqActive),
        shortcuts = pageShortcuts,
    ) { code, repeat ->
        if (!repeat && code == KeyEvent.KEYCODE_SPACE) {
            toggleEqBypass()
            return@RegisterScreenKeys true
        }
        when (page) {
            0 -> {
                if (repeat) return@RegisterScreenKeys false
                val gridIdx = PRESET_GRID_KEYCODES.indexOf(code)
                if (gridIdx >= 0) {
                    val list = if (presetCategory == 0) KumiEqPresets.STANDARDS else KumiEqPresets.GENRES
                    list.getOrNull(gridIdx)?.let { preset ->
                        selectedBuiltInId = preset.id
                        apply(KumiEqPresets.toProfile(preset, math), true)
                        return@RegisterScreenKeys true
                    }
                }
                when (code) {
                    KeyEvent.KEYCODE_Z -> { presetCategory = 1 - presetCategory; true }
                    KeyEvent.KEYCODE_C -> { savingCustom = true; true }
                    else -> false
                }
            }
            1 -> {
                val topIdx = TOP_ROW_KEYCODES.indexOf(code)
                if (!repeat && topIdx >= 0 && topIdx < profile.bands.size) {
                    selectedBand = topIdx
                    return@RegisterScreenKeys true
                }
                val band = profile.bands.getOrNull(selectedBand) ?: return@RegisterScreenKeys false
                val updateBand: (ParametricBand) -> Unit = { changed ->
                    selectedBuiltInId = null
                    val next = profile.bands.toMutableList().also { it[selectedBand] = changed }
                    apply(profile.copy(bands = next), false)
                }
                when (code) {
                    KeyEvent.KEYCODE_H -> {
                        if (band.kind != BandKind.LowPass && band.kind != BandKind.HighPass) {
                            updateBand(band.copy(gainCentiDb = (band.gainCentiDb - 50).coerceIn(-ParametricBand.MAX_GAIN_CENTI_DB, ParametricBand.MAX_GAIN_CENTI_DB)))
                            true
                        } else false
                    }
                    KeyEvent.KEYCODE_J -> {
                        if (band.kind != BandKind.LowPass && band.kind != BandKind.HighPass) {
                            updateBand(band.copy(gainCentiDb = (band.gainCentiDb + 50).coerceIn(-ParametricBand.MAX_GAIN_CENTI_DB, ParametricBand.MAX_GAIN_CENTI_DB)))
                            true
                        } else false
                    }
                    KeyEvent.KEYCODE_A -> {
                        val nextHz = (band.frequencyHz * 0.89).roundToInt().coerceIn(ParametricBand.MIN_HZ, ParametricBand.MAX_HZ)
                        updateBand(band.copy(frequencyHz = nextHz))
                        true
                    }
                    KeyEvent.KEYCODE_D -> {
                        val nextHz = (band.frequencyHz * 1.12).roundToInt().coerceAtLeast(band.frequencyHz + 1).coerceIn(ParametricBand.MIN_HZ, ParametricBand.MAX_HZ)
                        updateBand(band.copy(frequencyHz = nextHz))
                        true
                    }
                    KeyEvent.KEYCODE_K -> {
                        val nextQ = (band.qMilli * 0.85).roundToInt().coerceIn(ParametricBand.MIN_Q_MILLI, ParametricBand.MAX_Q_MILLI)
                        updateBand(band.copy(qMilli = nextQ))
                        true
                    }
                    KeyEvent.KEYCODE_L -> {
                        val nextQ = (band.qMilli * 1.18).roundToInt().coerceAtLeast(band.qMilli + 20).coerceIn(ParametricBand.MIN_Q_MILLI, ParametricBand.MAX_Q_MILLI)
                        updateBand(band.copy(qMilli = nextQ))
                        true
                    }
                    KeyEvent.KEYCODE_F -> if (!repeat) {
                        val kinds = BandKind.entries
                        updateBand(band.copy(kind = kinds[(kinds.indexOf(band.kind) + 1) % kinds.size]))
                        true
                    } else false
                    KeyEvent.KEYCODE_Z -> if (!repeat) {
                        updateBand(band.copy(gainCentiDb = 0))
                        true
                    } else false
                    else -> false
                }
            }
            else -> {
                val slotIdx = TOP_ROW_KEYCODES.indexOf(code)
                if (!repeat && slotIdx in 0 until EqProfile.SLOTS) {
                    slot = slotIdx
                    if (state.eqSlots.getOrNull(slotIdx) != null) kumi.selectEq(slotIdx)
                    return@RegisterScreenKeys true
                }
                when (code) {
                    KeyEvent.KEYCODE_H -> {
                        apply(profile.copy(preampCentiDb = (profile.preampCentiDb - 50).coerceIn(EqProfile.MIN_PREAMP_CENTI_DB, 0)), false)
                        true
                    }
                    KeyEvent.KEYCODE_J -> {
                        apply(profile.copy(preampCentiDb = (profile.preampCentiDb + 50).coerceIn(EqProfile.MIN_PREAMP_CENTI_DB, 0)), false)
                        true
                    }
                    KeyEvent.KEYCODE_A -> if (!repeat) { apply(math.withAutoPreamp(profile), true); true } else false
                    KeyEvent.KEYCODE_S -> if (!repeat) { savingCustom = true; true } else false
                    else -> false
                }
            }
        }
    }

    Column(Modifier.fillMaxSize()) {
        InlineTabHeader(
            tabs = listOf(
                stringResource(R.string.kumi_eq_tab_presets),
                stringResource(R.string.kumi_eq_tab_bands),
                stringResource(R.string.kumi_eq_tab_slots),
            ),
            selectedIndex = page,
            onSelect = { page = it },
            onBack = onBack,
            trailing = {
                val isOn = state.audio?.eqActive != null
                Surface(
                    color = if (isOn) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                    contentColor = if (isOn) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                    shape = CircleShape,
                    modifier = Modifier.height(34.dp).clip(CircleShape).clickable(onClick = toggleEqBypass),
                ) {
                    Box(Modifier.padding(horizontal = 10.dp), contentAlignment = Alignment.Center) {
                        Text(
                            if (isOn) stringResource(R.string.kumi_eq_slot_short, (state.audio?.eqActive ?: slot) + 1) else stringResource(R.string.kumi_eq_off),
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.Bold,
                        )
                    }
                }
            },
        )
        Column(
            Modifier
                .weight(1f)
                .fillMaxWidth()
                .verticalScroll(scrollState)
                .padding(horizontal = 12.dp, vertical = 2.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            KumiErrorBanner(state, kumi::clearError)
            if (state.isUacSleeping() && state.audio?.stream == null) {
                Note(stringResource(R.string.kumi_audio_unplugged_hint))
            }
            when (page) {
                0 -> EqPresetsPage(
                    slot = slot,
                    active = active,
                    slotLoaded = state.eqSlots.getOrNull(slot) != null,
                    profile = profile,
                    math = math,
                    liveSpectrum = kumi.liveSpectrum,
                    eqBase = state.eqBase,
                    toneEq = state.toneEq,
                    customPresets = settings.customEqPresets,
                    presetCategory = presetCategory,
                    selectedBuiltInId = selectedBuiltInId,
                    onSelectCategory = { presetCategory = it },
                    onUseSlot = { kumi.selectEq(slot) },
                    onSaveCustom = { savingCustom = true },
                    onApplyBuiltIn = { preset ->
                        selectedBuiltInId = preset.id
                        apply(KumiEqPresets.toProfile(preset, math), true)
                    },
                    onApplyCustom = { custom ->
                        selectedBuiltInId = null
                        apply(custom.profile, true)
                    },
                    onDeleteCustom = { deletingPreset = it },
                )
                1 -> EqBandsPage(
                    profile = profile,
                    math = math,
                    liveSpectrum = kumi.liveSpectrum,
                    eqBase = state.eqBase,
                    toneEq = state.toneEq,
                    selectedBand = selectedBand,
                    onSelectBand = { selectedBand = it },
                    onCommitProfile = { next ->
                        selectedBuiltInId = null
                        apply(next, true)
                    },
                )
                else -> EqSlotsPage(
                    slots = state.eqSlots,
                    selectedSlot = slot,
                    activeSlot = state.audio?.eqActive,
                    profile = profile,
                    math = math,
                    liveSpectrum = kumi.liveSpectrum,
                    eqBase = state.eqBase,
                    toneEq = state.toneEq,
                    onSelectSlot = { idx ->
                        slot = idx
                        if (state.eqSlots.getOrNull(idx) != null) kumi.selectEq(idx)
                    },
                    onTurnOff = { kumi.selectEq(null) },
                    onPreampChange = { preamp -> apply(profile.copy(preampCentiDb = preamp), true) },
                    onAutoPreamp = { apply(math.withAutoPreamp(profile), true) },
                    onSaveCustom = { savingCustom = true },
                    onReset = { confirmReset = true },
                    onSetEqBase = { kumi.setEqBase(it) },
                    onSetToneEq = { kumi.setToneEq(it) },
                )
            }
        }
        KeyboardHintStrip(
            hints = when (page) {
                0 -> listOf("Q–F" to shPreset, "Z" to shGroup, "↵" to shTab, "␣" to shAb)
                1 -> listOf("Q–P" to shBand, "H/J" to shGain, "A/D" to shFreq, "↵" to shTab)
                else -> listOf("Q–R" to shSlot, "H/J" to shPreamp, "↵" to shTab, "␣" to shAb)
            },
        )
    }

    if (savingCustom) {
        SavePresetDialog(
            onDismiss = { savingCustom = false },
            onSave = { name ->
                savingCustom = false
                prefs.saveCustomEqPreset(name, profile)
            },
        )
    }
    if (confirmReset) {
        ConfirmDialog(
            title = stringResource(R.string.kumi_eq_reset_confirm_title),
            text = stringResource(R.string.kumi_eq_reset_confirm_text),
            onConfirm = {
                selectedBuiltInId = KumiEqPresets.FLAT
                kumi.editEq(slot, EqProfile.FLAT)
            },
            onDismiss = { confirmReset = false },
        )
    }
    deletingPreset?.let { preset ->
        ConfirmDialog(
            title = stringResource(R.string.kumi_eq_delete_preset_title, preset.name),
            text = stringResource(R.string.kumi_eq_delete_preset_text),
            onConfirm = { prefs.deleteCustomEqPreset(preset.name) },
            onDismiss = { deletingPreset = null },
        )
    }
}

@Composable
private fun EqPresetsPage(
    slot: Int,
    active: Boolean,
    slotLoaded: Boolean,
    profile: EqProfile,
    math: EqMath,
    liveSpectrum: StateFlow<AudioSpectrum?>,
    eqBase: EqBaseSettings?,
    toneEq: ToneEqSettings?,
    customPresets: List<CustomEqPreset>,
    presetCategory: Int,
    selectedBuiltInId: String?,
    onSelectCategory: (Int) -> Unit,
    onUseSlot: () -> Unit,
    onSaveCustom: () -> Unit,
    onApplyBuiltIn: (BuiltInEqPreset) -> Unit,
    onApplyCustom: (CustomEqPreset) -> Unit,
    onDeleteCustom: (CustomEqPreset) -> Unit,
) {
    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(114.dp)
                .padding(horizontal = 10.dp, vertical = 8.dp),
        ) {
            ResponseCurve(
                profile = profile,
                math = math,
                modifier = Modifier.fillMaxSize(),
                liveSpectrum = liveSpectrum,
                eqBase = eqBase,
                toneEq = toneEq,
            )
            Row(
                Modifier.align(Alignment.TopEnd),
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StatusPill(
                    text = stringResource(R.string.kumi_eq_slot_short, slot + 1),
                    containerColor = if (active) {
                        MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.90f)
                    } else {
                        MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.86f)
                    },
                    contentColor = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (!active && slotLoaded) {
                    Surface(
                        color = MaterialTheme.colorScheme.secondaryContainer.copy(alpha = 0.90f),
                        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                        shape = CircleShape,
                        modifier = Modifier.clip(CircleShape).clickable(onClick = onUseSlot),
                    ) {
                        Text(
                            stringResource(R.string.kumi_eq_use),
                            style = MaterialTheme.typography.labelMedium,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
                        )
                    }
                }
                Surface(
                    color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.86f),
                    contentColor = MaterialTheme.colorScheme.primary,
                    shape = CircleShape,
                    modifier = Modifier.clip(CircleShape).clickable(onClick = onSaveCustom),
                ) {
                    Row(
                        Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(5.dp),
                    ) {
                        Text(
                            stringResource(R.string.action_save),
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                        KeycapBadge("C")
                    }
                }
            }
            val layersActive = listOfNotNull(
                if (eqBase?.enabled == true && eqBase.profile.bands.isNotEmpty()) "AutoEQ" else null,
                if (toneEq?.active == true) "Tone" else null,
            ).joinToString(" + ")
            if (layersActive.isNotEmpty()) {
                Surface(
                    color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.80f),
                    shape = CircleShape,
                    modifier = Modifier.align(Alignment.BottomStart),
                ) {
                    Text(
                        layersActive,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 7.dp, vertical = 2.dp),
                    )
                }
            }
        }
    }

    if (customPresets.isNotEmpty()) {
        LazyRow(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            items(customPresets, key = { it.name }) { custom ->
                InputChip(
                    selected = profile == custom.profile,
                    onClick = { onApplyCustom(custom) },
                    label = { Text(custom.name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    trailingIcon = {
                        IconButton(onClick = { onDeleteCustom(custom) }, modifier = Modifier.size(18.dp)) {
                            Icon(
                                painterResource(R.drawable.ic_delete),
                                contentDescription = stringResource(R.string.action_remove),
                                modifier = Modifier.size(14.dp),
                            )
                        }
                    },
                )
            }
        }
    }

    CompactPageTabs(
        labels = listOf(
            stringResource(R.string.kumi_eq_cat_standards),
            stringResource(R.string.kumi_eq_cat_genres),
        ),
        selectedIndex = presetCategory,
        onSelect = onSelectCategory,
    )

    val list = if (presetCategory == 0) KumiEqPresets.STANDARDS else KumiEqPresets.GENRES
    val matchedId = KumiEqPresets.ALL.firstOrNull { it.bands == profile.bands }?.id
    val rows = remember(list) { list.chunked(2) }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        rows.forEachIndexed { rowIdx, pair ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                pair.forEachIndexed { colIdx, preset ->
                    val flatIdx = rowIdx * 2 + colIdx
                    val selected = matchedId == preset.id || (matchedId == null && selectedBuiltInId == preset.id)
                    Surface(
                        color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerLow,
                        contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurface,
                        shape = RoundedCornerShape(14.dp),
                        modifier = Modifier
                            .weight(1f)
                            .height(44.dp)
                            .clip(RoundedCornerShape(14.dp))
                            .clickable { onApplyBuiltIn(preset) },
                    ) {
                        Row(
                            Modifier.fillMaxSize().padding(horizontal = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Text(
                                stringResource(presetNameRes(preset.id)),
                                style = MaterialTheme.typography.bodyMedium,
                                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            PRESET_GRID_LABELS.getOrNull(flatIdx)?.let { KeycapBadge(it, emphasized = selected) }
                        }
                    }
                }
                if (pair.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }

    val descId = matchedId ?: selectedBuiltInId
    if (descId != null) {
        Note(stringResource(presetDescRes(descId)), modifier = Modifier.padding(horizontal = 4.dp))
    }
}

@Composable
private fun EqBandsPage(
    profile: EqProfile,
    math: EqMath,
    liveSpectrum: StateFlow<AudioSpectrum?>,
    eqBase: EqBaseSettings?,
    toneEq: ToneEqSettings?,
    selectedBand: Int,
    onSelectBand: (Int) -> Unit,
    onCommitProfile: (EqProfile) -> Unit,
) {
    var draftBands by remember { mutableStateOf(profile.bands) }
    var dragging by remember { mutableStateOf(false) }
    var lastQCommitMs by remember { mutableLongStateOf(0L) }
    LaunchedEffect(profile.bands) {
        if (!dragging) draftBands = profile.bands
    }
    val shownProfile = remember(draftBands, profile.preampCentiDb) { profile.copy(bands = draftBands) }
    val safeIndex = if (draftBands.isNotEmpty()) selectedBand.coerceIn(0, draftBands.lastIndex) else -1
    val currentBand = draftBands.getOrNull(safeIndex)

    val commitBands: (List<ParametricBand>, Boolean) -> Unit = { bands, autoPreamp ->
        val raw = profile.copy(bands = bands)
        onCommitProfile(if (autoPreamp) math.withAutoPreamp(raw) else raw)
    }

    InteractiveResponseCurve(
        profile = shownProfile,
        selectedIndex = safeIndex,
        onSelectIndex = onSelectBand,
        onDragStateChange = { dragging = it },
        onDragBands = { next -> draftBands = next },
        onCommitBands = { next, _ ->
            draftBands = next
            commitBands(next, false)
        },
        modifier = Modifier.fillMaxWidth().height(194.dp),
        liveSpectrum = liveSpectrum,
        eqBase = eqBase,
        toneEq = toneEq,
    )

    val slotCount = (draftBands.size + if (draftBands.size < EqProfile.MAX_BANDS) 1 else 0).coerceAtLeast(1)
    val rowRanges = if (slotCount > 5) listOf(0 until 5, 5 until slotCount.coerceAtMost(10)) else listOf(0 until slotCount)
    Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
        rowRanges.forEach { range ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                for (cellIdx in range) {
                    if (cellIdx < draftBands.size) {
                        val active = cellIdx == safeIndex
                        Surface(
                            color = if (active) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                            contentColor = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                            shape = RoundedCornerShape(10.dp),
                            modifier = Modifier.weight(1f).height(34.dp).clip(RoundedCornerShape(10.dp)).clickable { onSelectBand(cellIdx) },
                        ) {
                            Row(
                                Modifier.fillMaxSize().padding(horizontal = 6.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.SpaceBetween,
                            ) {
                                Text(
                                    "B${cellIdx + 1}",
                                    style = MaterialTheme.typography.labelMedium,
                                    fontWeight = if (active) FontWeight.Bold else FontWeight.Medium,
                                    maxLines = 1,
                                )
                                TOP_ROW_LABELS.getOrNull(cellIdx)?.let { KeycapBadge(it, emphasized = active) }
                            }
                        }
                    } else {
                        Surface(
                            color = MaterialTheme.colorScheme.secondaryContainer,
                            contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                            shape = RoundedCornerShape(10.dp),
                            modifier = Modifier
                                .weight(1f)
                                .height(34.dp)
                                .clip(RoundedCornerShape(10.dp))
                                .clickable {
                                    val next = draftBands + nextBand(draftBands)
                                    draftBands = next
                                    onSelectBand(next.lastIndex)
                                    commitBands(next, false)
                                },
                        ) {
                            Box(contentAlignment = Alignment.Center) {
                                Text("+ B${draftBands.size + 1}", style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.Bold, maxLines = 1)
                            }
                        }
                    }
                }
                val missing = 5 - (range.last - range.first + 1)
                repeat(missing.coerceAtLeast(0)) { Spacer(Modifier.weight(1f)) }
            }
        }
    }

    if (currentBand == null) {
        Card(
            Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        ) {
            Column(
                Modifier.fillMaxWidth().padding(14.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Note(stringResource(R.string.kumi_eq_empty_bands))
                FilledTonalButton(onClick = {
                    val next = listOf(nextBand(emptyList()))
                    draftBands = next
                    onSelectBand(0)
                    commitBands(next, false)
                }) { Text(stringResource(R.string.kumi_eq_add_band)) }
            }
        }
    } else {
        val updateSelected: (ParametricBand) -> Unit = { changed ->
            val next = draftBands.toMutableList().also { it[safeIndex] = changed }
            draftBands = next
            commitBands(next, false)
        }
        Card(
            Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        ) {
            Column(Modifier.padding(horizontal = 12.dp, vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    LazyRow(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                        items(BandKind.entries.toList()) { kind ->
                            val selectedKind = currentBand.kind == kind
                            Surface(
                                color = if (selectedKind) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                                contentColor = if (selectedKind) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                                shape = CircleShape,
                                modifier = Modifier.height(30.dp).clip(CircleShape).clickable { updateSelected(currentBand.copy(kind = kind)) },
                            ) {
                                Box(Modifier.padding(horizontal = 10.dp), contentAlignment = Alignment.Center) {
                                    Text(
                                        stringResource(bandKindName(kind)),
                                        style = MaterialTheme.typography.labelMedium,
                                        fontWeight = if (selectedKind) FontWeight.SemiBold else FontWeight.Medium,
                                    )
                                }
                            }
                        }
                    }
                    KeycapBadge("F")
                    IconButton(
                        onClick = {
                            val next = draftBands.filterIndexed { i, _ -> i != safeIndex }
                            draftBands = next
                            onSelectBand((safeIndex - 1).coerceAtLeast(0))
                            commitBands(next, false)
                        },
                        modifier = Modifier.size(30.dp),
                    ) {
                        Icon(
                            painterResource(R.drawable.ic_delete),
                            contentDescription = stringResource(R.string.action_remove),
                            modifier = Modifier.size(18.dp),
                        )
                    }
                }
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Text(
                        "Q ${"%.2f".format(currentBand.qMilli / 1000.0)}",
                        style = MaterialTheme.typography.labelLarge,
                        maxLines = 1,
                        modifier = Modifier.widthIn(min = 52.dp),
                    )
                    CompactLogSlider(
                        value = currentBand.qMilli / 1000.0,
                        min = 0.1,
                        max = 20.0,
                        onPreview = { q ->
                            dragging = true
                            val qMilli = (q * 1000).roundToInt().coerceIn(ParametricBand.MIN_Q_MILLI, ParametricBand.MAX_Q_MILLI)
                            val band = draftBands.getOrNull(safeIndex) ?: return@CompactLogSlider
                            if (band.qMilli != qMilli) {
                                val next = draftBands.toMutableList().also { it[safeIndex] = band.copy(qMilli = qMilli) }
                                draftBands = next
                                val now = SystemClock.uptimeMillis()
                                if (now - lastQCommitMs >= LIVE_DRAG_COMMIT_MS) {
                                    lastQCommitMs = now
                                    commitBands(next, false)
                                }
                            }
                        },
                        onCommit = { q ->
                            dragging = false
                            val qMilli = (q * 1000).roundToInt().coerceIn(ParametricBand.MIN_Q_MILLI, ParametricBand.MAX_Q_MILLI)
                            val band = draftBands.getOrNull(safeIndex) ?: return@CompactLogSlider
                            updateSelected(band.copy(qMilli = qMilli))
                        },
                        modifier = Modifier.weight(1f),
                    )
                    if (currentBand.kind != BandKind.LowPass && currentBand.kind != BandKind.HighPass) {
                        SmallNudgeButton("−") {
                            val nextGain = (currentBand.gainCentiDb - 50).coerceIn(-ParametricBand.MAX_GAIN_CENTI_DB, ParametricBand.MAX_GAIN_CENTI_DB)
                            updateSelected(currentBand.copy(gainCentiDb = nextGain))
                        }
                        SmallNudgeButton("+") {
                            val nextGain = (currentBand.gainCentiDb + 50).coerceIn(-ParametricBand.MAX_GAIN_CENTI_DB, ParametricBand.MAX_GAIN_CENTI_DB)
                            updateSelected(currentBand.copy(gainCentiDb = nextGain))
                        }
                        SmallNudgeButton("0") {
                            updateSelected(currentBand.copy(gainCentiDb = 0))
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SmallNudgeButton(symbol: String, onClick: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        contentColor = MaterialTheme.colorScheme.onSurface,
        shape = CircleShape,
        modifier = Modifier.size(34.dp).clip(CircleShape).clickable(onClick = onClick),
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(symbol, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.Bold)
        }
    }
}

@Composable
private fun CompactLogSlider(
    value: Double,
    min: Double,
    max: Double,
    onPreview: (Double) -> Unit,
    onCommit: (Double) -> Unit,
    modifier: Modifier = Modifier,
) {
    val span = ln(max / min)
    val outside = (ln(value.coerceIn(min, max) / min) / span).toFloat().coerceIn(0f, 1f)
    var dragging by remember { mutableStateOf(false) }
    var position by remember { mutableFloatStateOf(outside) }
    if (!dragging && position != outside) position = outside
    Slider(
        value = position,
        onValueChange = {
            dragging = true
            position = it.coerceIn(0f, 1f)
            onPreview((min * exp(position * span)).coerceIn(min, max))
        },
        onValueChangeFinished = {
            dragging = false
            onCommit((min * exp(position.coerceIn(0f, 1f) * span)).coerceIn(min, max))
        },
        modifier = modifier.height(32.dp),
    )
}

@Composable
private fun InteractiveResponseCurve(
    profile: EqProfile,
    selectedIndex: Int,
    onSelectIndex: (Int) -> Unit,
    onDragStateChange: (Boolean) -> Unit,
    onDragBands: (List<ParametricBand>) -> Unit,
    onCommitBands: (List<ParametricBand>, Boolean) -> Unit,
    modifier: Modifier = Modifier,
    liveSpectrum: StateFlow<AudioSpectrum?>? = null,
    eqBase: EqBaseSettings? = null,
    toneEq: ToneEqSettings? = null,
) {
    val primary = MaterialTheme.colorScheme.primary
    val onPrimary = MaterialTheme.colorScheme.onPrimary
    val secondary = MaterialTheme.colorScheme.secondary
    val tertiary = MaterialTheme.colorScheme.tertiary
    val secondaryContainer = MaterialTheme.colorScheme.secondaryContainer
    val onSecondaryContainer = MaterialTheme.colorScheme.onSecondaryContainer
    val grid = MaterialTheme.colorScheme.outlineVariant
    val labelStyle = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Bold)
    val textMeasurer = rememberTextMeasurer()
    val haptic = LocalHapticFeedback.current
    val spectrumVisual = rememberSmoothedSpectrum(liveSpectrum)

    val selectedBand = profile.bands.getOrNull(selectedIndex)
    val compositeDb = remember(profile) { FloatArray(FastEqCurve.POINTS).also { FastEqCurve.evaluate(profile, it) } }
    val totalDb = remember(profile, eqBase, toneEq) {
        if ((eqBase?.enabled == true && eqBase.profile.bands.isNotEmpty()) || toneEq?.active == true) {
            FloatArray(FastEqCurve.POINTS).also { FastEqCurve.evaluateComposite(profile, eqBase, toneEq, it) }
        } else {
            null
        }
    }
    val bandDb = remember(selectedBand) {
        selectedBand?.let { band -> FloatArray(FastEqCurve.POINTS).also { FastEqCurve.evaluateBand(band, it) } }
    }

    val selectedLabels = remember(labelStyle, onPrimary) {
        Array(EqProfile.MAX_BANDS) { idx ->
            textMeasurer.measure((idx + 1).toString(), style = labelStyle.copy(color = onPrimary))
        }
    }
    val normalLabels = remember(labelStyle, onSecondaryContainer) {
        Array(EqProfile.MAX_BANDS) { idx ->
            textMeasurer.measure((idx + 1).toString(), style = labelStyle.copy(color = onSecondaryContainer))
        }
    }

    val latestBands by rememberUpdatedState(profile.bands)
    val latestSelected by rememberUpdatedState(selectedIndex)
    val latestOnSelect by rememberUpdatedState(onSelectIndex)
    val latestOnDragState by rememberUpdatedState(onDragStateChange)
    val latestOnDrag by rememberUpdatedState(onDragBands)
    val latestOnCommit by rememberUpdatedState(onCommitBands)

    val path = remember { Path() }
    val fillPath = remember { Path() }
    val totalPath = remember { Path() }
    val bandPath = remember { Path() }
    val bandFillPath = remember { Path() }
    val dashEffect = remember { PathEffect.dashPathEffect(floatArrayOf(10f, 7f), 0f) }
    val curveFillBrush = remember(primary) {
        Brush.verticalGradient(
            colors = listOf(primary.copy(alpha = 0.24f), primary.copy(alpha = 0.04f), primary.copy(alpha = 0.20f)),
        )
    }

    Card(
        modifier = modifier,
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Box(Modifier.fillMaxSize()) {
            Canvas(
                Modifier
                    .fillMaxSize()
                    .padding(horizontal = 8.dp, vertical = 10.dp)
                    .pointerInput(Unit) {
                        fun hzToX(hz: Double): Float = (ln(hz.coerceIn(EQ_MIN_HZ, EQ_MAX_HZ) / EQ_MIN_HZ) / EQ_LOG_SPAN * size.width).toFloat()
                        fun dbToY(db: Double): Float {
                            val half = size.height / 2f
                            return (half - (db.coerceIn(-EQ_GAIN_RANGE_DB, EQ_GAIN_RANGE_DB) / EQ_GAIN_RANGE_DB) * half).toFloat()
                        }
                        fun xToHz(x: Float): Int {
                            val frac = (x / size.width.coerceAtLeast(1)).coerceIn(0f, 1f)
                            return (EQ_MIN_HZ * exp(frac * EQ_LOG_SPAN)).roundToInt().coerceIn(ParametricBand.MIN_HZ, ParametricBand.MAX_HZ)
                        }
                        fun yToCentiDb(y: Float): Int {
                            val half = (size.height / 2f).coerceAtLeast(1f)
                            val db = ((half - y) / half) * EQ_GAIN_RANGE_DB
                            val raw = (db * 100.0).roundToInt()
                            val snapped = ((raw + if (raw >= 0) 5 else -5) / 10) * 10
                            return snapped.coerceIn(-ParametricBand.MAX_GAIN_CENTI_DB, ParametricBand.MAX_GAIN_CENTI_DB)
                        }

                        awaitEachGesture {
                            val down = awaitFirstDown(requireUnconsumed = false)
                            val bandsNow = latestBands
                            if (bandsNow.isEmpty()) return@awaitEachGesture
                            val hitRadius = 34.dp.toPx()
                            var picked = -1
                            var bestDist = Float.MAX_VALUE
                            bandsNow.forEachIndexed { idx, band ->
                                val bx = hzToX(band.frequencyHz.toDouble())
                                val by = dbToY(if (band.kind == BandKind.LowPass || band.kind == BandKind.HighPass) 0.0 else band.gainCentiDb / 100.0)
                                val dist = hypot(down.position.x - bx, down.position.y - by)
                                if (dist < bestDist && dist <= hitRadius) {
                                    bestDist = dist
                                    picked = idx
                                }
                            }
                            if (picked < 0) {
                                picked = latestSelected.coerceIn(0, bandsNow.lastIndex)
                            }
                            if (picked != latestSelected) {
                                latestOnSelect(picked)
                                haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                            }
                            var working = bandsNow
                            var lastCommitMs = SystemClock.uptimeMillis()
                            var prevZeroSign = working[picked].gainCentiDb.let { if (it > 0) 1 else if (it < 0) -1 else 0 }
                            var moved = false

                            latestOnDragState(true)
                            try {
                                drag(down.id) { change ->
                                    change.consume()
                                    moved = true
                                    val band = working.getOrNull(picked) ?: return@drag
                                    val nextHz = xToHz(change.position.x)
                                    val nextGain = if (band.kind == BandKind.LowPass || band.kind == BandKind.HighPass) {
                                        0
                                    } else {
                                        val g = yToCentiDb(change.position.y)
                                        if (g in -30..30) 0 else g
                                    }
                                    val sign = if (nextGain > 0) 1 else if (nextGain < 0) -1 else 0
                                    if (sign == 0 && prevZeroSign != 0) {
                                        haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                                    }
                                    prevZeroSign = sign
                                    val updated = band.copy(frequencyHz = nextHz, gainCentiDb = nextGain)
                                    if (updated != band) {
                                        working = working.toMutableList().also { it[picked] = updated }
                                        latestOnDrag(working)
                                        val now = SystemClock.uptimeMillis()
                                        if (now - lastCommitMs >= LIVE_DRAG_COMMIT_MS) {
                                            lastCommitMs = now
                                            latestOnCommit(working, false)
                                        }
                                    }
                                }
                            } finally {
                                latestOnDragState(false)
                            }
                            if (moved) {
                                latestOnCommit(working, true)
                            }
                        }
                    },
            ) {
                @Suppress("UNUSED_VARIABLE")
                val rev = spectrumVisual.tick.longValue
                val halfH = size.height / 2f
                fun y(db: Double): Float = (halfH - (db.coerceIn(-EQ_GAIN_RANGE_DB, EQ_GAIN_RANGE_DB) / EQ_GAIN_RANGE_DB) * halfH).toFloat()
                fun x(hz: Double): Float = (ln(hz.coerceIn(EQ_MIN_HZ, EQ_MAX_HZ) / EQ_MIN_HZ) / EQ_LOG_SPAN * size.width).toFloat()

                for (db in EQ_GRID_DB) {
                    val yy = y(db)
                    drawLine(
                        color = if (db == 0.0) grid else grid.copy(alpha = 0.45f),
                        start = Offset(0f, yy),
                        end = Offset(size.width, yy),
                        strokeWidth = if (db == 0.0) 2.2f else 1f,
                    )
                }
                for (hz in EQ_GRID_HZ) {
                    val xx = x(hz)
                    drawLine(grid.copy(alpha = 0.45f), Offset(xx, 0f), Offset(xx, size.height), strokeWidth = 1f)
                }

                drawRtaSpectrum(spectrumVisual, tertiary, primary, minHz = EQ_MIN_HZ, logSpan = EQ_LOG_SPAN)

                val zeroY = y(0.0)
                val stepX = size.width / (FastEqCurve.POINTS - 1).toFloat()

                if (totalDb != null) {
                    totalPath.reset()
                    for (i in 0 until FastEqCurve.POINTS) {
                        val px = i * stepX
                        val py = y(totalDb[i].toDouble())
                        if (i == 0) totalPath.moveTo(px, py) else totalPath.lineTo(px, py)
                    }
                    drawPath(
                        totalPath,
                        color = secondary.copy(alpha = 0.65f),
                        style = Stroke(width = 1.6.dp.toPx(), cap = StrokeCap.Round, pathEffect = dashEffect),
                    )
                }

                if (bandDb != null) {
                    bandPath.reset()
                    bandFillPath.reset()
                    for (i in 0 until FastEqCurve.POINTS) {
                        val px = i * stepX
                        val py = y(bandDb[i].toDouble())
                        if (i == 0) {
                            bandPath.moveTo(px, py)
                            bandFillPath.moveTo(px, zeroY)
                            bandFillPath.lineTo(px, py)
                        } else {
                            bandPath.lineTo(px, py)
                            bandFillPath.lineTo(px, py)
                        }
                    }
                    bandFillPath.lineTo(size.width, zeroY)
                    bandFillPath.close()
                    drawPath(bandFillPath, color = tertiary.copy(alpha = 0.14f))
                    drawPath(bandPath, color = tertiary.copy(alpha = 0.55f), style = Stroke(width = 1.25.dp.toPx(), cap = StrokeCap.Round))
                }

                path.reset()
                fillPath.reset()
                for (i in 0 until FastEqCurve.POINTS) {
                    val px = i * stepX
                    val py = y(compositeDb[i].toDouble())
                    if (i == 0) {
                        path.moveTo(px, py)
                        fillPath.moveTo(px, zeroY)
                        fillPath.lineTo(px, py)
                    } else {
                        path.lineTo(px, py)
                        fillPath.lineTo(px, py)
                    }
                }
                fillPath.lineTo(size.width, zeroY)
                fillPath.close()
                drawPath(fillPath, brush = curveFillBrush)
                drawPath(path, primary, style = Stroke(width = 3.dp.toPx(), cap = StrokeCap.Round))

                profile.bands.forEachIndexed { idx, band ->
                    val isSelected = idx == selectedIndex
                    val cx = x(band.frequencyHz.toDouble())
                    val cy = y(if (band.kind == BandKind.LowPass || band.kind == BandKind.HighPass) 0.0 else band.gainCentiDb / 100.0)
                    val center = Offset(cx, cy)
                    if (isSelected) {
                        drawCircle(primary.copy(alpha = 0.22f), radius = 18.dp.toPx(), center = center)
                    }
                    val radius = if (isSelected) 11.dp.toPx() else 8.5.dp.toPx()
                    drawCircle(
                        color = if (isSelected) primary else secondaryContainer,
                        radius = radius,
                        center = center,
                    )
                    if (!isSelected) {
                        drawCircle(
                            color = primary.copy(alpha = 0.7f),
                            radius = radius,
                            center = center,
                            style = Stroke(width = 1.5.dp.toPx()),
                        )
                    }
                    val measured = (if (isSelected) selectedLabels else normalLabels).getOrNull(idx)
                    if (measured != null) {
                        drawText(
                            textLayoutResult = measured,
                            topLeft = Offset(cx - measured.size.width / 2f, cy - measured.size.height / 2f),
                        )
                    }
                }
            }

            if (selectedBand != null) {
                val freqStr = if (selectedBand.frequencyHz >= 1_000) {
                    "%.1f kHz".format(selectedBand.frequencyHz / 1000.0)
                } else {
                    "${selectedBand.frequencyHz} Hz"
                }
                val gainStr = if (selectedBand.kind == BandKind.LowPass || selectedBand.kind == BandKind.HighPass) {
                    stringResource(bandKindName(selectedBand.kind))
                } else {
                    centiDb(selectedBand.gainCentiDb)
                }
                val qStr = "Q ${"%.2f".format(selectedBand.qMilli / 1000.0)}"
                Surface(
                    color = MaterialTheme.colorScheme.surfaceContainerHighest.copy(alpha = 0.90f),
                    shape = CircleShape,
                    modifier = Modifier.align(Alignment.TopStart).padding(6.dp),
                ) {
                    Text(
                        "B${selectedIndex + 1} · $freqStr · $gainStr · $qStr",
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp),
                    )
                }
            }

            Row(
                Modifier.align(Alignment.BottomCenter).fillMaxWidth().padding(horizontal = 12.dp, vertical = 2.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text("20", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f))
                Text("100", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f))
                Text("1k", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f))
                Text("10k", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f))
                Text("20k", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f))
            }
        }
    }
}

@Composable
private fun EqSlotsPage(
    slots: List<EqProfile?>,
    selectedSlot: Int,
    activeSlot: Int?,
    profile: EqProfile,
    math: EqMath,
    liveSpectrum: StateFlow<AudioSpectrum?>,
    eqBase: EqBaseSettings?,
    toneEq: ToneEqSettings?,
    onSelectSlot: (Int) -> Unit,
    onTurnOff: () -> Unit,
    onPreampChange: (Int) -> Unit,
    onAutoPreamp: () -> Unit,
    onSaveCustom: () -> Unit,
    onReset: () -> Unit,
    onSetEqBase: (EqBaseSettings) -> Unit,
    onSetToneEq: (ToneEqSettings) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        listOf(0 to 1, 2 to 3).forEach { (a, b) ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(a, b).forEach { idx ->
                    val slotProfile = slots.getOrNull(idx)
                    val isSelected = idx == selectedSlot
                    val isLive = idx == activeSlot
                    val matchedPreset = slotProfile?.let { p -> KumiEqPresets.ALL.firstOrNull { it.bands == p.bands } }
                    val subtitle = when {
                        slotProfile == null || slotProfile.bands.isEmpty() -> stringResource(R.string.kumi_eq_flat_short)
                        matchedPreset != null -> stringResource(presetNameRes(matchedPreset.id))
                        else -> stringResource(R.string.kumi_eq_bands_count, slotProfile.bands.size)
                    }
                    Card(
                        onClick = { onSelectSlot(idx) },
                        modifier = Modifier.weight(1f),
                        shape = RoundedCornerShape(16.dp),
                        colors = CardDefaults.cardColors(
                            containerColor = when {
                                isLive -> MaterialTheme.colorScheme.primaryContainer
                                isSelected -> MaterialTheme.colorScheme.secondaryContainer
                                else -> MaterialTheme.colorScheme.surfaceContainerLow
                            },
                        ),
                    ) {
                        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                                Text(
                                    stringResource(R.string.kumi_eq_slot, idx + 1),
                                    style = MaterialTheme.typography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    modifier = Modifier.weight(1f),
                                )
                                TOP_ROW_LABELS.getOrNull(idx)?.let { KeycapBadge(it, emphasized = isLive || isSelected) }
                                if (isLive) {
                                    StatusPill(
                                        text = stringResource(R.string.value_on).replaceFirstChar { it.uppercase() },
                                        containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.35f),
                                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                                    )
                                }
                            }
                            Text(subtitle, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            ResponseCurve(
                                profile = slotProfile ?: EqProfile.FLAT,
                                math = math,
                                modifier = Modifier.fillMaxWidth().height(44.dp),
                                liveSpectrum = if (isLive) liveSpectrum else null,
                                eqBase = if (isLive) eqBase else null,
                                toneEq = if (isLive) toneEq else null,
                            )
                        }
                    }
                }
            }
        }
    }

    var preampDragging by remember { mutableStateOf(false) }
    var preampCurrent by remember { mutableFloatStateOf(profile.preampCentiDb.toFloat()) }
    if (!preampDragging && preampCurrent != profile.preampCentiDb.toFloat()) {
        preampCurrent = profile.preampCentiDb.toFloat()
    }
    SectionCard(
        title = stringResource(R.string.kumi_preamp),
        trailing = {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(centiDb(preampCurrent.toInt()), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                if (activeSlot != null) {
                    TextButton(onClick = onTurnOff, contentPadding = PaddingValues(horizontal = 8.dp, vertical = 0.dp)) {
                        Text(stringResource(R.string.kumi_eq_off))
                    }
                }
            }
        },
    ) {
        Slider(
            value = preampCurrent,
            onValueChange = {
                preampDragging = true
                preampCurrent = (Math.round(it / 10f) * 10).toFloat()
            },
            onValueChangeFinished = {
                preampDragging = false
                if (preampCurrent.toInt() != profile.preampCentiDb) onPreampChange(preampCurrent.toInt())
            },
            valueRange = EqProfile.MIN_PREAMP_CENTI_DB.toFloat()..0f,
            modifier = Modifier.height(34.dp),
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilledTonalButton(onClick = onAutoPreamp, modifier = Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 10.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    KeycapBadge("A")
                    Text(stringResource(R.string.kumi_preamp_auto), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            OutlinedButton(onClick = onSaveCustom, modifier = Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 10.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                    KeycapBadge("S")
                    Text(stringResource(R.string.action_save), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        OutlinedButton(onClick = onReset, modifier = Modifier.fillMaxWidth(), contentPadding = PaddingValues(horizontal = 10.dp)) {
            Text(stringResource(R.string.kumi_eq_reset), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        val peakDb = remember(profile) { FastEqCurve.peakDb(profile) }
        if (peakDb > 0.2) {
            Note(stringResource(R.string.kumi_eq_editor_hint))
        }
    }

    if (eqBase != null) {
        SectionCard(
            title = stringResource(R.string.kumi_eq_base_title),
            trailing = {
                Switch(
                    checked = eqBase.enabled,
                    onCheckedChange = { onSetEqBase(eqBase.copy(enabled = it)) },
                )
            },
        ) {
            Note(stringResource(R.string.kumi_eq_base_desc))
            if (eqBase.profile.bands.isNotEmpty()) {
                ResponseCurve(eqBase.profile, math, Modifier.fillMaxWidth().height(42.dp))
            }
            LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                item {
                    Surface(
                        color = MaterialTheme.colorScheme.secondaryContainer,
                        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                        shape = CircleShape,
                        modifier = Modifier.clip(CircleShape).clickable {
                            if (profile.bands.isNotEmpty()) {
                                onSetEqBase(EqBaseSettings(enabled = true, profile = profile))
                            }
                        },
                    ) {
                        Text(
                            stringResource(R.string.kumi_eq_base_copy_slot),
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                        )
                    }
                }
                items(KumiEqPresets.AUTOEQ_BASES, key = { it.first }) { (name, bands) ->
                    val selected = eqBase.profile.bands == bands
                    Surface(
                        color = if (selected) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
                        contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                        shape = CircleShape,
                        modifier = Modifier.clip(CircleShape).clickable {
                            onSetEqBase(EqBaseSettings(enabled = true, profile = KumiEqPresets.toProfile(bands, math)))
                        },
                    ) {
                        Text(
                            name,
                            style = MaterialTheme.typography.labelMedium,
                            fontWeight = if (selected) FontWeight.Bold else FontWeight.Medium,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                        )
                    }
                }
            }
        }
    }

    if (toneEq != null) {
        SectionCard(
            title = stringResource(R.string.kumi_eq_tone_title),
            trailing = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    if (toneEq.subBassCentiDb != 0 || toneEq.bassCentiDb != 0 || toneEq.midCentiDb != 0 || toneEq.presenceCentiDb != 0 || toneEq.trebleCentiDb != 0) {
                        TextButton(
                            onClick = { onSetToneEq(ToneEqSettings(enabled = toneEq.enabled)) },
                            contentPadding = PaddingValues(horizontal = 8.dp, vertical = 0.dp),
                        ) {
                            Text(stringResource(R.string.kumi_tone_reset))
                        }
                    }
                    Switch(
                        checked = toneEq.enabled,
                        onCheckedChange = { onSetToneEq(toneEq.copy(enabled = it)) },
                    )
                }
            },
        ) {
            Note(stringResource(R.string.kumi_eq_tone_desc))
            CentiDbSlider(
                label = "${stringResource(R.string.kumi_tone_sub)} · 60 Hz",
                value = toneEq.subBassCentiDb,
                min = -KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                max = KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                step = 50,
            ) { onSetToneEq(toneEq.copy(enabled = true, subBassCentiDb = it)) }
            CentiDbSlider(
                label = "${stringResource(R.string.kumi_tone_bass)} · 200 Hz",
                value = toneEq.bassCentiDb,
                min = -KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                max = KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                step = 50,
            ) { onSetToneEq(toneEq.copy(enabled = true, bassCentiDb = it)) }
            CentiDbSlider(
                label = "${stringResource(R.string.kumi_tone_mid)} · 1 kHz",
                value = toneEq.midCentiDb,
                min = -KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                max = KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                step = 50,
            ) { onSetToneEq(toneEq.copy(enabled = true, midCentiDb = it)) }
            CentiDbSlider(
                label = "${stringResource(R.string.kumi_tone_presence)} · 3.5 kHz",
                value = toneEq.presenceCentiDb,
                min = -KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                max = KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                step = 50,
            ) { onSetToneEq(toneEq.copy(enabled = true, presenceCentiDb = it)) }
            CentiDbSlider(
                label = "${stringResource(R.string.kumi_tone_treble)} · 10 kHz",
                value = toneEq.trebleCentiDb,
                min = -KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                max = KumiLimits.TONE_MAX_GAIN_CENTI_DB,
                step = 50,
            ) { onSetToneEq(toneEq.copy(enabled = true, trebleCentiDb = it)) }
        }
    }
}

@Composable
private fun SavePresetDialog(onDismiss: () -> Unit, onSave: (String) -> Unit) {
    SuppressScreenKeys()
    var name by remember { mutableStateOf("") }
    val trimmed = name.trim()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.kumi_eq_save_preset_title)) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { if (it.length <= 32) name = it },
                label = { Text(stringResource(R.string.kumi_eq_preset_name_label)) },
                placeholder = { Text(stringResource(R.string.kumi_eq_preset_name_hint)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(onClick = { onSave(trimmed) }, enabled = trimmed.isNotEmpty()) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

fun presetNameRes(id: String): Int = when (id) {
    KumiEqPresets.FLAT -> R.string.kumi_eq_preset_flat
    KumiEqPresets.HARMAN_OE -> R.string.kumi_eq_preset_harman_oe
    KumiEqPresets.HARMAN -> R.string.kumi_eq_preset_harman
    KumiEqPresets.STUDIO -> R.string.kumi_eq_preset_studio
    KumiEqPresets.DIFFUSE_FIELD -> R.string.kumi_eq_preset_diffuse_field
    KumiEqPresets.WARM_SMOOTH -> R.string.kumi_eq_preset_warm
    KumiEqPresets.LATE_NIGHT -> R.string.kumi_eq_preset_late_night
    KumiEqPresets.DEEP_BASS -> R.string.kumi_eq_preset_deep_bass
    KumiEqPresets.EDM -> R.string.kumi_eq_preset_edm
    KumiEqPresets.ROCK -> R.string.kumi_eq_preset_rock
    KumiEqPresets.HIPHOP -> R.string.kumi_eq_preset_hiphop
    KumiEqPresets.JAZZ -> R.string.kumi_eq_preset_jazz
    KumiEqPresets.CLASSICAL -> R.string.kumi_eq_preset_classical
    KumiEqPresets.POP -> R.string.kumi_eq_preset_pop
    KumiEqPresets.ACOUSTIC -> R.string.kumi_eq_preset_acoustic
    KumiEqPresets.VOICE -> R.string.kumi_eq_preset_voice
    else -> R.string.kumi_eq_preset_flat
}

private fun presetDescRes(id: String): Int = when (id) {
    KumiEqPresets.FLAT -> R.string.kumi_eq_preset_flat_desc
    KumiEqPresets.HARMAN_OE -> R.string.kumi_eq_preset_harman_oe_desc
    KumiEqPresets.HARMAN -> R.string.kumi_eq_preset_harman_desc
    KumiEqPresets.STUDIO -> R.string.kumi_eq_preset_studio_desc
    KumiEqPresets.DIFFUSE_FIELD -> R.string.kumi_eq_preset_diffuse_field_desc
    KumiEqPresets.WARM_SMOOTH -> R.string.kumi_eq_preset_warm_desc
    KumiEqPresets.LATE_NIGHT -> R.string.kumi_eq_preset_late_night_desc
    KumiEqPresets.DEEP_BASS -> R.string.kumi_eq_preset_deep_bass_desc
    KumiEqPresets.EDM -> R.string.kumi_eq_preset_edm_desc
    KumiEqPresets.ROCK -> R.string.kumi_eq_preset_rock_desc
    KumiEqPresets.HIPHOP -> R.string.kumi_eq_preset_hiphop_desc
    KumiEqPresets.JAZZ -> R.string.kumi_eq_preset_jazz_desc
    KumiEqPresets.CLASSICAL -> R.string.kumi_eq_preset_classical_desc
    KumiEqPresets.POP -> R.string.kumi_eq_preset_pop_desc
    KumiEqPresets.ACOUSTIC -> R.string.kumi_eq_preset_acoustic_desc
    KumiEqPresets.VOICE -> R.string.kumi_eq_preset_voice_desc
    else -> R.string.kumi_eq_preset_flat_desc
}

private fun nextBand(bands: List<ParametricBand>): ParametricBand {
    val used = bands.map { it.frequencyHz }.toSet()
    val frequency = listOf(1_000, 100, 3_000, 250, 6_000, 60, 500, 10_000, 2_000, 30).firstOrNull { it !in used } ?: 1_000
    return ParametricBand(BandKind.Peak, frequency, 1_000, 0)
}

fun bandKindName(kind: BandKind): Int = when (kind) {
    BandKind.Peak -> R.string.kumi_band_peak
    BandKind.LowShelf -> R.string.kumi_band_low_shelf
    BandKind.HighShelf -> R.string.kumi_band_high_shelf
    BandKind.LowPass -> R.string.kumi_band_low_pass
    BandKind.HighPass -> R.string.kumi_band_high_pass
}
