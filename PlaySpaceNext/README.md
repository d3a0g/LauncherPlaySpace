# PlaySpace 2.0 — нативное приложение / Native app

Игровой лаунчер Windows с интерфейсом **React / TypeScript / SCSS**, сборкой **Vite** и нативными службами **Rust / Tauri 2**. Библиотека, рекомендации, мастерская, графика, сохранения, сеть и настройки используют общую навигацию. C#-адаптер и .NET не требуются. Основной интерфейс и инструменты поддерживают RU/EN; отдельные технические сообщения могут оставаться на английском.

[Продукт](../README.md) · [Документация](../docs/README.md) · [Изменения 2.0.0](../docs/releases/2.0.0.md) · [Скачать](https://github.com/d3a0g/playspace-releases/releases/latest)

## Запуск готовой сборки

Нужны Windows 10/11 x64 и WebView2 Runtime. Установи `LauncherPlaySpace-2.0.0-Setup.exe` либо распакуй `LauncherPlaySpace-2.0.0-win-x64.zip` и запусти **LauncherPlaySpace.exe**. Это нативный Rust EXE, переименованный при упаковке из Cargo-артефакта `playspace-next.exe`. Установка и обновления используют Inno Setup.

[Установка и импорт](../docs/2.0/INSTALLATION.md)

## Возможности и ограничения

- Библиотека установленных Steam/Epic/Riot игр, ручное добавление EXE, коллекции, закрепления, три представления и собственные обложки.
- Запуск через клиент или EXE, наблюдение за процессом, история и время; статистика и достижения Steam.
- Рекомендации Steam, жанры, поиск, скидки Steam и акции Epic; доступные сохранённые данные работают офлайн.
- 78 мод-адаптеров: 36 Thunderstore, 37 локальных и 5 Paradox. Проверка архивов, зависимости, учёт файлов, защита изменённых файлов при удалении и профили.
- Teardown: публичный Workshop с поиском, категориями, тремя сортировками и страницами по 30; кеш на пять минут и офлайн-страницы. Подписку и загрузку выполняет Steam. PlaySpace отдельно импортирует один ZIP-мод с `info.txt` в фактические Windows Documents/Teardown/mods. Подписки не входят в ZIP-профили; глобальные моды и карты запускаются в меню игры.
- Копии сохранений с хешами, проверка и план восстановления, страховочная копия; перенос игры с проверкой файлов и сохранением исходной папки.
- Поддерживаемые графические конфигурации, DLSS/OptiScaler, экспериментальный UniversalDLSS5; загрузки, WARP, звуки, трей и большой экран с Gamepad API.

Полное соответствие старому клиенту не заявляется. Глубокий поиск произвольных EXE по всем дискам, все прежние темы/настройки доступности и расширенное управление очередью не перенесены полностью. Совместимость модов и графических компонентов зависит от игры и оборудования; заданный FPS не гарантируется. Реальные игровые сессии и физический геймпад не тестировались.

[Моды и Workshop](../docs/2.0/MODS.md)

## Профиль и импорт

Release: `%LOCALAPPDATA%/io.playspace.next.local`; debug: `PlaySpaceNext/.local-data`. Для тестов путь переопределяется через `PLAYSPACE_NEXT_PROFILE`. Идентификатор сохранён для данных локальных сборок 2.0.

Импорт копирует библиотеку из `%LOCALAPPDATA%/GameLauncherPRO` с сохранением неизвестных полей; старый профиль не изменяется. Настройки, Steam-секреты и разрешения администратора автоматически не переносятся. Steam подключается отдельно; секреты хранятся в Windows Credential Manager.

Фоновая проверка удалённых игр работает каждые 15 секунд. После 30 секунд подтверждённого отсутствия запись может быть убрана, с сохранением полной записи в `removed-games` внутри профиля. Недоступные диски, неоднозначные пути, обновления Steam и активные сеансы защищены. Игровые файлы и сохранения этой проверкой не удаляются.

## Разработка

Нужны Node.js/npm, Rust stable MSVC x64, Visual Studio C++ Build Tools с Windows SDK и WebView2. Из этой папки:

```powershell
npm ci
npm run tauri dev
```

Проверки и сборка:

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build -- --no-bundle
```

`npm run dev` — веб-предпросмотр без системных операций. Cargo собирает `src-tauri/target/release/playspace-next.exe`; публичный пакет содержит `LauncherPlaySpace.exe`.

`tests/desktop.cjs` проверяет настоящий WebView2/IPC на изолированном профиле; `PLAYSPACE_NEXT_EXE` выбирает EXE. `tests/teardown.cjs` проверяет Workshop, кеш, офлайн и обе локали. Отчёты — в `test-results`. Эти сценарии не подтверждают реальные игровые сессии или совместимость каждого мода. Среда тестов описана в [DEVELOPMENT.md](../DEVELOPMENT.md).

## Обновления

Нативный updater проверяет подпись пакета по доверенному ключу, показывает прогресс и допускает отмену до установки. Канал 2.x — `latest-v2.json`; переход старого клиента использует совместимый `latest.json`. Оба пути поставляют нативный Inno-установщик.

Публичный ключ находится в `src-tauri/update-public-key.pub`, URL канала задан в `src-tauri/src/updater.rs` и `tauri.conf.json`. Подпись привязана к номеру версии (`requireSignedVersion`). Закрытый ключ не входит в репозиторий. **Windows Authenticode не подтверждён** и не заменяется подписью updater. Итоговые проверки опубликованной сборки перечислены в release notes.

[Каналы обновлений](../docs/2.0/UPDATES.md)

## Поддержка и лицензии

[Поддержка](https://t.me/PlaySpaceSupport_bot) · [Новости](https://t.me/LauncherPlaySpace) · [Добровольная поддержка через Robokassa](https://auth.robokassa.ru/merchant/Invoice/IYhjSKAkQEeZj7JPhZEHwA)

Приложение бесплатно; пожертвования не покупают функции или услуги. [Условия поддержки](../docs/legal/README.md). [MIT](../LICENSE) · [Лицензии компонентов](../THIRD_PARTY_NOTICES.md). Внешние графические компоненты имеют собственные условия и не включаются автоматически.

**English:** PlaySpace 2.0 uses React/TypeScript and Rust/Tauri with Russian and English UI. Run the distributed `LauncherPlaySpace.exe` on Windows x64 with WebView2. Build here with `npm ci` and `npm run tauri dev`; Rust MSVC and Windows C++ build tools are required. Legacy import copies the library only. Steam manages Workshop subscriptions; PlaySpace manages imported Teardown ZIP mods separately. Real gameplay, physical controllers and Windows code signing have not been verified.

[Поддержать через Robokassa — RU](https://auth.robokassa.ru/merchant/Invoice/IYhjSKAkQEeZj7JPhZEHwA) · [Support via Robokassa — EN](https://auth.robokassa.ru/merchant/Invoice/KL10bltZtkCT1pfk7R0EVQ)
