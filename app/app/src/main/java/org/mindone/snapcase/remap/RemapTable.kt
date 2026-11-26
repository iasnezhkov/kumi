// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.remap

object Keys {
    const val UNKNOWN = 0
    const val ALT_LEFT = 57
    const val ALT_RIGHT = 58
    const val SHIFT_LEFT = 59
    const val SHIFT_RIGHT = 60
    const val CTRL_LEFT = 113
    const val CTRL_RIGHT = 114
    const val CAPS_LOCK = 115
    const val META_LEFT = 117
    const val META_RIGHT = 118
    const val FUNCTION = 119
    const val MAX_KEYCODE = 400

    const val META_SHIFT_ON = 0x1
    const val META_ALT_ON = 0x2
    const val META_FUNCTION_ON = 0x8
    const val META_ALT_LEFT_ON = 0x10
    const val META_ALT_RIGHT_ON = 0x20
    const val META_SHIFT_LEFT_ON = 0x40
    const val META_SHIFT_RIGHT_ON = 0x80
    const val META_CTRL_ON = 0x1000
    const val META_CTRL_LEFT_ON = 0x2000
    const val META_CTRL_RIGHT_ON = 0x4000
    const val META_META_ON = 0x10000
    const val META_META_LEFT_ON = 0x20000
    const val META_META_RIGHT_ON = 0x40000

    val MODIFIER_META = mapOf(
        ALT_LEFT to (META_ALT_ON or META_ALT_LEFT_ON),
        ALT_RIGHT to (META_ALT_ON or META_ALT_RIGHT_ON),
        SHIFT_LEFT to (META_SHIFT_ON or META_SHIFT_LEFT_ON),
        SHIFT_RIGHT to (META_SHIFT_ON or META_SHIFT_RIGHT_ON),
        CTRL_LEFT to (META_CTRL_ON or META_CTRL_LEFT_ON),
        CTRL_RIGHT to (META_CTRL_ON or META_CTRL_RIGHT_ON),
        META_LEFT to (META_META_ON or META_META_LEFT_ON),
        META_RIGHT to (META_META_ON or META_META_RIGHT_ON),
        FUNCTION to META_FUNCTION_ON,
    )

    fun isModifier(keyCode: Int) = keyCode in MODIFIER_META
}

class RemapTable private constructor(val entries: Map<Int, Int>) {

    fun target(keyCode: Int): Int? = entries[keyCode]

    fun with(from: Int, to: Int): RemapTable {
        validate(from, to)
        return RemapTable(entries + (from to to))
    }

    fun without(from: Int): RemapTable = RemapTable(entries - from)

    fun serialize(): String = entries.toSortedMap().entries.joinToString(",") { "${it.key}:${it.value}" }

    override fun equals(other: Any?) = other is RemapTable && other.entries == entries

    override fun hashCode() = entries.hashCode()

    override fun toString() = "RemapTable(${serialize()})"

    companion object {
        val EMPTY = RemapTable(emptyMap())

        fun of(vararg pairs: Pair<Int, Int>): RemapTable = pairs.fold(EMPTY) { table, (from, to) -> table.with(from, to) }

        fun parse(text: String?): RemapTable {
            if (text.isNullOrBlank()) return EMPTY
            val result = LinkedHashMap<Int, Int>()
            for (item in text.split(',')) {
                val parts = item.trim().split(':')
                if (parts.size != 2) throw IllegalArgumentException("Bad remap entry '$item'")
                val from = parts[0].trim().toInt()
                val to = parts[1].trim().toInt()
                validate(from, to)
                if (from in result) throw IllegalArgumentException("Duplicate source key $from")
                result[from] = to
            }
            return RemapTable(result)
        }

        fun parseOrEmpty(text: String?): RemapTable = runCatching { parse(text) }.getOrDefault(EMPTY)

        private fun validate(from: Int, to: Int) {
            require(from in 1..Keys.MAX_KEYCODE) { "Source key $from out of range" }
            require(to in 0..Keys.MAX_KEYCODE) { "Target key $to out of range" }
            require(from != to) { "Key $from maps to itself" }
        }
    }
}

data class KeyInput(
    val keyCode: Int,
    val down: Boolean,
    val metaState: Int,
    val repeatCount: Int = 0,
    val fromTargetDevice: Boolean = true,
)

data class KeyOutput(val keyCode: Int, val down: Boolean, val metaState: Int, val repeatCount: Int)

sealed interface RemapDecision {
    data object PassThrough : RemapDecision
    data class Replace(val events: List<KeyOutput>) : RemapDecision
}

class RemapEngine(var table: RemapTable = RemapTable.EMPTY) {
    private val heldVirtual = HashMap<Int, Int>()
    private val pressedSources = HashMap<Int, Int>()
    private val rewrittenSources = HashSet<Int>()

    val virtualMeta: Int get() = heldVirtual.values.fold(0) { acc, meta -> acc or meta }

    fun reset() {
        heldVirtual.clear()
        pressedSources.clear()
        rewrittenSources.clear()
    }

    private val stripMask: Int
        get() = pressedSources.keys.fold(0) { acc, key -> acc or (Keys.MODIFIER_META[key] ?: 0) }

    private fun outputMeta(input: KeyInput): Int = (input.metaState and stripMask.inv()) or virtualMeta

    fun process(input: KeyInput): RemapDecision {
        if (!input.fromTargetDevice) return RemapDecision.PassThrough
        val target = if (input.down) {
            pressedSources[input.keyCode] ?: table.target(input.keyCode)?.also { pressedSources[input.keyCode] = it }
        } else {
            pressedSources.remove(input.keyCode)
        }
        if (target == null) {
            if (input.down) {
                if (input.keyCode !in rewrittenSources) {
                    if (input.repeatCount != 0 || (stripMask == 0 && virtualMeta == 0)) {
                        return RemapDecision.PassThrough
                    }
                    rewrittenSources += input.keyCode
                }
            } else if (!rewrittenSources.remove(input.keyCode)) {
                return RemapDecision.PassThrough
            }
            return RemapDecision.Replace(
                listOf(KeyOutput(input.keyCode, input.down, outputMeta(input), input.repeatCount)),
            )
        }
        val modifierMeta = Keys.MODIFIER_META[target]
        if (modifierMeta != null) {
            if (input.down) heldVirtual[input.keyCode] = modifierMeta else heldVirtual.remove(input.keyCode)
        }
        if (target == Keys.UNKNOWN) return RemapDecision.Replace(emptyList())
        return RemapDecision.Replace(listOf(KeyOutput(target, input.down, outputMeta(input), input.repeatCount)))
    }
}
