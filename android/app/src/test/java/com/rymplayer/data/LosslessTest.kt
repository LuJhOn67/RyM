package com.rymplayer.data

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LosslessTest {
    private fun file(name: String, title: String? = null, length: String? = null) = JSONObject().apply {
        put("name", name); title?.let { put("title", it) }; length?.let { put("length", it) }
    }

    @Test
    fun picksTheFlacWithTheSameTitleAndLength() {
        val files = JSONArray(listOf(file("01 - Intro.flac", "Intro", "61.2"), file("02 - Moonlight.mp3", "Moonlight", "200"), file("02 - Moonlight.flac", "Moonlight", "3:21")))
        assertEquals("02 - Moonlight.flac", Lossless.pickFile(files, "Moonlight", 200f))
        assertNull(Lossless.pickFile(files, "Moonlight", 260f))
        val untitled = JSONArray(listOf(file("disc1/03 Blue Sky.flac")))
        assertEquals("disc1/03 Blue Sky.flac", Lossless.pickFile(untitled, "Blue Sky", null))
        assertNull(Lossless.pickFile(untitled, "Red Sky", null))
    }

    @Test
    fun lengthsRead() {
        assertEquals(225f, Lossless.secsOf("3:45")!!, 0f)
        assertEquals(3723f, Lossless.secsOf("1:02:03")!!, 0f)
        assertNull(Lossless.secsOf(""))
    }
}
