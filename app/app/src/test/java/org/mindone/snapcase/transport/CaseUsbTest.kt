// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CaseUsbTest {
    @Test
    fun everyCaseModeHasOneUsbId() {
        assertEquals(CaseKind.STOCK, CaseUsb.kindOf(0x0661, 0x0881))
        assertEquals(CaseKind.SERVICE, CaseUsb.kindOf(0x0666, 0x0888))
        assertEquals(CaseKind.KUMI, CaseUsb.kindOf(0x1209, 0x0001))
        assertEquals(CaseKind.NONE, CaseUsb.kindOf(0x1209, 0x0002))
        assertEquals(listOf(CaseUsb.KUMI, CaseUsb.SERVICE, CaseUsb.STOCK), listOf(CaseKind.KUMI, CaseKind.SERVICE, CaseKind.STOCK).map(CaseUsb::id))
    }

    @Test
    fun bothRuntimesAreTheCaseKeyboardButServiceModeIsNot() {
        assertTrue(CaseUsb.isKeyboard(0x0661, 0x0881))
        assertTrue(CaseUsb.isKeyboard(0x1209, 0x0001))
        assertFalse(CaseUsb.isKeyboard(0x0666, 0x0888))
        assertFalse(CaseUsb.isKeyboard(0x046d, 0xc52b))
    }

    @Test
    fun theMostSpecificModeWinsWhileTheCaseReenumerates() {
        assertEquals(CaseKind.KUMI, CaseUsb.strongest(listOf(CaseKind.STOCK, CaseKind.KUMI)))
        assertEquals(CaseKind.SERVICE, CaseUsb.strongest(listOf(CaseKind.STOCK, CaseKind.SERVICE, CaseKind.NONE)))
        assertEquals(CaseKind.NONE, CaseUsb.strongest(emptyList()))
    }
}
