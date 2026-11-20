# Документация PlaySpace 2.0 / Documentation

PlaySpace 2.0 — приложение Windows на React/TypeScript и Rust/Tauri с интерфейсом RU/EN. Основные руководства относятся к нативной поставке.

| Руководство | Содержание |
| --- | --- |
| [Установка](2.0/INSTALLATION.md) | Windows x64, WebView2, Inno, ZIP, запуск `LauncherPlaySpace.exe` и импорт библиотеки. |
| [Мастерская модов](2.0/MODS.md) | 78 адаптеров, Thunderstore, локальные ZIP, Paradox и Teardown Workshop; ограничения профилей. |
| [Обновления](2.0/UPDATES.md) | Подписанные обновления 2.x через `latest-v2.json`, совместимый канал 1.x `latest.json`, Inno и статус проверок. |
| [Удалённые настройки](2.0/REMOTE-CONFIG.md) | Подписанные ссылки, объявления, переводы и каталоги без последующих обновлений EXE. |
| [Выпуск 2.0.0](releases/2.0.0.md) | Изменения, перенос данных и границы тестирования. |
| [Разработка](../DEVELOPMENT.md) | Среда, сборка, структура, проверки и упаковка. |
| [Нативный проект](../PlaySpaceNext/README.md) | Запуск из исходников, профиль и службы приложения. |
| [Сообщить об уязвимости](../SECURITY.md) | Правила сообщения о проблемах безопасности. |
| [Лицензии компонентов](../THIRD_PARTY_NOTICES.md) | Условия сторонних зависимостей. |
| [Добровольная поддержка](legal/README.md) | Условия поддержки и возврата; [публичная оферта](legal/support-offer.md). |

Скачать **LauncherPlaySpace-2.0.0-Setup.exe** или **LauncherPlaySpace-2.0.0-win-x64.zip**: [последний выпуск](https://github.com/d3a0g/playspace-releases/releases/latest). [Скриншоты](../README.md#скриншоты).

Профиль 2.0: `%LOCALAPPDATA%/io.playspace.next.local`. Профиль 1.x `%LOCALAPPDATA%/GameLauncherPRO` сохраняется; импорт копирует библиотеку, без автоматического переноса старых настроек и секретов.

Подпись обновления и кодовая подпись Windows — разные проверки. Authenticode не подтверждён. Реальные игровые сессии и физический геймпад не тестировались; наличие адаптера не гарантирует совместимость каждого мода.

Новости: [@LauncherPlaySpace](https://t.me/LauncherPlaySpace). Помощь: [@PlaySpaceSupport_bot](https://t.me/PlaySpaceSupport_bot) или [GitHub Issues](https://github.com/d3a0g/LauncherPlaySpace/issues). [Поддержать через Robokassa](https://auth.robokassa.ru/merchant/Invoice/IYhjSKAkQEeZj7JPhZEHwA) — добровольно, приложение бесплатно.

**English:** Start with [Installation](2.0/INSTALLATION.md), [Mods](2.0/MODS.md) and [Updates](2.0/UPDATES.md). Run the native `LauncherPlaySpace.exe`; WebView2 is required, .NET is not. Legacy library import preserves the old profile without automatically migrating settings. See [2.0.0 release notes](releases/2.0.0.md) for changes and verification limits.

[Поддержать через Robokassa — RU](https://auth.robokassa.ru/merchant/Invoice/IYhjSKAkQEeZj7JPhZEHwA) · [Support via Robokassa — EN](https://auth.robokassa.ru/merchant/Invoice/KL10bltZtkCT1pfk7R0EVQ)
