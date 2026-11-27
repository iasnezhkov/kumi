// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ui

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp

private data class BlueprintKey(val x: Float, val y: Float, val w: Float, val rx: Float = 7f)

private val SNAP_SPEAKER_XS = floatArrayOf(
    123f, 139f, 155.5f, 172f, 188.5f,
    407f, 423f, 439.5f, 455.5f, 472f, 488.5f, 504.5f,
)

private val SNAP_ROW_XS = floatArrayOf(31f, 86f, 140f, 195f, 251f, 305f, 360f, 415f, 470f, 525f)

private val SNAP_KEYS: List<BlueprintKey> = buildList {
    SNAP_ROW_XS.forEach { add(BlueprintKey(it, 692f, 47f)) }
    SNAP_ROW_XS.forEach { add(BlueprintKey(it, 748f, 47f)) }
    add(BlueprintKey(31f, 805f, 75f))
    floatArrayOf(113f, 168f, 223f, 278f, 333f, 388f, 442f).forEach { add(BlueprintKey(it, 805f, 47f)) }
    add(BlueprintKey(498f, 805f, 74f))
    add(BlueprintKey(31f, 862f, 75f, rx = 20f))
    add(BlueprintKey(113f, 862f, 47f))
    add(BlueprintKey(168f, 862f, 48f))
    add(BlueprintKey(223f, 862f, 157f))
    add(BlueprintKey(388f, 862f, 47f))
    add(BlueprintKey(442f, 862f, 48f))
    add(BlueprintKey(498f, 862f, 74f, rx = 20f))
}

@Composable
fun MindOneCaseIllustration(
    modifier: Modifier = Modifier,
    animated: Boolean = true,
) {
    val accent = MaterialTheme.colorScheme.primary
    val ink = MaterialTheme.colorScheme.onSurface
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val outline = MaterialTheme.colorScheme.outline
    val surfaceRaised = MaterialTheme.colorScheme.surfaceContainerHigh
    val surfaceDeep = MaterialTheme.colorScheme.surfaceContainerLowest
    val keyFill = MaterialTheme.colorScheme.surfaceContainer

    val progress = if (animated) {
        val transition = rememberInfiniteTransition(label = "mindOneDock")
        val value by transition.animateFloat(
            initialValue = 0f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(
                animation = tween(durationMillis = 2200, easing = FastOutSlowInEasing),
                repeatMode = RepeatMode.Reverse,
            ),
            label = "dockProgress",
        )
        value
    } else {
        1f
    }

    Canvas(
        modifier = modifier
            .fillMaxWidth()
            .height(172.dp),
    ) {
        val travel = if (animated) 115f else 0f
        val totalDesignH = 975f + travel
        val s = (size.height - 10.dp.toPx()) / totalDesignH
        val ox = (size.width - 600f * s) * 0.5f
        val oy = 5.dp.toPx() + travel * s

        fun px(x: Float) = ox + x * s
        fun py(y: Float) = oy + y * s

        val datumY = py(958f)
        drawLine(
            color = outline.copy(alpha = 0.25f),
            start = Offset(ox - 54f * s, datumY),
            end = Offset(ox + 654f * s, datumY),
            strokeWidth = 1.dp.toPx(),
        )

        drawRoundRect(
            color = surfaceRaised,
            topLeft = Offset(px(2f), py(2f)),
            size = Size(596f * s, 945f * s),
            cornerRadius = CornerRadius(28f * s, 28f * s),
        )
        drawRoundRect(
            color = ink.copy(alpha = 0.72f),
            topLeft = Offset(px(2f), py(2f)),
            size = Size(596f * s, 945f * s),
            cornerRadius = CornerRadius(28f * s, 28f * s),
            style = Stroke(width = (4.5f * s).coerceAtLeast(1.25.dp.toPx())),
        )

        drawRoundRect(
            color = surfaceDeep,
            topLeft = Offset(px(21f), py(15f)),
            size = Size(558f * s, 627f * s),
            cornerRadius = CornerRadius(14f * s, 14f * s),
        )
        drawRoundRect(
            color = ink.copy(alpha = 0.42f),
            topLeft = Offset(px(21f), py(15f)),
            size = Size(558f * s, 627f * s),
            cornerRadius = CornerRadius(14f * s, 14f * s),
            style = Stroke(width = (2.6f * s).coerceAtLeast(0.85.dp.toPx())),
        )

        val dashBay = PathEffect.dashPathEffect(floatArrayOf(12f * s, 12f * s), 0f)
        drawRoundRect(
            color = outline.copy(alpha = 0.35f),
            topLeft = Offset(px(33f), py(27f)),
            size = Size(534f * s, 603f * s),
            cornerRadius = CornerRadius(9f * s, 9f * s),
            style = Stroke(width = (1.8f * s).coerceAtLeast(0.7.dp.toPx()), pathEffect = dashBay),
        )

        val plugAlpha = 0.55f + 0.45f * progress
        val dashPlug = PathEffect.dashPathEffect(floatArrayOf(10f * s, 7f * s), 0f)
        drawRoundRect(
            color = surfaceRaised,
            topLeft = Offset(px(266f), py(602f)),
            size = Size(68f * s, 46f * s),
            cornerRadius = CornerRadius(8f * s, 8f * s),
        )
        drawRoundRect(
            color = accent.copy(alpha = plugAlpha),
            topLeft = Offset(px(266f), py(602f)),
            size = Size(68f * s, 46f * s),
            cornerRadius = CornerRadius(8f * s, 8f * s),
            style = Stroke(
                width = (4f * s).coerceAtLeast(1.1.dp.toPx()),
                pathEffect = if (progress > 0.9f) null else dashPlug,
            ),
        )
        drawLine(
            color = accent.copy(alpha = plugAlpha),
            start = Offset(px(282f), py(618f)),
            end = Offset(px(318f), py(618f)),
            strokeWidth = (3f * s).coerceAtLeast(0.9.dp.toPx()),
            cap = StrokeCap.Round,
        )
        drawLine(
            color = accent.copy(alpha = plugAlpha),
            start = Offset(px(282f), py(631f)),
            end = Offset(px(318f), py(631f)),
            strokeWidth = (3f * s).coerceAtLeast(0.9.dp.toPx()),
            cap = StrokeCap.Round,
        )

        val slideLift = travel * (1f - progress)
        val phoneY = 27f - slideLift
        val phoneTop = py(phoneY)
        val phoneLeft = px(33f)
        val phoneW = 534f * s
        val phoneH = 603f * s

        drawRoundRect(
            color = surfaceRaised,
            topLeft = Offset(phoneLeft, phoneTop),
            size = Size(phoneW, phoneH),
            cornerRadius = CornerRadius(14f * s, 14f * s),
        )
        drawRoundRect(
            color = accent.copy(alpha = 0.08f + 0.14f * progress),
            topLeft = Offset(phoneLeft + 14f * s, phoneTop + 20f * s),
            size = Size(phoneW - 28f * s, phoneH - 42f * s),
            cornerRadius = CornerRadius(8f * s, 8f * s),
        )
        drawRoundRect(
            color = ink.copy(alpha = 0.78f),
            topLeft = Offset(phoneLeft, phoneTop),
            size = Size(phoneW, phoneH),
            cornerRadius = CornerRadius(14f * s, 14f * s),
            style = Stroke(width = (3.4f * s).coerceAtLeast(1.1.dp.toPx())),
        )
        drawLine(
            color = muted.copy(alpha = 0.55f),
            start = Offset(px(262f), phoneTop + 10f * s),
            end = Offset(px(338f), phoneTop + 10f * s),
            strokeWidth = (2.6f * s).coerceAtLeast(0.9.dp.toPx()),
            cap = StrokeCap.Round,
        )

        val screenCx = px(300f)
        val screenCy = phoneTop + phoneH * 0.5f
        val dashCross = PathEffect.dashPathEffect(floatArrayOf(10f * s, 12f * s), 0f)
        drawLine(
            color = outline.copy(alpha = 0.25f),
            start = Offset(phoneLeft + 24f * s, screenCy),
            end = Offset(phoneLeft + phoneW - 24f * s, screenCy),
            strokeWidth = (1.6f * s).coerceAtLeast(0.6.dp.toPx()),
            pathEffect = dashCross,
        )
        drawLine(
            color = outline.copy(alpha = 0.25f),
            start = Offset(screenCx, phoneTop + 28f * s),
            end = Offset(screenCx, phoneTop + phoneH - 28f * s),
            strokeWidth = (1.6f * s).coerceAtLeast(0.6.dp.toPx()),
            pathEffect = dashCross,
        )
        drawCircle(
            color = accent.copy(alpha = 0.35f + 0.45f * progress),
            radius = 36f * s,
            center = Offset(screenCx, screenCy),
            style = Stroke(width = (2.2f * s).coerceAtLeast(0.8.dp.toPx())),
        )

        if (slideLift > 22f) {
            val guideAlpha = ((slideLift - 22f) / 90f).coerceIn(0f, 1f) * 0.72f
            val tipY = py(602f - 10f)
            val tailY = (phoneTop + phoneH + 8f * s).coerceAtMost(tipY - 12f * s)
            if (tipY > tailY + 4f * s) {
                drawLine(
                    color = accent.copy(alpha = guideAlpha),
                    start = Offset(px(300f), tailY),
                    end = Offset(px(300f), tipY),
                    strokeWidth = (2.4f * s).coerceAtLeast(0.9.dp.toPx()),
                    cap = StrokeCap.Round,
                )
            }
        }

        val dotRadius = (3.8f * s).coerceAtLeast(0.75.dp.toPx())
        for (x in SNAP_SPEAKER_XS) {
            drawCircle(
                color = muted.copy(alpha = 0.6f),
                radius = dotRadius,
                center = Offset(px(x), py(663.5f)),
            )
        }

        val keyStroke = Stroke(width = (2.2f * s).coerceAtLeast(0.75.dp.toPx()))
        for (k in SNAP_KEYS) {
            val isSpace = k.w > 120f
            drawRoundRect(
                color = if (isSpace) accent.copy(alpha = 0.18f + 0.12f * progress) else keyFill,
                topLeft = Offset(px(k.x), py(k.y)),
                size = Size(k.w * s, 47f * s),
                cornerRadius = CornerRadius(k.rx * s, k.rx * s),
            )
            drawRoundRect(
                color = if (isSpace) accent.copy(alpha = 0.65f) else ink.copy(alpha = 0.45f),
                topLeft = Offset(px(k.x), py(k.y)),
                size = Size(k.w * s, 47f * s),
                cornerRadius = CornerRadius(k.rx * s, k.rx * s),
                style = keyStroke,
            )
        }

        val dashPort = PathEffect.dashPathEffect(floatArrayOf(9f * s, 6f * s), 0f)
        drawRoundRect(
            color = surfaceRaised,
            topLeft = Offset(px(262f), py(939f)),
            size = Size(76f * s, 16f * s),
            cornerRadius = CornerRadius(5f * s, 5f * s),
        )
        drawRoundRect(
            color = muted.copy(alpha = 0.65f),
            topLeft = Offset(px(262f), py(939f)),
            size = Size(76f * s, 16f * s),
            cornerRadius = CornerRadius(5f * s, 5f * s),
            style = Stroke(width = (3f * s).coerceAtLeast(0.9.dp.toPx()), pathEffect = dashPort),
        )

        drawCircle(
            color = surfaceRaised,
            radius = 14f * s,
            center = Offset(px(494f), py(947f)),
        )
        drawCircle(
            color = accent.copy(alpha = 0.85f),
            radius = 14f * s,
            center = Offset(px(494f), py(947f)),
            style = Stroke(width = (3.6f * s).coerceAtLeast(1.1.dp.toPx())),
        )
        drawCircle(
            color = accent,
            radius = 5f * s,
            center = Offset(px(494f), py(947f)),
        )
    }
}
