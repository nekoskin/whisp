import { invoke } from "@tauri-apps/api/core";
import { onBackButtonPress } from "@tauri-apps/api/app";
import type { PluginListener } from "@tauri-apps/api/core";
import { writeText as clipboardWrite, readText as clipboardRead } from "@tauri-apps/plugin-clipboard-manager";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { readTextFile } from "@tauri-apps/plugin-fs";
import { scan as qrScan, cancel as qrCancel, Format as QrFormat, checkPermissions as qrCheckPermissions, requestPermissions as qrRequestPermissions } from "@tauri-apps/plugin-barcode-scanner";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { renderSVG as renderQrSvg } from "uqr";
import "./styles.css";

const ICONS = {
  power: `<svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18.36 6.64a9 9 0 1 1-12.73 0"/><line x1="12" y1="2" x2="12" y2="12"/></svg>`,
  plus: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>`,
  chevronDown: `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="6 9 12 15 18 9"/></svg>`,
  bolt: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>`,
  home: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>`,
  user: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>`,
  log: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>`,
  settings: `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>`,
  refresh: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>`,
  copy: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`,
  play: `<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>`,
  x: `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>`,
  link: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>`,
  ping: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/></svg>`,
  pencil: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>`,
  kebab: `<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/></svg>`,
  clipboard: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="8" y="2" width="8" height="4" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/></svg>`,
  qr: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><line x1="14" y1="14" x2="14" y2="21"/><line x1="21" y1="14" x2="21" y2="21"/><line x1="17.5" y1="14" x2="17.5" y2="17.5"/><line x1="14" y1="17.5" x2="21" y2="17.5"/></svg>`,
  eye: `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>`,
  eyeOff: `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></svg>`,
  code: `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>`,
};

interface AppSettings {
  conn_key: string;
  auto_connect: boolean;
  theme: string;
  mihomo_port: number;
  socks_addr: string;
  kill_switch: boolean;
  dns_redirect: boolean;
  ipv6: boolean;
  ipv6_fallback?: boolean;
  ipv6_only?: boolean;
  tun_stack: string;
  hwid: boolean;
  auth_tip: boolean;
  secret: string;
  custom_dns?: string[];
  vpn_dns?: string;
  tls_fingerprint?: string;
  bypass_ru?: boolean;
  quic?: boolean;
  tun_stack_android?: string;
  ping_mode?: string;
  socks_user?: string;
  socks_pass?: string;
  allow_lan?: boolean;
  log_level?: string;
  routing_mode?: string;

  external_link?: string;
  dns_mode?: string;
  dns_strategy?: string;
  mtu?: number;
  tls_fragment?: boolean;
  sub_auto_update?: boolean;
  proxy_mode?: string;
}

interface Profile { id: string; name: string; key: string; }

interface Subscription {
  id: string;
  name: string;
  url: string;
  keys: string[];
  servers: unknown[];
  updated: string;
}

type Page = "home" | "routing" | "logs" | "settings";
type Lang = "ru" | "en" | "zh" | "fa";

interface RoutingRule {
  id: string;
  kind: "domain" | "process" | "domain-keyword" | "domain-full" | "ip";
  value: string;
  action: "DIRECT" | "PROXY" | "REJECT";
}

const i18n: Record<Lang, Record<string, string>> = {
  ru: {
    home: "Главная", routing: "Маршруты", logs: "Журнал", settings: "Настройки",
    disconnected: "Отключено", connected: "Подключено",
    keyPlaceholder: "Вставьте ключ...", noKeySelected: "Нет ключа", connect: "Connect", disconnect: "Disconnect",
    system: "СИСТЕМА",
    addProfile: "Добавить ключ",
    keysSection: "Ключи", noKeys: "Нет сохранённых ключей и подписок",
    pasteFromClipboard: "Вставить из буфера обмена", qrScanFailed: "Не удалось отсканировать QR-код", pasteJson: "Импорт JSON из файла", copyJson: "Копировать JSON",
    qrCode: "QR-код", qrTabShare: "Поделиться", qrTabScan: "Сканировать", qrScanHint: "Наведите камеру на QR-код", shareQrHint: "На другом устройстве: «+» → «QR-код» → «Сканировать»",
    shareQrWarning: "Кто увидит этот код, получит доступ к вашему ключу", shareQrTooLong: "Превышена допустимая длина QR-кода. Используйте «Копировать»",
    nothingToShare: "Нет доступных ключей", shareKindKey: "Ключ", shareKindSub: "Подписка", close: "Закрыть",
    jsonParseError: "Некорректный JSON", jsonImported: "Импортировано записей:", jsonCopied: "JSON скопирован в буфер обмена",
    clipboardReadFailed: "Не удалось прочитать буфер обмена", clipboardWriteFailed: "Не удалось скопировать в буфер обмена",
    pasteUnrecognized: "Не удалось распознать содержимое. Ожидается whispera:// ключ или https:// ссылка",
    refreshAllSubs: "Обновить подписки", subsRefreshedSummary: "Обновлено подписок:",
    subAutoUpdateOn: "Автообновление: Вкл", subAutoUpdateOff: "Автообновление: Выкл",
    systemLog: "Системный журнал", logReady: "Система готова. Ожидание логов...",
    mixedPort: "Смешанный порт ", bindAddr: "Привязать адрес ", tunStack: "Tun Stack ", shareProxy: "Общий доступ к прокси",
    theme: "Тема ", dark: "Тёмная", auto: "Белая", dnsRedirect: "DNS перенаправление ",
    ipv6Label: "IPv6 ", secretLabel: "Secret ", copy: "Копировать",
    ipv6FallbackLabel: "IPv6, если IPv4 сервера недоступен ", ipv6OnlyLabel: "К серверу только по IPv6 ",
    autostart: "Автозапуск ",
    config: "Конфиг ", open: "Открыть", update: "Обновить ",
    openRepo: "Открыть репо", checkUpdates: "Проверить обновления", checking: "Проверка",
    installed: "Установлено в актуальной версии",
    profileName: "Имя профиля", profileKey: "Ключ подключения",
    save: "Сохранить", cancel: "Отмена",
    mihomo: "MIHOMO", whisp: "WHISP",
    clearLogs: "Очистить", active: "Активно",
    connecting: "Подключение...",
    fail_network_down: "Нет интернета: не отвечает даже российский адрес", fail_foreign_cut: "Работает только российский интернет: зарубежные адреса недоступны", fail_server_unreachable: "Сервер не отвечает, хотя остальной интернет работает: его IP заблокирован или сервер выключен", fail_server_refused: "Сервер отклоняет подключения: служба на нём не запущена", fail_handshake_blocked: "Подключение блокируется по пути", fail_live_blocked: "Соединения обрываются во время передачи", wire_forged_reset: "подменённый сброс (RST)", wire_hello_dropped: "приветствие TLS не доходит", wire_cut_off: "обрыв посреди рукопожатия", wire_retransmitted: "пакеты теряются", failFreezeAt: "заморозка после {kb} КБ", failNewPattern: "такого типа блокировки не было в обучении", failWhileConnected: "Новые подключения не проходят",
    killSwitch: "Kill Switch",
    routingTitle: "Маршруты",
    domain: "Домен", app: "Приложение",
    addSubscription: "Добавить подписку",
    subName: "Название", subUrl: "URL подписки", keyJson: "Показать JSON", keyJsonTitle: "Конфигурация ключа", keyJsonForeign: "Чужой протокол — только просмотр", keyJsonBad: "Не удалось разобрать ключ", keyJsonSaved: "Ключ обновлён", subUrlHint: "https://server/sub/TOKEN",
    noSubscriptions: "Нет подписок", subRefreshing: "Обновление...", subAdded: "Подписка добавлена",
    subRefresh: "Обновить", subDelete: "Удалить",
    subSelectKey: "Выбрать ключ", subRename: "Переименовать", more: "Ещё",
    pingWhileOn: "Замер доступен только при отключённом туннеле", pingKey: "Пинг", pingAll: "Пинг всех", pingMs: "мс", pingTimeout: "timeout", pingError: "ошибка", pingRunning: "...",
    loading: "Загрузка…",
    copied: "Скопировано",
    vpnConnected: "Подключено",
    switchingKey: "Переключение ключа...", pleaseWait: "Подождите, идёт переключение...",
    vpnDisconnected: "Отключено",
    ruleBlock: "Блок",
    ruleDirect: "Прямой",
    ruleSite: "домен",
    ruleKeyword: "ключ",
    ruleApp: "приложение",
    ruleNoRules: "Нет правил",
    ruleViaVpn: "Прокси",
    ruleDirect2: "Напрямую",
    ruleAdd: "Добавить правило",
    ruleSiteType: "Домен",
    ruleAppType: "Приложение",
    ruleKeywordType: "Ключевое слово",
    rulePlaceholder: "например: youtube.com",
    ruleKeywordPlaceholder: "ключевое слово",
    rulesBrowseExe: "Выбрать .exe",
    rulesRunning: "Из процессов",
    rulesPickApp: "Выбрать приложение",
    rulesSearchApp: "Поиск приложения...",
    rulesTitle: "Правила",
    rulesSearchProcess: "Поиск процесса...",
    rulesNoProcesses: "Нет процессов",
    subUpdated: "Подписка обновлена",
    subUpdateAvailableToast: "Доступно обновление подписки",
    subUpdateFailed: "Ошибка обновления",
    secretCopied: "Secret скопирован",
    reconnectToApply: "Изменение вступит в силу после переподключения",
    dnsServers: "DNS для прямых соединений",
    dnsCommaSep: "По умолчанию пусто",
    vpnDns: "Укажите DNS:",
    dnsMode: "Режим DNS",
    dnsStrategy: "DNS-резолвер",
    dnsFakeip: "Поддельный",
    dnsLocal: "Локальный",
    killSwitchLabel: "Kill Switch",
    killSwitchOpen: "Настройки системы",
    modeProxy: "Прокси",
    tunStackLabel: "Сетевой стек",
    quicLabel: "Разрешить QUIC",
    pingModeLabel: "Способ проверки",
    mtuLabel: "MTU",
    tlsFragment: "Фрагментация TLS",
    isp: "Провайдер",
    dnsService: "Сервис", dnsPrimary: "Основной", dnsSecondary: "Дополнительный", dnsCustom: "Свой", dnsCustomHint: "IP или https://…",
    bypassMode: "Режим обхода",
    bypassModeHint: "GeoIP и домены вашей страны автоматически идут напрямую.",
    advanced: "Расширенные",
    allowLan: "Разрешить LAN",
    allowLanHint: "Другие устройства в сети смогут использовать прокси",
    routingMode: "Режим маршрутизации",
    socksAuth: "SOCKS5 аутентификация",
    socksUser: "Логин",
    socksPass: "Пароль",
    socksShow: "Показать пароль",
    socksHide: "Скрыть пароль",
    socksSave: "Сохранить",
    reconnectRequired: "Переподключитесь для применения правила",
    logSearchPlaceholder: "Поиск в логах...",
    logAll: "Все",
    updateAvailableNew: "Доступно обновление",
    installUpdate: "Установить",
    openRelease: "Открыть релиз",
    currentVersion: "Текущая версия",
  },
  en: {
    home: "Home", routing: "Routing", logs: "Logs", settings: "Settings",
    disconnected: "Disconnected", connected: "Connected",
    keyPlaceholder: "Paste key...", noKeySelected: "No key", connect: "Connect", disconnect: "Disconnect",
    system: "SYSTEM",
    addProfile: "Add key",
    keysSection: "Keys", noKeys: "No saved keys or subscriptions",
    pasteFromClipboard: "Paste from clipboard", qrScanFailed: "Failed to scan QR code", pasteJson: "Import JSON file", copyJson: "Copy JSON",
    qrCode: "QR code", qrTabShare: "Share", qrTabScan: "Scan", qrScanHint: "Point the camera at a QR code", shareQrHint: "On the other device: + → QR code → Scan",
    shareQrWarning: "Anyone who sees this code gets access to your key", shareQrTooLong: "Exceeds the maximum QR code length. Use Copy",
    nothingToShare: "No keys available", shareKindKey: "Key", shareKindSub: "Subscription", close: "Close",
    jsonParseError: "Invalid JSON", jsonImported: "Imported entries:", jsonCopied: "JSON copied to clipboard",
    clipboardReadFailed: "Failed to read clipboard", clipboardWriteFailed: "Failed to copy to clipboard",
    pasteUnrecognized: "Could not recognize the content. Expected a whispera:// key or an https:// link",
    refreshAllSubs: "Refresh subscriptions", subsRefreshedSummary: "Subscriptions refreshed:",
    subAutoUpdateOn: "Auto-update: On", subAutoUpdateOff: "Auto-update: Off",
    systemLog: "System Log", logReady: "System ready. Waiting for logs...",
    mixedPort: "Mixed port ", bindAddr: "Bind address ", tunStack: "Tun Stack ", shareProxy: "Share proxy",
    theme: "Theme ", dark: "Dark", auto: "Light", dnsRedirect: "DNS redirect ",
    ipv6Label: "IPv6 ", secretLabel: "Secret ", copy: "Copy",
    ipv6FallbackLabel: "IPv6 if the server's IPv4 is blocked ", ipv6OnlyLabel: "Server over IPv6 only ",
    autostart: "Autostart ",
    config: "Config ", open: "Open", update: "Update ",
    openRepo: "Open repo", checkUpdates: "Check updates", checking: "Checking",
    installed: "Latest version installed",
    profileName: "Profile name", profileKey: "Connection key",
    save: "Save", cancel: "Cancel",
    mihomo: "MIHOMO", whisp: "WHISP",
    clearLogs: "Clear", active: "Active",
    connecting: "Connecting...",
    fail_network_down: "No internet: even the domestic control address does not answer", fail_foreign_cut: "Only domestic internet works: foreign addresses are unreachable", fail_server_unreachable: "The server does not answer while the rest of the internet does: its IP is blocked or the server is off", fail_server_refused: "The server refuses connections: its service is not running", fail_handshake_blocked: "The connection is blocked on the way", fail_live_blocked: "Connections are cut while carrying data", wire_forged_reset: "forged reset (RST)", wire_hello_dropped: "the TLS hello does not get through", wire_cut_off: "cut off mid-handshake", wire_retransmitted: "packets are lost", failFreezeAt: "freeze after {kb} KB", failNewPattern: "a block type not seen in training", failWhileConnected: "New connections fail",
    killSwitch: "Kill Switch",
    routingTitle: "Routing",
    domain: "Domain", app: "Application",
    addSubscription: "Add subscription",
    subName: "Name", subUrl: "Subscription URL", keyJson: "Show JSON", keyJsonTitle: "Key configuration", keyJsonForeign: "Foreign protocol — view only", keyJsonBad: "Could not parse the key", keyJsonSaved: "Key updated", subUrlHint: "https://server/sub/TOKEN",
    noSubscriptions: "No subscriptions", subRefreshing: "Refreshing...", subAdded: "Subscription added",
    subRefresh: "Refresh", subDelete: "Delete",
    subSelectKey: "Use key", subRename: "Rename", more: "More",
    pingWhileOn: "Measurement works only while disconnected", pingKey: "Ping", pingAll: "Ping all", pingMs: "ms", pingTimeout: "timeout", pingError: "error", pingRunning: "...",
    loading: "Loading…",
    copied: "Copied",
    vpnConnected: "Connected",
    switchingKey: "Switching key...", pleaseWait: "Please wait, switching in progress...",
    vpnDisconnected: "Disconnected",
    ruleBlock: "Block",
    ruleDirect: "Direct",
    ruleSite: "domain",
    ruleKeyword: "keyword",
    ruleApp: "app",
    ruleNoRules: "No rules",
    ruleViaVpn: "Proxy",
    ruleDirect2: "Direct",
    ruleAdd: "Add rule",
    ruleSiteType: "Domain",
    ruleAppType: "App",
    ruleKeywordType: "Keyword",
    rulePlaceholder: "e.g. youtube.com",
    ruleKeywordPlaceholder: "keyword",
    rulesBrowseExe: "Browse .exe",
    rulesRunning: "Running",
    rulesPickApp: "Pick App",
    rulesSearchApp: "Search app...",
    rulesTitle: "Rules",
    rulesSearchProcess: "Search process...",
    rulesNoProcesses: "No processes",
    subUpdated: "Subscription updated",
    subUpdateAvailableToast: "Subscription update available",
    subUpdateFailed: "Update failed",
    secretCopied: "Secret copied",
    reconnectToApply: "Change will take effect after reconnecting",
    dnsServers: "Direct DNS",
    dnsCommaSep: "Empty by default",
    vpnDns: "Set your DNS:",
    dnsMode: "DNS mode",
    dnsStrategy: "DNS resolver",
    dnsFakeip: "Fake-IP",
    dnsLocal: "Local",
    killSwitchLabel: "Kill switch",
    killSwitchOpen: "System settings",
    modeProxy: "Proxy",
    tunStackLabel: "Network stack",
    quicLabel: "Allow QUIC",
    pingModeLabel: "Check method",
    mtuLabel: "MTU",
    tlsFragment: "TLS fragmentation",
    isp: "ISP",
    dnsService: "Service", dnsPrimary: "Primary", dnsSecondary: "Secondary", dnsCustom: "Custom", dnsCustomHint: "IP or https://…",
    bypassMode: "Bypass Mode",
    bypassModeHint: "GeoIP and domains from your country are automatically routed to direct.",
    advanced: "Advanced",
    allowLan: "Allow LAN",
    allowLanHint: "Other devices on the network can use this proxy",
    routingMode: "Routing Mode",
    socksAuth: "SOCKS5 Authentication",
    socksUser: "Username",
    socksPass: "Password",
    socksShow: "Show password",
    socksHide: "Hide password",
    socksSave: "Save",
    reconnectRequired: "Reconnect to apply the rule",
    logSearchPlaceholder: "Search logs...",
    logAll: "All",
    updateAvailableNew: "Update Available",
    installUpdate: "Install",
    openRelease: "Open Release",
    currentVersion: "Current version",
  },
  zh: {
    home: "主页", routing: "路由", logs: "日志", settings: "设置",
    disconnected: "已断开", connected: "已连接",
    keyPlaceholder: "粘贴密钥...", noKeySelected: "无密钥", connect: "连接", disconnect: "断开",
    system: "系统",
    addProfile: "添加密钥",
    keysSection: "密钥", noKeys: "没有已保存的密钥或订阅",
    pasteFromClipboard: "从剪贴板粘贴", qrScanFailed: "二维码扫描失败", pasteJson: "从文件导入 JSON", copyJson: "复制 JSON",
    qrCode: "二维码", qrTabShare: "分享", qrTabScan: "扫描", qrScanHint: "将相机对准二维码", shareQrHint: "在另一台设备上：+ → 二维码 → 扫描",
    shareQrWarning: "看到此二维码的人即可使用您的密钥", shareQrTooLong: "超出二维码最大长度，请使用“复制”",
    nothingToShare: "没有可用的密钥", shareKindKey: "密钥", shareKindSub: "订阅", close: "关闭",
    jsonParseError: "JSON 格式无效", jsonImported: "已导入条目：", jsonCopied: "JSON 已复制到剪贴板",
    clipboardReadFailed: "读取剪贴板失败", clipboardWriteFailed: "复制到剪贴板失败",
    pasteUnrecognized: "无法识别内容。需要 whispera:// 密钥或 https:// 链接",
    refreshAllSubs: "更新订阅", subsRefreshedSummary: "已更新订阅：",
    subAutoUpdateOn: "自动更新：开", subAutoUpdateOff: "自动更新：关",
    systemLog: "系统日志", logReady: "系统就绪，等待日志...",
    mixedPort: "混合端口", bindAddr: "绑定地址", tunStack: "Tun堆栈", shareProxy: "共享代理",
    theme: "主题", dark: "深色", auto: "浅色", dnsRedirect: "DNS重定向",
    ipv6Label: "IPv6", secretLabel: "密钥", copy: "复制",
    ipv6FallbackLabel: "服务器 IPv4 被封时改用 IPv6", ipv6OnlyLabel: "仅通过 IPv6 连接服务器",
    autostart: "自动启动",
    config: "配置", open: "打开", update: "更新",
    openRepo: "打开仓库", checkUpdates: "检查更新", checking: "检查中",
    installed: "已安装最新版本",
    profileName: "配置名称", profileKey: "连接密钥",
    save: "保存", cancel: "取消",
    mihomo: "MIHOMO", whisp: "WHISP",
    clearLogs: "清空", active: "活跃",
    connecting: "连接中...",
    fail_network_down: "没有网络：连国内的检测地址也无响应", fail_foreign_cut: "只有国内网络可用：国外地址无法访问", fail_server_unreachable: "服务器无响应，但其他网络正常：其 IP 被封锁或服务器已关闭", fail_server_refused: "服务器拒绝连接：其服务未运行", fail_handshake_blocked: "连接在途中被阻断", fail_live_blocked: "连接在传输数据时被切断", wire_forged_reset: "伪造的重置 (RST)", wire_hello_dropped: "TLS 握手消息无法送达", wire_cut_off: "握手中途被切断", wire_retransmitted: "数据包丢失", failFreezeAt: "{kb} KB 后被冻结", failNewPattern: "训练中未见过的封锁类型", failWhileConnected: "新连接无法建立",
    killSwitch: "终止开关",
    routingTitle: "路由",
    domain: "域名", app: "应用",
    addSubscription: "添加订阅",
    subName: "名称", subUrl: "订阅URL", keyJson: "查看 JSON", keyJsonTitle: "密钥配置", keyJsonForeign: "外部协议 — 仅查看", keyJsonBad: "无法解析密钥", keyJsonSaved: "密钥已更新", subUrlHint: "https://server/sub/TOKEN",
    noSubscriptions: "无订阅", subRefreshing: "更新中...", subAdded: "订阅已添加",
    subRefresh: "刷新", subDelete: "删除",
    subSelectKey: "使用密钥", subRename: "重命名", more: "更多",
    pingWhileOn: "仅在断开连接时可测量", pingKey: "延迟", pingAll: "全部延迟", pingMs: "毫秒", pingTimeout: "超时", pingError: "错误", pingRunning: "...",
    loading: "加载中…",
    copied: "已复制",
    vpnConnected: "已连接",
    switchingKey: "正在切换密钥...", pleaseWait: "请稍候，正在切换...",
    vpnDisconnected: "已断开",
    ruleBlock: "拦截",
    ruleDirect: "直连",
    ruleSite: "域名",
    ruleKeyword: "关键词",
    ruleApp: "应用",
    ruleNoRules: "无规则",
    ruleViaVpn: "Proxy",
    ruleDirect2: "直连",
    ruleAdd: "添加规则",
    ruleSiteType: "域名",
    ruleAppType: "应用",
    ruleKeywordType: "关键词",
    rulePlaceholder: "例如：youtube.com",
    ruleKeywordPlaceholder: "关键词",
    rulesBrowseExe: "浏览 .exe",
    rulesRunning: "运行中",
    rulesPickApp: "选择应用",
    rulesSearchApp: "搜索应用...",
    rulesTitle: "规则",
    rulesSearchProcess: "搜索进程...",
    rulesNoProcesses: "无进程",
    subUpdated: "订阅已更新",
    subUpdateAvailableToast: "订阅有可用更新",
    subUpdateFailed: "更新失败",
    secretCopied: "Secret已复制",
    reconnectToApply: "重新连接后更改将生效",
    dnsServers: "直连 DNS",
    dnsCommaSep: "默认为空",
    vpnDns: "设置 DNS：",
    dnsMode: "DNS 模式",
    dnsStrategy: "DNS 解析器",
    dnsFakeip: "虚假 IP",
    dnsLocal: "本地",
    killSwitchLabel: "断网保护",
    killSwitchOpen: "系统设置",
    modeProxy: "代理",
    tunStackLabel: "网络栈",
    quicLabel: "允许 QUIC",
    pingModeLabel: "检测方式",
    mtuLabel: "MTU",
    tlsFragment: "TLS 分片",
    isp: "运营商",
    dnsService: "服务", dnsPrimary: "主要", dnsSecondary: "备用", dnsCustom: "自定义", dnsCustomHint: "IP 或 https://…",
    bypassMode: "绕过模式",
    bypassModeHint: "GeoIP 和您所在国家/地区的域名将自动直连。",
    advanced: "高级",
    allowLan: "允许 LAN",
    allowLanHint: "局域网其他设备可使用此代理",
    routingMode: "路由模式",
    socksAuth: "SOCKS5 认证",
    socksUser: "用户名",
    socksPass: "密码",
    socksShow: "显示密码",
    socksHide: "隐藏密码",
    socksSave: "保存",
    reconnectRequired: "重连以应用规则",
    logSearchPlaceholder: "搜索日志...",
    logAll: "全部",
    updateAvailableNew: "有可用更新",
    installUpdate: "安装",
    openRelease: "打开发布",
    currentVersion: "当前版本",
  },
  fa: {
    home: "خانه", routing: "مسیریابی", logs: "گزارش", settings: "تنظیمات",
    disconnected: "قطع شده", connected: "متصل",
    keyPlaceholder: "کلید را وارد کنید...", noKeySelected: "بدون کلید", connect: "اتصال", disconnect: "قطع",
    system: "سیستم",
    addProfile: "افزودن کلید",
    keysSection: "کلیدها", noKeys: "کلید یا اشتراکی ذخیره نشده",
    pasteFromClipboard: "جای‌گذاری از کلیپ‌بورد", qrScanFailed: "اسکن کد QR ناموفق بود", pasteJson: "وارد کردن JSON از فایل", copyJson: "کپی JSON",
    qrCode: "کد QR", qrTabShare: "اشتراک‌گذاری", qrTabScan: "اسکن", qrScanHint: "دوربین را به سمت کد QR بگیرید", shareQrHint: "در دستگاه دیگر: + ← کد QR ← اسکن",
    shareQrWarning: "هر کسی این کد را ببیند به کلید شما دسترسی پیدا می‌کند", shareQrTooLong: "از حداکثر طول کد QR بیشتر است. از «کپی» استفاده کنید",
    nothingToShare: "کلیدی در دسترس نیست", shareKindKey: "کلید", shareKindSub: "اشتراک", close: "بستن",
    jsonParseError: "JSON نامعتبر", jsonImported: "موارد وارد شده:", jsonCopied: "JSON در کلیپ‌بورد کپی شد",
    clipboardReadFailed: "خواندن کلیپ‌بورد ناموفق بود", clipboardWriteFailed: "کپی در کلیپ‌بورد ناموفق بود",
    pasteUnrecognized: "محتوا شناسایی نشد. کلید whispera:// یا لینک https:// مورد نیاز است",
    refreshAllSubs: "به‌روزرسانی اشتراک‌ها", subsRefreshedSummary: "اشتراک‌های به‌روزشده:",
    subAutoUpdateOn: "به‌روزرسانی خودکار: روشن", subAutoUpdateOff: "به‌روزرسانی خودکار: خاموش",
    systemLog: "گزارش سیستم", logReady: "سیستم آماده است. منتظر گزارش...",
    mixedPort: "پورت ترکیبی", bindAddr: "آدرس bind", tunStack: "Tun Stack", shareProxy: "اشتراک پراکسی",
    theme: "پوسته", dark: "تیره", auto: "روشن", dnsRedirect: "هدایت DNS",
    ipv6Label: "IPv6", secretLabel: "رمز", copy: "کپی",
    ipv6FallbackLabel: "IPv6 در صورت مسدود بودن IPv4 سرور", ipv6OnlyLabel: "اتصال به سرور فقط با IPv6",
    autostart: "شروع خودکار",
    config: "پیکربندی", open: "باز کردن", update: "بروزرسانی",
    openRepo: "باز کردن مخزن", checkUpdates: "بررسی بروزرسانی", checking: "در حال بررسی",
    installed: "نسخه به‌روز است",
    profileName: "نام پروفایل", profileKey: "کلید اتصال",
    save: "ذخیره", cancel: "لغو",
    mihomo: "MIHOMO", whisp: "WHISP",
    clearLogs: "پاک کردن", active: "فعال",
    connecting: "در حال اتصال...",
    fail_network_down: "اینترنت وصل نیست: حتی نشانی داخلی هم پاسخ نمی‌دهد", fail_foreign_cut: "فقط اینترنت داخلی کار می‌کند: نشانی‌های خارجی در دسترس نیستند", fail_server_unreachable: "سرور پاسخ نمی‌دهد در حالی که بقیه اینترنت کار می‌کند: IP آن مسدود است یا سرور خاموش است", fail_server_refused: "سرور اتصال را رد می‌کند: سرویس آن اجرا نمی‌شود", fail_handshake_blocked: "اتصال در مسیر مسدود می‌شود", fail_live_blocked: "اتصال‌ها هنگام انتقال داده قطع می‌شوند", wire_forged_reset: "بازنشانی جعلی (RST)", wire_hello_dropped: "پیام آغازین TLS نمی‌رسد", wire_cut_off: "قطع در میانه دست‌دهی", wire_retransmitted: "بسته‌ها گم می‌شوند", failFreezeAt: "توقف پس از {kb} کیلوبایت", failNewPattern: "نوعی مسدودسازی که در آموزش دیده نشده", failWhileConnected: "اتصال‌های جدید برقرار نمی‌شوند",
    killSwitch: "Kill Switch",
    routingTitle: "مسیریابی",
    domain: "دامنه", app: "برنامه",
    addSubscription: "افزودن اشتراک",
    subName: "نام", subUrl: "URL اشتراک", keyJson: "نمایش JSON", keyJsonTitle: "پیکربندی کلید", keyJsonForeign: "پروتکل خارجی — فقط مشاهده", keyJsonBad: "کلید قابل تجزیه نیست", keyJsonSaved: "کلید به‌روزرسانی شد", subUrlHint: "https://server/sub/TOKEN",
    noSubscriptions: "اشتراکی وجود ندارد", subRefreshing: "در حال بروزرسانی...", subAdded: "اشتراک اضافه شد",
    subRefresh: "بروزرسانی", subDelete: "حذف",
    subSelectKey: "استفاده از کلید", subRename: "تغییر نام", more: "بیشتر",
    pingWhileOn: "اندازه‌گیری فقط در حالت قطع", pingKey: "پینگ", pingAll: "پینگ همه", pingMs: "میلی‌ثانیه", pingTimeout: "تایم‌اوت", pingError: "خطا", pingRunning: "...",
    loading: "در حال بارگذاری…",
    copied: "کپی شد",
    vpnConnected: "متصل شد",
    switchingKey: "در حال تعویض کلید...", pleaseWait: "لطفاً صبر کنید، در حال تعویض...",
    vpnDisconnected: "قطع شد",
    ruleBlock: "مسدود",
    ruleDirect: "مستقیم",
    ruleSite: "دامنه",
    ruleKeyword: "کلیدواژه",
    ruleApp: "برنامه",
    ruleNoRules: "قانونی وجود ندارد",
    ruleViaVpn: "Proxy",
    ruleDirect2: "مستقیم",
    ruleAdd: "افزودن قانون",
    ruleSiteType: "دامنه",
    ruleAppType: "برنامه",
    ruleKeywordType: "کلیدواژه",
    rulePlaceholder: "مثلاً: youtube.com",
    ruleKeywordPlaceholder: "کلیدواژه",
    rulesBrowseExe: "انتخاب .exe",
    rulesRunning: "در حال اجرا",
    rulesPickApp: "انتخاب برنامه",
    rulesSearchApp: "جستجوی برنامه...",
    rulesTitle: "قوانین",
    rulesSearchProcess: "جستجوی پروسه...",
    rulesNoProcesses: "پروسه‌ای وجود ندارد",
    subUpdated: "اشتراک به‌روز شد",
    subUpdateAvailableToast: "بروزرسانی اشتراک موجود است",
    subUpdateFailed: "به‌روزرسانی ناموفق",
    secretCopied: "Secret کپی شد",
    reconnectToApply: "تغییر پس از اتصال مجدد اعمال خواهد شد",
    dnsServers: "DNS اتصال مستقیم",
    dnsCommaSep: "به‌طور پیش‌فرض خالی",
    vpnDns: "DNS خود را تنظیم کنید:",
    dnsMode: "حالت DNS",
    dnsStrategy: "رزولور DNS",
    dnsFakeip: "جعلی (Fake-IP)",
    dnsLocal: "محلی",
    killSwitchLabel: "قطع‌کن اضطراری",
    killSwitchOpen: "تنظیمات سیستم",
    modeProxy: "پراکسی",
    tunStackLabel: "پشته شبکه",
    quicLabel: "اجازه QUIC",
    pingModeLabel: "روش بررسی",
    mtuLabel: "MTU",
    tlsFragment: "قطعه‌قطعه‌سازی TLS",
    isp: "ISP",
    dnsService: "سرویس", dnsPrimary: "اصلی", dnsSecondary: "پشتیبان", dnsCustom: "سفارشی", dnsCustomHint: "IP یا https://…",
    bypassMode: "حالت دور زدن",
    bypassModeHint: "GeoIP و دامنه‌های کشور شما به‌طور خودکار مستقیم مسیردهی می‌شوند.",
    advanced: "پیشرفته",
    allowLan: "اجازه LAN",
    allowLanHint: "سایر دستگاه‌های شبکه می‌توانند از این پروکسی استفاده کنند",
    routingMode: "حالت مسیریابی",
    socksAuth: "احراز هویت SOCKS5",
    socksUser: "نام کاربری",
    socksPass: "رمز عبور",
    socksShow: "نمایش رمز",
    socksHide: "پنهان کردن رمز",
    socksSave: "ذخیره",
    reconnectRequired: "برای اعمال قانون دوباره متصل شوید",
    logSearchPlaceholder: "جستجو در گزارش‌ها...",
    logAll: "همه",
    updateAvailableNew: "به‌روزرسانی موجود است",
    installUpdate: "نصب",
    openRelease: "باز کردن انتشار",
    currentVersion: "نسخه فعلی",
  },
};

let currentPage: Page = "home";
let lang: Lang = "ru";
let isConnected = false;
// Whether the sidecars are alive, as opposed to whether the tunnel carries
// traffic. Reconnecting keys off this one: a tunnel that is briefly down
// recovers on its own, and tearing the engines down would interrupt that.
let enginesUp = false;
interface TunnelFailure { cause: string; symptom?: string; wire?: string; pattern?: number; down_bytes?: number; failures?: number }
const WARN_AFTER_FAILED_DIALS = 2;
let tunnelFailure: TunnelFailure | null = null;
let lastTunnelError = "";
let isConnecting = false;



const isAndroid = /android/i.test(navigator.userAgent);
const isWindows = /windows/i.test(navigator.userAgent);

let _notifGranted = false;
async function initNotifications(): Promise<void> {
  try {
    _notifGranted = await isPermissionGranted();
    if (!_notifGranted) {
      _notifGranted = (await requestPermission()) === "granted";
    }
  } catch { _notifGranted = false; }
}

function osNotify(title: string, body: string): void {
  if (!_notifGranted) return;
  try { sendNotification({ title, body }); } catch { /**/ }
}

let settings: AppSettings = {
  conn_key: "", auto_connect: false, theme: "dark", mihomo_port: 9887,
  socks_addr: "127.0.0.1", kill_switch: false, dns_redirect: false,
  ipv6: true, tun_stack: "Mixed", tun_stack_android: "gvisor", quic: false, ping_mode: "tcp", hwid: true, auth_tip: true, secret: "",
  dns_strategy: "fakeip", mtu: 1500, tls_fragment: false, sub_auto_update: true, proxy_mode: "tun",
};

let profiles: Profile[] = [];
let subscriptions: Subscription[] = [];
let activeRef = "";
let vpnLockdown = false;
let pingResults: Map<string, number | "pinging" | "timeout" | { err: string }> = new Map();

function refKey(ref: string): string | undefined {
  if (ref.startsWith("p:")) return profiles.find(p => p.id === ref.slice(2))?.key;
  if (ref.startsWith("s:")) {
    const [, sid, idx] = ref.split(":");
    return subscriptions.find(s => s.id === sid)?.keys[Number(idx)];
  }
  return undefined;
}

function activeKeyValue(): string {
  return (settings.external_link || settings.conn_key || "").trim();
}

function normalizeActiveRef(): void {
  const k = activeKeyValue();
  if (!k) { activeRef = ""; return; }
  if (activeRef && refKey(activeRef) === k) return;
  const p = profiles.find(p => p.key === k);
  if (p) { activeRef = "p:" + p.id; return; }
  for (const s of subscriptions) {
    const i = (s.keys || []).indexOf(k);
    if (i >= 0) { activeRef = "s:" + s.id + ":" + i; return; }
  }
  activeRef = "";
}
let subUpdateAvailable: Set<string> = new Set();
let subAutoCheckTimer: ReturnType<typeof setInterval> | null = null;
let routingRules: RoutingRule[] = [];
let blocklistRules: RoutingRule[] = [];
let logLines: string[] = [];
let logTimes: number[] = [];
let connectTime: number | null = null;
let sysInfo = { os: "—", uptime: "—", version: "v0.1.4", admin: false };

function t(key: string): string { return i18n[lang][key] || key; }

function getServerHost(): string {
  const key = activeKeyValue();
  if (FOREIGN_LINK.test(key)) {
    const hostport = key.split("://")[1]?.split(/[?#]/)[0] ?? "";
    const after = hostport.includes("@") ? hostport.split("@").pop() ?? "" : hostport;
    return after.split(":")[0] ?? "";
  }
  if (!key) return "";
  if (key.startsWith("whispera://")) {
    // Try base64-JSON format first
    try {
      const raw = key.slice("whispera://".length).split("?")[0];
      const decoded = atob(raw);
      const j = JSON.parse(decoded) as Record<string, unknown>;
      const srv = (j.server as string) || "";
      if (srv) return srv; // "host:port"
    } catch { /* not base64 JSON */ }
    // Legacy format: whispera://host:port?params
    try {
      const u = new URL(key);
      const host = u.hostname;
      if (!host || host.includes("=") || (host.length > 40 && !host.includes("."))) return "";
      return u.host;
    } catch { return ""; }
  }
  return "";
}

function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${sec}s`;
  return `${sec}s`;
}

function loadProfiles(): void {
  try { const r = localStorage.getItem("whisp_profiles"); if (r) profiles = JSON.parse(r); } catch {/**/ }
  const seen = new Set<string>();
  const before = profiles.length;
  profiles = profiles.filter(p => !!p?.key && !seen.has(p.key) && !!seen.add(p.key));
  if (profiles.length !== before) saveProfiles();
}
function saveProfiles(): void { localStorage.setItem("whisp_profiles", JSON.stringify(profiles)); }

async function loadSubscriptions(): Promise<void> {
  try { subscriptions = await invoke<Subscription[]>("get_subscriptions"); } catch {/**/ }
}

async function autoCheckSubscriptions(): Promise<void> {
  const newlyAvailable: Subscription[] = [];
  for (const sub of subscriptions) {
    try {
      const remote = await invoke<Subscription>("check_subscription_update", { id: sub.id });
      // Only flag+notify once per pending update — re-checking every cycle
      // shouldn't re-notify for the same still-unrefreshed version.
      if (remote && remote.updated !== sub.updated && !subUpdateAvailable.has(sub.id)) {
        subUpdateAvailable.add(sub.id);
        newlyAvailable.push(sub);
      }
    } catch {/**/}
  }
  if (newlyAvailable.length === 0) return;
  if (currentPage === "home") renderPage();
  const names = newlyAvailable.map(s => s.name || s.url).join(", ");
  showToast(`${t("subUpdateAvailableToast")}: ${names}`, "info", 5000);
  osNotify(t("subUpdateAvailableToast"), names);
}

const SUB_AUTO_CHECK_INTERVAL_MS = 10 * 60 * 1000;

function startSubAutoCheck(): void {
  stopSubAutoCheck();
  if (!settings.sub_auto_update) return;
  subAutoCheckTimer = setInterval(() => autoCheckSubscriptions(), SUB_AUTO_CHECK_INTERVAL_MS);
}

function stopSubAutoCheck(): void {
  if (subAutoCheckTimer) { clearInterval(subAutoCheckTimer); subAutoCheckTimer = null; }
}
function loadLang(): void { const s = localStorage.getItem("whisp_lang"); if (s === "en" || s === "ru" || s === "zh" || s === "fa") lang = s as Lang; }
function syncNativeLang(): void { if (isAndroid) void invoke("set_app_language", { lang }).catch(() => {}); }
function saveLang(): void { localStorage.setItem("whisp_lang", lang); syncNativeLang(); }

/* ===================== BACKEND ===================== */
async function loadSettings(): Promise<void> {
  try { const s = await invoke<AppSettings>("get_app_settings"); settings = { ...settings, ...s }; } catch {/**/ }
}

async function persistSettings(): Promise<void> {
  try { await invoke("save_app_setting", { settings }); } catch (e) { showToast(String(e), "error", 3000); }
}

async function loadRoutingRules(): Promise<void> {
  try { routingRules = await invoke<RoutingRule[]>("get_routing_rules"); } catch {/**/ }
}

async function persistRoutingRules(): Promise<void> {
  try { await invoke("save_routing_rules", { rules: routingRules }); } catch {/**/ }
}

async function loadBlocklist(): Promise<void> {
  try { blocklistRules = await invoke<RoutingRule[]>("get_blocklist"); } catch {/**/ }
}

async function persistBlocklist(): Promise<void> {
  try { await invoke("save_blocklist", { rules: blocklistRules }); } catch {/**/ }
}


async function doConnect(): Promise<void> {
  isConnecting = true;
  if (currentPage === "home") renderPage();

  try {
    const msg = await invoke<string>("connect");
    isConnected = true;
    connectTime = Date.now();
    localStorage.setItem("connectTime", String(connectTime));
    addLog("✓ " + msg);
    startLogPolling();
    showToast(t("vpnConnected"), "success", 4000);
    if (!isAndroid) osNotify("Whisp VPN", t("vpnConnected"));
  } catch (e) {
    addLog("✗ " + e);
    showToast(String(e), "error", 5000);
    osNotify("Whisp VPN — ошибка", String(e));
  }
  isConnecting = false;
  if (currentPage === "home") renderPage();
}

async function doDisconnect(): Promise<void> {
  isConnecting = true;
  if (currentPage === "home") renderPage();
  try {
    const msg = await invoke<string>("disconnect");
    isConnected = false;
    connectTime = null;
    localStorage.removeItem("connectTime");
    addLog("○ " + msg);
    showToast(t("vpnDisconnected"), "info");
    if (!isAndroid) osNotify("Whisp VPN", t("vpnDisconnected"));
  } catch (e) {
    addLog("✗ " + e);
    showToast(String(e), "error", 5000);
  }
  isConnecting = false;
  if (currentPage === "home") renderPage();
}

// Switches the active connection key. If a tunnel is already up (or mid-
// connect), live-reconnects with the new key instead of just storing it —
// otherwise the change would silently do nothing until the user manually
// disconnects and reconnects themselves.
async function switchToKey(newKey: string, ref = ""): Promise<void> {
  if (isConnecting) {
    showToast(t("pleaseWait"), "info", 2000);
    return;
  }
  const wasActive = isConnected;
  const foreign = /^(vless|vmess|trojan|ss):\/\//.test(newKey);
  const isSameKey = foreign ? settings.external_link === newKey : settings.conn_key === newKey;
  if (foreign) {
    settings.external_link = newKey;
  } else {
    settings.external_link = "";
    settings.conn_key = newKey;
  }
  activeRef = ref;
  persistSettings();
  currentPage = "home";
  renderNav();
  renderPage();
  if (!wasActive || isSameKey) return;
  showToast(t("switchingKey"), "info", 2500);
  await doDisconnect();
  if (isAndroid) await waitForVpnStopped(2500);
  await doConnect();
}

async function waitForVpnStopped(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try { if (!(await invoke<boolean>("get_status"))) return; } catch { return; }
    await new Promise(r => setTimeout(r, 150));
  }
}

// Returns false when the backend could not be asked at all. That is not the
// same as "disconnected", and the caller must not treat it as one: acting on a
// question we failed to ask is how a working tunnel gets torn down.
async function checkStatus(): Promise<boolean> {
  try {
    const was = isConnected;
    enginesUp = await invoke<boolean>("get_status");
    isConnected = enginesUp;
    let tunnelSince = NaN;
    if (!enginesUp) tunnelFailure = null;
    if (enginesUp) {
      const t = await invoke<{ known: boolean; up: boolean; error: string | null; failure: TunnelFailure | null; connected_at: string | null }>("get_tunnel_state");
      tunnelFailure = t.known ? t.failure : null;
      if (t.known && !t.up) {
        isConnected = false;
        if (t.error && t.error !== lastTunnelError) {
          lastTunnelError = t.error;
          addLog(`[whisp] engines are up but no transport is connected: ${t.error}`);
        }
      } else if (t.up) {
        lastTunnelError = "";
        if (t.connected_at) tunnelSince = Date.parse(t.connected_at);
      }
    }
    if (isConnected && !was && connectTime === null) {
      const saved = parseInt(localStorage.getItem("connectTime") || "", 10);
      connectTime = Number.isFinite(tunnelSince) ? tunnelSince : Number.isFinite(saved) && saved > 0 ? saved : Date.now();
    }
    return true;
  } catch {
    return false;
  }
}

/* Site checks — update DOM in-place, no flicker */
async function fetchSysInfo(): Promise<void> {
  try {
    const info = await invoke<{ os: string; uptime: string; version: string; admin: boolean }>("get_system_info");
    sysInfo = info;
  } catch {
    sysInfo = { os: "Windows (x64)", uptime: "0h 0m", version: "v0.1.4", admin: false };
  }
  updateSysDOM();
}

function updateSysDOM(): void {
  const os = document.getElementById("sys-os");
  const up = document.getElementById("sys-uptime");
  const ver = document.getElementById("sys-ver");
  const adm = document.getElementById("sys-admin");
  if (os) os.textContent = sysInfo.os;
  if (up) up.textContent = sysInfo.uptime;
  if (ver) ver.textContent = sysInfo.version;
  if (adm) { adm.textContent = sysInfo.admin ? "ON" : "OFF"; adm.className = "info-value " + (sysInfo.admin ? "badge-on" : "badge-off"); }
}

function tailOf(lines: string[]): string[] {
  return lines.length > LOG_RENDER_LINES ? lines.slice(lines.length - LOG_RENDER_LINES) : lines;
}

function colorize(lines: string[]): string {
  return lines.map(l => {
    const lvl = logLineLevel(l);
    const cls = "log-line" + (lvl ? ` log-${lvl}` : "");
    return `<div class="${cls}">${esc(l)}</div>`;
  }).join("");
}

function filteredLogLines(): string[] {
  return logLines.filter(l => {
    if (logFilter !== "all" && logLineLevel(l) !== logFilter) return false;
    if (logSearch && !l.toLowerCase().includes(logSearch.toLowerCase())) return false;
    return true;
  });
}

let _logRefreshPending = false;

function _scheduleLogRefresh(): void {
  if (_logRefreshPending) return;
  _logRefreshPending = true;
  requestAnimationFrame(() => {
    _logRefreshPending = false;
    _refreshLogBox();
  });
}

function _refreshLogBox(): void {
  const box = document.getElementById("log-box");
  if (!box) return;
  const filtered = filteredLogLines();
  const colorized = colorize(tailOf(filtered));
  box.innerHTML = colorized || `<div class="log-line log-info">${t("logReady")}</div>`;
  box.scrollTop = box.scrollHeight;
  const cnt = document.querySelector(".log-count");
  if (cnt) cnt.textContent = `${filtered.length}/${logLines.length}`;
}

const LOG_RETENTION_MS = 60 * 60 * 1000;
// The window keeps a short tail; the full history lives in the log file.
const LOG_MAX_LINES = 140;
const LOG_RENDER_LINES = 140;

const GO_LOG_TS = /^(\d{4})\/(\d{2})\/(\d{2}) (\d{2}):(\d{2}):(\d{2})/;

function addLog(line: string): void {
  const m = GO_LOG_TS.exec(line);
  const now = m
    ? new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]).getTime()
    : Date.now();
  const text = m
    ? "[" + m[4] + ":" + m[5] + ":" + m[6] + "]" + line.slice(m[0].length)
    : "[" + new Date(now).toLocaleTimeString() + "] " + line;
  logLines.push(text);
  logTimes.push(now);
  const cutoff = Date.now() - LOG_RETENTION_MS;
  let drop = 0;
  while (drop < logTimes.length && logTimes[drop] < cutoff) drop++;
  if (logLines.length - drop > LOG_MAX_LINES) drop = logLines.length - LOG_MAX_LINES;
  if (drop > 0) {
    logLines.splice(0, drop);
    logTimes.splice(0, drop);
  }
  _scheduleLogRefresh();
}

// go-client's console encoder pads short level names to a fixed width inside
// the brackets (e.g. "[INFO ]", "[WARN ]"), so match tolerating whitespace
// rather than an exact "[INFO]" substring.
function logLineLevel(line: string): "error" | "warn" | "info" {
  const u = line.toUpperCase();
  if (/\[\s*ERROR\s*\]/.test(u) || u.includes('"LEVEL":"ERROR"')) return "error";
  if (/\[\s*WARN\s*\]/.test(u) || u.includes('"LEVEL":"WARN"')) return "warn";
  if (/\[\s*INFO\s*\]/.test(u) || u.includes('"LEVEL":"INFO"')) return "info";
  // go-client (desktop + Android) logs via Go's stdlib `log` package — plain
  // "date time message" lines with no bracketed level tag. It marks failures
  // and warnings with literal text instead ("... failed: ...", "WARNING: ..."),
  // so classify by that real convention; anything else is routine info output.
  // "WARNING:" is checked first since a warning line can still mention "failed"
  // in its message body (e.g. "WARNING: Failed to connect...").
  if (u.includes("WARNING:")) return "warn";
  if (line.startsWith("✗") || u.includes("FATAL") || u.includes("FAILED") || u.includes("ERROR")) return "error";
  return "info";
}

function renderShell(): void {
  const app = document.getElementById("app");
  if (!app) return;

  if (!document.getElementById("toast-container")) {
    const tc = document.createElement("div");
    tc.id = "toast-container";
    tc.className = "toast-container";
    document.body.appendChild(tc);
  }

  document.body.classList.toggle("is-desktop", !isAndroid);

  app.innerHTML = `
    <header class="top-bar" id="top-bar">
      <span class="logo-wordmark">whisp</span>
      <div class="top-switchers">
        <div class="lang-switcher mode-switcher" id="mode-sw"${isWindows ? "" : " hidden"}></div>
        <div class="lang-switcher" id="lang-sw"></div>
      </div>
    </header>
    <div class="main-content" id="main-content"></div>
    <nav class="bottom-nav" id="bottom-nav"></nav>
  `;

  renderNav();
  renderPage();
}

function renderNav(): void {
  const nav = document.getElementById("bottom-nav")!;
  const langSw = document.getElementById("lang-sw")!;

  const routeIcon = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="3"/><circle cx="18" cy="18" r="3"/><path d="M6 9v3a3 3 0 0 0 3 3h6"/></svg>`;
  const items: { id: Page; icon: string; label: string }[] = [
    { id: "home",        icon: ICONS.home,     label: t("home") },
    { id: "routing",    icon: routeIcon,      label: t("routing") },
    { id: "logs",       icon: ICONS.log,      label: t("logs") },
    { id: "settings",   icon: ICONS.settings, label: t("settings") },
  ];

  nav.innerHTML = items.map(n =>
    `<div class="bnav-item${currentPage === n.id ? " active" : ""}" data-page="${n.id}">
      <span class="bnav-icon">${n.icon}</span>
      <span class="bnav-label">${n.label}</span>
    </div>`
  ).join("");

  // Scroll active item into view
  const active = nav.querySelector<HTMLElement>(".bnav-item.active");
  active?.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "center" });

  langSw.innerHTML = `
    <button class="lang-btn ${lang === "ru" ? "active" : ""}" data-lang="ru">RU</button>
    <button class="lang-btn ${lang === "en" ? "active" : ""}" data-lang="en">EN</button>
    <button class="lang-btn ${lang === "zh" ? "active" : ""}" data-lang="zh">中</button>
    <button class="lang-btn ${lang === "fa" ? "active" : ""}" data-lang="fa">FA</button>
  `;

  nav.querySelectorAll<HTMLElement>(".bnav-item[data-page]").forEach(el => {
    el.addEventListener("click", () => {
      currentPage = el.dataset.page as Page;
      renderNav();
      renderPage();
      // Lockdown is a system setting the user can change behind our back, so read
      // it fresh every time the page that shows it is opened.
      if (currentPage === "settings" && isAndroid) {
        invoke<boolean>("vpn_lockdown").then(on => {
          if (on !== vpnLockdown) { vpnLockdown = on; renderPage(); }
        }).catch(() => {});
      }
    });
  });
  document.querySelectorAll<HTMLElement>(".lang-btn[data-lang]").forEach(el => {
    el.addEventListener("click", () => { lang = el.dataset.lang as Lang; saveLang(); renderNav(); renderPage(); });
  });
  renderModeSwitch();
}

function renderModeSwitch(): void {
  const modeSw = document.getElementById("mode-sw");
  if (!modeSw || !isWindows) return;
  const mode = settings.proxy_mode === "system" ? "system" : "tun";
  modeSw.innerHTML = `
    <button class="lang-btn ${mode === "tun" ? "active" : ""}" data-mode="tun">TUN</button>
    <button class="lang-btn ${mode === "system" ? "active" : ""}" data-mode="system">${t("modeProxy")}</button>
  `;
  modeSw.querySelectorAll<HTMLElement>(".lang-btn[data-mode]").forEach(el => {
    el.addEventListener("click", async () => {
      if (isConnecting || el.dataset.mode === mode) return;
      settings.proxy_mode = el.dataset.mode;
      renderModeSwitch();
      await persistSettings();
      if (isConnected) await doConnect();
    });
  });
}

function renderPage(): void {
  document.body.classList.toggle("theme-light", settings.theme === "auto");
  const main = document.getElementById("main-content")!;
  switch (currentPage) {
    case "home": {
      const scrollY = main.scrollTop;
      main.innerHTML = renderHome();
      bindHomeEvents();
      bindProfileEvents();
      main.scrollTop = scrollY;
      break;
    }
    case "routing": main.innerHTML = renderRouting(); bindRoutingEvents(); break;
    case "logs":
      main.innerHTML = renderLogs();
      bindLogEvents();
      document.getElementById("btn-clear-logs")?.addEventListener("click", async () => {
        try {
          await invoke("clear_vpn_log");
        } catch (e) {
          console.error("clear_vpn_log", e);
        }
        logLines = [];
        logTimes = [];
        logSearch = "";
        logFilter = "all";
        renderPage();
      });
      break;
    case "settings": main.innerHTML = renderSettings(); bindSettingsEvents(); break;
  }
  document.querySelectorAll<HTMLElement>(".copy-icon[data-copy]").forEach(el => {
    el.addEventListener("click", () => {
      clipboardWrite(el.dataset.copy || "");
      showToast(t("copied"), "success", 1800);
    });
  });
}

function updateHome(): void {
  if (currentPage !== "home") return;
  // Try targeted DOM patches first — avoids scroll reset and animation restarts.
  const dot = document.querySelector<HTMLElement>(".status-dot");
  const btnConnect = document.getElementById("btn-connect") as HTMLButtonElement | null;
  // If the connection card structure has changed (connected↔disconnected) or
  // the page hasn't been rendered yet, fall back to a full re-render.
  const cardIsConnected = btnConnect?.classList.contains("connected") ?? null;
  const stateMatches = cardIsConnected === isConnected;
  if (!dot || !btnConnect || !stateMatches) {
    renderPage();
    return;
  }
  const failure = document.getElementById("status-failure");
  if (failure) failure.textContent = failureText();
}

function showToast(msg: string, type: "success" | "error" | "info" = "info", duration = 3500): void {
  const container = document.getElementById("toast-container");
  if (!container) return;
  const toast = document.createElement("div");
  toast.className = `toast toast-${type}`;
  toast.innerHTML = `<span class="toast-dot"></span><span class="toast-msg">${esc(msg)}</span>`;
  container.appendChild(toast);
  requestAnimationFrame(() => requestAnimationFrame(() => toast.classList.add("toast-visible")));
  setTimeout(() => {
    toast.classList.remove("toast-visible");
    toast.classList.add("toast-hiding");
    setTimeout(() => toast.remove(), 280);
  }, duration);
}

function tickUptime(): void {
  const el = document.getElementById("status-uptime");
  if (el) el.textContent = isConnected && connectTime ? formatDuration(Date.now() - connectTime) : "";
  const el2 = document.getElementById("conn-uptime");
  if (el2) el2.textContent = isConnected && connectTime ? formatDuration(Date.now() - connectTime) : "—";
}

function failureText(): string {
  const f = tunnelFailure;
  if (!enginesUp || !f) return "";
  if (isConnected && (f.failures ?? 0) < WARN_AFTER_FAILED_DIALS) return "";
  const parts = [isConnected ? `${t("failWhileConnected")}: ${t(`fail_${f.cause}`)}` : t(`fail_${f.cause}`)];
  if (f.wire && f.wire !== "none") parts.push(t(`wire_${f.wire}`));
  if (f.symptom === "stall_volume" && f.down_bytes) parts.push(t("failFreezeAt").replace("{kb}", String(Math.round(f.down_bytes / 1024))));
  if (f.pattern) parts.push(t("failNewPattern"));
  return parts.join(" · ");
}

function hasUsableKey(): boolean {
  const k = activeKeyValue();
  if (!k) return false;
  return profiles.some(p => p.key === k) ||
    subscriptions.some(s => (s.keys || []).includes(k));
}

function renderHome(): string {
  if (!isConnected) {
    const before = settings.conn_key;
    if (settings.conn_key && !hasUsableKey()) settings.conn_key = "";
    if (!settings.conn_key) {
      const first = profiles[0]?.key ?? subscriptions.find(s => (s.keys || []).length > 0)?.keys[0];
      if (first) settings.conn_key = first;
    }
    if (settings.conn_key !== before) persistSettings();
  }
  normalizeActiveRef();
  const profileName = profiles.find(p => p.key === activeKeyValue())?.name;
  const serverHost = getServerHost();
  const uptimeStr = isConnected && connectTime ? formatDuration(Date.now() - connectTime) : "";
  const dis = isConnecting;
  const noKey = !isConnected && !hasUsableKey();

  const powerBlock = `
    <div class="power-wrap">
      <button class="btn-power${isConnected ? " connected" : ""}${dis ? " connecting" : ""}${noKey ? " nokey" : ""}" id="btn-connect"${dis || noKey ? " disabled" : ""}>${ICONS.power}</button>
    </div>
    <div class="power-status">
      <div class="status-text">${dis ? t("connecting") : isConnected ? t("connected") : noKey ? t("noKeySelected") : t("disconnected")}</div>
      <div class="status-failure" id="status-failure">${esc(failureText())}</div>
      ${isConnected ? `<div class="status-server">${profileName ? esc(profileName) + " · " : ""}${serverHost ? esc(serverHost) + " · " : ""}<span id="status-uptime">${uptimeStr}</span></div>` : ""}
    </div>`;

  return `<div class="home-grid">
    ${powerBlock}

    <div class="section-header">
      <span class="section-title">${t("keysSection")}</span>
      <div class="section-actions">
      <button class="btn-icon-add" id="btn-ping-all" title="${t("pingAll")}">${ICONS.ping}</button>
      <div class="key-menu-wrap">
        <button class="btn-icon-add" id="btn-add-key">${ICONS.plus}</button>
        <div class="key-menu" data-addmenu hidden style="min-width:210px">
          <button class="km-item" id="btn-add-key-single">${ICONS.user}<span>${t("addProfile")}</span></button>
          <button class="km-item" id="btn-add-key-sub">${ICONS.link}<span>${t("addSubscription")}</span></button>
          <button class="km-item" id="btn-add-key-paste">${ICONS.clipboard}<span>${t("pasteFromClipboard")}</span></button>
          <button class="km-item" id="btn-qr">${ICONS.qr}<span>${t("qrCode")}</span></button>
          <button class="km-item" id="btn-add-key-import-json">${ICONS.code}<span>${t("pasteJson")}</span></button>
          <button class="km-item" id="btn-add-key-export-json">${ICONS.copy}<span>${t("copyJson")}</span></button>
          <button class="km-item" id="btn-refresh-all-subs">${ICONS.refresh}<span>${t("refreshAllSubs")}</span></button>
          <button class="km-item" id="btn-toggle-sub-auto">${ICONS.bolt}<span>${settings.sub_auto_update ? t("subAutoUpdateOn") : t("subAutoUpdateOff")}</span></button>
        </div>
      </div>
      </div>
    </div>
    ${renderKeysList()}
  </div>`;
}

function bindHomeEvents(): void {
  document.getElementById("btn-connect")?.addEventListener("click", async () => {
    if (isConnecting) return;
    if (!isConnected && !hasUsableKey()) {
      showToast(t("keyPlaceholder"), "error", 2500);
      return;
    }
    isConnected ? await doDisconnect() : await doConnect();
  });

}

function renderKeysList(): string {
  if (profiles.length === 0 && subscriptions.length === 0) {
    return `<div class="empty-state"><div class="empty-icon">${ICONS.user}</div><p>${t("noKeys")}</p></div>`;
  }
  return renderProfileList() + renderSubList();
}

function pingLabel(pr: number | "pinging" | "timeout" | { err: string } | undefined): string {
  if (pr === undefined) return "";
  if (pr === "pinging") return `<span class="ping-val pinging">${t("pingRunning")}</span>`;
  if (pr === "timeout") return `<span class="ping-val timeout">${t("pingTimeout")}</span>`;
  // A failure that is not a timeout carries its reason in the tooltip — folding
  // everything into "timeout" once hid a key that could not even be parsed.
  if (typeof pr === "object") return `<span class="ping-val timeout" title="${esc(pr.err)}">${t("pingError")}</span>`;
  return `<span class="ping-val ok">${pr}${t("pingMs")}</span>`;
}

function pingFailure(e: unknown): "timeout" | { err: string } {
  const msg = String(e ?? "");
  return msg.includes("timeout") ? "timeout" : { err: msg };
}

function renderProfileList(): string {
  return profiles.length === 0
    ? ""
    : profiles.map(p => {
        const isSelected = activeRef === "p:" + p.id;
        const isActive = isConnected && isSelected;
        return `
        <div class="profile-card${isActive ? " key-active" : isSelected ? " key-selected" : ""}">
          <div class="profile-info"><span>${ICONS.user}</span><span>${esc(p.name)}</span>${isActive ? `<span class="badge-on" style="font-size:10px;padding:1px 6px;flex-shrink:0">${t("active")}</span>` : ""}</div>
          <div class="profile-actions">
            ${pingLabel(pingResults.get("p:" + p.id))}
            <button class="btn-use-profile" data-id="${p.id}" title="${t("subSelectKey")}">${ICONS.play}</button>
            <div class="key-menu-wrap">
              <button class="btn-profile-menu" data-id="${p.id}" title="${t("more")}">${ICONS.kebab}</button>
              <div class="key-menu" data-profile="${p.id}" hidden>
                <button class="km-item btn-ping-profile" data-id="${p.id}" data-key="${esc(p.key)}">${ICONS.ping}<span>${t("pingKey")}</span></button>
                <button class="km-item btn-copy-profile" data-key="${esc(p.key)}">${ICONS.copy}<span>${t("copy")}</span></button>
                <button class="km-item km-danger btn-del-profile" data-id="${p.id}">${ICONS.x}<span>${t("subDelete")}</span></button>
              </div>
            </div>
          </div>
        </div>`;
      }).join("");
}

const expandedSubs = new Set<string>();

function renderSubList(): string {
  return subscriptions.length === 0
    ? ""
    : subscriptions.map(s => {
        const keyRows = s.keys.map((k, i) => {
          const rowPing = pingLabel(pingResults.get(`${s.id}:${i}`));
          const keyIsSelected = activeRef === "s:" + s.id + ":" + i;
          const keyIsActive = isConnected && keyIsSelected;
          return `
          <div class="sub-key-row sub-key-pick${keyIsActive ? " key-active" : keyIsSelected ? " key-selected" : ""}" data-sub="${s.id}" data-idx="${i}" title="${t("subSelectKey")}">
            <span class="sub-key-val" title="${esc(k)}">${esc(k.length > 50 ? k.slice(0, 50) + "…" : k)}</span>
            ${keyIsActive ? `<span class="badge-on" style="font-size:10px;padding:1px 6px;flex-shrink:0">${t("active")}</span>` : ""}
            ${rowPing}
            <div class="key-menu-wrap">
              <button class="btn-key-menu" data-sub="${s.id}" data-idx="${i}" title="${t("more")}">${ICONS.kebab}</button>
              <div class="key-menu" data-sub="${s.id}" data-idx="${i}" hidden>
                <button class="km-item btn-ping-key" data-sub="${s.id}" data-idx="${i}" data-key="${esc(k)}">${ICONS.ping}<span>${t("pingKey")}</span></button>
                <button class="km-item btn-json-key" data-sub="${s.id}" data-idx="${i}" data-key="${esc(k)}">${ICONS.pencil}<span>${t("keyJson")}</span></button>
                <button class="km-item btn-copy-key" data-key="${esc(k)}">${ICONS.copy}<span>${t("copy")}</span></button>
                <button class="km-item km-danger btn-del-key" data-sub="${s.id}" data-idx="${i}">${ICONS.x}<span>${t("subDelete")}</span></button>
              </div>
            </div>
          </div>`;
        }).join("");
        return `
          <div class="profile-card sub-card">
            <div class="profile-info sub-collapse-hdr" data-sub-id="${s.id}" style="cursor:pointer">
              <span class="sub-collapse-arrow" data-sub-id="${s.id}" style="opacity:.4;font-size:11px;flex-shrink:0">${expandedSubs.has(s.id) ? "▼" : "▶"}</span>
              <span class="sub-name" title="${esc(s.name || s.url)}">${esc(s.name || s.url)}</span>
            </div>
            <div class="profile-actions">
              <button class="btn-ping-all-sub" data-id="${s.id}" title="${t("pingAll")}">${ICONS.ping}</button>
              <button class="btn-rename-sub" data-id="${s.id}" title="${t("subRename")}">${ICONS.pencil}</button>
              <button class="btn-refresh-sub" data-id="${s.id}" title="${t("subRefresh")}">${subUpdateAvailable.has(s.id) ? '<span class="sub-update-dot"></span>' : ""}${ICONS.refresh}</button>
              <button class="btn-del-sub" data-id="${s.id}" title="${t("subDelete")}">${ICONS.x}</button>
            </div>
            ${s.keys.length > 0 ? `<div class="sub-keys" data-sub-id="${s.id}" style="display:${expandedSubs.has(s.id) ? "block" : "none"}">${keyRows}</div>` : ""}
          </div>`;
      }).join("");
}

function bindProfileEvents(): void {
  document.getElementById("btn-ping-all")?.addEventListener("click", async () => {
    const targets: Array<{ id: string; key: string }> = [
      ...profiles.map(p => ({ id: "p:" + p.id, key: p.key })),
      ...subscriptions.flatMap(sub => sub.keys.map((k, i) => ({ id: `${sub.id}:${i}`, key: k }))),
    ];
    if (targets.length === 0) return;
    targets.forEach(x => pingResults.set(x.id, "pinging"));
    renderPage();
    const mode = settings.ping_mode || "tcp";
    await Promise.all(targets.map(async x => {
      try {
        pingResults.set(x.id, await invoke<number>("ping_key", { key: x.key, mode }));
      } catch (e) {
        pingResults.set(x.id, pingFailure(e));
      }
    }));
    renderPage();
  });

  document.getElementById("btn-add-key")?.addEventListener("click", (e) => {
    e.stopPropagation();
    const menu = document.querySelector<HTMLElement>(".key-menu[data-addmenu]");
    const isOpen = menu ? !menu.hidden : false;
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    if (menu) menu.hidden = isOpen;
  });
  document.getElementById("btn-add-key-single")?.addEventListener("click", (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    showProfileModal();
  });
  document.getElementById("btn-add-key-sub")?.addEventListener("click", (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    showSubModal();
  });
  document.getElementById("btn-add-key-paste")?.addEventListener("click", async (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    let text: string | null = null;
    // Native Android clipboard read — the tauri clipboard-manager plugin does
    // not implement readText on Android and WebView blocks navigator.clipboard.
    try {
      text = await invoke<string>("read_clipboard_text");
    } catch { /* not android / not available — fall through */ }
    if (!text) {
      try { text = await clipboardRead(); } catch { /* fall through */ }
    }
    if (!text) {
      try { text = await navigator.clipboard?.readText(); } catch { /* fall through */ }
    }
    if (text && text.trim()) {
      handlePastedOrScannedText(text.trim());
    } else {
      showToast(t("clipboardReadFailed"), "info", 2000);
      showProfileModal();
    }
  });
  document.getElementById("btn-qr")?.addEventListener("click", (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    showQrModal();
  });
  document.getElementById("btn-add-key-import-json")?.addEventListener("click", (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    void importJsonFromFile();
  });
  document.getElementById("btn-add-key-export-json")?.addEventListener("click", async (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    try {
      await clipboardWrite(JSON.stringify({ profiles, subscriptions }));
      showToast(t("jsonCopied"), "success", 3000);
      osNotify(t("jsonCopied"), `${profiles.length + subscriptions.length}`);
    } catch {
      showToast(t("clipboardWriteFailed"), "error", 2500);
    }
  });
  document.getElementById("btn-refresh-all-subs")?.addEventListener("click", async (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    const total = subscriptions.length;
    if (total === 0) { showToast(t("noSubscriptions"), "error", 2000); return; }
    const results = await Promise.all(subscriptions.map(s =>
      invoke<Subscription>("refresh_subscription", { id: s.id }).catch(() => null)
    ));
    let ok = 0;
    results.forEach((updated, i) => {
      if (updated) { subscriptions[i] = updated; ok++; }
    });
    showToast(`${t("subsRefreshedSummary")} ${ok}/${total}`, ok === total ? "success" : "info", 3000);
    if (ok > 0) osNotify(t("subUpdated"), `${t("subsRefreshedSummary")} ${ok}/${total}`);
    renderPage();
  });
  document.getElementById("btn-toggle-sub-auto")?.addEventListener("click", (e) => {
    e.stopPropagation();
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
    settings.sub_auto_update = !settings.sub_auto_update;
    persistSettings();
    if (settings.sub_auto_update) startSubAutoCheck(); else stopSubAutoCheck();
    renderPage();
  });

  document.querySelectorAll<HTMLElement>(".btn-use-profile").forEach(el => {
    el.addEventListener("click", () => {
      const p = profiles.find(x => x.id === el.dataset.id);
      if (p) switchToKey(p.key, "p:" + p.id);
    });
  });
  document.querySelectorAll<HTMLElement>(".btn-profile-menu").forEach(el => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      const id = el.dataset.id!;
      const menu = document.querySelector<HTMLElement>(`.key-menu[data-profile="${id}"]`);
      const isOpen = menu ? !menu.hidden : false;
      document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
      if (menu) menu.hidden = isOpen;
    });
  });
  document.querySelectorAll<HTMLElement>(".btn-ping-profile").forEach(el => {
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (isConnected) {
        showToast(t("pingWhileOn"), "info", 3500);
        return;
      }
      const key = el.dataset.key!;
      const mapKey = "p:" + el.dataset.id!;
      document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
      pingResults.set(mapKey, "pinging");
      renderPage();
      try {
        const ms = await invoke<number>("ping_key", { key, mode: settings.ping_mode || "tcp" });
        pingResults.set(mapKey, ms);
      } catch (err) {
        pingResults.set(mapKey, pingFailure(err));
      }
      renderPage();
    });
  });
  document.querySelectorAll<HTMLElement>(".btn-copy-profile").forEach(el => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      clipboardWrite(el.dataset.key ?? "");
      document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
      showToast(t("copied"), "success", 1500);
    });
  });
  document.querySelectorAll<HTMLElement>(".btn-del-profile").forEach(el => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      profiles = profiles.filter(x => x.id !== el.dataset.id); saveProfiles(); renderPage();
    });
  });

  document.querySelectorAll<HTMLElement>(".sub-collapse-hdr").forEach(hdr => {
    hdr.addEventListener("click", () => {
      const id = hdr.dataset.subId!;
      const body = document.querySelector<HTMLElement>(`.sub-keys[data-sub-id="${id}"]`);
      const arrow = document.querySelector<HTMLElement>(`.sub-collapse-arrow[data-sub-id="${id}"]`);
      if (!body) return;
      const open = body.style.display !== "none";
      body.style.display = open ? "none" : "block";
      if (open) {
        expandedSubs.delete(id);
      } else {
        expandedSubs.add(id);
      }
      if (arrow) arrow.textContent = open ? "▶" : "▼";
    });
  });

  document.querySelectorAll<HTMLElement>(".btn-key-menu").forEach(el => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      const subId = el.dataset.sub!;
      const idx = el.dataset.idx!;
      const menu = document.querySelector<HTMLElement>(`.key-menu[data-sub="${subId}"][data-idx="${idx}"]`);
      const isOpen = menu ? !menu.hidden : false;
      document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
      if (menu) menu.hidden = isOpen;
    });
  });
  document.querySelectorAll<HTMLElement>(".btn-json-key").forEach(el => {
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
      const subId = el.dataset.sub!;
      const idx = parseInt(el.dataset.idx ?? "0", 10);
      showKeyJson(el.dataset.key ?? "", async (updated) => {
        const sub = subscriptions.find(s => s.id === subId);
        if (!sub) return;
        sub.keys[idx] = updated;
        try {
          await invoke("import_subscriptions", { entries: subscriptions });
        } catch { /* kept in memory either way */ }
        renderPage();
      });
    });
  });

  document.querySelectorAll<HTMLElement>(".btn-copy-key").forEach(el => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      clipboardWrite(el.dataset.key ?? "");
      document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
      showToast(t("copied"), "success", 1500);
    });
  });
  document.querySelectorAll<HTMLElement>(".btn-del-key").forEach(el => {
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      const subId = el.dataset.sub!;
      const idx = parseInt(el.dataset.idx ?? "0", 10);
      try {
        const updated = await invoke<Subscription>("delete_sub_key", { id: subId, index: idx });
        subscriptions = subscriptions.map(s => s.id === subId ? updated : s);
      } catch (err) {
        showToast(String(err), "error", 3000);
      }
      renderPage();
    });
  });

  document.querySelectorAll<HTMLElement>(".btn-ping-key").forEach(el => {
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      if (isConnected) {
        showToast(t("pingWhileOn"), "info", 3500);
        return;
      }
      const subId = el.dataset.sub!;
      const idx = parseInt(el.dataset.idx ?? "0", 10);
      const key = el.dataset.key!;
      const mapKey = `${subId}:${idx}`;
      document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
      pingResults.set(mapKey, "pinging");
      renderPage();
      try {
        const ms = await invoke<number>("ping_key", { key, mode: settings.ping_mode || "tcp" });
        pingResults.set(mapKey, ms);
      } catch (e) {
        pingResults.set(mapKey, pingFailure(e));
      }
      renderPage();
    });
  });

  document.querySelectorAll<HTMLElement>(".btn-ping-all-sub").forEach(el => {
    el.addEventListener("click", async () => {
      if (isConnected) {
        showToast(t("pingWhileOn"), "info", 3500);
        return;
      }
      const subId = el.dataset.id!;
      const sub = subscriptions.find(s => s.id === subId);
      if (!sub) return;
      sub.keys.forEach((_, i) => pingResults.set(`${subId}:${i}`, "pinging"));
      renderPage();
      await Promise.all(sub.keys.map(async (k, i) => {
        try {
          const ms = await invoke<number>("ping_key", { key: k, mode: settings.ping_mode || "tcp" });
          pingResults.set(`${subId}:${i}`, ms);
        } catch (e) {
          pingResults.set(`${subId}:${i}`, pingFailure(e));
        }
      }));
      renderPage();
    });
  });

  document.querySelectorAll<HTMLElement>(".btn-rename-sub").forEach(el => {
    el.addEventListener("click", async () => {
      const subId = el.dataset.id!;
      const sub = subscriptions.find(s => s.id === subId);
      if (!sub) return;
      const newName = await askText(t("subName"), sub.name || sub.url);
      if (newName === null) return;
      invoke("rename_subscription", { id: subId, name: newName.trim() }).then(() => {
        sub.name = newName.trim();
        renderPage();
      }).catch((e: unknown) => showToast(String(e), "error", 4000));
    });
  });

  document.querySelectorAll<HTMLElement>(".btn-refresh-sub").forEach(el => {
    el.addEventListener("click", async () => {
      const id = el.dataset.id!;
      (el as HTMLButtonElement).disabled = true;
      el.classList.add("spinning");
      try {
        const updated = await invoke<Subscription>("refresh_subscription", { id });
        subscriptions = subscriptions.map(s => s.id === id ? updated : s);
        subUpdateAvailable.delete(id);
        showToast(t("subUpdated"), "success");
        osNotify(t("subUpdated"), updated.name || updated.url);
      } catch { showToast(t("subUpdateFailed"), "error"); }
      renderPage();
    });
  });
  document.querySelectorAll<HTMLElement>(".btn-del-sub").forEach(el => {
    el.addEventListener("click", async () => {
      const id = el.dataset.id!;
      try {
        await invoke("delete_subscription", { id });
        subscriptions = subscriptions.filter(s => s.id !== id);
        renderPage();
      } catch (e) {
        showToast(String(e), "error", 4000);
      }
    });
  });
  document.querySelectorAll<HTMLElement>(".sub-key-pick").forEach(el => {
    el.addEventListener("click", (e) => {
      // The kebab, its menu and the ping badge keep their own behaviour.
      if ((e.target as HTMLElement).closest(".key-menu-wrap, .sub-key-ping")) return;
      const sub = subscriptions.find(s => s.id === el.dataset.sub);
      const idx = parseInt(el.dataset.idx ?? "0", 10);
      if (sub && sub.keys[idx]) switchToKey(sub.keys[idx], "s:" + sub.id + ":" + idx);
    });
  });
}

const FOREIGN_LINK = /^(vless|vmess|trojan|ss):\/\//;

function b64ToUtf8(b64: string): string {
  const bin = atob(b64.replace(/-/g, "+").replace(/_/g, "/"));
  const bytes = Uint8Array.from(bin, c => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

function utf8ToB64(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  bytes.forEach(b => { bin += String.fromCharCode(b); });
  return btoa(bin);
}

function keyAsJson(key: string): { text: string; editable: boolean } | null {
  const k = key.trim();
  if (FOREIGN_LINK.test(k)) {
    const [scheme, rest] = k.split("://");
    const noTag = rest.split("#")[0];
    const [body, query = ""] = noTag.split("?");
    const [userinfo, hostport = ""] = body.includes("@") ? body.split("@") : ["", body];
    const [host, port] = hostport.split(":");
    const params: Record<string, string> = {};
    query.split("&").filter(Boolean).forEach(p => {
      const [a, b = ""] = p.split("=");
      params[a] = decodeURIComponent(b);
    });
    return {
      text: JSON.stringify({ scheme, id: userinfo, server: host, port: Number(port) || 0, params }, null, 2),
      editable: false,
    };
  }
  try {
    const b64 = k.replace(/^whispera:\/\//, "").replace(/^wpn:\/\//, "").split(/[?#]/)[0];
    return { text: JSON.stringify(JSON.parse(b64ToUtf8(b64)), null, 2), editable: true };
  } catch {
    return null;
  }
}

function showKeyJson(key: string, onSave?: (k: string) => void): void {
  const parsed = keyAsJson(key);
  if (!parsed) {
    showToast(t("keyJsonBad"), "error", 3000);
    return;
  }
  const ov = document.createElement("div");
  ov.className = "modal-overlay";
  ov.innerHTML = `
    <div class="modal" style="max-width:640px">
      <h3>${t("keyJsonTitle")}</h3>
      ${parsed.editable ? "" : `<div style="font-size:11px;opacity:.6;margin-bottom:6px">${t("keyJsonForeign")}</div>`}
      <textarea id="key-json-area" spellcheck="false" ${parsed.editable ? "" : "readonly"}
        style="width:100%;box-sizing:border-box;height:340px;font-family:monospace;font-size:11px;resize:vertical">${esc(parsed.text)}</textarea>
      <div class="modal-actions">
        <button class="btn-sm" id="key-json-copy">${t("copy")}</button>
        <button class="btn-sm" id="key-json-close">${t("cancel")}</button>
        ${parsed.editable && onSave ? `<button class="btn-sm btn-primary" id="key-json-save">${t("save")}</button>` : ""}
      </div>
    </div>`;
  document.body.appendChild(ov);

  const area = ov.querySelector<HTMLTextAreaElement>("#key-json-area")!;
  const close = () => ov.remove();
  ov.addEventListener("click", e => { if (e.target === ov) close(); });
  ov.querySelector("#key-json-close")?.addEventListener("click", close);
  ov.querySelector("#key-json-copy")?.addEventListener("click", () => {
    clipboardWrite(area.value);
    showToast(t("copied"), "success", 1500);
  });
  ov.querySelector("#key-json-save")?.addEventListener("click", () => {
    let rebuilt: string;
    try {
      rebuilt = "whispera://" + utf8ToB64(JSON.stringify(JSON.parse(area.value)));
    } catch {
      showToast(t("keyJsonBad"), "error", 3000);
      return;
    }
    onSave?.(rebuilt);
    showToast(t("keyJsonSaved"), "success", 2500);
    close();
  });
}

function askText(title: string, initial: string): Promise<string | null> {
  return new Promise(resolve => {
    const ov = document.createElement("div");
    ov.className = "modal-overlay";
    ov.innerHTML = `
      <div class="modal" style="max-width:420px">
        <h3>${esc(title)}</h3>
        <input id="ask-text-input" value="${esc(initial)}" style="width:100%;box-sizing:border-box"/>
        <div class="modal-actions">
          <button class="btn-sm" id="ask-text-cancel">${t("cancel")}</button>
          <button class="btn-sm btn-primary" id="ask-text-ok">${t("save")}</button>
        </div>
      </div>`;
    document.body.appendChild(ov);
    const input = ov.querySelector<HTMLInputElement>("#ask-text-input")!;
    input.focus();
    input.select();
    const done = (v: string | null) => { ov.remove(); resolve(v); };
    ov.addEventListener("click", e => { if (e.target === ov) done(null); });
    ov.querySelector("#ask-text-cancel")?.addEventListener("click", () => done(null));
    ov.querySelector("#ask-text-ok")?.addEventListener("click", () => done(input.value.trim()));
    input.addEventListener("keydown", e => {
      if (e.key === "Enter") done(input.value.trim());
      if (e.key === "Escape") done(null);
    });
  });
}

function showSubModal(prefillUrl?: string): void {
  const ov = document.createElement("div");
  ov.className = "modal-overlay";
  ov.innerHTML = `
    <div class="modal">
      <h3>${t("addSubscription")}</h3>
      <div class="modal-field">
        <label>${t("subName")}</label>
        <input id="sub-modal-name" placeholder="${t("subName")}" />
      </div>
      <div class="modal-field">
        <label>${t("subUrl")}</label>
        <input id="sub-modal-url" placeholder="${t("subUrlHint")}" value="${prefillUrl ? esc(prefillUrl) : ""}" />
      </div>
      <div id="sub-modal-err" style="color:var(--danger,#e55);font-size:12px;margin-top:4px;word-break:break-all;overflow-wrap:anywhere;display:none"></div>
      <div class="modal-actions">
        <button class="btn-cancel" id="sub-modal-cancel">${t("cancel")}</button>
        <button class="btn-save" id="sub-modal-save">${t("save")}</button>
      </div>
    </div>`;
  document.body.appendChild(ov);
  document.getElementById("sub-modal-cancel")?.addEventListener("click", () => ov.remove());
  document.getElementById("sub-modal-save")?.addEventListener("click", async () => {
    const name = (document.getElementById("sub-modal-name") as HTMLInputElement).value.trim();
    const url = (document.getElementById("sub-modal-url") as HTMLInputElement).value.trim();
    const errEl = document.getElementById("sub-modal-err")!;
    if (!url) { errEl.textContent = t("subUrlHint"); errEl.style.display = ""; return; }
    const btn = document.getElementById("sub-modal-save") as HTMLButtonElement;
    btn.disabled = true; btn.textContent = t("subRefreshing");
    try {
      const entry = await invoke<Subscription>("add_subscription", { name, url });
      subscriptions.push(entry);
      ov.remove();
      showToast(t("subAdded"), "success", 3000);
      if (currentPage === "home") renderPage();
    } catch (e) {
      errEl.textContent = String(e);
      errEl.style.display = "";
      btn.disabled = false; btn.textContent = t("save");
    }
  });
}

async function importJsonFromFile(): Promise<void> {
  try {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (selected === null) return;
    if (typeof selected === "string") {
      await importJsonText(await readTextFile(selected));
      return;
    }
  } catch {
    // native picker/fs unavailable — fall back to the web file input
  }
  importJsonFromFileWeb();
}

function importJsonFromFileWeb(): void {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".json,application/json";
  input.style.display = "none";
  input.addEventListener("change", () => {
    const file = input.files?.[0];
    input.remove();
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => { void importJsonText(String(reader.result ?? "")); };
    reader.onerror = () => showToast(t("jsonParseError"), "error", 2500);
    reader.readAsText(file);
  });
  document.body.appendChild(input);
  input.click();
}

async function importJsonText(raw: string): Promise<void> {
  let parsed: { profiles?: Profile[]; subscriptions?: Subscription[] };
  try {
    parsed = JSON.parse(raw.trim());
  } catch {
    showToast(t("jsonParseError"), "error", 2500);
    return;
  }
  try {
    let importedProfiles = 0;
    let importedSubs = 0;
    if (Array.isArray(parsed.profiles)) {
      parsed.profiles.forEach((p, i) => {
        if (p?.name && p?.key && !profiles.some(x => x.key === p.key)) {
          profiles.push({ id: (Date.now() + i).toString(), name: p.name, key: p.key });
          importedProfiles++;
        }
      });
      if (importedProfiles > 0) saveProfiles();
    }
    if (Array.isArray(parsed.subscriptions) && parsed.subscriptions.length > 0) {
      const imported = await invoke<Subscription[]>("import_subscriptions", { entries: parsed.subscriptions });
      subscriptions.push(...imported);
      importedSubs = imported.length;
    }
    showToast(`${t("jsonImported")} ${importedProfiles + importedSubs}`, "success", 3000);
    osNotify(t("jsonImported"), `${importedProfiles + importedSubs}`);
    if (currentPage === "home") renderPage();
  } catch (e) {
    showToast(String(e), "error", 3000);
  }
}

function renderRouting(): string {
  // Unified rules list: routing + blocklist together
  const allRules: Array<{ r: RoutingRule; src: "routing" | "block" }> = [
    ...routingRules.map(r => ({ r, src: "routing" as const })),
    ...blocklistRules.map(r => ({ r, src: "block" as const })),
  ];

  const actionBadge = (r: RoutingRule) => {
    if (r.action === "REJECT") return `<span class="rule-action-badge reject">${t("ruleBlock")}</span>`;
    if (r.action === "PROXY")  return `<span class="rule-action-badge proxy">${t("ruleViaVpn")}</span>`;
    return `<span class="rule-action-badge direct">${t("ruleDirect")}</span>`;
  };
  const kindLabel = (r: RoutingRule) =>
    r.kind === "domain" ? t("ruleSite")
    : r.kind === "domain-keyword" ? t("ruleKeyword")
    : r.kind === "ip" ? "IP"
    : t("ruleApp");

  const unifiedRows = allRules.length === 0
    ? `<div class="empty-state"><p>${t("ruleNoRules")}</p></div>`
    : allRules.map(({ r, src }) => `
        <div class="rule-row" data-id="${r.id}" data-src="${src}">
          <span class="rule-kind">${kindLabel(r)}</span>
          <span class="rule-value" title="${esc(r.value)}">${esc(r.kind === "process" ? r.value.split(/[\\/]/).pop() || r.value : r.value)}</span>
          ${actionBadge(r)}
          <button class="btn-del-rule" data-id="${r.id}" data-src="${src}">${ICONS.x}</button>
        </div>`).join("");

  return `
    <div class="page-header">
      <h2 class="page-title">${t("routingTitle")}</h2>
    </div>

    <div class="card" style="margin-bottom:10px">
      <div class="card-header" style="padding-bottom:6px">
        <span class="card-title">${t("ruleAdd")}</span>
      </div>

      <div class="rule-type-bar" id="rule-type-bar">
        <button class="rule-type-btn active" data-type="domain">${t("ruleSiteType")}</button>
        <button class="rule-type-btn" data-type="process">${t("ruleAppType")}</button>
        <button class="rule-type-btn" data-type="domain-keyword">${t("ruleKeywordType")}</button>
        <button class="rule-type-btn" data-type="ip">IP</button>
      </div>

      <div class="rule-add-row" style="margin-top:8px">
        <div id="rule-input-wrap" style="flex:1;display:flex;align-items:center;gap:6px;min-width:0">
          <input type="text" id="rule-value-input" placeholder="${t("rulePlaceholder")}" class="rule-input" style="flex:1"/>
        </div>
        <div class="pill-group" id="rule-action-pills">
          <button class="pill-btn active" data-act="DIRECT">${t("ruleDirect2")}</button>
          <button class="pill-btn" data-act="PROXY">${t("ruleViaVpn")}</button>
          <button class="pill-btn" data-act="REJECT" style="color:var(--danger,#e74c3c)">${t("ruleBlock")}</button>
        </div>
        <button class="btn-sm" id="btn-add-rule">+</button>
      </div>

      <div id="rule-app-row" style="display:none;margin-top:6px;display:none">
        <div class="rule-add-row" style="margin:0">
          ${isAndroid ? "" : `<label class="btn-sm" style="cursor:pointer">
            ${t("rulesBrowseExe")}
            <input type="file" id="rule-exe-input" accept=".exe" style="display:none"/>
          </label>`}
          <button class="btn-sm" id="btn-pick-process">${isAndroid ? t("rulesPickApp") : t("rulesRunning")}</button>
          <span class="rule-exe-display" id="rule-exe-display">—</span>
        </div>
      </div>
    </div>

    <div id="process-picker-overlay" style="display:none;position:fixed;inset:0;background:rgba(0,0,0,.55);z-index:900;align-items:center;justify-content:center">
      <div style="background:var(--card-bg);border:1px solid var(--border);border-radius:12px;width:340px;max-height:520px;display:flex;flex-direction:column;padding:0;overflow:hidden">
        <div style="padding:12px 14px 8px;border-bottom:1px solid var(--border);display:flex;align-items:center;gap:8px">
          <input id="process-search" type="text" placeholder="${isAndroid ? t("rulesSearchApp") : t("rulesSearchProcess")}" class="input-inline" style="flex:1"/>
          <button id="btn-process-close" class="btn-sm" style="padding:4px 8px">✕</button>
        </div>
        <div id="process-list-inner" style="overflow-y:auto;flex:1;padding:4px 0"></div>
      </div>
    </div>

    <div class="card" style="margin-bottom:10px">
      <div class="card-header" style="padding-bottom:4px">
        <span class="card-title">${t("rulesTitle")} <span style="opacity:.4;font-weight:400;font-size:12px">${allRules.length}</span></span>
      </div>
      <div id="rules-list">${unifiedRows}</div>
    </div>

    </div>`;
}

let _selectedExe = "";

function bindRoutingEvents(): void {
  // Rule type bar
  let _ruleType = "domain";
  document.querySelectorAll<HTMLElement>("#rule-type-bar .rule-type-btn").forEach(el => {
    el.addEventListener("click", () => {
      document.querySelectorAll("#rule-type-bar .rule-type-btn").forEach(b => b.classList.remove("active"));
      el.classList.add("active");
      _ruleType = el.dataset.type || "domain";
      const inputWrap = document.getElementById("rule-input-wrap");
      const appRow = document.getElementById("rule-app-row");
      const valInput = document.getElementById("rule-value-input") as HTMLInputElement;
      if (_ruleType === "process") {
        if (inputWrap) inputWrap.style.display = "none";
        if (appRow) appRow.style.display = "block";
      } else {
        if (inputWrap) inputWrap.style.display = "flex";
        if (appRow) appRow.style.display = "none";
        if (valInput) valInput.placeholder = _ruleType === "ip"
          ? "e.g. 1.2.3.4/24"
          : _ruleType === "domain-keyword"
            ? t("ruleKeywordPlaceholder")
            : t("rulePlaceholder");
      }
    });
  });

  // Action pills
  document.querySelectorAll<HTMLElement>("#rule-action-pills .pill-btn").forEach(el => {
    el.addEventListener("click", () => {
      document.querySelectorAll("#rule-action-pills .pill-btn").forEach(b => b.classList.remove("active"));
      el.classList.add("active");
    });
  });

  // File browse for process
  document.getElementById("rule-exe-input")?.addEventListener("change", (e) => {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (file) {
      _selectedExe = file.name;
      const display = document.getElementById("rule-exe-display");
      if (display) display.textContent = file.name;
    }
  });

  // Process picker button
  document.getElementById("btn-pick-process")?.addEventListener("click", async () => {
    const overlay = document.getElementById("process-picker-overlay") as HTMLElement;
    const inner = document.getElementById("process-list-inner") as HTMLElement;
    if (!overlay || !inner) return;
    inner.innerHTML = `<div style="padding:16px;text-align:center;opacity:.5">${t("loading")}</div>`;
    overlay.style.display = "flex";
    try {
      const procs: { name: string; label: string; pid: number }[] = await invoke("list_processes");
      renderProcessList(procs, inner, "");
      const search = document.getElementById("process-search") as HTMLInputElement;
      if (search) {
        search.value = "";
        search.oninput = () => renderProcessList(procs, inner, search.value);
        // На Android автофокус на input открывает клавиатуру и перекрывает список
        if (!isAndroid) setTimeout(() => search.focus(), 50);
      }
    } catch (e) {
      inner.innerHTML = `<div style="padding:16px;color:var(--danger)">${esc(String(e))}</div>`;
    }
  });

  document.getElementById("btn-process-close")?.addEventListener("click", () => {
    (document.getElementById("process-picker-overlay") as HTMLElement).style.display = "none";
  });

  document.getElementById("process-picker-overlay")?.addEventListener("click", (e) => {
    if (e.target === e.currentTarget) (e.currentTarget as HTMLElement).style.display = "none";
  });

  // Add rule
  document.getElementById("btn-add-rule")?.addEventListener("click", async () => {
    const action = ((document.querySelector("#rule-action-pills .pill-btn.active") as HTMLElement)?.dataset.act || "DIRECT") as "DIRECT" | "PROXY" | "REJECT";
    const currentType = (document.querySelector("#rule-type-bar .rule-type-btn.active") as HTMLElement)?.dataset.type || "domain";
    let value = "";
    if (currentType === "process") {
      value = _selectedExe;
    } else {
      const inp = document.getElementById("rule-value-input") as HTMLInputElement;
      value = inp?.value.trim() || "";
      if (currentType === "domain") value = value.replace(/^https?:\/\//, "").replace(/\/.*$/, "");
    }
    if (!value) return;

    const rule: RoutingRule = { id: Date.now().toString(), kind: currentType as RoutingRule["kind"], value, action };
    if (action === "REJECT") {
      blocklistRules.push(rule);
      await persistBlocklist();
    } else {
      routingRules.push(rule);
      await persistRoutingRules();
    }
    _selectedExe = "";
    const inp = document.getElementById("rule-value-input") as HTMLInputElement;
    if (inp) inp.value = "";
    if (isAndroid) showToast(t("reconnectRequired"), "info", 3500);
    renderPage();
  });

  // Delete rule (unified list)
  document.querySelectorAll<HTMLElement>(".btn-del-rule").forEach(el => {
    el.addEventListener("click", async () => {
      const id = el.dataset.id;
      const src = el.dataset.src;
      if (src === "block") {
        blocklistRules = blocklistRules.filter(r => r.id !== id);
        await persistBlocklist();
      } else {
        routingRules = routingRules.filter(r => r.id !== id);
        await persistRoutingRules();
      }
      if (isAndroid) showToast(t("reconnectRequired"), "info", 3500);
      renderPage();
    });
  });

}

function renderProcessList(procs: { name: string; label: string; pid: number }[], container: HTMLElement, filter: string): void {
  const q = filter.toLowerCase();
  const filtered = q
    ? procs.filter(p => (p.label || p.name).toLowerCase().includes(q) || p.name.toLowerCase().includes(q))
    : procs;

  if (filtered.length === 0) {
    container.innerHTML = `<div style="padding:12px;text-align:center;opacity:.5">${t("rulesNoProcesses")}</div>`;
    return;
  }

  const showSub = filtered.some(p => p.label && p.label !== p.name);
  container.innerHTML = filtered.slice(0, 300).map(p => {
    const displayName = p.label || p.name;
    const sub = showSub && p.label !== p.name ? p.name : (p.pid > 0 ? `PID ${p.pid}` : "");
    return `<div class="process-pick-row" data-name="${esc(p.name)}" data-label="${esc(displayName)}"
      style="padding:${showSub ? "9px" : "7px"} 14px;cursor:pointer;display:flex;flex-direction:${showSub ? "column" : "row"};align-items:${showSub ? "flex-start" : "center"};gap:${showSub ? "2px" : "10px"};border-bottom:1px solid var(--border)">
      <span style="font-size:13px;${showSub ? "font-weight:500" : "flex:1"}">${esc(displayName)}</span>
      ${sub ? `<span style="font-size:11px;opacity:.45">${esc(sub)}</span>` : ""}
    </div>`;
  }).join("");

  container.querySelectorAll<HTMLElement>(".process-pick-row").forEach(row => {
    row.onmouseenter = () => row.style.background = "var(--hover-bg, rgba(255,255,255,.06))";
    row.onmouseleave = () => row.style.background = "";
    row.addEventListener("click", () => {
      const name = row.dataset.name || "";
      const label = row.dataset.label || name;
      _selectedExe = name;
      const display = document.getElementById("rule-exe-display");
      if (display) display.textContent = label;
      const overlay = document.getElementById("process-picker-overlay") as HTMLElement;
      if (overlay) overlay.style.display = "none";
    });
  });
}

let _logPollTimer: ReturnType<typeof setInterval> | null = null;

function startLogPolling(): void {
  if (_logPollTimer !== null) return;
  _logPollTimer = setInterval(async () => {
    try {
      const lines = await invoke<string[]>("get_vpn_log");
      lines.forEach(l => addLog(l));
    } catch { /* ignore */ }
  }, 2000);
}

let logFilter = "all";
let logSearch = "";

function renderLogs(): string {
  const filtered = filteredLogLines();
  const colorized = colorize(tailOf(filtered));
  const txt = colorized || `<div class="log-line log-info">${t("logReady")}</div>`;
  return `
    <div class="page-header">
      <h2 class="page-title">${t("systemLog")}</h2>
      <div style="display:flex;align-items:center;gap:8px">
        <span class="log-status">${isConnected ? t("connected") : t("disconnected")}</span>
        <button class="btn-sm" id="btn-clear-logs">${t("clearLogs")}</button>
      </div>
    </div>
    <div class="log-toolbar">
      <input type="text" class="log-search" id="log-search" placeholder="${t("logSearchPlaceholder")}" value="${esc(logSearch)}"/>
      <div class="pill-group">
        <button class="pill-btn log-filter-btn ${logFilter === "all" ? "active" : ""}" data-filter="all">${t("logAll")}</button>
        <button class="pill-btn log-filter-btn ${logFilter === "error" ? "active" : ""}" data-filter="error">Error</button>
        <button class="pill-btn log-filter-btn ${logFilter === "warn" ? "active" : ""}" data-filter="warn">Warn</button>
        <button class="pill-btn log-filter-btn ${logFilter === "info" ? "active" : ""}" data-filter="info">Info</button>
      </div>
      <span class="log-count">${filtered.length}/${logLines.length}</span>
    </div>
    <div class="log-box" id="log-box">${txt}</div>`;
}

function bindLogEvents(): void {
  document.getElementById("log-search")?.addEventListener("input", function () {
    logSearch = (this as HTMLInputElement).value;
    _refreshLogBox();
  });
  document.querySelectorAll<HTMLElement>(".log-filter-btn").forEach(btn => btn.addEventListener("click", () => {
    logFilter = btn.dataset.filter || "all";
    document.querySelectorAll<HTMLElement>(".log-filter-btn").forEach(b => b.classList.toggle("active", b.dataset.filter === logFilter));
    _refreshLogBox();
  }));
}

function socksAuthRow(): string {
  const user = settings.socks_user || "";
  const pass = settings.socks_pass || "";
  const url = `socks5://${esc(user)}:${esc(pass)}@127.0.0.1:${settings.mihomo_port}`;
  const hint = user || pass
    ? `<span class="setting-hint socks-auth-url"><code id="socks-proxy-url" title="${url}">${url}</code><button class="btn-icon" id="btn-copy-socks-url" title="${t("copy")}">${ICONS.copy}</button></span>`
    : "";
  return `<div class="setting-row">
        <span class="setting-label">${t("socksAuth")}</span>
        <div class="setting-value socks-auth">
          <input type="text" id="set-socks-user" value="${esc(user)}" placeholder="${t("socksUser")}" autocomplete="off"/>
          <div class="field-with-icon">
            <input type="password" id="set-socks-pass" value="${esc(pass)}" placeholder="${t("socksPass")}" autocomplete="new-password"/>
            <button class="btn-icon" id="btn-toggle-socks-pass" title="${t("socksShow")}">${ICONS.eye}</button>
          </div>
          <button class="btn-sm" id="btn-save-socks-auth">${t("socksSave")}</button>
        </div>
        ${hint}
      </div>`;
}

function row(label: string, value: string, hint = "", valueAttrs = ""): string {
  return `<div class="setting-row"><span class="setting-label">${label}</span>`
    + (hint ? `<span class="setting-hint">${hint}</span>` : "")
    + (value ? `<div class="setting-value"${valueAttrs}>${value}</div>` : "")
    + `</div>`;
}

function toggleBox(id: string, on: boolean): string {
  return `<label class="toggle"><input type="checkbox" id="${id}"${on ? " checked" : ""}/><span class="toggle-slider"></span></label>`;
}

function pills(attr: string, active: string, opts: [string, string][], attrs = ""): string {
  return `<div class="pill-group"${attrs}>`
    + opts.map(([v, l]) => `<button class="pill-btn${v === active ? " active" : ""}" ${attr}="${v}">${l}</button>`).join("")
    + `</div>`;
}

function textField(id: string, value: string, placeholder: string, readonly = false): string {
  return `<input type="text" id="${id}" value="${esc(value)}" placeholder="${placeholder}"${readonly ? " readonly" : ""} style="width:100%;box-sizing:border-box;text-align:left"/>`;
}

const KNOWN_DNS: [string, string[]][] = [
  ["Cloudflare", ["1.1.1.1", "1.0.0.1"]],
  ["Google", ["8.8.8.8", "8.8.4.4"]],
  ["Quad9", ["9.9.9.9", "149.112.112.112"]],
  ["OpenDNS", ["208.67.222.222", "208.67.220.220"]],
  ["AdGuard", ["94.140.14.14", "94.140.15.15"]],
  ["CleanBrowsing", ["185.228.168.9", "185.228.169.9"]],
  ["Control D", ["76.76.2.0", "76.76.10.0"]],
  ["DNS.SB", ["185.222.222.222", "45.11.45.11"]],
  ["Comodo", ["8.26.56.26", "8.20.247.20"]],
  ["Level3", ["4.2.2.1", "4.2.2.2"]],
];
const SYSTEM_DNS = "system";

function dnsPicker(id: string, value: string, placeholder: string): string {
  const address = (ip: string) =>
    `<button class="dns-picker-ip${ip === value ? " active" : ""}" data-dns="${ip}">${ip}</button>`;
  const known = KNOWN_DNS.some(([, ips]) => ips.includes(value)) || value === SYSTEM_DNS;
  const items = `<div class="dns-table">
      <span class="dns-table-head">${t("dnsService")}</span>
      <span class="dns-table-head">${t("dnsPrimary")}</span>
      <span class="dns-table-head">${t("dnsSecondary")}</span>
      ${KNOWN_DNS.map(([name, [primary, secondary]]) =>
        `<span class="dns-table-name">${esc(name)}</span>${address(primary)}${address(secondary)}`).join("")}
      <span class="dns-table-name">${t("isp")}</span>${address(SYSTEM_DNS)}<span></span>
      <div class="dns-table-custom">
        <span class="dns-table-name">${t("dnsCustom")}</span>
        <input type="text" id="dns-custom" value="${known ? "" : esc(value)}" placeholder="${t("dnsCustomHint")}"/>
        <button class="btn-sm" id="dns-custom-save">${t("save")}</button>
      </div>
    </div>`;
  return `<div class="dns-picker">
    ${textField(id, value, placeholder, true)}
    <span class="dns-picker-chevron">${ICONS.chevronDown}</span>
    <div class="key-menu dns-picker-menu" hidden>${items}</div>
  </div>`;
}

function whispSection(extraRows: string): string {
  return `<div class="settings-section">
      <div class="settings-section-header"><span class="settings-section-title">${t("whisp")}</span><span class="settings-link" id="whisp-update-status"></span></div>
      ${row(t("autostart"), toggleBox("set-autostart", !!settings.auto_connect))}
      ${row(t("update"), `<button class="btn-sm" id="btn-open-repo">${t("openRepo")}</button>`)}
      ${extraRows}
    </div>`;
}

function updatesSection(): string {
  return `<div class="settings-section">
      <div class="settings-section-title">${t("checkUpdates")}</div>
      ${row(t("currentVersion"), "", sysInfo.version)}
      <div class="setting-row" id="update-result-row" style="display:none;flex-direction:column;align-items:flex-start;gap:6px">
        <div id="update-result-content"></div>
      </div>
      <div class="setting-row">
        <button class="btn-sm" id="btn-check-updates" style="width:100%">${t("checkUpdates")}</button>
      </div>
    </div>`;
}

const COLUMN_VALUE = ` style="flex-direction:column;align-items:stretch;gap:6px"`;

function renderSettings(): string {
  const vpnDnsVal = settings.vpn_dns || "1.1.1.1";
  const vpnDnsPills = pills("data-vpndns", vpnDnsVal, [
    ["1.1.1.1", "1.1.1.1"],
    ["8.8.8.8", "8.8.8.8"],
    ["77.88.8.8", "Yandex"],
    ["system", t("isp")],
  ], ` style="flex-wrap:wrap;gap:4px"`);
  const themePills = pills("data-theme", settings.theme || "", [["dark", t("dark")], ["auto", t("auto")]]);
  const pingPills = pills("data-pingmode", settings.ping_mode || "tcp", [["tcp", "TCP"], ["get", "GET"], ["head", "HEAD"], ["icmp", "ICMP"]]);

  if (isAndroid) {
    return `<div class="page-header"><h2 class="page-title">${t("settings")}</h2></div>
    <div class="settings-section">
      <div class="settings-section-title">sing-box</div>
      ${row(t("vpnDns"), vpnDnsPills + textField("set-vpn-dns", vpnDnsVal, "1.1.1.1"), "", COLUMN_VALUE)}
      ${row(t("dnsMode"), pills("data-dnsmode", settings.dns_mode || "tcp", [["udp", "UDP"], ["tcp", "TCP"], ["doh", "DoH"]]))}
      ${row(t("dnsStrategy"), pills("data-dnsstrategy", settings.dns_strategy || "fakeip", [["fakeip", t("dnsFakeip")], ["local", t("dnsLocal")]]))}
      ${row(t("mtuLabel"), `<input type="number" id="set-mtu" min="576" max="9000" value="${settings.mtu ?? 1500}" style="width:80px;box-sizing:border-box;text-align:right"/>`)}
      ${row(t("tlsFragment"), toggleBox("set-tls-fragment", !!settings.tls_fragment))}
      ${row(t("ipv6Label"), toggleBox("set-ipv6", !!settings.ipv6))}
      ${row(t("ipv6FallbackLabel"), toggleBox("set-ipv6-fallback", settings.ipv6_fallback !== false))}
      ${row(t("ipv6OnlyLabel"), toggleBox("set-ipv6-only", !!settings.ipv6_only))}
      ${row(t("tunStackLabel"), pills("data-tunstack", settings.tun_stack_android || "gvisor", [["gvisor", "gVisor"], ["system", "System"], ["mixed", "Mixed"]]))}
      ${row(t("quicLabel"), toggleBox("set-quic", !!settings.quic))}
      ${row(t("killSwitchLabel"),
          `<span class="ping-val ${vpnLockdown ? "ok" : "timeout"}">${vpnLockdown ? "ON" : "OFF"}</span>`
          + `<button class="btn-sm" id="btn-open-vpn-settings">${t("killSwitchOpen")}</button>`)}
      ${row(t("pingModeLabel"), pingPills)}
      ${row(t("bypassMode"), toggleBox("set-bypass-ru", settings.bypass_ru !== false))}
      ${row(t("theme"), themePills)}
    </div>
    <div class="settings-section">
      <div class="settings-section-title">${t("shareProxy")}</div>
      ${row(t("mixedPort"), `<input type="number" id="set-port" value="${settings.mihomo_port}"/>`)}
      ${row(t("allowLan"), toggleBox("set-allow-lan", !!settings.allow_lan))}
      ${socksAuthRow()}
    </div>
    ${whispSection("")}
    ${updatesSection()}
    `;
  }

  return `<div class="page-header"><h2 class="page-title">${t("settings")}</h2></div>
    <div class="settings-section">
      <div class="settings-section-title">${t("mihomo")}</div>
      ${row(t("mixedPort"), `<input type="number" id="set-port" value="${settings.mihomo_port}"/>`)}
      ${row(t("bindAddr"), `<input type="text" id="set-bind" value="${settings.socks_addr}"/>`)}
      ${row(t("allowLan"), toggleBox("set-allow-lan", !!settings.allow_lan), t("allowLanHint"))}
      ${row(t("routingMode"), pills("data-rmode", settings.routing_mode || "rule", [["rule", "Rule"], ["global", "Global"], ["direct", "Direct"]]))}
      ${row(t("tunStack"), pills("data-tun", settings.tun_stack || "", [["Mixed", "Mixed"], ["gVisor", "gVisor"], ["System", "System"]]))}
      ${row(t("theme"), themePills)}
      ${row(t("dnsRedirect"), toggleBox("set-dns", !!settings.dns_redirect))}
      ${row(t("dnsServers"),
          textField("set-custom-dns", (settings.custom_dns || []).join(", "), "77.88.8.8, 8.8.8.8")
          + `<span class="setting-hint">${t("dnsCommaSep")}</span>`,
          "", ` style="flex-direction:column;align-items:stretch;gap:4px"`)}
      ${row(t("vpnDns"), dnsPicker("set-vpn-dns", vpnDnsVal, "1.1.1.1:53"), "", COLUMN_VALUE)}
      ${row(t("ipv6Label"), toggleBox("set-ipv6", !!settings.ipv6))}
      ${row(t("ipv6FallbackLabel"), toggleBox("set-ipv6-fallback", settings.ipv6_fallback !== false))}
      ${row(t("ipv6OnlyLabel"), toggleBox("set-ipv6-only", !!settings.ipv6_only))}
      ${row(t("bypassMode"), toggleBox("set-bypass-ru", settings.bypass_ru !== false), t("bypassModeHint"))}
      ${row(t("secretLabel"), `<span class="secret-value">${settings.secret}</span><button class="btn-sm" id="btn-copy-secret">${t("copy")}</button>`)}
      ${socksAuthRow()}
    </div>
    <div class="settings-section">
      <div class="settings-section-title">${t("advanced")}</div>
      ${row(t("killSwitch"), toggleBox("set-ks", !!settings.kill_switch))}
      ${row(t("pingModeLabel"), pingPills)}
    </div>
    ${whispSection(`${row(t("config"), `<button class="btn-sm" id="btn-open-config">${t("open")}</button>`)}`)}
    ${updatesSection()}
    `;
}

function bindSettingsEvents(): void {
  (document.getElementById("set-ks") as HTMLInputElement)?.addEventListener("change", function () {
    settings.kill_switch = this.checked;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });

  (document.getElementById("set-port") as HTMLInputElement)?.addEventListener("change", function () {
    settings.mihomo_port = parseInt(this.value, 10) || 7890;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  (document.getElementById("set-bind") as HTMLInputElement)?.addEventListener("change", function () {
    settings.socks_addr = this.value;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  (document.getElementById("set-custom-dns") as HTMLInputElement)?.addEventListener("change", function () {
    const servers = this.value.split(",").map(s => s.trim()).filter(s => s.length > 0);
    settings.custom_dns = servers;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  document.querySelectorAll<HTMLElement>(".pill-btn[data-tun]").forEach(el => el.addEventListener("click", () => {
    settings.tun_stack = el.dataset.tun || "Mixed";
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
    renderPage();
  }));
  document.querySelectorAll<HTMLElement>(".pill-btn[data-rmode]").forEach(el => el.addEventListener("click", () => {
    settings.routing_mode = el.dataset.rmode || "rule";
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
    renderPage();
  }));
  document.querySelectorAll<HTMLElement>(".pill-btn[data-theme]").forEach(el => el.addEventListener("click", () => { settings.theme = el.dataset.theme || "dark"; persistSettings(); renderPage(); }));
  document.querySelectorAll<HTMLElement>(".pill-btn[data-dnsmode]").forEach(el => el.addEventListener("click", () => {
    settings.dns_mode = el.dataset.dnsmode || "tcp";
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
    renderPage();
  }));
  document.querySelectorAll<HTMLElement>(".pill-btn[data-dnsstrategy]").forEach(el => el.addEventListener("click", () => {
    settings.dns_strategy = el.dataset.dnsstrategy || "fakeip";
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
    renderPage();
  }));
  (document.getElementById("set-mtu") as HTMLInputElement)?.addEventListener("change", function () {
    const v = parseInt(this.value, 10);
    settings.mtu = Number.isFinite(v) ? Math.min(9000, Math.max(576, v)) : 1500;
    this.value = String(settings.mtu);
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  (document.getElementById("set-tls-fragment") as HTMLInputElement)?.addEventListener("change", function () {
    settings.tls_fragment = this.checked;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  (document.getElementById("set-quic") as HTMLInputElement)?.addEventListener("change", function () {
    settings.quic = this.checked;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  document.getElementById("btn-open-vpn-settings")?.addEventListener("click", async () => {
    try { await invoke("open_vpn_settings"); } catch (e) { showToast(String(e), "error", 3000); }
  });
  document.querySelectorAll<HTMLElement>(".pill-btn[data-pingmode]").forEach(el => el.addEventListener("click", () => {
    settings.ping_mode = el.dataset.pingmode || "tcp";
    persistSettings();
    renderPage();
  }));
  document.querySelectorAll<HTMLElement>(".pill-btn[data-tunstack]").forEach(el => el.addEventListener("click", () => {
    settings.tun_stack_android = el.dataset.tunstack || "gvisor";
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
    renderPage();
  }));
  document.querySelectorAll<HTMLElement>(".pill-btn[data-vpndns]").forEach(el => el.addEventListener("click", () => {
    const val = el.dataset.vpndns || "1.1.1.1:53";
    settings.vpn_dns = val === "1.1.1.1:53" && !el.classList.contains("active") ? val : val;
    const inp = document.getElementById("set-vpn-dns") as HTMLInputElement;
    if (inp) inp.value = val;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
    renderPage();
  }));
  (document.getElementById("set-vpn-dns") as HTMLInputElement)?.addEventListener("change", function () {
    settings.vpn_dns = this.value.trim();
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  const dnsMenu = document.querySelector<HTMLElement>(".dns-picker-menu");
  if (dnsMenu) {
    const dnsInput = document.getElementById("set-vpn-dns") as HTMLInputElement;
    const customInput = document.getElementById("dns-custom") as HTMLInputElement;
    const pickDns = (val: string) => {
      if (!val) return;
      dnsInput.value = val;
      settings.vpn_dns = val;
      persistSettings();
      if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
      renderPage();
    };
    dnsInput.addEventListener("click", e => { e.stopPropagation(); dnsMenu.hidden = !dnsMenu.hidden; });
    dnsMenu.addEventListener("click", e => e.stopPropagation());
    dnsMenu.querySelectorAll<HTMLElement>("[data-dns]").forEach(el => el.addEventListener("click", () => pickDns(el.dataset.dns || "")));
    document.getElementById("dns-custom-save")?.addEventListener("click", () => pickDns(customInput.value.trim()));
    customInput.addEventListener("keydown", e => { if (e.key === "Enter") pickDns(customInput.value.trim()); });
  }
  document.getElementById("btn-toggle-socks-pass")?.addEventListener("click", function () {
    const inp = document.getElementById("set-socks-pass") as HTMLInputElement | null;
    if (!inp) return;
    const show = inp.type === "password";
    inp.type = show ? "text" : "password";
    this.innerHTML = show ? ICONS.eyeOff : ICONS.eye;
    this.title = show ? t("socksHide") : t("socksShow");
    this.classList.toggle("on", show);
  });
  const saveSocksAuth = () => {
    settings.socks_user = (document.getElementById("set-socks-user") as HTMLInputElement | null)?.value.trim() ?? "";
    settings.socks_pass = (document.getElementById("set-socks-pass") as HTMLInputElement | null)?.value ?? "";
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
    renderPage();
  };
  document.getElementById("btn-save-socks-auth")?.addEventListener("click", saveSocksAuth);
  ["set-socks-user", "set-socks-pass"].forEach((id) => {
    document.getElementById(id)?.addEventListener("keydown", (e) => {
      if ((e as KeyboardEvent).key === "Enter") saveSocksAuth();
    });
  });
  document.getElementById("btn-copy-socks-url")?.addEventListener("click", () => {
    const url = (document.getElementById("socks-proxy-url") as HTMLElement | null)?.textContent ?? "";
    clipboardWrite(url);
  });

  (document.getElementById("set-external-link") as HTMLInputElement)?.addEventListener("change", function () {
    settings.external_link = this.value.trim();
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  const toggles: [string, keyof AppSettings][] = [["set-dns", "dns_redirect"], ["set-ipv6", "ipv6"], ["set-ipv6-fallback", "ipv6_fallback"], ["set-ipv6-only", "ipv6_only"], ["set-autostart", "auto_connect"], ["set-bypass-ru", "bypass_ru"]];
  toggles.forEach(([id, key]) => {
    (document.getElementById(id) as HTMLInputElement)?.addEventListener("change", function () {
      (settings as any)[key] = this.checked;
      persistSettings();
      if (isConnected && key !== "auto_connect") showToast(t("reconnectToApply"), "info", 3000);
    });
  });
  (document.getElementById("set-allow-lan") as HTMLInputElement)?.addEventListener("change", function () {
    settings.allow_lan = this.checked;
    persistSettings();
    if (isConnected) showToast(t("reconnectToApply"), "info", 3000);
  });
  document.getElementById("btn-copy-secret")?.addEventListener("click", () => {
    clipboardWrite(settings.secret);
    showToast(t("secretCopied"), "success", 1800);
  });
  document.getElementById("btn-open-repo")?.addEventListener("click", () => invoke("open_url", { url: "https://github.com/nekoskin/Whisp" }).catch(() => { }));
  document.getElementById("btn-open-config")?.addEventListener("click", () => invoke("open_config_dir").catch(() => { }));
  (async () => {
    const el = document.getElementById("whisp-update-status");
    if (!el) return;
    try {
      const info = await invoke<{ tag: string; is_newer: boolean; html_url: string }>("check_for_updates");
      if (info.is_newer && info.tag) {
        el.textContent = `${t("updateAvailableNew")}: ${info.tag}`;
        el.style.cursor = "pointer";
        el.onclick = () => { invoke("open_url", { url: info.html_url }).catch(() => { }); };
      } else {
        el.textContent = t("installed");
      }
    } catch {/**/ }
  })();
  document.getElementById("btn-check-updates")?.addEventListener("click", async () => {
    const btn = document.getElementById("btn-check-updates") as HTMLButtonElement;
    const row = document.getElementById("update-result-row") as HTMLElement;
    const content = document.getElementById("update-result-content") as HTMLElement;
    if (!btn || !row || !content) return;

    btn.disabled = true;
    btn.textContent = t("checking") + "...";
    row.style.display = "none";

    try {
      const info = await invoke<{
        tag: string; name: string; body: string;
        html_url: string; download_url: string; is_newer: boolean;
      }>("check_for_updates");

      row.style.display = "flex";

      if (info.is_newer) {
        osNotify(t("updateAvailableNew"), info.tag);
        content.innerHTML = `
          <div style="font-size:13px;font-weight:600;color:var(--md-primary)">${t("updateAvailableNew")}: ${esc(info.tag)}</div>
          ${info.body ? `<div style="font-size:11px;opacity:.6;margin-top:4px;max-height:80px;overflow-y:auto;white-space:pre-wrap">${esc(info.body.slice(0, 400))}</div>` : ""}
          <div style="display:flex;gap:8px;margin-top:8px;flex-wrap:wrap">
            ${info.download_url ? `<button class="btn-sm" id="btn-install-update">${t("installUpdate")}</button>` : ""}
            <button class="btn-sm" id="btn-open-release" style="opacity:.7">${t("openRelease")}</button>
          </div>`;

        document.getElementById("btn-install-update")?.addEventListener("click", async () => {
          const b = document.getElementById("btn-install-update") as HTMLButtonElement;
          if (b) { b.disabled = true; b.textContent = t("loading") + "..."; }
          try {
            await invoke("install_update", { downloadUrl: info.download_url, htmlUrl: info.html_url });
          } catch (e) {
            showToast(String(e), "error", 5000);
            if (b) { b.disabled = false; b.textContent = t("installUpdate"); }
          }
        });

        document.getElementById("btn-open-release")?.addEventListener("click", () => {
          invoke("open_url", { url: info.html_url }).catch(() => {});
        });
      } else {
        content.innerHTML = `<div style="font-size:13px;color:var(--md-primary)">${t("installed")}${info.tag ? ` (${esc(info.tag)})` : ""}</div>`;
      }
    } catch (e) {
      row.style.display = "flex";
      content.innerHTML = `<div style="color:var(--danger);font-size:12px">${esc(String(e))}</div>`;
    } finally {
      btn.disabled = false;
      btn.textContent = t("checkUpdates");
    }
  });
}

function showProfileModal(prefillKey?: string): void {
  const ov = document.createElement("div");
  ov.className = "modal-overlay";
  ov.innerHTML = `<div class="modal"><h3>${t("addProfile")}</h3>
    <div class="modal-field"><label>${t("profileName")}</label><input type="text" id="modal-name"/></div>
    <div class="modal-field"><label>${t("profileKey")}</label><textarea id="modal-key" rows="3">${prefillKey ? esc(prefillKey) : ""}</textarea></div>
    <div class="modal-actions"><button class="btn-cancel" id="modal-cancel">${t("cancel")}</button><button class="btn-save" id="modal-save">${t("save")}</button></div>
  </div>`;
  document.body.appendChild(ov);
  ov.addEventListener("click", e => { if (e.target === ov) ov.remove(); });
  document.getElementById("modal-cancel")?.addEventListener("click", () => ov.remove());
  document.getElementById("modal-save")?.addEventListener("click", () => {
    const name = (document.getElementById("modal-name") as HTMLInputElement).value.trim();
    const key = (document.getElementById("modal-key") as HTMLTextAreaElement).value.trim();
    if (!name || !key) return;
    const existing = profiles.find(p => p.key === key);
    if (existing) {
      settings.conn_key = key; persistSettings();
      ov.remove(); renderPage();
      showToast(existing.name, "info", 2000);
      return;
    }
    profiles.push({ id: Date.now().toString(), name, key });
    saveProfiles();
    if (!settings.conn_key) { settings.conn_key = key; persistSettings(); }
    ov.remove(); renderPage();
  });
}

const SHARE_QR_ECC = "L";
const SHARE_QR_QUIET_ZONE = 4;
const SHARE_QR_LIGHT = "#ffffff";
const SHARE_QR_DARK = "#000000";
const SHARE_QR_MAX_WIDTH = 320;

interface ShareItem { label: string; payload: string; }

function shareItems(): ShareItem[] {
  return [
    ...profiles.map(p => ({ label: `${t("shareKindKey")}: ${p.name}`, payload: p.key })),
    ...subscriptions.map(s => ({ label: `${t("shareKindSub")}: ${s.name || s.url}`, payload: s.url })),
  ];
}

function activeShareIndex(items: ShareItem[]): number {
  const sub = subscriptions.find(s => s.keys.includes(settings.conn_key));
  const wanted = sub ? sub.url : settings.conn_key;
  return Math.max(0, items.findIndex(i => i.payload === wanted));
}

function shareQrSvg(payload: string): string | null {
  try {
    return renderQrSvg(payload, {
      ecc: SHARE_QR_ECC,
      boostEcc: true,
      border: SHARE_QR_QUIET_ZONE,
      pixelSize: 1,
      whiteColor: SHARE_QR_LIGHT,
      blackColor: SHARE_QR_DARK,
    });
  } catch {
    return null;
  }
}

function fitQrToDevicePixels(svg: SVGSVGElement, available: number): void {
  const modules = svg.viewBox.baseVal.width;
  const ratio = window.devicePixelRatio || 1;
  const pixelsPerModule = Math.max(1, Math.floor(available * ratio / modules));
  const side = `${pixelsPerModule * modules / ratio}px`;
  svg.style.width = side;
  svg.style.height = side;
}

function reportScanFailure(err: unknown): void {
  console.error("qr scan failed", err);
  if (/cancel/i.test(String((err as { message?: string })?.message ?? err ?? ""))) return;
  showToast(t("qrScanFailed"), "error", 2500);
}

function scanBehindOverlay(): Promise<string> {
  return new Promise(resolve => {
    document.documentElement.classList.add("qr-scanning");
    const overlay = document.createElement("div");
    overlay.className = "qr-scan-overlay";
    overlay.innerHTML = `<div class="qr-scan-frame"></div>
      <div class="qr-scan-hint">${t("qrScanHint")}</div>
      <button class="btn-save qr-scan-cancel" id="qr-scan-cancel">${t("cancel")}</button>`;
    document.body.appendChild(overlay);
    let settled = false;
    let backListener: Promise<PluginListener | null> = Promise.resolve(null);
    const finish = (content: string): void => {
      if (settled) return;
      settled = true;
      overlay.remove();
      document.documentElement.classList.remove("qr-scanning");
      void backListener.then(l => l?.unregister());
      resolve(content);
    };
    const stop = (): void => {
      void qrCancel().catch(() => {});
      finish("");
    };
    const cancelButton = overlay.querySelector<HTMLButtonElement>("#qr-scan-cancel")!;
    cancelButton.addEventListener("click", stop);
    cancelButton.focus();
    backListener = onBackButtonPress(stop).catch(() => null);
    qrScan({ windowed: true, formats: [QrFormat.QRCode] })
      .then(result => finish(result?.content ?? ""))
      .catch(err => {
        if (!settled) reportScanFailure(err);
        finish("");
      });
  });
}

async function scanQrAndImport(): Promise<boolean> {
  try {
    let perm = await qrCheckPermissions();
    if (perm !== "granted") perm = await qrRequestPermissions();
    if (perm !== "granted") { showToast(t("qrScanFailed"), "error", 2500); return false; }
  } catch (err) {
    reportScanFailure(err);
    return false;
  }
  const content = await scanBehindOverlay();
  if (!content) return false;
  handlePastedOrScannedText(content);
  return true;
}

function showQrModal(): void {
  const items = shareItems();
  const sharePane = items.length
    ? `<div class="modal-field"><select id="share-qr-item">${items.map((it, i) => `<option value="${i}">${esc(it.label)}</option>`).join("")}</select></div>
    <div class="share-qr-code" id="share-qr-code"></div>
    <div class="share-qr-warning">${t("shareQrWarning")}</div>
    <div class="share-qr-hint">${t("shareQrHint")}</div>`
    : `<div class="share-qr-empty">${t("nothingToShare")}</div>`;
  const ov = document.createElement("div");
  ov.className = "modal-overlay";
  ov.innerHTML = `<div class="modal share-qr-modal"><h3>${t("qrCode")}</h3>
    ${isAndroid ? pills("data-qrtab", "share", [["share", t("qrTabShare")], ["scan", t("qrTabScan")]]) : ""}
    ${sharePane}
    <div class="modal-actions">${items.length ? `<button class="btn-cancel" id="share-qr-copy">${t("copy")}</button>` : ""}<button class="btn-save" id="share-qr-close">${t("close")}</button></div>
  </div>`;
  document.body.appendChild(ov);
  const life = new AbortController();
  const close = (): void => {
    life.abort();
    ov.remove();
  };
  ov.addEventListener("click", e => { if (e.target === ov) close(); });
  ov.querySelector("#share-qr-close")?.addEventListener("click", close);
  const scanTab = ov.querySelector<HTMLElement>('[data-qrtab="scan"]');
  scanTab?.addEventListener("click", async () => {
    close();
    if (!(await scanQrAndImport())) showQrModal();
  });
  const select = ov.querySelector<HTMLSelectElement>("#share-qr-item");
  if (!select) {
    (scanTab ?? ov.querySelector<HTMLElement>("#share-qr-close"))?.focus();
    return;
  }
  const code = ov.querySelector<HTMLElement>("#share-qr-code")!;
  const selected = (): ShareItem => items[Number(select.value)];
  const draw = (): void => {
    const svg = shareQrSvg(selected().payload);
    code.innerHTML = svg ?? `<div class="share-qr-error">${t("shareQrTooLong")}</div>`;
    const drawn = code.querySelector("svg");
    if (drawn) fitQrToDevicePixels(drawn, Math.min(code.clientWidth, SHARE_QR_MAX_WIDTH));
  };
  select.value = String(activeShareIndex(items));
  draw();
  select.addEventListener("change", draw);
  window.addEventListener("resize", draw, { signal: life.signal });
  ov.querySelector("#share-qr-copy")?.addEventListener("click", async () => {
    try {
      await clipboardWrite(selected().payload);
      showToast(t("copied"), "success", 2000);
    } catch {
      showToast(t("clipboardWriteFailed"), "error", 2500);
    }
  });
  select.focus();
}

function classifyPastedText(text: string): { kind: "key" | "url" | "unknown"; value: string } {
  const trimmed = text.trim();
  if (/^whispera:\/\//i.test(trimmed)) return { kind: "key", value: trimmed };
  if (/^https?:\/\//i.test(trimmed)) return { kind: "url", value: trimmed };
  return { kind: "unknown", value: trimmed };
}

function addKeyDirect(key: string): void {
  const existing = profiles.find(p => p.key === key);
  if (existing) {
    settings.conn_key = key; persistSettings();
    renderPage();
    showToast(existing.name, "info", 2000);
    return;
  }
  const name = "Key-" + Math.random().toString(36).slice(2, 6).toUpperCase();
  profiles.push({ id: Date.now().toString(), name, key });
  saveProfiles();
  if (!settings.conn_key) { settings.conn_key = key; persistSettings(); }
  renderPage();
  showToast(name, "success", 2000);
}

function handlePastedOrScannedText(text: string): void {
  const { kind, value } = classifyPastedText(text);
  if (kind === "key") addKeyDirect(value);
  else if (kind === "url") showSubModal(value);
  else showToast(t("pasteUnrecognized"), "error", 3000);
}

function esc(s: string): string { return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;"); }

/* ===================== TV / D-PAD NAVIGATION ===================== */
const DPAD_FOCUSABLE = 'a[href],button:not([disabled]),input:not([disabled]),textarea:not([disabled]),select:not([disabled]),[tabindex]:not([tabindex="-1"]),.bnav-item,.pill-btn,.km-item,label.toggle';
const DPAD_NEEDS_TABINDEX = '.bnav-item,.pill-btn,.km-item,label.toggle';

function dpadFocusables(): HTMLElement[] {
  const overlays = document.querySelectorAll<HTMLElement>(".modal-overlay");
  const scope: ParentNode = overlays.length ? overlays[overlays.length - 1] : document;
  return Array.from(scope.querySelectorAll<HTMLElement>(DPAD_FOCUSABLE))
    .filter(el => el.offsetParent !== null && el.getClientRects().length > 0 && !el.hasAttribute("disabled"));
}

function dpadEnsureTabindex(): void {
  document.querySelectorAll<HTMLElement>(DPAD_NEEDS_TABINDEX).forEach(el => {
    if (!el.hasAttribute("tabindex")) el.tabIndex = 0;
  });
}

function dpadIsTyping(el: Element | null): boolean {
  const t = el as HTMLInputElement | null;
  if (!t) return false;
  if (t.tagName === "TEXTAREA") return true;
  return t.tagName === "INPUT" && t.type !== "checkbox" && t.type !== "radio";
}

function dpadMove(dir: "up" | "down" | "left" | "right"): void {
  const list = dpadFocusables();
  if (!list.length) return;
  const cur = document.activeElement as HTMLElement | null;
  if (!cur || !list.includes(cur)) { list[0].focus(); return; }
  const cr = cur.getBoundingClientRect();
  const cx = cr.left + cr.width / 2, cy = cr.top + cr.height / 2;
  let best: HTMLElement | null = null, bestScore = Infinity;
  for (const el of list) {
    if (el === cur) continue;
    const r = el.getBoundingClientRect();
    const dx = r.left + r.width / 2 - cx, dy = r.top + r.height / 2 - cy;
    let primary: number, cross: number;
    if (dir === "up") { if (dy >= -1) continue; primary = -dy; cross = Math.abs(dx); }
    else if (dir === "down") { if (dy <= 1) continue; primary = dy; cross = Math.abs(dx); }
    else if (dir === "left") { if (dx >= -1) continue; primary = -dx; cross = Math.abs(dy); }
    else { if (dx <= 1) continue; primary = dx; cross = Math.abs(dy); }
    const score = primary + cross * 2.5;
    if (score < bestScore) { bestScore = score; best = el; }
  }
  if (best) { best.scrollIntoView({ block: "nearest", inline: "nearest" }); best.focus(); }
}

function setupDpadNav(): void {
  dpadEnsureTabindex();
  let scheduled = false;
  const obs = new MutationObserver(() => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => { scheduled = false; dpadEnsureTabindex(); });
  });
  obs.observe(document.body, { childList: true, subtree: true });

  document.addEventListener("keydown", (e) => {
    const active = document.activeElement;
    const typing = dpadIsTyping(active);
    switch (e.key) {
      case "ArrowUp": dpadMove("up"); e.preventDefault(); break;
      case "ArrowDown": dpadMove("down"); e.preventDefault(); break;
      case "ArrowLeft": if (typing) return; dpadMove("left"); e.preventDefault(); break;
      case "ArrowRight": if (typing) return; dpadMove("right"); e.preventDefault(); break;
      case "Enter":
        if (active && !typing) { (active as HTMLElement).click(); e.preventDefault(); }
        break;
    }
  });

  setTimeout(() => {
    if (!dpadFocusables().includes(document.activeElement as HTMLElement)) dpadFocusables()[0]?.focus();
  }, 300);
}

/* ===================== ANDROID BACK BUTTON ===================== */
function openDismissable(): HTMLElement | null {
  const overlays = document.querySelectorAll<HTMLElement>(".modal-overlay");
  if (overlays.length) return overlays[overlays.length - 1];
  const picker = document.getElementById("process-picker-overlay");
  if (picker && picker.style.display !== "none") return picker;
  return document.querySelector<HTMLElement>(".key-menu:not([hidden])");
}

function dismissTopmost(): void {
  if (document.documentElement.classList.contains("qr-scanning")) return;
  const top = openDismissable();
  if (!top) return;
  if (top.classList.contains("key-menu")) { top.hidden = true; return; }
  const closer = top.querySelector<HTMLElement>('[id$="-close"], [id$="-cancel"]');
  if (closer) { closer.click(); return; }
  top.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  if (top.isConnected && top.classList.contains("modal-overlay")) top.remove();
}

function setupBackToDismiss(): void {
  let listener: Promise<PluginListener | null> | null = null;
  let scheduled = false;
  const sync = (): void => {
    scheduled = false;
    const open = openDismissable() !== null;
    if (open && !listener) listener = onBackButtonPress(dismissTopmost).catch(() => null);
    if (!open && listener) {
      const active = listener;
      listener = null;
      void active.then(l => l?.unregister());
    }
  };
  new MutationObserver(() => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(sync);
  }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["hidden", "style"] });
}

/* ===================== INIT ===================== */
window.addEventListener("DOMContentLoaded", async () => {
  loadLang(); loadProfiles();
  syncNativeLang();
  setupDpadNav();
  if (isAndroid) setupBackToDismiss();
  await loadSettings();
  initNotifications().catch(() => {});
  document.addEventListener("click", () => {
    document.querySelectorAll<HTMLElement>(".key-menu").forEach(m => { m.hidden = true; });
  });
  const [, , , statusKnown] = await Promise.all([
    loadSubscriptions(),
    loadRoutingRules(),
    loadBlocklist(),
    checkStatus(),
  ]);
  // The user's own last action is the one thing we always know: connecting
  // stores the timestamp, disconnecting removes it. When the backend cannot be
  // asked, show that instead of guessing, and never act on the guess.
  if (!statusKnown) {
    isConnected = localStorage.getItem("connectTime") !== null;
  }
  renderShell();
  fetchSysInfo();
  startSubAutoCheck();
  try {
    const hist = await invoke<string[]>("read_vpn_log_history");
    hist.forEach(l => addLog(l));
  } catch { /* ignore */ }
  startLogPolling();

  // On Android autostart means "reconnect once after a phone reboot" — handled
  // natively by BootReceiver, NOT on every app launch. So skip auto-connect on
  // open for Android; on desktop keep connect-on-launch behaviour.
  if (!isAndroid && statusKnown && settings.auto_connect && !enginesUp && settings.conn_key && !isConnecting) {
    doConnect();
  }
  setInterval(() => { if (isConnected && connectTime) tickUptime(); }, 1000);

  // silent periodic status check — no re-render unless status changed
  setInterval(async () => {
    const prev = isConnected;
    await checkStatus();
    if (prev !== isConnected) updateHome();
  }, 10000);
});
