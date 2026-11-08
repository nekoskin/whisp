package com.whispera.whisp

import android.content.Context

object Autostart {
    private const val PREFS = "whisp_autostart"
    private const val ENABLED = "enabled"
    private const val LEGACY_ENABLED = "auto_connect"

    @JvmStatic fun set(ctx: Context, on: Boolean): Boolean =
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(ENABLED, on).commit()

    @JvmStatic fun isOn(ctx: Context): Boolean {
        val prefs = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        if (prefs.contains(ENABLED)) return prefs.getBoolean(ENABLED, false)
        return ctx.getSharedPreferences(WhispVpnService.PREFS, Context.MODE_PRIVATE)
            .getBoolean(LEGACY_ENABLED, false)
    }
}
