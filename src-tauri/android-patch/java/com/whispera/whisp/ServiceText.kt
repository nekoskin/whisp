package com.whispera.whisp

import android.content.Context
import java.io.File

data class ServiceText(
    val active: String,
    val failedTitle: String,
    val failedBody: String,
    val interrupted: String,
    val eventsChannel: String,
    val permissionDenied: String,
) {
    companion object {
        private const val LANG_FILE = "ui_lang"
        private const val DEFAULT_LANG = "ru"

        private val BY_LANG = mapOf(
            "ru" to ServiceText(
                active = "VPN активен",
                failedTitle = "Whisp VPN — ошибка",
                failedBody = "Ошибка подключения",
                interrupted = "Соединение прервано",
                eventsChannel = "Whisp VPN события",
                permissionDenied = "Нет разрешения на VPN",
            ),
            "en" to ServiceText(
                active = "VPN is on",
                failedTitle = "Whisp VPN — error",
                failedBody = "Connection failed",
                interrupted = "Connection lost",
                eventsChannel = "Whisp VPN events",
                permissionDenied = "VPN permission not granted",
            ),
            "zh" to ServiceText(
                active = "VPN 已开启",
                failedTitle = "Whisp VPN — 错误",
                failedBody = "连接失败",
                interrupted = "连接已断开",
                eventsChannel = "Whisp VPN 事件",
                permissionDenied = "未授予 VPN 权限",
            ),
            "fa" to ServiceText(
                active = "VPN روشن است",
                failedTitle = "Whisp VPN — خطا",
                failedBody = "اتصال ناموفق بود",
                interrupted = "اتصال قطع شد",
                eventsChannel = "رویدادهای Whisp VPN",
                permissionDenied = "مجوز VPN داده نشد",
            ),
        )

        fun save(context: Context, lang: String) {
            File(context.filesDir, LANG_FILE).writeText(lang)
        }

        fun of(context: Context): ServiceText {
            val lang = runCatching { File(context.filesDir, LANG_FILE).readText().trim() }.getOrNull()
            return BY_LANG[lang] ?: BY_LANG.getValue(DEFAULT_LANG)
        }
    }
}
