package com.whispera.whisp

import android.app.Activity
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.util.Log

object WhispVpnPrep {
    const val REQ_CODE = 1717

    @Volatile private var currentActivity: Activity? = null

    @Volatile var hasPending: Boolean = false
    @Volatile private var pendingRulesJson: String = ""
    @Volatile private var pendingConnKey: String = ""
    @Volatile private var pendingVpnDns: String = "1.1.1.1"
    @Volatile private var pendingIpv6: Boolean = true
    @Volatile private var pendingHwid: Boolean = true
    @Volatile private var pendingTlsFingerprint: String = ""
    @Volatile private var pendingMixedPort: Int = 0
    @Volatile private var pendingAllowLan: Boolean = false
    @Volatile private var pendingSocksUser: String = ""
    @Volatile private var pendingSocksPass: String = ""
    @Volatile private var pendingDnsMode: String = "tcp"
    @Volatile private var pendingDnsStrategy: String = "fakeip"
    @Volatile private var pendingMtu: Int = 1500
    @Volatile private var pendingTlsFragment: Boolean = false
    @Volatile private var pendingAutoConnect: Boolean = false
    @Volatile private var pendingTunStack: String = "gvisor"
    @Volatile private var pendingQuic: Boolean = false
    @Volatile private var pendingServerIpv6: String = "auto"

    @JvmStatic fun setActivity(a: Activity?) { currentActivity = a }

    @JvmStatic fun isPrepared(): Boolean {
        val a = currentActivity ?: return false
        return VpnService.prepare(a) == null
    }

    @JvmStatic fun requestPermission(): Int {
        val a = currentActivity ?: return -1
        val intent: Intent? = try { VpnService.prepare(a) } catch (t: Throwable) {
            Log.e("WhispVpnPrep", "prepare failed", t); return -2
        }
        if (intent == null) return 1
        try {
            a.runOnUiThread { a.startActivityForResult(intent, REQ_CODE) }
            return 0
        } catch (t: Throwable) {
            Log.e("WhispVpnPrep", "startActivityForResult failed", t); return -3
        }
    }

    @JvmStatic fun savePending(rulesJson: String, connKey: String, vpnDns: String, ipv6: Boolean, hwid: Boolean, tlsFingerprint: String, mixedPort: Int, allowLan: Boolean, socksUser: String, socksPass: String, dnsMode: String, dnsStrategy: String, mtu: Int, tlsFragment: Boolean, autoConnect: Boolean, tunStack: String, quic: Boolean, serverIpv6: String) {
        pendingRulesJson = rulesJson
        pendingConnKey   = connKey
        pendingVpnDns    = vpnDns.ifEmpty { "1.1.1.1" }
        pendingIpv6      = ipv6
        pendingHwid      = hwid
        pendingTlsFingerprint = tlsFingerprint
        pendingMixedPort = mixedPort
        pendingAllowLan  = allowLan
        pendingSocksUser = socksUser
        pendingSocksPass = socksPass
        pendingDnsMode   = dnsMode.ifEmpty { "tcp" }
        pendingDnsStrategy = dnsStrategy.ifEmpty { "fakeip" }
        pendingMtu       = if (mtu in 576..9000) mtu else 1500
        pendingTlsFragment = tlsFragment
        pendingAutoConnect = autoConnect
        pendingTunStack  = if (tunStack in listOf("system", "gvisor", "mixed")) tunStack else "gvisor"
        pendingQuic      = quic
        pendingServerIpv6 = serverIpv6
        hasPending       = true
        Log.d("WhispVpnPrep", "savePending: key=${connKey.take(6)}… dns=$vpnDns ipv6=$ipv6 hwid=$hwid fp=$tlsFingerprint mixedPort=$mixedPort allowLan=$allowLan dnsMode=$pendingDnsMode dnsStrategy=$pendingDnsStrategy mtu=$pendingMtu tlsFragment=$pendingTlsFragment")
    }

    // Android's own "Block connections without VPN" is the only thing that keeps
    // traffic in when this process dies, and no app can switch it on for the
    // user — so report its real state and take them to it.
    @JvmStatic fun isLockdown(): Boolean {
        val a = currentActivity ?: return false
        val p = a.getSharedPreferences(WhispVpnService.PREFS, Context.MODE_PRIVATE)
        return p.getBoolean("lockdown", false)
    }

    @JvmStatic fun setAutostart(on: Boolean): Boolean {
        val a = currentActivity ?: return false
        return Autostart.set(a, on)
    }

    @JvmStatic fun setLanguage(lang: String): Boolean {
        val a = currentActivity ?: return false
        ServiceText.save(a, lang)
        val nm = a.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (nm.activeNotifications.any { it.id == WhispVpnService.NOTIFICATION_ID }) {
            nm.notify(WhispVpnService.NOTIFICATION_ID, WhispVpnService.buildVpnNotification(a))
        }
        return true
    }

    @JvmStatic fun openVpnSettings(): Int {
        val a = currentActivity ?: return -1
        return try {
            a.startActivity(Intent("android.net.vpn.SETTINGS").apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            })
            0
        } catch (t: Throwable) {
            try {
                a.startActivity(Intent(android.provider.Settings.ACTION_SETTINGS).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                })
                0
            } catch (t2: Throwable) {
                Log.e("WhispVpnPrep", "openVpnSettings failed", t2); -2
            }
        }
    }

    @JvmStatic fun startPending(ctx: Context) {
        if (!hasPending) return
        hasPending = false
        Log.d("WhispVpnPrep", "startPending → launching WhispVpnService")
        val intent = Intent(ctx, WhispVpnService::class.java).apply {
            action = WhispVpnService.ACTION_START
            putExtra(WhispVpnService.EXTRA_CONN_KEY,   pendingConnKey)
            putExtra(WhispVpnService.EXTRA_RULES_JSON, pendingRulesJson)
            putExtra(WhispVpnService.EXTRA_VPN_DNS,    pendingVpnDns)
            putExtra(WhispVpnService.EXTRA_IPV6,       if (pendingIpv6) "1" else "0")
            putExtra(WhispVpnService.EXTRA_HWID,       if (pendingHwid) "1" else "0")
            putExtra(WhispVpnService.EXTRA_TLS_FINGERPRINT, pendingTlsFingerprint)
            putExtra(WhispVpnService.EXTRA_MIXED_PORT, pendingMixedPort.toString())
            putExtra(WhispVpnService.EXTRA_ALLOW_LAN,  if (pendingAllowLan) "1" else "0")
            putExtra(WhispVpnService.EXTRA_SOCKS_USER, pendingSocksUser)
            putExtra(WhispVpnService.EXTRA_SOCKS_PASS, pendingSocksPass)
            putExtra(WhispVpnService.EXTRA_DNS_MODE,   pendingDnsMode)
            putExtra(WhispVpnService.EXTRA_DNS_STRATEGY, pendingDnsStrategy)
            putExtra(WhispVpnService.EXTRA_MTU,        pendingMtu.toString())
            putExtra(WhispVpnService.EXTRA_TLS_FRAGMENT, if (pendingTlsFragment) "1" else "0")
            putExtra(WhispVpnService.EXTRA_AUTO_CONNECT, if (pendingAutoConnect) "1" else "0")
            putExtra(WhispVpnService.EXTRA_TUN_STACK, pendingTunStack)
            putExtra(WhispVpnService.EXTRA_QUIC, if (pendingQuic) "1" else "0")
            putExtra(WhispVpnService.EXTRA_SERVER_IPV6, pendingServerIpv6)
        }
        ctx.startForegroundService(intent)
    }
}
