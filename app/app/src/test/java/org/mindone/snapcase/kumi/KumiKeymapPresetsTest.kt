// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class KumiKeymapPresetsTest {
    @Test
    fun allKeymapPresetsAreValidAndUnique() {
        assertTrue(KumiKeymapPresets.ALL.size >= 5)
        val ids = KumiKeymapPresets.ALL.map { it.id }
        assertEquals(ids.size, ids.toSet().size)
        KumiKeymapPresets.ALL.forEach { preset ->
            assertTrue("preset ${preset.id} invalid", preset.valid)
            assertNotNull(KumiKeymapPresets.byId(preset.id))
            val keysPerLayer = preset.entries.map { it.layer to it.source }
            assertEquals("duplicate layer/source in ${preset.id}", keysPerLayer.size, keysPerLayer.toSet().size)
        }
    }
}
