// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.kumi

open class FakeBridge : KumiBridge {
    override val available: Boolean = true
    override fun info(): String = unexpected("info")
    override fun installGates(): String = unexpected("installGates")
    override fun openPackage(data: ByteArray, key: String): String = unexpected("openPackage")
    override fun identify(link: KumiLink): String = unexpected("identify")
    override fun backup(link: KumiLink, path: String, progress: KumiProgress?): String = unexpected("backup")
    override fun install(link: KumiLink, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?): String =
        unexpected("install")
    override fun simInstall(simulator: Long, backup: String, backupSha: String, pkg: ByteArray, key: String, from: Int, progress: KumiProgress?): String =
        unexpected("simInstall")
    override fun restoreStock(link: KumiLink, backup: String, backupSha: String, exact: Boolean, from: Int, progress: KumiProgress?): String =
        unexpected("restoreStock")
    override fun rebootFromService(link: KumiLink): String = unexpected("rebootFromService")
    override fun call(link: KumiLink, path: String, request: String): String = unexpected("call $path")
    override fun pollTopics(link: KumiLink, maxFrames: Int): String = """{"ok":[]}"""
    override fun resetSession() = Unit
    override fun update(link: KumiLink, pkg: ByteArray, key: String, progress: KumiProgress?): String = unexpected("update")
    override fun eqResponse(profile: String, sampleRate: Int, points: Int, lowHz: Double, highHz: Double): String = unexpected("eqResponse")
    override fun eqAutoPreamp(profile: String, sampleRate: Int): String = unexpected("eqAutoPreamp")

    private fun unexpected(what: String): Nothing = throw AssertionError("unexpected bridge call: $what")
}
