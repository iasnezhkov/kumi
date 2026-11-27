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
    val hangul: Boolean = false,
    val bundled: Boolean = false,
    val japanese: Boolean = false,
    val pinyin: Boolean = false,
    val traditional: Boolean = false,
)

class DictionaryPackManager(private val context: Context) {
    private val cache = ConcurrentHashMap<String, SnapDictBinary>()
    private val pinyinCache = ConcurrentHashMap<String, PinyinDict>()
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
            } else if (file.isFile && file.name.endsWith(".snappy")) {
                result.add(file.name.removeSuffix(".snappy").replace('_', '-'))
            }
        }
        return result
    }

    fun isDownloaded(code: String): Boolean {
        val key = code.lowercase()
        val pyKey = key.replace('-', '_')
        return File(packDir, "$key.snapdict").isFile ||
            File(packDir, "$key.snapja").isFile ||
            File(packDir, "$pyKey.snappy").isFile
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

    fun getPinyin(code: String): PinyinDict? {
        val key = code.lowercase().replace('-', '_')
        pinyinCache[key]?.let { return it }
        val userFile = File(packDir, "$key.snappy")
        val loaded = if (userFile.isFile) {
            runCatching { userFile.inputStream().use(PinyinDict::load) }.getOrNull()
        } else {
            runCatching { context.assets.open("dict/$key.snappy").use(PinyinDict::load) }.getOrNull()
        }
        if (loaded != null) pinyinCache[key] = loaded
        return loaded
    }

    private fun httpGetText(urlString: String, readTimeoutMs: Int = 25_000): String {
        val url = URL(urlString)
        val conn = (url.openConnection() as HttpURLConnection).apply {
            connectTimeout = 10_000
            readTimeout = readTimeoutMs
            requestMethod = "GET"
        }
        try {
            if (conn.responseCode !in 200..299) throw IllegalStateException("HTTP ${conn.responseCode}")
            return conn.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        } finally {
            conn.disconnect()
        }
    }

    suspend fun downloadPack(pack: LanguagePackInfo): Result<Int> = withContext(Dispatchers.IO) {
        runCatching {
            if (pack.japanese) {
                val combined = StringBuilder()
                for (shard in 0..2) {
                    combined.append(httpGetText("https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss/dictionary0$shard.txt"))
                    combined.append('\n')
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
            if (pack.pinyin) {
                val pyKey = pack.code.lowercase().replace('-', '_')
                val compiled = if (pack.traditional) {
                    val yaml = httpGetText("https://raw.githubusercontent.com/rime/rime-luna-pinyin/master/luna_pinyin.dict.yaml", 35_000)
                    val essay = runCatching {
                        httpGetText("https://raw.githubusercontent.com/rime/rime-essay/master/essay.txt", 25_000)
                    }.getOrNull()
                    PinyinDict.compileFromRimeYaml(yaml, essayText = essay, maxEntries = 30_000)
                } else {
                    val yaml = httpGetText("https://raw.githubusercontent.com/rime/rime-pinyin-simp/master/pinyin_simp.dict.yaml", 35_000)
                    PinyinDict.compileFromRimeYaml(yaml, maxEntries = 30_000)
                }
                val binary = PinyinDict.fromBytes(compiled)
                packDir.mkdirs()
                val target = File(packDir, "$pyKey.snappy")
                val tmp = File(packDir, "$pyKey.tmp")
                tmp.writeBytes(compiled)
                if (!tmp.renameTo(target)) {
                    target.writeBytes(compiled)
                    tmp.delete()
                }
                pinyinCache[pyKey] = binary
                return@runCatching binary.count
            }
            val text = httpGetText("https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/${pack.code}/${pack.code}_50k.txt", 20_000)
            val compiled = SnapDictBinary.compileFromFrequencyText(text, maxWords = 25_000, cyrillic = pack.cyrillic, hangul = pack.hangul)
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
        }
    }

    fun removeDownloaded(code: String) {
        val key = code.lowercase()
        val pyKey = key.replace('-', '_')
        File(packDir, "$key.snapdict").delete()
        File(packDir, "$key.snapja").delete()
        File(packDir, "$pyKey.snappy").delete()
        cache.remove(key)
        pinyinCache.remove(pyKey)
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
            LanguagePackInfo("ko", "한국어", hangul = true, bundled = true),
            LanguagePackInfo("zh-cn", "简体中文", bundled = true, pinyin = true),
            LanguagePackInfo("zh-tw", "繁體中文", bundled = true, pinyin = true, traditional = true),
            LanguagePackInfo("es", "Español"),
            LanguagePackInfo("de", "Deutsch"),
            LanguagePackInfo("fr", "Français"),
            LanguagePackInfo("it", "Italiano"),
            LanguagePackInfo("pt", "Português"),
            LanguagePackInfo("pl", "Polski"),
            LanguagePackInfo("nl", "Nederlands"),
            LanguagePackInfo("tr", "Türkçe"),
            LanguagePackInfo("cs", "Čeština"),
            LanguagePackInfo("sv", "Svenska"),
            LanguagePackInfo("fi", "Suomi"),
            LanguagePackInfo("ro", "Română"),
            LanguagePackInfo("hu", "Magyar"),
        )
    }
}
