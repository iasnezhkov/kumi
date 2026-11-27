// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
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
import androidx.compose.foundation.selection.toggleable
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
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.DialogProperties
import org.mindone.snapcase.R
import org.mindone.snapcase.kumi.AudioMeters
import org.mindone.snapcase.kumi.ReleaseHosts

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
    compact: Boolean = false,
    trailing: (@Composable () -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
    ) {
        Column(
            Modifier.padding(horizontal = 12.dp, vertical = if (compact) 8.dp else 12.dp),
            verticalArrangement = Arrangement.spacedBy(if (compact) 4.dp else 7.dp),
        ) {
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
        Modifier
            .fillMaxWidth()
            .heightIn(min = if (subtitle != null) 44.dp else 36.dp)
            .clip(RoundedCornerShape(8.dp))
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onCheckedChange),
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
        Switch(checked = checked, onCheckedChange = null, enabled = enabled)
    }
}

@Composable
private fun rememberVuLevel(target: Float, attackMs: Int, releaseMs: Int): Float {
    val anim = remember { Animatable(target) }
    LaunchedEffect(target) {
        val ms = if (target >= anim.value) attackMs else releaseMs
        anim.animateTo(target, animationSpec = tween(durationMillis = ms, easing = LinearEasing))
    }
    return anim.value
}

@Composable
fun DismissibleErrorBanner(
    message: String,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        shape = RoundedCornerShape(14.dp),
        modifier = modifier.fillMaxWidth(),
    ) {
        Row(
            Modifier.padding(horizontal = 10.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                message,
                style = MaterialTheme.typography.labelMedium,
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(
                stringResource(R.string.action_dismiss),
                style = MaterialTheme.typography.labelMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.clickable(onClick = onDismiss).padding(horizontal = 4.dp, vertical = 2.dp),
            )
        }
    }
}

@Composable
fun StereoVuMeter(meters: AudioMeters, modifier: Modifier = Modifier) {
    fun norm(centiDb: Int): Float = ((centiDb / 100f + 60f) / 60f).coerceIn(0f, 1f)
    val peakL = rememberVuLevel(if (meters.active) norm(meters.peakLeftCentiDb) else 0f, attackMs = 18, releaseMs = 95)
    val peakR = rememberVuLevel(if (meters.active) norm(meters.peakRightCentiDb) else 0f, attackMs = 18, releaseMs = 95)
    val rmsL = rememberVuLevel(if (meters.active) norm(meters.rmsLeftCentiDb) else 0f, attackMs = 28, releaseMs = 125)
    val rmsR = rememberVuLevel(if (meters.active) norm(meters.rmsRightCentiDb) else 0f, attackMs = 28, releaseMs = 125)
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
    val peakL = rememberVuLevel(if (active) norm(meters.peakLeftCentiDb) else 0f, attackMs = 18, releaseMs = 95)
    val peakR = rememberVuLevel(if (active) norm(meters.peakRightCentiDb) else 0f, attackMs = 18, releaseMs = 95)
    val rmsL = rememberVuLevel(if (active) norm(meters.rmsLeftCentiDb) else 0f, attackMs = 28, releaseMs = 125)
    val rmsR = rememberVuLevel(if (active) norm(meters.rmsRightCentiDb) else 0f, attackMs = 28, releaseMs = 125)
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

internal sealed interface ChangelogBlock {
    data class Heading(val text: String, val level: Int) : ChangelogBlock
    data class Bullet(val text: String, val indent: Int = 0) : ChangelogBlock
    data class Quote(val text: String) : ChangelogBlock
    data class Paragraph(val text: String) : ChangelogBlock
    data object Divider : ChangelogBlock
}

private val FULL_CHANGELOG_LINE = Regex("""^\*{0,2}Full Changelog\*{0,2}:\s*(https://github\.com/\S+)\s*$""", RegexOption.IGNORE_CASE)
private val INLINE_TOKEN = Regex("""\[([^\]\n]+)\]\((https://[^\s)]+)\)|\*\*([^*\n]+)\*\*|`([^`\n]+)`|(https://[^\s)>,]+)""")

internal fun extractChangelogUrl(notes: String, fallbackUrl: String? = null): String? {
    val validFallback = fallbackUrl?.takeIf { ReleaseHosts.webUrl(it) }
    val fromBody = notes.replace("\r\n", "\n")
        .lineSequence()
        .mapNotNull { line -> FULL_CHANGELOG_LINE.matchEntire(line.trim())?.groupValues?.getOrNull(1) }
        .firstOrNull { ReleaseHosts.webUrl(it) }
    return validFallback ?: fromBody
}

internal fun parseChangelogBlocks(notes: String): List<ChangelogBlock> {
    val out = mutableListOf<ChangelogBlock>()
    for (raw in notes.replace("\r\n", "\n").lines()) {
        val leadingSpaces = raw.indexOfFirst { !it.isWhitespace() }.let { if (it < 0) 0 else it }
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) continue
        if (FULL_CHANGELOG_LINE.matches(trimmed)) continue
        when {
            trimmed == "---" || trimmed == "***" -> out += ChangelogBlock.Divider
            trimmed.startsWith("### ") -> out += ChangelogBlock.Heading(trimmed.drop(4).trim(), 3)
            trimmed.startsWith("## ") -> out += ChangelogBlock.Heading(trimmed.drop(3).trim(), 2)
            trimmed.startsWith("# ") -> out += ChangelogBlock.Heading(trimmed.drop(2).trim(), 1)
            trimmed.startsWith("- ") || trimmed.startsWith("* ") ->
                out += ChangelogBlock.Bullet(trimmed.drop(2).trim(), if (leadingSpaces >= 2) 1 else 0)
            trimmed.startsWith("> ") -> out += ChangelogBlock.Quote(trimmed.drop(2).trim())
            else -> out += ChangelogBlock.Paragraph(trimmed)
        }
    }
    return out
}

@Composable
private fun rememberFormattedChangelogText(text: String): AnnotatedString {
    val linkColor = MaterialTheme.colorScheme.primary
    val codeBg = MaterialTheme.colorScheme.surfaceContainerHighest
    return remember(text, linkColor, codeBg) {
        buildAnnotatedString {
            var cursor = 0
            for (match in INLINE_TOKEN.findAll(text)) {
                if (match.range.first > cursor) {
                    append(text.substring(cursor, match.range.first))
                }
                val mdLabel = match.groups[1]?.value
                val mdUrl = match.groups[2]?.value
                val boldText = match.groups[3]?.value
                val codeText = match.groups[4]?.value
                val bareUrl = match.groups[5]?.value
                when {
                    mdLabel != null && mdUrl != null -> {
                        if (ReleaseHosts.webUrl(mdUrl)) {
                            pushLink(
                                LinkAnnotation.Url(
                                    url = mdUrl,
                                    styles = TextLinkStyles(
                                        style = SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline),
                                    ),
                                ),
                            )
                            append(mdLabel)
                            pop()
                        } else {
                            append(mdLabel)
                        }
                    }
                    boldText != null -> {
                        pushStyle(SpanStyle(fontWeight = FontWeight.SemiBold))
                        append(boldText)
                        pop()
                    }
                    codeText != null -> {
                        pushStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = codeBg))
                        append(" $codeText ")
                        pop()
                    }
                    bareUrl != null -> {
                        val cleanUrl = bareUrl.trimEnd('.', ',', ';')
                        val suffix = bareUrl.removePrefix(cleanUrl)
                        if (ReleaseHosts.webUrl(cleanUrl)) {
                            pushLink(
                                LinkAnnotation.Url(
                                    url = cleanUrl,
                                    styles = TextLinkStyles(
                                        style = SpanStyle(color = linkColor, textDecoration = TextDecoration.Underline),
                                    ),
                                ),
                            )
                            append(cleanUrl.removePrefix("https://"))
                            pop()
                        } else {
                            append(cleanUrl)
                        }
                        if (suffix.isNotEmpty()) append(suffix)
                    }
                }
                cursor = match.range.last + 1
            }
            if (cursor < text.length) {
                append(text.substring(cursor))
            }
        }
    }
}

@Composable
fun ChangelogDialog(
    title: String,
    subtitle: String?,
    notes: String,
    releaseUrl: String? = null,
    primaryLabel: String? = null,
    onPrimary: (() -> Unit)? = null,
    onDismiss: () -> Unit,
) {
    val uriHandler = LocalUriHandler.current
    val blocks = remember(notes) { parseChangelogBlocks(notes) }
    val linkUrl = remember(notes, releaseUrl) { extractChangelogUrl(notes, releaseUrl) }
    AlertDialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
        title = {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                if (!subtitle.isNullOrBlank()) {
                    Note(subtitle)
                }
            }
        },
        text = {
            Column(
                Modifier.fillMaxWidth().heightIn(max = 420.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                if (blocks.isEmpty()) {
                    Note(stringResource(R.string.kumi_changelog_empty))
                } else {
                    blocks.forEach { block ->
                        when (block) {
                            is ChangelogBlock.Heading -> {
                                Text(
                                    rememberFormattedChangelogText(block.text),
                                    style = if (block.level <= 2) MaterialTheme.typography.titleSmall else MaterialTheme.typography.labelLarge,
                                    fontWeight = FontWeight.SemiBold,
                                    color = if (block.level <= 2) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                                    modifier = Modifier.padding(top = if (block.level <= 2) 6.dp else 3.dp),
                                )
                            }
                            is ChangelogBlock.Bullet -> {
                                Row(
                                    Modifier.fillMaxWidth().padding(start = (block.indent * 12).dp),
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                    verticalAlignment = Alignment.Top,
                                ) {
                                    Text(
                                        if (block.indent == 0) "•" else "◦",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.primary,
                                    )
                                    Text(
                                        rememberFormattedChangelogText(block.text),
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurface,
                                        modifier = Modifier.weight(1f),
                                    )
                                }
                            }
                            is ChangelogBlock.Quote -> {
                                Text(
                                    rememberFormattedChangelogText(block.text),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    modifier = Modifier.padding(top = 4.dp),
                                )
                            }
                            is ChangelogBlock.Paragraph -> {
                                Text(
                                    rememberFormattedChangelogText(block.text),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurface,
                                )
                            }
                            ChangelogBlock.Divider -> {
                                HorizontalDivider(Modifier.padding(vertical = 2.dp))
                            }
                        }
                    }
                }
                if (linkUrl != null) {
                    TextButton(
                        onClick = { runCatching { uriHandler.openUri(linkUrl) } },
                        modifier = Modifier.padding(top = 2.dp),
                    ) {
                        Text(stringResource(R.string.kumi_updates_open_release))
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
