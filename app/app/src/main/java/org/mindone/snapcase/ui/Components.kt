// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.AudioMeters

enum class Evidence { MEASURED, UNVERIFIED, EXPERIMENTAL }

@Composable
fun CompactPageTabs(
    labels: List<String>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth().height(36.dp),
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Row(
            Modifier.padding(3.dp).fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(3.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            labels.forEachIndexed { index, label ->
                val selected = index == selectedIndex
                Surface(
                    color = if (selected) MaterialTheme.colorScheme.primaryContainer else Color.Transparent,
                    contentColor = if (selected) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                    shape = CircleShape,
                    modifier = Modifier.weight(1f).height(30.dp).clip(CircleShape).clickable { onSelect(index) },
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            label,
                            style = MaterialTheme.typography.labelLarge,
                            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.padding(horizontal = 8.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun SectionCard(
    title: String,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(7.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                trailing?.invoke()
            }
            content()
        }
    }
}

@Composable
fun ExpandableSection(
    title: String,
    subtitle: String? = null,
    initiallyExpanded: Boolean = false,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    var expanded by rememberSaveable(title) { mutableStateOf(initiallyExpanded) }
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(20.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column {
            Row(
                Modifier.fillMaxWidth().clickable { expanded = !expanded }.padding(horizontal = 14.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(title, style = MaterialTheme.typography.titleMedium)
                    if (subtitle != null && !expanded) {
                        Note(subtitle)
                    }
                }
                Text(
                    if (expanded) stringResource(R.string.action_hide) else stringResource(R.string.action_show),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            AnimatedVisibility(visible = expanded) {
                Column(
                    Modifier.padding(start = 14.dp, end = 14.dp, bottom = 14.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    HorizontalDivider()
                    content()
                }
            }
        }
    }
}

@Composable
fun TonalTile(
    @DrawableRes icon: Int,
    label: String,
    value: String,
    active: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val container = if (active) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh
    val contentColor = if (active) MaterialTheme.colorScheme.onSecondaryContainer else MaterialTheme.colorScheme.onSurface
    Card(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier.heightIn(min = 66.dp),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = container, contentColor = contentColor),
    ) {
        Column(Modifier.padding(10.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Icon(painterResource(icon), contentDescription = null, modifier = Modifier.size(18.dp))
                Text(label, style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Text(value, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
fun StatusPill(
    text: String,
    containerColor: Color = MaterialTheme.colorScheme.surfaceContainerHighest,
    contentColor: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    modifier: Modifier = Modifier,
) {
    Surface(color = containerColor, contentColor = contentColor, shape = CircleShape, modifier = modifier) {
        Text(
            text,
            style = MaterialTheme.typography.labelMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
        )
    }
}

@Composable
fun ToggleRow(
    title: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    subtitle: String? = null,
    enabled: Boolean = true,
    keyHint: String? = null,
) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = if (subtitle != null) 44.dp else 36.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        if (keyHint != null) {
            KeycapBadge(keyHint)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Text(title, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (subtitle != null) {
                Note(subtitle)
            }
        }
        Switch(checked = checked, onCheckedChange = onCheckedChange, enabled = enabled)
    }
}

@Composable
fun StereoVuMeter(meters: AudioMeters, modifier: Modifier = Modifier) {
    fun norm(centiDb: Int): Float = ((centiDb / 100f + 60f) / 60f).coerceIn(0f, 1f)
    val peakL by animateFloatAsState(if (meters.active) norm(meters.peakLeftCentiDb) else 0f, tween(110), label = "peakL")
    val peakR by animateFloatAsState(if (meters.active) norm(meters.peakRightCentiDb) else 0f, tween(110), label = "peakR")
    val rmsL by animateFloatAsState(if (meters.active) norm(meters.rmsLeftCentiDb) else 0f, tween(150), label = "rmsL")
    val rmsR by animateFloatAsState(if (meters.active) norm(meters.rmsRightCentiDb) else 0f, tween(150), label = "rmsR")
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    val primary = MaterialTheme.colorScheme.primary
    val hot = MaterialTheme.colorScheme.tertiary
    val clip = MaterialTheme.colorScheme.error

    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        VuChannelRow(stringResource(R.string.kumi_vu_left), peakL, rmsL, meters.peakLeftCentiDb, meters.active, track, primary, hot, clip)
        VuChannelRow(stringResource(R.string.kumi_vu_right), peakR, rmsR, meters.peakRightCentiDb, meters.active, track, primary, hot, clip)
    }
}

@Composable
private fun VuChannelRow(
    channel: String,
    peak: Float,
    rms: Float,
    peakCentiDb: Int,
    active: Boolean,
    track: Color,
    primary: Color,
    hot: Color,
    clip: Color,
) {
    val peakColor = when {
        peak >= 0.98f -> clip
        peak >= 0.86f -> hot
        else -> primary
    }
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(channel, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(12.dp))
        Canvas(Modifier.weight(1f).height(8.dp)) {
            val radius = CornerRadius(size.height / 2f, size.height / 2f)
            drawRoundRect(track, cornerRadius = radius)
            val peakWidth = size.width * peak
            if (peakWidth > 1f) {
                drawRoundRect(peakColor.copy(alpha = 0.35f), size = Size(peakWidth, size.height), cornerRadius = radius)
            }
            val rmsWidth = size.width * rms
            if (rmsWidth > 1f) {
                drawRoundRect(peakColor, size = Size(rmsWidth, size.height), cornerRadius = radius)
            }
            if (peakWidth > 3f) {
                drawLine(peakColor, Offset(peakWidth - 1.5f, 0f), Offset(peakWidth - 1.5f, size.height), strokeWidth = 3f)
            }
            val headMarkX = size.width * 0.9f
            drawLine(track, Offset(headMarkX, 0f), Offset(headMarkX, size.height), strokeWidth = 1.5f)
        }
        val dbText = if (active && peakCentiDb > -9000) {
            val rounded = Math.round(peakCentiDb / 100.0).toInt()
            if (rounded == 0) "0 dB" else if (rounded > 0) "+$rounded dB" else "$rounded dB"
        } else {
            "—"
        }
        Text(
            dbText,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(44.dp),
        )
    }
}

@Composable
fun VerticalStereoVuMeter(
    meters: AudioMeters?,
    streaming: Boolean,
    modifier: Modifier = Modifier,
) {
    fun norm(centiDb: Int): Float = ((centiDb / 100f + 60f) / 60f).coerceIn(0f, 1f)
    val live = meters != null && (streaming || meters.active)
    val active = live && meters.active
    val peakL by animateFloatAsState(if (active) norm(meters.peakLeftCentiDb) else 0f, tween(90), label = "vPeakL")
    val peakR by animateFloatAsState(if (active) norm(meters.peakRightCentiDb) else 0f, tween(90), label = "vPeakR")
    val rmsL by animateFloatAsState(if (active) norm(meters.rmsLeftCentiDb) else 0f, tween(130), label = "vRmsL")
    val rmsR by animateFloatAsState(if (active) norm(meters.rmsRightCentiDb) else 0f, tween(130), label = "vRmsR")
    val track = MaterialTheme.colorScheme.surfaceContainerHighest
    val primary = MaterialTheme.colorScheme.primary
    val hot = MaterialTheme.colorScheme.tertiary
    val clip = MaterialTheme.colorScheme.error
    val labelColor = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = if (live) 0.9f else 0.5f)
    val clipped = active && (meters.clippedSamples > 0 || peakL >= 0.98f || peakR >= 0.98f)

    Column(
        modifier = modifier,
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        Canvas(Modifier.weight(1f).fillMaxWidth()) {
            val gap = 4.dp.toPx()
            val barW = ((size.width - gap) / 2f).coerceAtLeast(4f)
            val radius = CornerRadius(barW / 2f, barW / 2f)
            val headY = size.height * 0.1f

            fun drawBar(x: Float, peak: Float, rms: Float) {
                val color = when {
                    peak >= 0.98f -> clip
                    peak >= 0.86f -> hot
                    else -> primary
                }
                drawRoundRect(
                    color = track,
                    topLeft = Offset(x, 0f),
                    size = Size(barW, size.height),
                    cornerRadius = radius,
                )
                val peakH = size.height * peak
                if (peakH > 1f) {
                    drawRoundRect(
                        color = color.copy(alpha = 0.35f),
                        topLeft = Offset(x, size.height - peakH),
                        size = Size(barW, peakH),
                        cornerRadius = radius,
                    )
                }
                val rmsH = size.height * rms
                if (rmsH > 1f) {
                    drawRoundRect(
                        color = color,
                        topLeft = Offset(x, size.height - rmsH),
                        size = Size(barW, rmsH),
                        cornerRadius = radius,
                    )
                }
                if (peakH > 3f) {
                    val y = size.height - peakH + 1.5f
                    drawLine(color, Offset(x, y), Offset(x + barW, y), strokeWidth = 3f)
                }
                drawLine(track, Offset(x, headY), Offset(x + barW, headY), strokeWidth = 1.5f)
                if (clipped && peak >= 0.98f) {
                    drawCircle(clip, radius = barW * 0.28f, center = Offset(x + barW / 2f, barW * 0.45f))
                }
            }

            drawBar(0f, peakL, rmsL)
            drawBar(barW + gap, peakR, rmsR)
        }
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceEvenly,
        ) {
            Text(
                stringResource(R.string.kumi_vu_left),
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 9.sp, lineHeight = 10.sp),
                fontWeight = FontWeight.Medium,
                color = labelColor,
            )
            Text(
                stringResource(R.string.kumi_vu_right),
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 9.sp, lineHeight = 10.sp),
                fontWeight = FontWeight.Medium,
                color = labelColor,
            )
        }
    }
}

@Composable
fun ChangelogDialog(
    title: String,
    subtitle: String?,
    notes: String,
    primaryLabel: String? = null,
    onPrimary: (() -> Unit)? = null,
    onDismiss: () -> Unit,
) {
    val lines = rememberChangelogLines(notes)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(title)
                if (!subtitle.isNullOrBlank()) {
                    Note(subtitle)
                }
            }
        },
        text = {
            Column(
                Modifier.fillMaxWidth().heightIn(max = 380.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                if (lines.isEmpty()) {
                    Note(stringResource(R.string.kumi_changelog_empty))
                } else {
                    lines.forEach { line ->
                        when {
                            line.startsWith("### ") || line.startsWith("## ") || line.startsWith("# ") ->
                                Text(
                                    line.trimStart('#', ' '),
                                    style = MaterialTheme.typography.titleSmall,
                                    fontWeight = FontWeight.SemiBold,
                                    modifier = Modifier.padding(top = 4.dp),
                                )
                            line.startsWith("- ") || line.startsWith("* ") ->
                                Text("• " + line.drop(2).trim(), style = MaterialTheme.typography.bodyMedium)
                            else ->
                                Text(line, style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                }
            }
        },
        confirmButton = {
            if (primaryLabel != null && onPrimary != null) {
                Button(onClick = {
                    onDismiss()
                    onPrimary()
                }) { Text(primaryLabel) }
            } else {
                TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_close)) }
            }
        },
        dismissButton = if (primaryLabel != null && onPrimary != null) {
            { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_close)) } }
        } else {
            null
        },
    )
}

private fun rememberChangelogLines(notes: String): List<String> =
    notes.replace("\r\n", "\n")
        .lines()
        .map { it.trim().replace("**", "").replace("`", "") }
        .filter { it.isNotEmpty() }

@Composable
fun InfoRow(label: String, value: String?, evidence: Evidence? = null) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.width(8.dp))
        Text(
            value ?: stringResource(R.string.value_unavailable),
            style = MaterialTheme.typography.labelLarge,
            fontWeight = FontWeight.Medium,
        )
        if (evidence != null) {
            Spacer(Modifier.width(6.dp))
            EvidenceTag(evidence)
        }
    }
}

@Composable
fun EvidenceTag(evidence: Evidence) {
    val (text, container) = when (evidence) {
        Evidence.MEASURED -> stringResource(R.string.tag_measured) to MaterialTheme.colorScheme.secondaryContainer
        Evidence.UNVERIFIED -> stringResource(R.string.tag_unverified) to MaterialTheme.colorScheme.tertiaryContainer
        Evidence.EXPERIMENTAL -> stringResource(R.string.tag_experimental) to MaterialTheme.colorScheme.errorContainer
    }
    Surface(color = container, shape = MaterialTheme.shapes.small) {
        Text(text, Modifier.padding(horizontal = 6.dp, vertical = 2.dp), style = MaterialTheme.typography.labelSmall)
    }
}

@Composable
fun Note(text: String, color: Color = MaterialTheme.colorScheme.onSurfaceVariant, modifier: Modifier = Modifier) {
    Text(text, modifier, style = MaterialTheme.typography.bodySmall, color = color)
}

@Composable
fun Mono(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
}

@Composable
fun DemoBanner() {
    Surface(color = MaterialTheme.colorScheme.tertiaryContainer, modifier = Modifier.fillMaxWidth()) {
        Text(
            stringResource(R.string.demo_banner),
            Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onTertiaryContainer,
        )
    }
}

@Composable
fun ConfirmDialog(title: String, text: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            TextButton(onClick = {
                onDismiss()
                onConfirm()
            }) { Text(stringResource(R.string.action_confirm)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}
