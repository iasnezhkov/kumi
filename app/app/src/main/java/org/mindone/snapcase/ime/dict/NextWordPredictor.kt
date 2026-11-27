// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
package org.mindone.snapcase.ime.dict

import org.mindone.snapcase.ime.StripLang

class NextWordPredictor(
    private val personal: PersonalDictionary? = null,
) {
    fun predict(
        previousWord: String,
        lang: StripLang,
        capitalize: Boolean = false,
        limit: Int = 3,
    ): List<String> {
        val key = normalizeKey(previousWord, lang)
        if (key.isEmpty()) return emptyList()
        val out = LinkedHashSet<String>(limit + 2)
        personal?.nextWords(lang.code, key, limit = limit + 2)?.forEach { word ->
            if (out.size < limit) out.add(formatCase(word, capitalize))
        }
        val table = bigramsFor(lang)
        table[key]?.forEach { word ->
            if (out.size < limit) out.add(formatCase(word, capitalize))
        }
        return out.take(limit)
    }

    fun predictedSet(previousWord: String, lang: StripLang): Set<String> {
        val key = normalizeKey(previousWord, lang)
        if (key.isEmpty()) return emptySet()
        val out = LinkedHashSet<String>(8)
        personal?.nextWords(lang.code, key, limit = 6)?.forEach { out.add(SnapDictBinary.foldKey(it)) }
        val table = bigramsFor(lang)
        table[key]?.forEach { out.add(SnapDictBinary.foldKey(it)) }
        return out
    }

    private fun bigramsFor(lang: StripLang): Map<String, List<String>> = when (lang) {
        StripLang.RU -> RU_BIGRAMS
        StripLang.EN -> EN_BIGRAMS
        StripLang.UK -> UK_BIGRAMS
        StripLang.JA -> JA_BIGRAMS
        StripLang.KO -> KO_BIGRAMS
        StripLang.ZH_CN -> ZH_CN_BIGRAMS
        StripLang.ZH_TW -> ZH_TW_BIGRAMS
        else -> LatinBigrams.forLang(lang)
    }

    private fun normalizeKey(word: String, lang: StripLang): String {
        val trimmed = word.trim()
        if (trimmed.isEmpty()) return ""
        return if (lang.isCjkNoSpace || lang.isHangul) trimmed else SnapDictBinary.foldKey(trimmed)
    }

    private fun formatCase(word: String, capitalize: Boolean): String =
        if (capitalize && word.isNotEmpty() && word[0].isLetter()) word.replaceFirstChar { it.uppercaseChar() } else word

    companion object {
        private val RU_BIGRAMS: Map<String, List<String>> = mapOf(
            "привет" to listOf("как", "что", "ты"),
            "здравствуй" to listOf("как", "что", "ты"),
            "здравствуйте" to listOf("подскажите", "можно", "как"),
            "доброе" to listOf("утро", "время", "дело"),
            "добрый" to listOf("день", "вечер", "человек"),
            "доброй" to listOf("ночи", "дороги"),
            "спокойной" to listOf("ночи"),
            "как" to listOf("дела", "ты", "раз"),
            "дела" to listOf("нормально", "хорошо", "отлично"),
            "что" to listOf("ты", "там", "нового"),
            "я" to listOf("не", "уже", "сейчас"),
            "ты" to listOf("где", "как", "уже"),
            "мы" to listOf("уже", "сегодня", "будем"),
            "он" to listOf("уже", "не", "сказал"),
            "она" to listOf("уже", "не", "сказала"),
            "они" to listOf("уже", "не", "сказали"),
            "не" to listOf("знаю", "могу", "буду"),
            "да" to listOf("конечно", "хорошо", "я"),
            "нет" to listOf("не", "спасибо", "ещё"),
            "все" to listOf("хорошо", "равно", "нормально"),
            "спасибо" to listOf("большое", "за", "тебе"),
            "большое" to listOf("спасибо"),
            "пожалуйста" to listOf("подскажите", "проверь", "посмотри"),
            "извини" to listOf("за", "я", "пожалуйста"),
            "прости" to listOf("пожалуйста", "за", "я"),
            "хорошо" to listOf("давай", "спасибо", "договорились"),
            "ладно" to listOf("давай", "пока", "хорошо"),
            "ок" to listOf("спасибо", "давай", "понял"),
            "давай" to listOf("завтра", "позже", "попробуем"),
            "можно" to listOf("будет", "ли", "попробовать"),
            "нужно" to listOf("будет", "сделать", "проверить"),
            "надо" to listOf("будет", "сделать", "посмотреть"),
            "хочу" to listOf("сказать", "попробовать", "сделать"),
            "буду" to listOf("ждать", "дома", "скоро"),
            "будет" to listOf("хорошо", "удобно", "лучше"),
            "если" to listOf("что", "не", "будет"),
            "когда" to listOf("ты", "будет", "сможешь"),
            "где" to listOf("ты", "это", "можно"),
            "почему" to listOf("ты", "не", "так"),
            "потому" to listOf("что"),
            "так" to listOf("что", "как", "же"),
            "уже" to listOf("дома", "еду", "всё"),
            "еще" to listOf("не", "раз", "есть"),
            "сейчас" to listOf("посмотрю", "сделаю", "приду"),
            "потом" to listOf("напишу", "посмотрю", "поговорим"),
            "завтра" to listOf("утром", "днём", "вечером"),
            "сегодня" to listOf("вечером", "утром", "не"),
            "вчера" to listOf("вечером", "было", "я"),
            "у" to listOf("меня", "тебя", "нас"),
            "меня" to listOf("есть", "нет", "всё"),
            "тебя" to listOf("как", "есть", "всё"),
            "нас" to listOf("есть", "всё", "будет"),
            "на" to listOf("самом", "всякий", "работе"),
            "самом" to listOf("деле"),
            "всякий" to listOf("случай"),
            "в" to listOf("общем", "итоге", "принципе"),
            "общем" to listOf("всё", "да", "я"),
            "по" to listOf("поводу", "сути", "идее"),
            "до" to listOf("встречи", "завтра", "сих"),
            "сих" to listOf("пор"),
            "с" to listOf("удовольствием", "тобой", "утра"),
            "за" to listOf("информацию", "помощь", "тобой"),
            "из" to listOf("за", "дома", "этого"),
            "для" to listOf("этого", "меня", "тебя"),
            "без" to listOf("проблем", "этого", "меня"),
            "очень" to listOf("хорошо", "круто", "удобно"),
            "просто" to listOf("хотел", "так", "супер"),
            "только" to listOf("что", "если", "не"),
            "тоже" to listOf("хорошо", "так", "хочу"),
            "конечно" to listOf("можно", "да", "без"),
            "наверное" to listOf("да", "нет", "лучше"),
            "кстати" to listOf("да", "а", "ты"),
            "вообще" to listOf("не", "то", "да"),
            "думаю" to listOf("что", "да", "стоит"),
            "кажется" to listOf("что", "да", "это"),
            "надеюсь" to listOf("что", "всё", "скоро"),
            "знаю" to listOf("что", "как", "где"),
            "могу" to listOf("помочь", "сделать", "прийти"),
            "есть" to listOf("вопрос", "идея", "время"),
            "это" to listOf("очень", "хорошо", "просто"),
            "тут" to listOf("всё", "есть", "не"),
            "там" to listOf("есть", "всё", "уже"),
            "понял" to listOf("спасибо", "хорошо", "сейчас"),
            "понятно" to listOf("спасибо", "хорошо", "ладно"),
            "отлично" to listOf("спасибо", "давай", "договорились"),
            "супер" to listOf("спасибо", "отлично", "давай"),
            "скоро" to listOf("буду", "приду", "напишу"),
            "никуда" to listOf("не"),
            "ничего" to listOf("страшного", "не", "себе"),
            "куда" to listOf("ты", "пойдём", "ехать"),
            "сколько" to listOf("стоит", "времени", "будет"),
            "как-то" to listOf("так", "странно", "не"),
        )

        private val EN_BIGRAMS: Map<String, List<String>> = mapOf(
            "i" to listOf("am", "think", "will"),
            "i'm" to listOf("not", "on", "going"),
            "you" to listOf("can", "are", "know"),
            "you're" to listOf("welcome", "right", "going"),
            "we" to listOf("can", "are", "should"),
            "they" to listOf("are", "have", "will"),
            "he" to listOf("is", "was", "said"),
            "she" to listOf("is", "was", "said"),
            "it" to listOf("is", "was", "works"),
            "it's" to listOf("not", "a", "going"),
            "this" to listOf("is", "one", "looks"),
            "that" to listOf("is", "was", "sounds"),
            "how" to listOf("are", "about", "is"),
            "what" to listOf("do", "is", "about"),
            "where" to listOf("are", "is", "do"),
            "when" to listOf("you", "we", "is"),
            "why" to listOf("not", "do", "is"),
            "can" to listOf("you", "we", "i"),
            "could" to listOf("you", "be", "we"),
            "would" to listOf("be", "you", "like"),
            "should" to listOf("be", "we", "i"),
            "will" to listOf("be", "do", "check"),
            "do" to listOf("you", "not", "it"),
            "don't" to listOf("know", "think", "worry"),
            "did" to listOf("you", "not", "it"),
            "are" to listOf("you", "we", "they"),
            "is" to listOf("it", "that", "there"),
            "was" to listOf("just", "a", "not"),
            "have" to listOf("a", "to", "you"),
            "has" to listOf("been", "a", "to"),
            "thank" to listOf("you"),
            "thanks" to listOf("a", "for", "so"),
            "good" to listOf("morning", "night", "idea"),
            "great" to listOf("thanks", "idea", "work"),
            "sounds" to listOf("good", "great", "like"),
            "see" to listOf("you", "if", "what"),
            "let" to listOf("me", "us", "know"),
            "let's" to listOf("do", "go", "try"),
            "please" to listOf("let", "check", "send"),
            "sorry" to listOf("for", "about", "i"),
            "just" to listOf("a", "wanted", "checking"),
            "not" to listOf("sure", "yet", "a"),
            "no" to listOf("problem", "worries", "way"),
            "yes" to listOf("please", "sure", "that"),
            "on" to listOf("the", "my", "it"),
            "my" to listOf("way", "side", "phone"),
            "in" to listOf("the", "a", "case"),
            "at" to listOf("the", "least", "all"),
            "to" to listOf("be", "do", "the"),
            "for" to listOf("the", "me", "now"),
            "of" to listOf("the", "course", "it"),
            "with" to listOf("the", "you", "me"),
            "from" to listOf("the", "my", "here"),
            "about" to listOf("the", "it", "that"),
            "if" to listOf("you", "it", "we"),
            "going" to listOf("to", "be", "home"),
            "need" to listOf("to", "a", "more"),
            "want" to listOf("to", "a", "some"),
            "think" to listOf("it", "so", "we"),
            "know" to listOf("if", "what", "how"),
            "like" to listOf("to", "this", "that"),
            "get" to listOf("it", "back", "the"),
            "make" to listOf("sure", "it", "sense"),
            "take" to listOf("a", "care", "it"),
            "look" to listOf("at", "like", "good"),
            "come" to listOf("back", "on", "over"),
            "right" to listOf("now", "here", "away"),
            "a" to listOf("lot", "few", "bit"),
            "so" to listOf("much", "far", "good"),
            "all" to listOf("good", "the", "right"),
            "very" to listOf("good", "nice", "much"),
            "really" to listOf("good", "nice", "appreciate"),
            "by" to listOf("the", "now", "tomorrow"),
            "the" to listOf("same", "way", "best"),
        )

        private val UK_BIGRAMS: Map<String, List<String>> = mapOf(
            "привіт" to listOf("як", "що", "ти"),
            "як" to listOf("справи", "ти", "раз"),
            "що" to listOf("ти", "нового", "там"),
            "я" to listOf("не", "вже", "зараз"),
            "ти" to listOf("де", "як", "вже"),
            "ми" to listOf("вже", "сьогодні", "будемо"),
            "не" to listOf("знаю", "можу", "буду"),
            "дякую" to listOf("велике", "за", "тобі"),
            "добрий" to listOf("день", "вечір"),
            "доброго" to listOf("ранку", "дня", "вечора"),
            "добре" to listOf("давай", "дякую", "домовились"),
            "давай" to listOf("завтра", "пізніше", "спробуємо"),
            "можна" to listOf("буде", "спробувати", "перевірити"),
            "треба" to listOf("буде", "зробити", "перевірити"),
            "потрібно" to listOf("буде", "зробити", "перевірити"),
            "хочу" to listOf("сказати", "спробувати", "зробити"),
            "буду" to listOf("чекати", "вдома", "скоро"),
            "якщо" to listOf("що", "не", "буде"),
            "коли" to listOf("ти", "буде", "зможеш"),
            "де" to listOf("ти", "це", "можна"),
            "тому" to listOf("що"),
            "вже" to listOf("вдома", "їду", "все"),
            "зараз" to listOf("подивлюсь", "зроблю", "прийду"),
            "завтра" to listOf("зранку", "вдень", "ввечері"),
            "сьогодні" to listOf("ввечері", "зранку", "не"),
            "у" to listOf("мене", "тебе", "нас"),
            "на" to listOf("всяк", "роботі", "зв'язку"),
            "до" to listOf("зустрічі", "завтра", "речі"),
            "дуже" to listOf("добре", "дякую", "круто"),
            "все" to listOf("добре", "нормально", "одно"),
            "це" to listOf("дуже", "добре", "просто"),
            "будь" to listOf("ласка"),
        )

        private val JA_BIGRAMS: Map<String, List<String>> = mapOf(
            "おはよう" to listOf("ございます"),
            "ありがとう" to listOf("ございます", "ございました"),
            "よろしく" to listOf("お願いします", "お願いいたします"),
            "お疲れ" to listOf("様です", "様でした"),
            "お願い" to listOf("します", "いたします"),
            "すみません" to listOf("が", "でした"),
            "大丈夫" to listOf("です", "ですか", "だよ"),
            "了解" to listOf("です", "しました"),
            "わかり" to listOf("ました"),
            "今日" to listOf("は", "も", "の"),
            "明日" to listOf("は", "も", "の"),
            "これ" to listOf("は", "を", "が"),
            "それ" to listOf("は", "を", "が"),
            "私" to listOf("は", "の", "も"),
            "日本" to listOf("語", "の", "で"),
            "今" to listOf("から", "は", "すぐ"),
            "また" to listOf("明日", "後で", "ね"),
        )

        private val KO_BIGRAMS: Map<String, List<String>> = mapOf(
            "안녕" to listOf("하세요", "하십니까"),
            "감사" to listOf("합니다", "드려요"),
            "죄송" to listOf("합니다", "해요"),
            "좋은" to listOf("하루", "아침", "생각"),
            "오늘" to listOf("도", "저녁", "시간"),
            "내일" to listOf("봐요", "아침", "시간"),
            "지금" to listOf("가고", "바로", "시간"),
            "정말" to listOf("감사합니다", "좋아요", "다행이네요"),
            "너무" to listOf("좋아요", "감사합니다", "고마워"),
            "알겠" to listOf("습니다", "어요"),
            "수고" to listOf("하셨습니다", "하세요"),
            "잘" to listOf("부탁드립니다", "자요", "지내세요"),
            "어디" to listOf("세요", "에", "야"),
            "무슨" to listOf("일", "말씀"),
            "괜찮" to listOf("아요", "습니다", "아"),
            "고마워" to listOf("요"),
        )

        private val ZH_CN_BIGRAMS: Map<String, List<String>> = mapOf(
            "你好" to listOf("请问", "吗", "呀"),
            "谢谢" to listOf("你", "大家", "啦"),
            "好的" to listOf("谢谢", "没问题", "我"),
            "没问题" to listOf("谢谢", "好的"),
            "不好意思" to listOf("打扰", "请问", "我"),
            "今天" to listOf("天气", "有空", "晚上"),
            "明天" to listOf("见", "早上", "有空"),
            "现在" to listOf("有空", "过去", "开始"),
            "我们" to listOf("一起", "可以", "现在"),
            "可以" to listOf("吗", "的", "一起"),
            "什么" to listOf("时候", "意思", "地方"),
            "怎么" to listOf("样", "办", "回事"),
            "非常" to listOf("感谢", "好", "开心"),
            "辛苦" to listOf("了", "啦"),
            "晚安" to listOf("好梦"),
        )

        private val ZH_TW_BIGRAMS: Map<String, List<String>> = mapOf(
            "你好" to listOf("請問", "嗎", "呀"),
            "謝謝" to listOf("你", "大家", "喔"),
            "好的" to listOf("謝謝", "沒問題", "我"),
            "沒問題" to listOf("謝謝", "好的"),
            "不好意思" to listOf("打擾", "請問", "我"),
            "今天" to listOf("天氣", "有空", "晚上"),
            "明天" to listOf("見", "早上", "有空"),
            "現在" to listOf("有空", "過去", "開始"),
            "我們" to listOf("一起", "可以", "現在"),
            "可以" to listOf("嗎", "的", "一起"),
            "什麼" to listOf("時候", "意思", "地方"),
            "怎麼" to listOf("樣", "辦", "回事"),
            "非常" to listOf("感謝", "好", "開心"),
            "辛苦" to listOf("了", "啦"),
            "晚安" to listOf("好夢"),
        )
    }
}
