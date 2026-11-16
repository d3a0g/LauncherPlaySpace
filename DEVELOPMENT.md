# Разработка PlaySpace 2.0 / Development

Публичные исходники: [d3a0g/LauncherPlaySpace](https://github.com/d3a0g/LauncherPlaySpace). Пакеты: [последний выпуск](https://github.com/d3a0g/playspace-releases/releases/latest). Публичная поставка 2.0 содержит нативный проект `PlaySpaceNext`, документацию и выбранные инструменты сопровождения. C#-клиент и переходный bridge в неё не входят; прежняя реализация сохраняется в истории и приватных материалах.

## Среда и запуск

Нужны Windows 10/11 x64, Node.js/npm, Rust stable для `x86_64-pc-windows-msvc`, Visual Studio C++ Build Tools с Windows SDK и WebView2 Runtime. Зависимости фиксируются в `PlaySpaceNext/package-lock.json` и `PlaySpaceNext/src-tauri/Cargo.lock`.

```powershell
cd PlaySpaceNext
npm ci
npm run tauri dev
```

Интерфейс — React 19, TypeScript, SCSS и Vite; системные операции — Rust/Tauri 2. `npm run dev` запускает только веб-предпросмотр. .NET для основного приложения не нужен.

## Структура

| Путь | Назначение |
| --- | --- |
| `PlaySpaceNext/src` | Библиотека, рекомендации, встроенные инструменты, мастерская, большой экран и RU/EN. |
| `PlaySpaceNext/src-tauri/src` | Обнаружение и запуск игр, профиль, Steam, моды, сохранения, графика, загрузки, обновления и Windows. |
| `PlaySpaceNext/src-tauri/catalog` | Каталоги и 78 адаптеров: 36 Thunderstore, 37 локальных, 5 Paradox. |
| `PlaySpaceNext/tests` | Сценарии настоящего Tauri/WebView2 и тестовые процессы. |
| `docs/2.0` | Пользовательские руководства нативной версии. |
| `docs/releases` | Изменения выпусков и ограничения проверок. |

## Проверки и сборка

Из `PlaySpaceNext`:

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build -- --no-bundle
```

Rust-тесты используют временные данные библиотеки, сохранений, модов и переноса файлов. Сетевые проверки запускаются отдельно и зависят от внешних сервисов. Frontend build проверяет TypeScript и собирает Vite; он не проверяет IPC или Windows.

`tests/desktop.cjs` проверяет настоящий WebView2 на отдельном профиле. Нужны собранный EXE, Node.js и Playwright (`playwright-core` либо путь `PLAYSPACE_PLAYWRIGHT_PACKAGE`). Сценарий запуска тестового процесса использует отдельную .NET-фикстуру, которая не является зависимостью приложения. `PLAYSPACE_NEXT_EXE` позволяет выбрать упакованный `LauncherPlaySpace.exe` вместо исходного `src-tauri/target/release/playspace-next.exe`. Отчёты — в `test-results`.

`tests/teardown.cjs` проверяет Workshop, кеш, офлайн, RU/EN и геометрию. Путь к исходному release EXE указан в сценарии; первоначальное заполнение кеша требует сети. Подписку Steam и запуск Teardown сценарий не выполняет.

При изменениях интерфейса проверяйте RU/EN и минимальное окно 900×640. Перед публикацией отдельно проверяйте финальные установщик, ZIP и переход между версиями. Исторические результаты не означают повторный прогон финальной сборки.

## Профили и импорт

Release: `%LOCALAPPDATA%/io.playspace.next.local`; debug: `PlaySpaceNext/.local-data`. Для изоляции задавайте `PLAYSPACE_NEXT_PROFILE` на отдельную тестовую папку. Идентификатор сохраняется ради данных локальных сборок 2.0.

Импорт из `%LOCALAPPDATA%/GameLauncherPRO` копирует библиотеку с сохранением неизвестных полей и не изменяет старый профиль. Настройки 1.x, Steam-секреты и административные разрешения автоматически не переносятся. Подключение Steam хранится отдельно в Windows Credential Manager.

## Упаковка и обновления

Файлы поставки: `LauncherPlaySpace-2.0.0-Setup.exe`, `LauncherPlaySpace-2.0.0-win-x64.zip`, внутри — **LauncherPlaySpace.exe**. Cargo создаёт `playspace-next.exe`; при упаковке нативный файл переименовывается.

Единый установщик — Inno Setup. Канал `latest.json` предназначен для совместимого перехода с 1.x, `latest-v2.json` — для подписанных обновлений 2.x. Форматы каналов и проверок подписи различаются; не считайте их взаимозаменяемыми. См. [руководство](docs/2.0/UPDATES.md).

Публичный ключ updater хранится в `PlaySpaceNext/src-tauri/update-public-key.pub`, URL — в `updater.rs` и `tauri.conf.json`. Подпись включает номер версии (`requireSignedVersion`). Закрытый ключ отсутствует в репозитории; официальный ключ владельца локально защищён Windows DPAPI. `tools/Build-PlaySpace2Release.ps1` собирает нативный payload, проверяет Microsoft bootstrapper, создаёт Inno/ZIP и подписывает пакет. `tools/Test-PlaySpace2Installer.ps1` проверяет установку и переход 1.0.6 → 2.0 на отдельном пути. Windows Authenticode отдельно не подтверждён; подпись обновления его не заменяет.

## Совместная работа и приватные данные

Проверяйте ветку и рабочее дерево; не перезаписывайте чужие изменения снимком каталога. Коммиты, push и публикация выполняются по актуальному поручению. Публикация исходников сама по себе не создаёт пользовательский выпуск и не переключает канал обновлений.

Не включайте токены, закрытые ключи подписи, личные библиотеки, `.local-data`, `.secrets`, `.tools`, `artifacts`, `target`, обращения поддержки и архивы переписки. Тесты не должны использовать рабочие сохранения или рабочий бот без отдельной задачи.

Реальные игровые сессии, физический геймпад и Windows Authenticode имеющимися проверками не подтверждены. Не обещайте прирост FPS или универсальную совместимость графических компонентов.

**English:** Build inside `PlaySpaceNext` with `npm ci` and `npm run tauri dev`. Rust MSVC, Windows C++ build tools and WebView2 are required. The shipped executable is `LauncherPlaySpace.exe`. Legacy import copies the library only, without automatic settings or credential migration. Keep personal data and signing secrets out of source control. Real gameplay and Windows code signing have not been verified.
