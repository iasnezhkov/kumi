// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import android.content.Intent
import android.provider.Settings
import android.view.KeyEvent
import androidx.annotation.DrawableRes
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.mindone.snapcase.R
import org.mindone.snapcase.protocol.DacFilter
import org.mindone.snapcase.protocol.EqPreset
import org.mindone.snapcase.protocol.EqPresets
import org.mindone.snapcase.protocol.SnapProtocol
import org.mindone.snapcase.stock.StockSound
import org.mindone.snapcase.stock.StockRepository
import org.mindone.snapcase.stock.CaseLink
import org.mindone.snapcase.kumi.AppReleaseAsset
import org.mindone.snapcase.kumi.FirmwarePackage
import org.mindone.snapcase.data.AppPreferences

@Composable
fun HomeScreen(
    stock: StockRepository,
    prefs: AppPreferences,
    offer: FirmwarePackage?,
    appUpdate: AppReleaseAsset? = null,
    onOpenKeys: () -> Unit,
    onOpenIme: () -> Unit,
    onOpenShortcuts: () -> Unit,
    onOpenTester: () -> Unit,
    onOpenFirmware: () -> Unit,
    onOpenUpdates: () -> Unit = onOpenFirmware,
    showHeader: Boolean = false,
    overflowMenu: @Composable () -> Unit = {},
) {
    val link by stock.link.collectAsStateWithLifecycle()
    val connection by stock.connection.collectAsStateWithLifecycle()
    val status by stock.status.collectAsStateWithLifecycle()
    val sound by stock.sound.collectAsStateWithLifecycle()
    val settings by prefs.settings.collectAsStateWithLifecycle()
    var pending by remember { mutableStateOf<(() -> Unit)?>(null) }
    val usable = link == CaseLink.CONNECTED || link == CaseLink.DEMO
    val guarded: (() -> Unit) -> Unit = { action -> if (settings.experimentalWrites) action() else pending = action }
    val mergedOffer = offer.takeIf { link == CaseLink.NO_ACCESS }

    val shStrip = stringResource(R.string.kumi_tile_strip)
    val shShortcuts = stringResource(R.string.shortcuts_tile)
    val shFirmware = stringResource(R.string.kumi_tab_case)
    RegisterScreenKeys(
        key = Unit,
        shortcuts = listOf(
            "S" to shStrip,
            "F" to shShortcuts,
            "R" to shFirmware,
        ),
    ) { code, repeat ->
        if (repeat) return@RegisterScreenKeys false
        when (code) {
            KeyEvent.KEYCODE_S -> { onOpenIme(); true }
            KeyEvent.KEYCODE_F -> { onOpenShortcuts(); true }
            KeyEvent.KEYCODE_R -> { onOpenFirmware(); true }
            else -> false
        }
    }

    Column(Modifier.fillMaxSize()) {
        if (showHeader) {
            InlineTitleHeader(
                title = stringResource(R.string.app_title),
                trailing = overflowMenu,
            )
        }
        LazyColumn(
            Modifier.weight(1f).fillMaxWidth(),
            contentPadding = PaddingValues(start = 10.dp, end = 10.dp, top = 2.dp, bottom = 14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            appUpdate?.let { asset -> item { AppUpdateBannerCard(asset, onOpenUpdates) } }
            if (mergedOffer != null) {
                item {
                    StockUpgradeHeroCard(
                        pkg = mergedOffer,
                        onUpgrade = onOpenFirmware,
                        onDemo = { prefs.setDemoMode(true) },
                    )
                }
            } else {
                offer?.let { pkg -> item { InstallOfferCard(pkg, onOpenFirmware) { prefs.dismissOffer(pkg.version.toString()) } } }
                item { CaseCard(link, connection.version, status.presence.audioInterface, onDemo = prefs::setDemoMode, onRetry = stock::reconnect, onOpenFirmware = onOpenFirmware) }
            }
            if (usable) {
                item {
                    EqualizerCard(
                        sound = sound,
                        enabled = sound.loaded && !sound.busy,
                        onPreset = { preset -> guarded { stock.applyPreset(preset) } },
                        onGain = { band, gain -> guarded { stock.setEqGain(band, gain) } },
                    )
                }
                item {
                    SoundCard(
                        sound = sound,
                        enabled = sound.loaded && !sound.busy,
                        onFilter = { filter -> guarded { stock.chooseDacFilter(filter) } },
                        onMic = { db -> guarded { stock.chooseMicGain(db) } },
                    )
                }
            }
            item { KeyboardCard(settings.remapTable.entries.size, onOpenKeys, onOpenIme, onOpenShortcuts, onOpenTester) }
            if (mergedOffer == null && offer == null && link != CaseLink.SERVICE_MODE) {
                item {
                    Card(Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(horizontal = 12.dp, vertical = 4.dp)) {
                            NavRow(R.drawable.ic_memory, R.string.kumi_offer_title, stringResource(R.string.kumi_offer_compact_text), onOpenFirmware, keyHint = "R")
                        }
                    }
                }
            }
        }
        KeyboardHintStrip(
            hints = listOf(
                "S" to shStrip,
                "F" to shShortcuts,
                "R" to shFirmware,
            ),
        )
    }

    pending?.let { action ->
        ConfirmDialog(
            title = stringResource(R.string.home_consent_title),
            text = stringResource(R.string.home_consent_text),
            onConfirm = {
                stock.acceptWrites()
                action()
            },
            onDismiss = { pending = null },
        )
    }
}

@Composable
private fun StockUpgradeHeroCard(pkg: FirmwarePackage, onUpgrade: () -> Unit, onDemo: () -> Unit) {
    Card(
        Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(painterResource(R.drawable.ic_memory), contentDescription = null, modifier = Modifier.size(20.dp))
                Text(stringResource(R.string.kumi_offer_title), style = MaterialTheme.typography.titleMedium)
            }
            Text(
                stringResource(R.string.kumi_offer_text, pkg.version.toString()),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.88f),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onUpgrade) { Text(stringResource(R.string.kumi_upgrade_cta)) }
                TextButton(
                    onClick = onDemo,
                    colors = androidx.compose.material3.ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                    ),
                ) { Text(stringResource(R.string.action_try_demo)) }
            }
        }
    }
}

@Composable
private fun AppUpdateBannerCard(asset: AppReleaseAsset, onOpen: () -> Unit) {
    Card(
        onClick = onOpen,
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        ),
    ) {
        Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Icon(painterResource(R.drawable.ic_bolt), contentDescription = null, modifier = Modifier.size(22.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    stringResource(R.string.kumi_updates_app_available, asset.displayVersion),
                    style = MaterialTheme.typography.titleSmall,
                )
                Text(
                    listOf(
                        stringResource(R.string.kumi_updates_size_mb, asset.size / (1024.0 * 1024.0)),
                        stringResource(R.string.kumi_updates_banner_hint),
                    ).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onPrimaryContainer.copy(alpha = 0.85f),
                )
            }
            Button(onClick = onOpen) { Text(stringResource(R.string.kumi_updates_cta_short)) }
        }
    }
}

@Composable
private fun CaseCard(link: CaseLink, version: String?, audioActive: Boolean, onDemo: (Boolean) -> Unit, onRetry: () -> Unit, onOpenFirmware: () -> Unit) {
    val (icon, title, text) = when (link) {
        CaseLink.CHECKING, CaseLink.CONNECTING -> Triple(R.drawable.ic_keyboard, R.string.case_checking, R.string.case_checking_text)
        CaseLink.CONNECTED -> Triple(R.drawable.ic_check, R.string.case_connected, if (audioActive) R.string.case_connected_text else R.string.case_connected_keyboard_only)
        CaseLink.ABSENT -> Triple(R.drawable.ic_keyboard, R.string.case_absent, R.string.case_absent_text)
        CaseLink.NO_ACCESS -> Triple(R.drawable.ic_lock, R.string.case_no_access, R.string.case_no_access_text)
        CaseLink.NOT_RESPONDING -> Triple(R.drawable.ic_keyboard, R.string.case_not_responding, R.string.case_not_responding_text)
        CaseLink.SERVICE_MODE -> Triple(R.drawable.ic_service, R.string.case_service, R.string.case_service_text)
        CaseLink.DEMO -> Triple(R.drawable.ic_status, R.string.case_demo, R.string.case_demo_text)
    }
    val good = link == CaseLink.CONNECTED
    if (link == CaseLink.ABSENT) {
        Card(
            Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
        ) {
            Column(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 14.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                MindOneCaseIllustration(animated = true)
                Text(stringResource(title), style = MaterialTheme.typography.titleMedium)
                Text(
                    stringResource(text),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
                TextButton(onClick = { onDemo(true) }) {
                    Text(stringResource(R.string.action_try_demo))
                }
            }
        }
        return
    }
    Card(
        Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = if (good) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        ),
    ) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.Top) {
            Icon(painterResource(icon), contentDescription = null, modifier = Modifier.size(28.dp))
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(stringResource(title), style = MaterialTheme.typography.titleMedium)
                if (good && version != null) {
                    Text(stringResource(R.string.case_firmware, version), style = MaterialTheme.typography.labelMedium)
                }
                Text(stringResource(text), style = MaterialTheme.typography.bodyMedium)
                if (link == CaseLink.CHECKING || link == CaseLink.CONNECTING) LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 4.dp))
                when (link) {
                    CaseLink.NO_ACCESS -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Button(onClick = onOpenFirmware) { Text(stringResource(R.string.kumi_upgrade_cta)) }
                        TextButton(onClick = { onDemo(true) }) { Text(stringResource(R.string.action_try_demo)) }
                    }
                    CaseLink.ABSENT -> TextButton(onClick = { onDemo(true) }) {
                        Text(stringResource(R.string.action_try_demo))
                    }
                    CaseLink.NOT_RESPONDING -> TextButton(onClick = { onRetry() }) { Text(stringResource(R.string.action_retry)) }
                    CaseLink.SERVICE_MODE -> Button(onClick = onOpenFirmware) { Text(stringResource(R.string.kumi_open_firmware)) }
                    CaseLink.DEMO -> TextButton(onClick = { onDemo(false) }) { Text(stringResource(R.string.action_leave_demo)) }
                    else -> Unit
                }
            }
        }
    }
}

@Composable
private fun EqualizerCard(
    sound: StockSound,
    enabled: Boolean,
    onPreset: (EqPreset) -> Unit,
    onGain: (Int, Double) -> Unit,
) {
    val eq = sound.eq
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.eq_title), style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                Text(
                    eq?.let { stringResource(it.preset?.let(::presetName) ?: R.string.preset_custom) } ?: "",
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                items(EqPreset.entries.toList()) { preset ->
                    FilterChip(
                        selected = eq?.preset == preset,
                        onClick = { onPreset(preset) },
                        enabled = enabled,
                        label = { Text(stringResource(presetName(preset))) },
                    )
                }
            }
            Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
                EqPresets.FREQUENCIES.forEachIndexed { index, frequency ->
                    BandSlider(
                        label = if (frequency >= 1000) stringResource(R.string.eq_khz, frequency / 1000) else frequency.toString(),
                        gain = eq?.gains?.get(index) ?: 0.0,
                        enabled = enabled && eq != null,
                        onCommit = { onGain(index, it) },
                        modifier = Modifier.weight(1f),
                    )
                }
            }
            Text(
                stringResource(R.string.eq_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }
    }
}

@Composable
private fun BandSlider(label: String, gain: Double, enabled: Boolean, onCommit: (Double) -> Unit, modifier: Modifier = Modifier) {
    var dragging by remember { mutableStateOf(false) }
    var value by remember { mutableFloatStateOf(gain.toFloat()) }
    if (!dragging && value != gain.toFloat()) value = gain.toFloat()
    val max = EqPresets.MAX_GAIN_DB.toFloat()
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally) {
        Text(formatDb(value.toDouble()), style = MaterialTheme.typography.labelSmall, maxLines = 1)
        Box(Modifier.height(SLIDER_LENGTH).fillMaxWidth(), contentAlignment = Alignment.Center) {
            Slider(
                value = value,
                onValueChange = {
                    dragging = true
                    value = EqPresets.clampGain(it.toDouble()).toFloat()
                },
                onValueChangeFinished = {
                    dragging = false
                    onCommit(value.toDouble())
                },
                valueRange = -max..max,
                enabled = enabled,
                modifier = Modifier.vertical().width(SLIDER_LENGTH),
            )
        }
        Text(label, style = MaterialTheme.typography.labelSmall, maxLines = 1, textAlign = TextAlign.Center)
    }
}

private val SLIDER_LENGTH = 168.dp

private fun Modifier.vertical(): Modifier = this
    .graphicsLayer {
        rotationZ = 270f
        transformOrigin = TransformOrigin(0f, 0f)
    }
    .layout { measurable, constraints ->
        val placeable = measurable.measure(
            Constraints(
                minWidth = constraints.minHeight,
                maxWidth = constraints.maxHeight,
                minHeight = constraints.minWidth,
                maxHeight = constraints.maxWidth,
            ),
        )
        layout(placeable.height, placeable.width) { placeable.place(-placeable.width, 0) }
    }

private fun formatDb(value: Double): String = when {
    value > 0.01 -> "+%s".format(trimDb(value))
    value < -0.01 -> "−%s".format(trimDb(-value))
    else -> "0"
}

private fun trimDb(value: Double): String = if (value % 1.0 == 0.0) "%.0f".format(value) else "%.1f".format(value)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SoundCard(sound: StockSound, enabled: Boolean, onFilter: (DacFilter) -> Unit, onMic: (Double) -> Unit) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.sound_title), style = MaterialTheme.typography.titleMedium)
            var open by remember { mutableStateOf(false) }
            ExposedDropdownMenuBox(expanded = open && enabled, onExpandedChange = { if (enabled) open = it }) {
                OutlinedTextField(
                    value = sound.dacFilter?.let { stringResource(filterTitle(it)) } ?: "",
                    onValueChange = {},
                    readOnly = true,
                    enabled = enabled,
                    label = { Text(stringResource(R.string.sound_filter)) },
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
                    modifier = Modifier.menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, enabled).fillMaxWidth(),
                )
                ExposedDropdownMenu(expanded = open && enabled, onDismissRequest = { open = false }) {
                    DacFilter.entries.forEach { filter ->
                        DropdownMenuItem(
                            text = {
                                Column {
                                    Text(stringResource(filterTitle(filter)))
                                    Text(stringResource(filterText(filter)), style = MaterialTheme.typography.bodySmall)
                                }
                            },
                            onClick = {
                                open = false
                                if (filter != sound.dacFilter) onFilter(filter)
                            },
                        )
                    }
                }
            }
            Text(
                stringResource(R.string.sound_filter_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            HorizontalDivider()
            MicSlider(sound.micGainDb, enabled && sound.micGainDb != null, onMic)
            val rate = sound.sampleRateHz
            if (rate != null) {
                Text(
                    listOfNotNull(stringResource(R.string.sound_rate, if (rate % 1000 == 0L) (rate / 1000).toString() else "%.1f".format(rate / 1000.0)), sound.bits?.let { stringResource(R.string.sound_bits, it) })
                        .joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun MicSlider(gain: Double?, enabled: Boolean, onCommit: (Double) -> Unit) {
    val step = SnapProtocol.RECORD_GAIN_STEP_RAW / 256.0
    val min = SnapProtocol.RECORD_GAIN_MIN_DB
    val max = SnapProtocol.RECORD_GAIN_MAX_DB
    var dragging by remember { mutableStateOf(false) }
    var value by remember { mutableFloatStateOf((gain ?: 0.0).toFloat()) }
    if (!dragging && gain != null && value != gain.toFloat()) value = gain.toFloat()
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.sound_mic), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(if (gain == null) "" else stringResource(R.string.unit_db, formatDb(value.toDouble())), style = MaterialTheme.typography.labelLarge)
        }
        Slider(
            value = value,
            onValueChange = {
                dragging = true
                value = (Math.round(it / step) * step).toFloat()
            },
            onValueChangeFinished = {
                dragging = false
                onCommit(value.toDouble())
            },
            valueRange = min.toFloat()..max.toFloat(),
            steps = ((max - min) / step).toInt() - 1,
            enabled = enabled,
        )
        Text(
            stringResource(R.string.sound_mic_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun KeyboardCard(
    mappings: Int,
    onOpenKeys: () -> Unit,
    onOpenIme: () -> Unit,
    onOpenShortcuts: () -> Unit,
    onOpenTester: () -> Unit,
) {
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
            Text(
                stringResource(R.string.kb_title),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(vertical = 4.dp),
            )
            NavRow(R.drawable.ic_keyboard, R.string.ime_settings_title, stringResource(R.string.ime_settings_subtitle), onOpenIme, keyHint = "S")
            NavRow(R.drawable.ic_search, R.string.shortcuts_title, stringResource(R.string.shortcuts_subtitle), onOpenShortcuts, keyHint = "F")
            AndroidKeyboardRows(mappings, onOpenKeys)
            NavRow(R.drawable.ic_check, R.string.kb_tester_title, stringResource(R.string.kb_tester_text), onOpenTester)
        }
    }
}

@Composable
fun AndroidKeyboardRows(mappings: Int, onOpenKeys: () -> Unit, remapKeyHint: String? = null) {
    val context = LocalContext.current
    val accessibilityActive = rememberAccessibilityActive()
    NavRow(R.drawable.ic_keyboard, R.string.kb_layout, stringResource(R.string.kb_layout_text), onClick = {
        context.startActivity(Intent(Settings.ACTION_HARD_KEYBOARD_SETTINGS))
    })
    NavRow(
        R.drawable.ic_swap,
        R.string.kb_remap,
        when {
            mappings == 0 -> stringResource(R.string.kb_remap_none)
            !accessibilityActive -> stringResource(R.string.kb_remap_off, mappings)
            else -> stringResource(R.string.kb_remap_on, mappings)
        },
        onOpenKeys,
        keyHint = remapKeyHint,
    )
}

@Composable
fun NavRow(@DrawableRes icon: Int, title: Int, text: String, onClick: () -> Unit, keyHint: String? = null) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Icon(painterResource(icon), contentDescription = null, modifier = Modifier.size(21.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(stringResource(title), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                text,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (keyHint != null) {
            KeycapBadge(keyHint)
        }
    }
}

fun presetName(preset: EqPreset): Int = when (preset) {
    EqPreset.FLAT -> R.string.preset_flat
    EqPreset.ROCK -> R.string.preset_rock
    EqPreset.POP -> R.string.preset_pop
    EqPreset.JAZZ -> R.string.preset_jazz
    EqPreset.CLASSICAL -> R.string.preset_classical
    EqPreset.BASS -> R.string.preset_bass
    EqPreset.VOCAL -> R.string.preset_vocal
    EqPreset.ELECTRONIC -> R.string.preset_electronic
    EqPreset.DANCE -> R.string.preset_dance
}

fun filterTitle(filter: DacFilter): Int = when (filter) {
    DacFilter.FAST_LOW_LATENCY -> R.string.filter_fast_title
    DacFilter.FAST_PHASE_COMPENSATED -> R.string.filter_fast_phase_title
    DacFilter.SLOW_LOW_LATENCY -> R.string.filter_slow_title
    DacFilter.SLOW_PHASE_COMPENSATED -> R.string.filter_slow_phase_title
    DacFilter.NON_OVERSAMPLING -> R.string.filter_nos_title
}

fun filterText(filter: DacFilter): Int = when (filter) {
    DacFilter.FAST_LOW_LATENCY -> R.string.filter_fast_text
    DacFilter.FAST_PHASE_COMPENSATED -> R.string.filter_fast_phase_text
    DacFilter.SLOW_LOW_LATENCY -> R.string.filter_slow_text
    DacFilter.SLOW_PHASE_COMPENSATED -> R.string.filter_slow_phase_text
    DacFilter.NON_OVERSAMPLING -> R.string.filter_nos_text
}
