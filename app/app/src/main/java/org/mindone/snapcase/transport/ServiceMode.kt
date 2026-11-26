// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.delay
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.withTimeoutOrNull
import org.mindone.snapcase.kumi.KumiLink

class ReplugException(message: String) : IllegalStateException(message)

class ServiceBusyException : IllegalStateException("Service mode is already owned by another operation")

class ServiceMode(private val port: CasePort, private val pollMs: Long = POLL_MS) {
    private val owner = Mutex()

    val held: Boolean get() = owner.isLocked

    inner class Lease internal constructor(private val channel: CaseChannel) : KumiLink by channel, AutoCloseable {
        private val open = AtomicBoolean(true)

        val isOpen: Boolean get() = open.get()

        override fun close() {
            if (!open.compareAndSet(true, false)) return
            runCatching { channel.close() }
            owner.unlock()
        }
    }

    suspend fun enter(timeoutMs: Long, sendEntry: Boolean = true): Lease {
        if (!owner.tryLock()) throw ServiceBusyException()
        try {
            if (sendEntry && port.kind() == CaseKind.STOCK) port.sendServiceEntry()
            if (!waitFor(CaseKind.SERVICE, timeoutMs)) {
                throw ReplugException("The case did not enter service mode within ${timeoutMs / 1000} s")
            }
            return Lease(port.openService())
        } catch (error: Throwable) {
            owner.unlock()
            throw error
        }
    }

    suspend fun waitFor(kind: CaseKind, timeoutMs: Long): Boolean = poll(timeoutMs) { port.kind() == kind }

    suspend fun waitWhile(kind: CaseKind, timeoutMs: Long): Boolean = poll(timeoutMs) { port.kind() != kind }

    suspend fun awaitRuntime(timeoutMs: Long): CaseKind? {
        var seen: CaseKind? = null
        poll(timeoutMs) { port.kind().also { seen = it }.let { it == CaseKind.STOCK || it == CaseKind.KUMI } }
        return seen?.takeIf { it == CaseKind.STOCK || it == CaseKind.KUMI }
    }

    private suspend fun poll(timeoutMs: Long, done: () -> Boolean): Boolean {
        if (done()) return true
        return withTimeoutOrNull(timeoutMs) {
            while (!done()) delay(pollMs)
            true
        } ?: false
    }

    companion object {
        const val POLL_MS = 250L
    }
}
