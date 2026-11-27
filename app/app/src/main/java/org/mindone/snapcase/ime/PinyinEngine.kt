// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime

import org.mindone.snapcase.ime.dict.PinyinDict

object PinyinEngine {
    private val SYLLABLES: Set<String> = setOf(
        "a", "ai", "an", "ang", "ao",
        "ba", "bai", "ban", "bang", "bao", "bei", "ben", "beng", "bi", "bian", "biao", "bie", "bin", "bing", "bo", "bu",
        "ca", "cai", "can", "cang", "cao", "ce", "cei", "cen", "ceng", "cha", "chai", "chan", "chang", "chao", "che", "chen", "cheng", "chi", "chong", "chou", "chu", "chua", "chuai", "chuan", "chuang", "chui", "chun", "chuo", "ci", "cong", "cou", "cu", "cuan", "cui", "cun", "cuo",
        "da", "dai", "dan", "dang", "dao", "de", "dei", "den", "deng", "di", "dia", "dian", "diao", "die", "ding", "diu", "dong", "dou", "du", "duan", "dui", "dun", "duo",
        "e", "ei", "en", "eng", "er",
        "fa", "fan", "fang", "fei", "fen", "feng", "fiao", "fo", "fou", "fu",
        "ga", "gai", "gan", "gang", "gao", "ge", "gei", "gen", "geng", "gong", "gou", "gu", "gua", "guai", "guan", "guang", "gui", "gun", "guo",
        "ha", "hai", "han", "hang", "hao", "he", "hei", "hen", "heng", "hong", "hou", "hu", "hua", "huai", "huan", "huang", "hui", "hun", "huo",
        "ji", "jia", "jian", "jiang", "jiao", "jie", "jin", "jing", "jiong", "jiu", "ju", "juan", "jue", "jun",
        "ka", "kai", "kan", "kang", "kao", "ke", "kei", "ken", "keng", "kong", "kou", "ku", "kua", "kuai", "kuan", "kuang", "kui", "kun", "kuo",
        "la", "lai", "lan", "lang", "lao", "le", "lei", "leng", "li", "lia", "lian", "liang", "liao", "lie", "lin", "ling", "liu", "lo", "long", "lou", "lu", "luan", "lue", "lun", "luo", "lv", "lve",
        "ma", "mai", "man", "mang", "mao", "me", "mei", "men", "meng", "mi", "mian", "miao", "mie", "min", "ming", "miu", "mo", "mou", "mu",
        "na", "nai", "nan", "nang", "nao", "ne", "nei", "nen", "neng", "ni", "nian", "niang", "niao", "nie", "nin", "ning", "niu", "nong", "nou", "nu", "nuan", "nue", "nun", "nuo", "nv", "nve",
        "o", "ou",
        "pa", "pai", "pan", "pang", "pao", "pei", "pen", "peng", "pi", "pian", "piao", "pie", "pin", "ping", "po", "pou", "pu",
        "qi", "qia", "qian", "qiang", "qiao", "qie", "qin", "qing", "qiong", "qiu", "qu", "quan", "que", "qun",
        "ran", "rang", "rao", "re", "ren", "reng", "ri", "rong", "rou", "ru", "rua", "ruan", "rui", "run", "ruo",
        "sa", "sai", "san", "sang", "sao", "se", "sen", "seng", "sha", "shai", "shan", "shang", "shao", "she", "shei", "shen", "sheng", "shi", "shou", "shu", "shua", "shuai", "shuan", "shuang", "shui", "shun", "shuo", "si", "song", "sou", "su", "suan", "sui", "sun", "suo",
        "ta", "tai", "tan", "tang", "tao", "te", "tei", "teng", "ti", "tian", "tiao", "tie", "ting", "tong", "tou", "tu", "tuan", "tui", "tun", "tuo",
        "wa", "wai", "wan", "wang", "wei", "wen", "weng", "wo", "wu",
        "xi", "xia", "xian", "xiang", "xiao", "xie", "xin", "xing", "xiong", "xiu", "xu", "xuan", "xue", "xun",
        "ya", "yan", "yang", "yao", "ye", "yi", "yin", "ying", "yo", "yong", "you", "yu", "yuan", "yue", "yun",
        "za", "zai", "zan", "zang", "zao", "ze", "zei", "zen", "zeng", "zha", "zhai", "zhan", "zhang", "zhao", "zhe", "zhei", "zhen", "zheng", "zhi", "zhong", "zhou", "zhu", "zhua", "zhuai", "zhuan", "zhuang", "zhui", "zhun", "zhuo", "zi", "zong", "zou", "zu", "zuan", "zui", "zun", "zuo",
    )

    fun segment(rawInput: String): List<String> {
        val clean = rawInput.lowercase()
        val parts = clean.split('\'').filter { it.isNotEmpty() }
        val result = ArrayList<String>(6)
        for (part in parts) {
            result.addAll(segmentContiguous(part))
        }
        return result
    }

    fun formatComposing(rawInput: String): String {
        val segs = segment(rawInput)
        return if (segs.isEmpty()) rawInput.lowercase() else segs.joinToString("'")
    }

    fun lookup(rawInput: String, dict: PinyinDict?, limit: Int = 6): List<String> {
        val compact = rawInput.lowercase().filter { it in 'a'..'z' }
        if (compact.isEmpty()) return emptyList()
        if (dict == null) return listOf(formatComposing(rawInput))
        val out = LinkedHashSet<String>(limit + 2)
        dict.lookupExact(compact, limit).forEach { if (out.size < limit) out.add(it.first) }
        if (out.size < limit) {
            dict.lookupPrefix(compact, limit).forEach { if (out.size < limit) out.add(it.first) }
        }
        val segs = segment(rawInput)
        if (segs.size >= 2 && out.size < limit) {
            val composed = StringBuilder()
            var matchedAny = false
            for (seg in segs) {
                val top = dict.lookupExact(seg, 1).firstOrNull()?.first
                    ?: dict.lookupPrefix(seg, 1).firstOrNull()?.first
                if (top != null) {
                    composed.append(top)
                    matchedAny = true
                } else {
                    composed.append(seg)
                }
            }
            if (matchedAny && composed.isNotEmpty()) {
                out.add(composed.toString())
            }
            val firstHits = dict.lookupExact(segs.first(), 3)
            firstHits.forEach { if (out.size < limit) out.add(it.first) }
        }
        return out.take(limit)
    }

    private fun segmentContiguous(input: String): List<String> {
        val out = ArrayList<String>(4)
        var i = 0
        val n = input.length
        while (i < n) {
            var matched = 0
            val maxLen = (n - i).coerceAtMost(6)
            for (len in maxLen downTo 1) {
                val sub = input.substring(i, i + len)
                if (sub in SYLLABLES) {
                    if (len >= 2 && i + len < n && sub.endsWith('n') && sub != "n") {
                        val nextChar = input[i + len]
                        if (nextChar in "aeiou") {
                            val shorter = sub.dropLast(1)
                            val withN = "n" + input.substring(i + len, (i + len + 4).coerceAtMost(n))
                            if (shorter in SYLLABLES && hasSyllablePrefix(withN)) {
                                continue
                            }
                        }
                    }
                    matched = len
                    break
                }
            }
            if (matched > 0) {
                out.add(input.substring(i, i + matched))
                i += matched
            } else {
                out.add(input.substring(i, i + 1))
                i++
            }
        }
        return out
    }

    private fun hasSyllablePrefix(s: String): Boolean {
        val maxLen = s.length.coerceAtMost(6)
        for (len in maxLen downTo 2) {
            if (s.substring(0, len) in SYLLABLES) return true
        }
        return false
    }
}
