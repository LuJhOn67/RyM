package com.rymplayer.playback

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The PC's address typed in Settings: with or without its port. */
class PcAddressTest {
    @Test
    fun readsTheAddress() {
        assertEquals("192.168.100.3" to Link.PC_TCP_PORT, Link.addressOf(" 192.168.100.3 "))
        assertEquals("192.168.100.3" to 5000, Link.addressOf("192.168.100.3:5000"))
        assertEquals("192.168.100.3" to Link.PC_TCP_PORT, Link.addressOf("http://192.168.100.3/"))
        assertNull(Link.addressOf("  "))
    }
}
