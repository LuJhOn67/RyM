package com.rymplayer.data

/**
 * Word-by-word lyrics, the same as the PC's lyrics.rs: word times from LRCLIB's Enhanced LRC
 * (`<mm:ss.xx>` tags), NetEase's YRC and KuGou's KRC, and the sung line split into what's been
 * sung and what's left.
 */
object WordTimes {
    /** One line's words: (start seconds, the word with the space after it). */
    private fun clockSecs(tag: String): Float? {
        val (m, s) = tag.split(':').takeIf { it.size == 2 } ?: return null
        return (m.trim().toFloatOrNull() ?: return null) * 60 + (s.trim().toFloatOrNull() ?: return null)
    }

    /** LRC with Enhanced LRC's word tags: the lines (tags taken out) and each line's words (empty without tags; all empty: none). */
    fun parseEnhanced(src: String): Pair<List<Pair<Float, String>>, List<List<Pair<Float, String>>>> {
        val out = mutableListOf<Triple<Float, String, List<Pair<Float, String>>>>()
        val lineTag = Regex("""^\[([^\]]+)]""")
        val wordTag = Regex("""<(\d+:\d+(?:\.\d+)?)>""")
        for (raw in src.lines()) {
            var rest = raw.trim()
            val times = mutableListOf<Float>()
            while (true) {
                val m = lineTag.find(rest) ?: break
                clockSecs(m.groupValues[1])?.let { times += it }
                rest = rest.substring(m.range.last + 1)
            }
            val words = mutableListOf<Pair<Float, String>>()
            val tags = wordTag.findAll(rest).toList()
            for ((k, m) in tags.withIndex()) {
                val end = if (k + 1 < tags.size) tags[k + 1].range.first else rest.length
                val text = rest.substring(m.range.last + 1, end)
                if (text.isNotEmpty()) words += (clockSecs(m.groupValues[1]) ?: continue) to text
            }
            val text = if (words.isEmpty()) rest.trim() else wordTag.replace(rest, "").trim()
            times.forEach { out += Triple(it, text, words) }
        }
        out.sortBy { it.first }
        val words = if (out.any { it.third.isNotEmpty() }) out.map { it.third } else emptyList()
        return out.map { it.first to it.second } to words
    }

    /** NetEase's YRC (`[start_ms,dur](word_ms,dur,0)word…`) and KuGou's KRC (`[start_ms,dur]<offset_ms,dur,0>word…`). */
    fun parseTimed(src: String): Pair<List<Pair<Float, String>>, List<List<Pair<Float, String>>>> {
        val lines = mutableListOf<Pair<Float, String>>()
        val all = mutableListOf<List<Pair<Float, String>>>()
        val head = Regex("""^\[(\d+),\d+]""")
        val word = Regex("""[(<](\d+),\d+(?:,\d+)?[)>]""")
        for (raw in src.lines()) {
            val line = raw.trim()
            val h = head.find(line) ?: continue
            val start = h.groupValues[1].toFloat()
            val body = line.substring(h.range.last + 1)
            val tags = word.findAll(body).toList()
            val words = mutableListOf<Pair<Float, String>>()
            for ((k, m) in tags.withIndex()) {
                val end = if (k + 1 < tags.size) tags[k + 1].range.first else body.length
                val text = body.substring(m.range.last + 1, end)
                val at = m.groupValues[1].toFloat().let { if (m.value.startsWith("<")) start + it else it }
                if (text.isNotEmpty()) words += at / 1000f to text
            }
            if (words.isEmpty()) continue
            lines += start / 1000f to words.joinToString("") { it.second }.trim()
            all += words
        }
        return lines to all
    }

    /** KuGou's KRC file after base64: "krc1", then XOR-scrambled zlib text. */
    fun decodeKrc(raw: ByteArray): String? {
        val key = intArrayOf(64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105)
        if (raw.size < 4 || String(raw, 0, 4, Charsets.US_ASCII) != "krc1") return null
        val body = ByteArray(raw.size - 4) { i -> (raw[i + 4].toInt() xor key[i % key.size]).toByte() }
        return runCatching {
            val inflater = java.util.zip.Inflater()
            inflater.setInput(body)
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(8192)
            while (!inflater.finished()) {
                val n = inflater.inflate(buf)
                if (n == 0 && (inflater.needsInput() || inflater.needsDictionary())) break
                out.write(buf, 0, n)
            }
            inflater.end()
            out.toString("UTF-8").trimStart('﻿')
        }.getOrNull()
    }

    /** The sung line split at [pos]: whole words sung, then the letters of the word being sung by its time; and what's left. */
    fun sungSplit(words: List<Pair<Float, String>>, lineEnd: Float, pos: Float): Pair<String, String> {
        val sung = StringBuilder()
        val left = StringBuilder()
        for ((i, w) in words.withIndex()) {
            val (start, text) = w
            val end = maxOf(words.getOrNull(i + 1)?.first ?: lineEnd, start + 0.05f)
            when {
                pos >= end -> sung.append(text)
                pos <= start -> left.append(text)
                else -> {
                    val lit = kotlin.math.round(((pos - start) / (end - start)) * text.length).toInt().coerceIn(0, text.length)
                    sung.append(text, 0, lit)
                    left.append(text, lit, text.length)
                }
            }
        }
        return sung.toString() to left.toString()
    }
}
