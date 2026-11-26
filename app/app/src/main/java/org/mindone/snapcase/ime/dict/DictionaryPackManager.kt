// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import android.content.Context
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

data class LanguagePackInfo(
    val code: String,
    val nativeName: String,
    val cyrillic: Boolean = false,
    val bundled: Boolean = false,
    val japanese: Boolean = false,
)

class DictionaryPackManager(private val context: Context) {
    private val cache = ConcurrentHashMap<String, SnapDictBinary>()
    @Volatile private var jaCache: JapaneseDict? = null
    private val packDir: File = File(File(context.filesDir, "ime"), "dicts")

    fun installedCodes(): Set<String> {
        val result = LinkedHashSet<String>()
        CATALOG.filter { it.bundled }.forEach { result.add(it.code) }
        packDir.listFiles()?.forEach { file ->
            if (file.isFile && file.name.endsWith(".snapdict")) {
                result.add(file.name.removeSuffix(".snapdict"))
            } else if (file.isFile && file.name.endsWith(".snapja")) {
                result.add(file.name.removeSuffix(".snapja"))
            }
        }
        return result
    }

    fun isDownloaded(code: String): Boolean {
        val key = code.lowercase()
        return File(packDir, "$key.snapdict").isFile || File(packDir, "$key.snapja").isFile
    }

    fun get(code: String): SnapDictBinary? {
        val key = code.lowercase()
        cache[key]?.let { return it }
        val userFile = File(packDir, "$key.snapdict")
        val loaded = if (userFile.isFile) {
            runCatching { userFile.inputStream().use(SnapDictBinary::load) }.getOrNull()
        } else {
            runCatching { context.assets.open("dict/$key.snapdict").use(SnapDictBinary::load) }.getOrNull()
        }
        if (loaded != null) cache[key] = loaded
        return loaded
    }

    fun getJapanese(): JapaneseDict? {
        jaCache?.let { return it }
        val userFile = File(packDir, "ja.snapja")
        val loaded = if (userFile.isFile) {
            runCatching { userFile.inputStream().use(JapaneseDict::load) }.getOrNull()
        } else {
            runCatching { context.assets.open("dict/ja.snapja").use(JapaneseDict::load) }.getOrNull()
        }
        if (loaded != null) jaCache = loaded
        return loaded
    }

    suspend fun downloadPack(pack: LanguagePackInfo): Result<Int> = withContext(Dispatchers.IO) {
        runCatching {
            if (pack.japanese) {
                val combined = StringBuilder()
                for (shard in 0..2) {
                    val url = URL("https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss/dictionary0$shard.txt")
                    val conn = (url.openConnection() as HttpURLConnection).apply {
                        connectTimeout = 10_000
                        readTimeout = 25_000
                        requestMethod = "GET"
                    }
                    try {
                        if (conn.responseCode !in 200..299) throw IllegalStateException("HTTP ${conn.responseCode}")
                        combined.append(conn.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() })
                        combined.append('\n')
                    } finally {
                        conn.disconnect()
                    }
                }
                val compiled = JapaneseDict.compileFromMozcTsv(combined.toString(), maxEntries = 30_000)
                val binary = JapaneseDict.fromBytes(compiled)
                packDir.mkdirs()
                val target = File(packDir, "ja.snapja")
                val tmp = File(packDir, "ja.tmp")
                tmp.writeBytes(compiled)
                if (!tmp.renameTo(target)) {
                    target.writeBytes(compiled)
                    tmp.delete()
                }
                jaCache = binary
                return@runCatching binary.count
            }
            val url = URL("https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/${pack.code}/${pack.code}_50k.txt")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                connectTimeout = 10_000
                readTimeout = 20_000
                requestMethod = "GET"
            }
            try {
                if (conn.responseCode !in 200..299) {
                    throw IllegalStateException("HTTP ${conn.responseCode}")
                }
                val text = conn.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
                val compiled = SnapDictBinary.compileFromFrequencyText(text, maxWords = 25_000, cyrillic = pack.cyrillic)
                val binary = SnapDictBinary.fromBytes(compiled)
                packDir.mkdirs()
                val target = File(packDir, "${pack.code.lowercase()}.snapdict")
                val tmp = File(packDir, "${pack.code.lowercase()}.tmp")
                tmp.writeBytes(compiled)
                if (!tmp.renameTo(target)) {
                    target.writeBytes(compiled)
                    tmp.delete()
                }
                cache[pack.code.lowercase()] = binary
                binary.count
            } finally {
                conn.disconnect()
            }
        }
    }

    fun removeDownloaded(code: String) {
        val key = code.lowercase()
        File(packDir, "$key.snapdict").delete()
        File(packDir, "$key.snapja").delete()
        cache.remove(key)
        if (key == "ja") jaCache = null
    }

    companion object {
        @Volatile private var instance: DictionaryPackManager? = null

        fun shared(context: Context): DictionaryPackManager =
            instance ?: synchronized(this) {
                instance ?: DictionaryPackManager(context.applicationContext).also { instance = it }
            }

        val CATALOG: List<LanguagePackInfo> = listOf(
            LanguagePackInfo("en", "English", bundled = true),
            LanguagePackInfo("ru", "Русский", cyrillic = true, bundled = true),
            LanguagePackInfo("uk", "Українська", cyrillic = true, bundled = true),
            LanguagePackInfo("ja", "日本語", bundled = true, japanese = true),
        )
    }
}
