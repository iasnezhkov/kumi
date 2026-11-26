// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

import java.util.concurrent.atomic.AtomicBoolean
import org.mindone.snapcase.transport.CaseChannel
import org.mindone.snapcase.transport.CaseKind
import org.mindone.snapcase.transport.CasePort

interface KumiLink {
    fun send(data: ByteArray): Boolean
    fun receive(): ByteArray?
    fun tryReceive(): ByteArray? = receive()
}

fun interface KumiProgress {
    fun progress(done: Long, total: Long, label: String)
}

object KumiNative {
    val loaded: Boolean by lazy { runCatching { System.loadLibrary("kumi_jni") }.isSuccess }

    @JvmStatic external fun info(): String
    @JvmStatic external fun installGates(): String
    @JvmStatic external fun resetSession()
    @JvmStatic external fun openPackage(data: ByteArray, key: String): String
    @JvmStatic external fun identify(link: KumiLink): String
    @JvmStatic external fun backup(link: KumiLink, path: String, progress: KumiProgress?): String
    @JvmStatic external fun install(
        link: KumiLink,
        backup: String,
        backupSha: String,
        pkg: ByteArray,
        key: String,
        from: Int,
        progress: KumiProgress?,
    ): String
    @JvmStatic external fun simInstall(
        handle: Long,
        backup: String,
        backupSha: String,
        pkg: ByteArray,
        key: String,
        from: Int,
        progress: KumiProgress?,
    ): String
    @JvmStatic external fun restoreStock(
        link: KumiLink,
        backup: String,
        backupSha: String,
        exact: Boolean,
        from: Int,
        progress: KumiProgress?,
    ): String
    @JvmStatic external fun rebootFromService(link: KumiLink): String
    @JvmStatic external fun call(link: KumiLink, path: String, request: String): String
    @JvmStatic external fun pollTopics(link: KumiLink, maxFrames: Int): String
    @JvmStatic external fun update(link: KumiLink, pkg: ByteArray, key: String, progress: KumiProgress?): String
    @JvmStatic external fun eqResponse(profile: String, sampleRate: Int, points: Int, lowHz: Double, highHz: Double): String
    @JvmStatic external fun eqAutoPreamp(profile: String, sampleRate: Int): String
    @JvmStatic external fun simCreate(): Long
    @JvmStatic external fun simDestroy(handle: Long)
    @JvmStatic external fun simMode(handle: Long): Int
    @JvmStatic external fun simSetMode(handle: Long, mode: Int)
    @JvmStatic external fun simSend(handle: Long, data: ByteArray): Boolean
    @JvmStatic external fun simReceive(handle: Long): ByteArray?
    @JvmStatic external fun simPublicKey(): String
    @JvmStatic external fun simPackage(major: Int, minor: Int, patch: Int, generation: Int): ByteArray
}

interface KumiBridge {
    val available: Boolean
    fun info(): String
    fun installGates(): String
    fun openPackage(data: ByteArray, key: String): String
    fun identify(link: KumiLink): String
    fun backup(link: KumiLink, path: String, progress: KumiProgress?): String
    fun install(link: KumiLink, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?): String
    fun simInstall(simulator: Long, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?): String
    fun restoreStock(link: KumiLink, backup: String, backupSha: String, exact: Boolean, from: Int, progress: KumiProgress?): String
    fun rebootFromService(link: KumiLink): String
    fun call(link: KumiLink, path: String, request: String): String
    fun pollTopics(link: KumiLink, maxFrames: Int): String
    fun update(link: KumiLink, pkg: ByteArray, key: String, progress: KumiProgress?): String
    fun eqResponse(profile: String, sampleRate: Int, points: Int, lowHz: Double, highHz: Double): String
    fun eqAutoPreamp(profile: String, sampleRate: Int): String
    fun resetSession() {}
}

object NativeBridge : KumiBridge {
    override val available: Boolean get() = KumiNative.loaded
    override fun info() = KumiNative.info()
    override fun installGates() = KumiNative.installGates()
    override fun resetSession() {
        if (KumiNative.loaded) KumiNative.resetSession()
    }
    override fun openPackage(data: ByteArray, key: String) = KumiNative.openPackage(data, key)
    override fun identify(link: KumiLink) = KumiNative.identify(link)
    override fun backup(link: KumiLink, path: String, progress: KumiProgress?) = KumiNative.backup(link, path, progress)
    override fun install(link: KumiLink, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?) =
        KumiNative.install(link, backup, backupSha, pkg, key, from, progress)
    override fun simInstall(simulator: Long, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?) =
        KumiNative.simInstall(simulator, backup, backupSha, pkg, key, from, progress)
    override fun restoreStock(link: KumiLink, backup: String, backupSha: String, exact: Boolean, from: Int, progress: KumiProgress?) =
        KumiNative.restoreStock(link, backup, backupSha, exact, from, progress)
    override fun rebootFromService(link: KumiLink) = KumiNative.rebootFromService(link)
    override fun call(link: KumiLink, path: String, request: String) = KumiNative.call(link, path, request)
    override fun pollTopics(link: KumiLink, maxFrames: Int) = KumiNative.pollTopics(link, maxFrames)
    override fun update(link: KumiLink, pkg: ByteArray, key: String, progress: KumiProgress?) = KumiNative.update(link, pkg, key, progress)
    override fun eqResponse(profile: String, sampleRate: Int, points: Int, lowHz: Double, highHz: Double) =
        KumiNative.eqResponse(profile, sampleRate, points, lowHz, highHz)
    override fun eqAutoPreamp(profile: String, sampleRate: Int) = KumiNative.eqAutoPreamp(profile, sampleRate)
}

class SimulatedCasePort(private val sim: SimulatedCase) : CasePort {
    override val simulated: Boolean = true

    override val simulator: Long get() = sim.handle

    override fun kind(): CaseKind = when (sim.mode) {
        SimulatedCase.Mode.STOCK -> CaseKind.STOCK
        SimulatedCase.Mode.SERVICE -> CaseKind.SERVICE
        SimulatedCase.Mode.KUMI -> CaseKind.KUMI
    }

    override suspend fun sendServiceEntry() {
        if (sim.mode == SimulatedCase.Mode.STOCK) sim.mode = SimulatedCase.Mode.SERVICE
    }

    override suspend fun openService(): CaseChannel = channel(SimulatedCase.Mode.SERVICE)

    override suspend fun openKumi(): CaseChannel = channel(SimulatedCase.Mode.KUMI)

    private fun channel(mode: SimulatedCase.Mode): CaseChannel {
        if (sim.mode != mode) throw IllegalStateException("The simulated case is in ${sim.mode} mode")
        return object : CaseChannel, KumiLink by sim.link {
            override fun close() = Unit
        }
    }
}

object NativeDemoPackages : DemoPackages {
    override val publicKey: String get() = SimulatedCase.publicKey()
    override fun build(version: KumiVersion, generation: Int): ByteArray = SimulatedCase.demoPackage(version, generation)
}

class SimulatedCase private constructor(val handle: Long) : AutoCloseable {
    enum class Mode { STOCK, SERVICE, KUMI }

    private val closed = AtomicBoolean(false)

    val link: KumiLink = object : KumiLink {
        override fun send(data: ByteArray): Boolean = !closed.get() && KumiNative.simSend(handle, data)
        override fun receive(): ByteArray? = if (closed.get()) null else KumiNative.simReceive(handle)
    }

    var mode: Mode
        get() = if (closed.get()) Mode.STOCK else Mode.entries.getOrElse(KumiNative.simMode(handle)) { Mode.STOCK }
        set(value) {
            if (!closed.get()) KumiNative.simSetMode(handle, value.ordinal)
        }

    override fun close() {
        if (closed.compareAndSet(false, true)) KumiNative.simDestroy(handle)
    }

    companion object {
        fun create(): SimulatedCase? = if (KumiNative.loaded) SimulatedCase(KumiNative.simCreate()) else null
        fun publicKey(): String = KumiNative.simPublicKey()
        fun demoPackage(version: KumiVersion, generation: Int): ByteArray =
            KumiNative.simPackage(version.major, version.minor, version.patch, generation)
    }
}
