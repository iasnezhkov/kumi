// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.protocol

import java.io.Closeable
import java.io.IOException

open class CaseException(message: String, cause: Throwable? = null) : IOException(message, cause)

class CaseTimeoutException(message: String) : CaseException(message)

class CaseAsleepException(cause: Throwable) : CaseException("Case did not answer the version probe", cause)

class WritesDisabledException : CaseException("Experimental writes are disabled")

interface Transport : Closeable {
    val description: String
    val isDemo: Boolean

    fun exchange(request: ByteArray, timeoutMs: Long = DEFAULT_TIMEOUT_MS): ByteArray

    fun sendOnly(request: ByteArray) {
        throw CaseException("$description cannot send without a reply")
    }

    override fun close() {}

    companion object {
        const val DEFAULT_TIMEOUT_MS = 1000L
    }
}
