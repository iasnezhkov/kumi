// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.transport

import java.io.Closeable
import org.mindone.snapcase.kumi.KumiLink

interface CaseChannel : KumiLink, Closeable

interface CasePort {
    val simulated: Boolean

    val simulator: Long? get() = null

    fun kind(): CaseKind

    fun deviceKey(): String? = null

    suspend fun sendServiceEntry()

    suspend fun openService(): CaseChannel

    suspend fun openKumi(): CaseChannel
}

class PermissionDeniedException(device: String) : SecurityException("USB permission denied for $device")

class SwitchableCasePort(@Volatile var target: CasePort) : CasePort {
    override val simulated: Boolean get() = target.simulated
    override val simulator: Long? get() = target.simulator
    override fun kind(): CaseKind = target.kind()
    override fun deviceKey(): String? = target.deviceKey()
    override suspend fun sendServiceEntry() = target.sendServiceEntry()
    override suspend fun openService(): CaseChannel = target.openService()
    override suspend fun openKumi(): CaseChannel = target.openKumi()
}
