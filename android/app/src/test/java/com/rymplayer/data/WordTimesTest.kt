package com.rymplayer.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WordTimesTest {
    @Test
    fun enhancedLrcGivesWordsAndCleanLines() {
        val (lines, words) = WordTimes.parseEnhanced("[00:01.00]<00:01.00>Hello <00:01.50>dear <00:02.00>world\n[00:03.00]plain line")
        assertEquals(listOf(1f to "Hello dear world", 3f to "plain line"), lines)
        assertEquals(listOf(1f to "Hello ", 1.5f to "dear ", 2f to "world"), words[0])
        assertTrue(words[1].isEmpty())
        assertTrue(WordTimes.parseEnhanced("[00:01.00]just a line").second.isEmpty())
    }

    @Test
    fun yrcAndKrcGiveWords() {
        val (l, w) = WordTimes.parseTimed("{\"t\":0}\n[16210,3460](16210,670,0)Hello (16880,410,0)world")
        assertEquals(listOf(16.21f to "Hello world"), l)
        assertEquals(listOf(16.21f to "Hello ", 16.88f to "world"), w[0])
        val (l2, w2) = WordTimes.parseTimed("[ar:Someone]\n[1000,2000]<0,500,0>こん<500,500,0>にちは")
        assertEquals(listOf(1f to "こんにちは"), l2)
        assertEquals(listOf(1f to "こん", 1.5f to "にちは"), w2[0])
    }

    @Test
    fun theSungPartGrowsLetterByLetter() {
        val words = listOf(1f to "Hello ", 2f to "world")
        assertEquals("" to "Hello world", WordTimes.sungSplit(words, 3f, 0.5f))
        assertEquals("Hello " to "world", WordTimes.sungSplit(words, 3f, 2f))
        assertEquals("Hello wor" to "ld", WordTimes.sungSplit(words, 3f, 2.6f))
        assertEquals("Hello world" to "", WordTimes.sungSplit(words, 3f, 9f))
    }

    @Test
    fun krcUnscrambles() {
        val key = intArrayOf(64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105)
        val deflater = java.util.zip.Deflater()
        deflater.setInput("[1000,2000]<0,500,0>hi".toByteArray())
        deflater.finish()
        val buf = ByteArray(256)
        val n = deflater.deflate(buf)
        val raw = "krc1".toByteArray() + ByteArray(n) { i -> (buf[i].toInt() xor key[i % key.size]).toByte() }
        assertEquals("[1000,2000]<0,500,0>hi", WordTimes.decodeKrc(raw))
        assertNull(WordTimes.decodeKrc("nope".toByteArray()))
    }
}
