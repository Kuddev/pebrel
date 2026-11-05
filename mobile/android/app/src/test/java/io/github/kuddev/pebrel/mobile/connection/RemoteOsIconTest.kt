package io.github.kuddev.pebrel.mobile.connection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class RemoteOsIconTest {
    @Test fun distributionsAndTheirDeclaredFamiliesPickCatalogIcons() {
        assertEquals("ubuntu", remoteOsIcon("Linux", "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n"))
        assertEquals("ubuntu", remoteOsIcon("Linux", "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n"))
        assertEquals("rhel", remoteOsIcon("Linux", "ID=\"almalinux\"\nID_LIKE=\"rhel centos fedora\"\n"))
        assertEquals("suse", remoteOsIcon("Linux", "ID=opensuse-leap\nID_LIKE=\"suse opensuse\"\n"))
        assertEquals("rpi", remoteOsIcon("Linux", "ID=raspbian\nID_LIKE=debian\n"))
        assertEquals("linux", remoteOsIcon("Linux", "ID=gentoo\n"))
        assertEquals("macos", remoteOsIcon("Darwin", ""))
    }

    @Test fun roleIconsAreNotReplacedBySystemDetection() {
        listOf("docker", "server", "cloud", "rpi").forEach { assertFalse(it in systemOsIcons) }
    }
}
