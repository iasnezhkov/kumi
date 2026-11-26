// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.mindone.snapcase.kumi.FakePort

@OptIn(ExperimentalCoroutinesApi::class)
class ServiceModeTest {
    @Test
    fun enteringFromTheStockRuntimeSendsTheEntryOnceAndOpensTheChannel() = runTest {
        val port = FakePort(CaseKind.STOCK)
        val mode = ServiceMode(port)
        val lease = mode.enter(1_000)
        assertEquals(1, port.entries)
        assertTrue(mode.held)
        lease.close()
        lease.close()
        assertFalse(mode.held)
        assertTrue(port.channels.single().closed)
    }

    @Test
    fun aSecondOwnerIsRefused() = runTest {
        val port = FakePort(CaseKind.SERVICE)
        val mode = ServiceMode(port)
        val lease = mode.enter(0)
        assertTrue(runCatching { mode.enter(0) }.exceptionOrNull() is ServiceBusyException)
        lease.close()
        mode.enter(0).close()
    }

    @Test
    fun aCaseThatNeverArrivesTimesOutAndReleasesTheOwner() = runTest {
        val port = FakePort(CaseKind.STOCK).apply { entryLands = null }
        val mode = ServiceMode(port)
        assertTrue(runCatching { mode.enter(10_000) }.exceptionOrNull() is ReplugException)
        assertFalse(mode.held)
        assertEquals(0, port.serviceOpens)
        assertFalse(mode.waitFor(CaseKind.KUMI, 5_000))
        port.kind = CaseKind.SERVICE
        assertNull(mode.awaitRuntime(20_000))
        port.kind = CaseKind.KUMI
        assertEquals(CaseKind.KUMI, mode.awaitRuntime(0))
    }

    @Test
    fun withoutTheEntryOnlyAnAlreadyPresentServiceDeviceIsUsed() = runTest {
        val port = FakePort(CaseKind.STOCK)
        val mode = ServiceMode(port)
        assertTrue(runCatching { mode.enter(1_000, sendEntry = false) }.exceptionOrNull() is ReplugException)
        assertEquals(0, port.entries)
    }
}
