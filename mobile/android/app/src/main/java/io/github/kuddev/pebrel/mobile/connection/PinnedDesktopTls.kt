package io.github.kuddev.pebrel.mobile.connection

import android.util.Base64
import okhttp3.OkHttpClient
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.X509TrustManager

/** A scanned invitation can pin a private certificate without trusting other self-signed servers. */
internal object PinnedDesktopTls {
    fun client(base: OkHttpClient, pin: String): OkHttpClient {
        val expected = Base64.decode(pin.removePrefix("sha256/"), Base64.NO_WRAP)
        require(expected.size == 32)
        val trust = object : X509TrustManager {
            override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
            override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {
                throw CertificateException("Client certificates are not accepted")
            }
            override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {
                val leaf = chain.firstOrNull() ?: throw CertificateException("Missing server certificate")
                leaf.checkValidity()
                val actual = MessageDigest.getInstance("SHA-256").digest(leaf.publicKey.encoded)
                if (!MessageDigest.isEqual(expected, actual)) throw PairingCertificateChanged()
            }
        }
        val context = SSLContext.getInstance("TLS").apply { init(null, arrayOf(trust), null) }
        // OkHttp's normal SAN/hostname verification remains enabled.
        return base.newBuilder().sslSocketFactory(context.socketFactory, trust).build()
    }
}
