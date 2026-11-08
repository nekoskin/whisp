package com.whispera.whisp

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

class BootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent?) {
        val receivedAction = intent?.action ?: return
        if (receivedAction != Intent.ACTION_BOOT_COMPLETED &&
            receivedAction != "android.intent.action.QUICKBOOT_POWERON"
        ) return

        val p = ctx.getSharedPreferences(WhispVpnService.PREFS, Context.MODE_PRIVATE)
        val key = p.getString("conn_key", "") ?: ""
        val auto = Autostart.isOn(ctx)
        if (key.isEmpty() || !auto) {
            Log.i("BootReceiver", "boot: skip (key empty=${key.isEmpty()} auto=$auto)")
            return
        }

        Log.i("BootReceiver", "boot: starting VPN (autostart)")
        val svc = Intent(ctx, WhispVpnService::class.java).apply {
            action = WhispVpnService.ACTION_BOOT
        }
        try {
            ctx.startForegroundService(svc)
        } catch (t: Throwable) {
            Log.w("BootReceiver", "startForegroundService failed: ${t.message}")
        }
    }
}
