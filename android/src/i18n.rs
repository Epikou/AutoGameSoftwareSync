//! Textes de l'application Android, 5 langues (en, fr, ru, es, zh). Memes mots que le PC (coffre, proteger, remonter le
//! temps, appareil). Les traductions RU / ES / ZH sont automatiques et n'ont pas ete relues par des locuteurs natifs.
//! Une cle sans l'une des 5 langues ne compile pas (tuple de 5). Tout texte visible passe par ici : notes des emulateurs,
//! libelles d'emplacements des profils, messages d'erreur du moteur (table de correspondance), pluriels.
pub use agss::i18n::Lang;

macro_rules! texts {
    ($($id:ident => ($en:expr, $fr:expr, $ru:expr, $es:expr, $zh:expr)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum T { $($id),* }
        pub const ALL: &[T] = &[$(T::$id),*];
        pub fn tr(l: Lang, t: T) -> &'static str {
            match t {
                $(T::$id => match l { Lang::En => $en, Lang::Fr => $fr, Lang::Ru => $ru, Lang::Es => $es, Lang::Zh => $zh }),*
            }
        }
    };
}

texts! {
    AppName => ("AutoGameSoftwareSync", "AutoGameSoftwareSync", "AutoGameSoftwareSync", "AutoGameSoftwareSync", "AutoGameSoftwareSync"),
    Next => ("Next", "Suivant", "Далее", "Siguiente", "下一步"),
    Back => ("Back", "Retour", "Назад", "Atrás", "返回"),
    Cancel => ("Cancel", "Annuler", "Отмена", "Cancelar", "取消"),
    Close => ("Close", "Fermer", "Закрыть", "Cerrar", "关闭"),
    Ok => ("OK", "OK", "ОК", "Aceptar", "确定"),
    Yes => ("Yes", "Oui", "Да", "Sí", "是"),
    No => ("No", "Non", "Нет", "No", "否"),
    Wait => ("Please wait…", "Un instant…", "Подождите…", "Un momento…", "请稍候…"),
    PadHint => ("A select · B back · L1/R1 scroll", "A valider · B retour · L1/R1 défiler", "A выбрать · B назад · L1/R1 прокрутка", "A elegir · B atrás · L1/R1 desplazar", "A 选择 · B 返回 · L1/R1 滚动"),
    Step => ("Step {n} of {total}", "Étape {n} sur {total}", "Шаг {n} из {total}", "Paso {n} de {total}", "第 {n} 步，共 {total} 步"),

    // Premier lancement
    LangTitle => ("Choose your language", "Choisis ta langue", "Выберите язык", "Elige tu idioma", "选择语言"),
    PermTitle => ("Allow access to your files", "Autorise l'accès à tes fichiers", "Разрешите доступ к файлам", "Permite el acceso a tus archivos", "允许访问你的文件"),
    PermBody => ("The app reads and writes the save folders of your emulators and your vault folder. Android asks you to allow « All files access » for this app, then come back here.", "L'appli lit et écrit les dossiers de sauvegarde de tes émulateurs et ton coffre. Android te demande d'autoriser « Accès à tous les fichiers » pour cette appli, puis reviens ici.", "Приложение читает и записывает папки сохранений эмуляторов и папку хранилища. Android попросит разрешить «Доступ ко всем файлам» для этого приложения, затем вернитесь сюда.", "La app lee y escribe las carpetas de guardado de tus emuladores y tu caja fuerte. Android te pedirá permitir « Acceso a todos los archivos » para esta app; luego vuelve aquí.", "本应用会读写模拟器的存档文件夹和你的保险库文件夹。Android 会要求你为本应用允许「所有文件访问权限」，然后回到这里。"),
    PermButton => ("Open the permission screen", "Ouvrir l'écran d'autorisation", "Открыть экран разрешений", "Abrir la pantalla de permisos", "打开权限设置"),
    PermOk => ("Access granted", "Accès accordé", "Доступ разрешён", "Acceso concedido", "已授权"),
    PermMissing => ("Not granted yet", "Pas encore accordé", "Ещё не разрешено", "Aún no concedido", "尚未授权"),
    PermLostTitle => ("File access is missing", "Accès aux fichiers manquant", "Нет доступа к файлам", "Falta el acceso a los archivos", "缺少文件访问权限"),
    PermLostBody => ("« All files access » has been switched off for this app. It cannot see your saves any more: nothing is copied or put back until you allow it again.", "« Accès à tous les fichiers » a été retiré à cette appli. Elle ne voit plus tes sauvegardes : rien n'est copié ni remis tant que tu ne l'autorises pas à nouveau.", "«Доступ ко всем файлам» для этого приложения отключён. Оно больше не видит сохранения: ничего не копируется и не возвращается, пока вы снова не разрешите доступ.", "« Acceso a todos los archivos » se ha desactivado para esta app. Ya no ve tus guardados: nada se copia ni se restablece hasta que lo permitas de nuevo.", "本应用的「所有文件访问权限」已被关闭。它无法再看到你的存档：在你重新允许之前，不会复制或恢复任何内容。"),
    DeviceTitle => ("Name this device", "Donne un nom à cet appareil", "Назовите это устройство", "Ponle nombre a este dispositivo", "为此设备命名"),
    DeviceBody => ("This name and colour mark the versions made here, on every device.", "Ce nom et cette couleur marquent les versions faites ici, sur tous tes appareils.", "Это имя и цвет отмечают версии, сделанные здесь, на всех устройствах.", "Este nombre y color marcan las versiones hechas aquí, en todos tus dispositivos.", "此名称和颜色会在所有设备上标记在这里创建的版本。"),
    DeviceName => ("Device name", "Nom de l'appareil", "Имя устройства", "Nombre del dispositivo", "设备名称"),
    DeviceColor => ("Colour", "Couleur", "Цвет", "Color", "颜色"),
    DeviceNameEmpty => ("Give the device a name.", "Donne un nom à l'appareil.", "Введите имя устройства.", "Ponle un nombre al dispositivo.", "请输入设备名称。"),
    KbShow => ("On-screen keyboard", "Clavier à l'écran", "Экранная клавиатура", "Teclado en pantalla", "屏幕键盘"),
    KbHide => ("Hide the keyboard", "Masquer le clavier", "Скрыть клавиатуру", "Ocultar el teclado", "隐藏键盘"),
    KbSpace => ("Space", "Espace", "Пробел", "Espacio", "空格"),
    KbDone => ("Done", "Terminé", "Готово", "Listo", "完成"),
    VaultTitle => ("Choose your vault", "Choisis ton coffre", "Выберите хранилище", "Elige tu caja fuerte", "选择保险库"),
    VaultBody => ("Your vault is a folder that another app keeps in sync with your PC (Syncthing, FolderSync, Autosync…). Pick that folder here: the same vault on the PC and on this device.", "Ton coffre est un dossier qu'une autre appli garde synchronisé avec ton PC (Syncthing, FolderSync, Autosync…). Choisis ce dossier ici : le même coffre sur le PC et sur cet appareil.", "Хранилище — это папка, которую другое приложение синхронизирует с вашим ПК (Syncthing, FolderSync, Autosync…). Выберите эту папку здесь: одно хранилище на ПК и на этом устройстве.", "Tu caja fuerte es una carpeta que otra app mantiene sincronizada con tu PC (Syncthing, FolderSync, Autosync…). Elige esa carpeta aquí: la misma caja fuerte en el PC y en este dispositivo.", "保险库是由另一个应用（Syncthing、FolderSync、Autosync…）与电脑保持同步的文件夹。在这里选择该文件夹：电脑和本设备使用同一个保险库。"),
    VaultFound => ("Folders found", "Dossiers trouvés", "Найденные папки", "Carpetas encontradas", "找到的文件夹"),
    VaultBrowse => ("Browse…", "Parcourir…", "Обзор…", "Explorar…", "浏览…"),
    VaultSyncthing => ("Syncthing folder", "Dossier Syncthing", "Папка Syncthing", "Carpeta de Syncthing", "Syncthing 文件夹"),
    VaultExisting => ("Vault already used by {devices}", "Coffre déjà utilisé par {devices}", "Хранилище уже используется: {devices}", "Caja fuerte ya usada por {devices}", "已被 {devices} 使用的保险库"),
    VaultNamed => ("Folder named AutoGameSoftwareSync", "Dossier nommé AutoGameSoftwareSync", "Папка с именем AutoGameSoftwareSync", "Carpeta llamada AutoGameSoftwareSync", "名为 AutoGameSoftwareSync 的文件夹"),
    VaultNone => ("No sync folder found. Install Syncthing (or another sync app), share a folder with your PC, then come back and pick it with Browse.", "Aucun dossier synchronisé trouvé. Installe Syncthing (ou une autre appli de synchro), partage un dossier avec ton PC, puis reviens le choisir avec Parcourir.", "Синхронизируемая папка не найдена. Установите Syncthing (или другое приложение), откройте общую папку с ПК, затем вернитесь и выберите её через Обзор.", "No se encontró ninguna carpeta sincronizada. Instala Syncthing (u otra app de sincronización), comparte una carpeta con tu PC y vuelve a elegirla con Explorar.", "未找到同步文件夹。请安装 Syncthing（或其他同步应用），与电脑共享一个文件夹，然后回来用「浏览」选择它。"),
    VaultChosen => ("Vault: {path}", "Coffre : {path}", "Хранилище: {path}", "Caja fuerte: {path}", "保险库：{path}"),
    VaultWillCreate => ("A sub-folder AutoGameSoftwareSync is created inside; nothing else is touched.", "Un sous-dossier AutoGameSoftwareSync y est créé ; rien d'autre n'est touché.", "Внутри создаётся подпапка AutoGameSoftwareSync; больше ничего не меняется.", "Se crea dentro una subcarpeta AutoGameSoftwareSync; nada más se toca.", "会在其中创建 AutoGameSoftwareSync 子文件夹；不会改动其他内容。"),
    VaultError => ("This folder cannot be used: {msg}", "Ce dossier ne peut pas servir : {msg}", "Эту папку нельзя использовать: {msg}", "Esta carpeta no sirve: {msg}", "无法使用此文件夹：{msg}"),
    ItemsTitle => ("What do you want to protect?", "Que veux-tu protéger ?", "Что защитить?", "¿Qué quieres proteger?", "要保护什么？"),
    ItemsBody => ("Emulators found on this device. Save states are unticked: large, and tied to the emulator version.", "Émulateurs trouvés sur cet appareil. Les états instantanés sont décochés : lourds, et liés à la version de l'émulateur.", "Эмуляторы, найденные на устройстве. Быстрые сохранения не отмечены: они большие и зависят от версии эмулятора.", "Emuladores encontrados en este dispositivo. Los estados guardados están sin marcar: pesados y ligados a la versión del emulador.", "在此设备上找到的模拟器。即时存档默认不勾选：体积大，且依赖模拟器版本。"),
    ItemsNone => ("No emulator folder found. See « Emulators » in Settings for where each emulator must store its files, or protect a personal folder.", "Aucun dossier d'émulateur trouvé. Voir « Émulateurs » dans les Réglages pour l'endroit où chaque émulateur doit ranger ses fichiers, ou protège un dossier perso.", "Папки эмуляторов не найдены. См. «Эмуляторы» в настройках: где каждый эмулятор должен хранить файлы, или защитите личную папку.", "No se encontró ninguna carpeta de emulador. Mira « Emuladores » en Ajustes para saber dónde debe guardar cada emulador, o protege una carpeta personal.", "未找到模拟器文件夹。请在设置中的「模拟器」查看每个模拟器应把文件放在哪里，或保护一个个人文件夹。"),
    ItemsPersonal => ("Personal folders", "Dossiers perso", "Личные папки", "Carpetas personales", "个人文件夹"),
    ItemsFinish => ("Protect", "Protéger", "Защитить", "Proteger", "保护"),
    ItemsAlready => ("already protected", "déjà protégé", "уже защищено", "ya protegido", "已受保护"),
    ItemsNothingTicked => ("Tick at least one item, or skip.", "Coche au moins un élément, ou passe.", "Отметьте хотя бы один элемент или пропустите.", "Marca al menos un elemento, o sáltatelo.", "至少勾选一项，或跳过。"),
    Skip => ("Skip", "Passer", "Пропустить", "Omitir", "跳过"),

    // Choix d'un dossier
    BrowseTitle => ("Pick a folder", "Choisis un dossier", "Выберите папку", "Elige una carpeta", "选择文件夹"),
    BrowseChoose => ("Use this folder", "Utiliser ce dossier", "Использовать эту папку", "Usar esta carpeta", "使用此文件夹"),
    BrowseUp => ("Up one level", "Dossier parent", "На уровень вверх", "Carpeta superior", "上一级"),
    BrowseInternal => ("Internal storage", "Stockage interne", "Внутренняя память", "Almacenamiento interno", "内部存储"),
    BrowseSd => ("SD card {name}", "Carte SD {name}", "SD-карта {name}", "Tarjeta SD {name}", "SD 卡 {name}"),
    BrowseEmpty => ("No sub-folder here", "Aucun sous-dossier ici", "Здесь нет подпапок", "No hay subcarpetas aquí", "这里没有子文件夹"),

    // Accueil
    HomeTitle => ("My protections", "Mes protections", "Мои защиты", "Mis protecciones", "我的保护"),
    SyncNow => ("Sync now", "Synchroniser maintenant", "Синхронизировать", "Sincronizar ahora", "立即同步"),
    Syncing => ("Copying…", "Copie en cours…", "Копирование…", "Copiando…", "正在复制…"),
    StopCopy => ("Stop", "Arrêter", "Остановить", "Detener", "停止"),
    AllSafe => ("Everything is safe", "Tout est à l'abri", "Всё в безопасности", "Todo está a salvo", "一切安全"),
    NeedAttention => ("{n} to check", "{n} à regarder", "{n} требует внимания", "{n} por revisar", "{n} 项需查看"),
    VaultUnreachable => ("Your vault is unreachable. Is the sync app running? Is the SD card in?", "Ton coffre est injoignable. L'appli de synchro tourne-t-elle ? La carte SD est-elle là ?", "Хранилище недоступно. Запущено ли приложение синхронизации? Вставлена ли SD-карта?", "Tu caja fuerte no está accesible. ¿Está abierta la app de sincronización? ¿Está la tarjeta SD?", "无法访问保险库。同步应用在运行吗？SD 卡插着吗？"),
    NothingYet => ("Nothing protected yet", "Rien de protégé pour l'instant", "Пока ничего не защищено", "Nada protegido todavía", "尚未保护任何内容"),
    Add => ("Add", "Ajouter", "Добавить", "Añadir", "添加"),
    Settings => ("Settings", "Réglages", "Настройки", "Ajustes", "设置"),
    LastCopy => ("Last copy {ago}", "Dernière copie {ago}", "Последняя копия {ago}", "Última copia {ago}", "上次复制 {ago}"),
    StNever => ("Never protected", "Jamais protégé", "Ни разу не защищено", "Nunca protegido", "从未保护"),
    StSafe => ("Safe", "À l'abri", "В безопасности", "A salvo", "安全"),
    StCopied => ("Copy done", "Copie faite", "Копия сделана", "Copia hecha", "已复制"),
    StReceived => ("Newer version received from {device}", "Version plus récente reçue de {device}", "Получена новая версия с {device}", "Versión más reciente recibida de {device}", "已收到来自 {device} 的新版本"),
    StAvailable => ("Newer version from {device}: open to put it back", "Version plus récente de {device} : ouvre pour la remettre", "Новая версия с {device}: откройте, чтобы вернуть", "Versión más reciente de {device}: ábrelo para restablecerla", "来自 {device} 的新版本：打开以恢复"),
    StConflict => ("Two devices changed it at the same time", "Deux appareils l'ont modifié en même temps", "Два устройства изменили одновременно", "Dos dispositivos lo cambiaron a la vez", "两台设备同时修改了它"),
    StError => ("Problem", "Problème", "Проблема", "Problema", "问题"),
    StMissing => ("Folder not found on this device", "Dossier introuvable sur cet appareil", "Папка не найдена на устройстве", "Carpeta no encontrada en este dispositivo", "在此设备上找不到文件夹"),
    StEmpty => ("Empty folder: nothing to copy yet", "Dossier vide : rien à copier pour l'instant", "Папка пуста: пока нечего копировать", "Carpeta vacía: nada que copiar aún", "文件夹为空：暂无可复制内容"),
    StEmptyWas => ("Folder is empty now (it had saves before)", "Dossier vide maintenant (il avait des sauvegardes)", "Папка теперь пуста (раньше были сохранения)", "La carpeta está vacía ahora (antes tenía guardados)", "文件夹现在为空（之前有存档）"),
    StNoAccess => ("File access missing", "Accès aux fichiers manquant", "Нет доступа к файлам", "Falta el acceso a los archivos", "缺少文件访问权限"),
    StPending => ("Waiting", "En attente", "Ожидание", "En espera", "等待中"),
    AgoNow => ("just now", "à l'instant", "только что", "ahora mismo", "刚刚"),
    AgoMin => ("{n} min ago", "il y a {n} min", "{n} мин назад", "hace {n} min", "{n} 分钟前"),
    AgoHour => ("{n} h ago", "il y a {n} h", "{n} ч назад", "hace {n} h", "{n} 小时前"),
    AgoYesterday => ("yesterday", "hier", "вчера", "ayer", "昨天"),
    AgoDay => ("{n} days ago", "il y a {n} j", "{n} дн. назад", "hace {n} días", "{n} 天前"),
    ClockWrong => ("This device's clock looks wrong ({date}). Versions made here will carry a wrong date; nothing is decided from the clock, and conflicts always ask you.", "L'heure de cet appareil semble fausse ({date}). Les versions faites ici porteront une mauvaise date ; rien n'est décidé d'après l'heure, et les conflits te sont toujours demandés.", "Часы этого устройства, похоже, сбиты ({date}). Версии, сделанные здесь, получат неверную дату; по часам ничего не решается, а при конфликте вас всегда спрашивают.", "La hora de este dispositivo parece incorrecta ({date}). Las versiones hechas aquí llevarán una fecha errónea; nada se decide por la hora y los conflictos siempre te preguntan.", "此设备的时钟似乎不正确（{date}）。在这里创建的版本日期会出错；不会根据时钟做任何决定，冲突时总会询问你。"),
    CleanedUp => ("An interrupted copy from a previous run was cleaned up", "Une copie interrompue d'un précédent lancement a été nettoyée", "Прерванная копия из прошлого запуска удалена", "Se limpió una copia interrumpida de una ejecución anterior", "已清理上次运行中被中断的复制"),

    // Fiche
    ProtectNow => ("Protect now", "Protéger maintenant", "Защитить сейчас", "Proteger ahora", "立即保护"),
    TimeTravel => ("Go back in time", "Remonter le temps", "Вернуться назад во времени", "Volver atrás en el tiempo", "回到过去"),
    VersionsOne => ("{n} version in the vault", "{n} version dans le coffre", "{n} версия в хранилище", "{n} versión en la caja fuerte", "保险库中有 {n} 个版本"),
    VersionsFew => ("{n} versions in the vault", "{n} versions dans le coffre", "{n} версии в хранилище", "{n} versiones en la caja fuerte", "保险库中有 {n} 个版本"),
    Versions => ("{n} versions in the vault", "{n} versions dans le coffre", "{n} версий в хранилище", "{n} versiones en la caja fuerte", "保险库中有 {n} 个版本"),
    Unprotect => ("Stop protecting", "Ne plus protéger", "Больше не защищать", "Dejar de proteger", "停止保护"),
    UnprotectBody => ("Nothing is deleted: the folder stays on this device and the versions stay in your vault.", "Rien n'est supprimé : le dossier reste sur cet appareil et les versions restent dans ton coffre.", "Ничего не удаляется: папка остаётся на устройстве, версии — в хранилище.", "No se borra nada: la carpeta queda en este dispositivo y las versiones en tu caja fuerte.", "不会删除任何内容：文件夹保留在此设备上，版本保留在保险库中。"),
    Folder => ("Folder", "Dossier", "Папка", "Carpeta", "文件夹"),
    FilesOne => ("{n} file", "{n} fichier", "{n} файл", "{n} archivo", "{n} 个文件"),
    FilesFew => ("{n} files", "{n} fichiers", "{n} файла", "{n} archivos", "{n} 个文件"),
    Files => ("{n} files", "{n} fichiers", "{n} файлов", "{n} archivos", "{n} 个文件"),
    Current => ("what this device has", "ce que cet appareil a", "то, что на этом устройстве", "lo que tiene este dispositivo", "此设备当前内容"),
    Active => ("latest", "la plus récente", "последняя", "la más reciente", "最新"),
    SafetyCopy => ("Safety copy made on this device", "Copie de sécurité faite sur cet appareil", "Резервная копия, сделанная на этом устройстве", "Copia de seguridad hecha en este dispositivo", "在此设备上创建的安全副本"),
    ConflictBranch => ("kept during a conflict", "gardée lors d'un conflit", "сохранена при конфликте", "guardada durante un conflicto", "冲突时保留"),
    Restore => ("Put back this version", "Remettre cette version", "Вернуть эту версию", "Volver a esta versión", "恢复此版本"),
    RestoreBody => ("Your current files are set aside in a safety copy first. You can undo right after.", "Tes fichiers actuels sont d'abord mis de côté dans une copie de sécurité. Tu peux annuler juste après.", "Текущие файлы сначала сохраняются в резервную копию. Сразу после можно отменить.", "Tus archivos actuales se apartan primero en una copia de seguridad. Puedes deshacer justo después.", "会先把你当前的文件放入安全副本。之后可以立即撤销。"),
    Restored => ("Version put back", "Version remise", "Версия возвращена", "Versión restablecida", "版本已恢复"),
    Undo => ("Undo", "Annuler", "Отменить", "Deshacer", "撤销"),
    Undone => ("Undone: your files are back as before", "Annulé : tes fichiers sont revenus comme avant", "Отменено: файлы возвращены как были", "Deshecho: tus archivos están como antes", "已撤销：你的文件已恢复原样"),
    NoVersions => ("No version in the vault yet", "Aucune version dans le coffre pour l'instant", "В хранилище пока нет версий", "Aún no hay versiones en la caja fuerte", "保险库中还没有版本"),
    AvailableTitle => ("Newer version from {device}", "Version plus récente de {device}", "Новая версия с {device}", "Versión más reciente de {device}", "来自 {device} 的新版本"),
    AvailableBody => ("Another device made a newer version. Nothing is put back by itself on this device: close the emulator first, then confirm. Your current files are set aside in a safety copy; you can undo right after.", "Un autre appareil a fait une version plus récente. Rien n'est remis tout seul sur cet appareil : ferme d'abord l'émulateur, puis valide. Tes fichiers actuels sont mis de côté dans une copie de sécurité ; tu peux annuler juste après.", "Другое устройство сделало более новую версию. На этом устройстве ничего не возвращается само: сначала закройте эмулятор, затем подтвердите. Текущие файлы сохраняются в резервную копию; сразу после можно отменить.", "Otro dispositivo hizo una versión más reciente. Nada se restablece solo en este dispositivo: cierra primero el emulador y confirma. Tus archivos actuales se apartan en una copia de seguridad; puedes deshacer justo después.", "另一台设备创建了更新的版本。此设备不会自动恢复任何内容：请先关闭模拟器，再确认。你当前的文件会放入安全副本；之后可以立即撤销。"),
    TakeVersion => ("Put back the version from {device}", "Remettre la version de {device}", "Вернуть версию с {device}", "Restablecer la versión de {device}", "恢复来自 {device} 的版本"),
    ConflictBody => ("Both versions are kept in the vault. Choose which one this device should use; the other stays in Go back in time.", "Les deux versions sont gardées dans le coffre. Choisis celle que cet appareil doit utiliser ; l'autre reste dans Remonter le temps.", "Обе версии сохранены в хранилище. Выберите, какую использовать на этом устройстве; другая останется в «Назад во времени».", "Ambas versiones se guardan en la caja fuerte. Elige cuál debe usar este dispositivo; la otra queda en Volver atrás en el tiempo.", "两个版本都保留在保险库中。选择此设备应使用的版本；另一个保留在「回到过去」中。"),
    KeepMine => ("Keep mine", "Garder la mienne", "Оставить мою", "Conservar la mía", "保留我的"),
    TakeOther => ("Take the other device's", "Prendre celle de l'autre appareil", "Взять с другого устройства", "Tomar la del otro dispositivo", "使用另一台设备的"),
    Resolved => ("Settled: both versions stay in the vault", "Réglé : les deux versions restent dans le coffre", "Решено: обе версии остаются в хранилище", "Resuelto: ambas versiones quedan en la caja fuerte", "已解决：两个版本都保留在保险库中"),

    // Reglages
    SetLang => ("Language", "Langue", "Язык", "Idioma", "语言"),
    SetDevice => ("This device", "Cet appareil", "Это устройство", "Este dispositivo", "此设备"),
    SetVault => ("Vault", "Coffre", "Хранилище", "Caja fuerte", "保险库"),
    SetChange => ("Change", "Changer", "Изменить", "Cambiar", "更改"),
    SetConflict => ("When two devices changed at the same time", "Quand deux appareils ont modifié en même temps", "Когда два устройства изменили одновременно", "Cuando dos dispositivos cambiaron a la vez", "当两台设备同时修改时"),
    ConflictPolicy => ("Both versions are kept and the app always asks you (Keep mine / Take the other device's). On this device nothing is ever put back automatically: the clock of a handheld can be wrong, and an emulator may still be open in the background.", "Les deux versions sont gardées et l'appli te demande toujours (Garder la mienne / Prendre celle de l'autre appareil). Sur cet appareil, rien n'est jamais remis automatiquement : l'heure d'une console peut être fausse, et un émulateur peut rester ouvert en arrière-plan.", "Обе версии сохраняются, и приложение всегда спрашивает вас (Оставить мою / Взять с другого устройства). На этом устройстве ничего не возвращается автоматически: часы консоли могут быть сбиты, а эмулятор может оставаться открытым в фоне.", "Ambas versiones se guardan y la app siempre te pregunta (Conservar la mía / Tomar la del otro dispositivo). En este dispositivo nunca se restablece nada automáticamente: la hora de una consola puede estar mal y un emulador puede seguir abierto en segundo plano.", "两个版本都会保留，应用总会询问你（保留我的 / 使用另一台设备的）。在此设备上不会自动恢复任何内容：掌机的时钟可能不准，模拟器也可能仍在后台运行。"),
    SetEmulators => ("Emulators", "Émulateurs", "Эмуляторы", "Emuladores", "模拟器"),
    SetAbout => ("About", "À propos", "О приложении", "Acerca de", "关于"),
    AboutBody => ("Version {v} (preview). The vault is a folder another app keeps in sync with your PC. Updates: see the GitHub page.", "Version {v} (préversion). Le coffre est un dossier qu'une autre appli garde synchronisé avec ton PC. Mises à jour : voir la page GitHub.", "Версия {v} (предварительная). Хранилище — папка, которую другое приложение синхронизирует с ПК. Обновления: см. страницу GitHub.", "Versión {v} (versión previa). La caja fuerte es una carpeta que otra app sincroniza con tu PC. Actualizaciones: ver la página de GitHub.", "版本 {v}（预览版）。保险库是由另一个应用与电脑同步的文件夹。更新：见 GitHub 页面。"),
    AboutLicenses => ("Licences: GPL-3.0 · egui, eframe (MIT / Apache-2.0) · winit (Apache-2.0, modified copy for the controller) · Phosphor Icons (MIT)", "Licences : GPL-3.0 · egui, eframe (MIT / Apache-2.0) · winit (Apache-2.0, copie modifiée pour la manette) · Phosphor Icons (MIT)", "Лицензии: GPL-3.0 · egui, eframe (MIT / Apache-2.0) · winit (Apache-2.0, изменённая копия для геймпада) · Phosphor Icons (MIT)", "Licencias: GPL-3.0 · egui, eframe (MIT / Apache-2.0) · winit (Apache-2.0, copia modificada para el mando) · Phosphor Icons (MIT)", "许可证：GPL-3.0 · egui、eframe（MIT / Apache-2.0）· winit（Apache-2.0，为手柄修改的副本）· Phosphor Icons（MIT）"),
    OpenGitHub => ("Open the GitHub page", "Ouvrir la page GitHub", "Открыть страницу GitHub", "Abrir la página de GitHub", "打开 GitHub 页面"),
    AutoTitle => ("What runs by itself", "Ce qui se fait tout seul", "Что происходит само", "Qué se hace solo", "自动执行的内容"),
    AutoBody => ("Copies to the vault run when the app opens or comes back to the front, and with « Sync now ». A newer version made elsewhere is never put back by itself: the item shows it and asks you. Nothing runs in the background: open the app before and after playing.", "Les copies vers le coffre se font à l'ouverture de l'appli ou quand elle revient au premier plan, et avec « Synchroniser maintenant ». Une version plus récente faite ailleurs n'est jamais remise toute seule : l'élément la signale et te demande. Rien ne tourne en arrière-plan : ouvre l'appli avant et après avoir joué.", "Копии в хранилище делаются при открытии приложения или его возврате на передний план, а также по «Синхронизировать». Более новая версия с другого устройства никогда не возвращается сама: элемент показывает её и спрашивает вас. В фоне ничего не работает: открывайте приложение до и после игры.", "Las copias a la caja fuerte se hacen al abrir la app o cuando vuelve al frente, y con « Sincronizar ahora ». Una versión más reciente hecha en otro sitio nunca se restablece sola: el elemento la muestra y te pregunta. Nada corre en segundo plano: abre la app antes y después de jugar.", "在打开应用或应用回到前台时，以及点击「立即同步」时会复制到保险库。在别处创建的新版本不会自动恢复：条目会显示它并询问你。后台不会运行任何内容：游戏前后请打开应用。"),
    CompatSame => ("Same files on PC and Android", "Mêmes fichiers sur PC et Android", "Одни и те же файлы на ПК и Android", "Mismos archivos en PC y Android", "电脑和 Android 使用相同文件"),
    CompatSaves => ("Saves compatible; save states need the same emulator version", "Sauvegardes compatibles ; les états instantanés exigent la même version de l'émulateur", "Сохранения совместимы; быстрые сохранения требуют той же версии эмулятора", "Guardados compatibles; los estados guardados exigen la misma versión del emulador", "存档兼容；即时存档需要相同的模拟器版本"),
    CompatPartial => ("Compatible under conditions (see note)", "Compatible sous conditions (voir la note)", "Совместимо при условиях (см. примечание)", "Compatible con condiciones (ver nota)", "有条件兼容（见说明）"),
    Found => ("found on this device", "trouvé sur cet appareil", "найдено на устройстве", "encontrado en este dispositivo", "已在此设备上找到"),
    NotFound => ("not found on this device", "pas trouvé sur cet appareil", "не найдено на устройстве", "no encontrado en este dispositivo", "未在此设备上找到"),
    ExpectedIn => ("Expected folder", "Dossier attendu", "Ожидаемая папка", "Carpeta esperada", "预期文件夹"),
    Journal => ("Journal", "Journal", "Журнал", "Registro", "日志"),
    Problem => ("Problem: {msg}", "Problème : {msg}", "Проблема: {msg}", "Problema: {msg}", "问题：{msg}"),
    Removed => ("No longer protected (nothing deleted)", "Plus protégé (rien de supprimé)", "Больше не защищается (ничего не удалено)", "Ya no está protegido (nada borrado)", "不再保护（未删除任何内容）"),
    AddedOne => ("{n} item added", "{n} élément ajouté", "Добавлен {n} элемент", "{n} elemento añadido", "已添加 {n} 项"),
    AddedFew => ("{n} items added", "{n} éléments ajoutés", "Добавлено {n} элемента", "{n} elementos añadidos", "已添加 {n} 项"),
    Added => ("{n} items added", "{n} éléments ajoutés", "Добавлено {n} элементов", "{n} elementos añadidos", "已添加 {n} 项"),
    Minimize => ("Hide the app", "Masquer l'appli", "Скрыть приложение", "Ocultar la app", "隐藏应用"),

    // Erreurs du moteur (table de correspondance, voir `error_text`)
    ErrRefused => ("refused: the program is still running, close it first", "refus : le programme tourne encore, ferme-le d'abord", "отказано: программа ещё запущена, сначала закройте её", "rechazado: el programa sigue abierto, ciérralo primero", "已拒绝：程序仍在运行，请先关闭"),
    ErrNotFound => ("folder not found on this device", "dossier introuvable sur cet appareil", "папка не найдена на устройстве", "carpeta no encontrada en este dispositivo", "在此设备上找不到文件夹"),
    ErrEmpty => ("the folder is empty: nothing is ever replaced by nothing", "le dossier est vide : on ne remplace jamais une sauvegarde par du vide", "папка пуста: пустота никогда не заменяет сохранение", "la carpeta está vacía: nunca se reemplaza un guardado por nada", "文件夹为空：绝不会用空内容替换存档"),
    ErrCorrupt => ("a vault version is corrupted or incomplete; nothing was changed", "une version du coffre est corrompue ou incomplète ; rien n'a été modifié", "версия в хранилище повреждена или неполна; ничего не изменено", "una versión de la caja fuerte está dañada o incompleta; nada se modificó", "保险库中的某个版本已损坏或不完整；未做任何更改"),
    ErrUnreachable => ("the vault is unreachable; nothing was changed", "le coffre est injoignable ; rien n'a été modifié", "хранилище недоступно; ничего не изменено", "la caja fuerte no está accesible; nada se modificó", "无法访问保险库；未做任何更改"),
    ErrCancelled => ("stopped before anything was written", "arrêté avant toute écriture", "остановлено до записи", "detenido antes de escribir nada", "在写入任何内容前已停止"),
    ErrNoConflict => ("there is no conflict to settle any more", "il n'y a plus de conflit à régler", "конфликта больше нет", "ya no hay conflicto que resolver", "已没有需要解决的冲突"),
    ErrOther => ("see the journal (Settings > Journal)", "voir le journal (Réglages > Journal)", "см. журнал (Настройки > Журнал)", "ver el registro (Ajustes > Registro)", "请查看日志（设置 > 日志）"),

    // Libelles des emplacements des profils (memes mots que le profil PC, traduits pour l'ecran seulement)
    LocSaves => ("Saves (SRAM, memory cards)", "Sauvegardes (SRAM, cartes mémoire)", "Сохранения (SRAM, карты памяти)", "Guardados (SRAM, tarjetas de memoria)", "存档（SRAM、记忆卡）"),
    LocStates => ("Save states", "États instantanés", "Быстрые сохранения", "Estados guardados", "即时存档"),
    LocSaveData => ("Save data", "Données de sauvegarde", "Данные сохранений", "Datos de guardado", "存档数据"),
    LocMemcards => ("Memory cards", "Cartes mémoire", "Карты памяти", "Tarjetas de memoria", "记忆卡"),
    LocGcCards => ("GameCube memory cards", "Cartes mémoire GameCube", "Карты памяти GameCube", "Tarjetas de memoria de GameCube", "GameCube 记忆卡"),
    LocWii => ("Wii saves", "Sauvegardes Wii", "Сохранения Wii", "Guardados de Wii", "Wii 存档"),
    LocSdmc => ("Save data (SD card)", "Données de sauvegarde (carte SD)", "Данные сохранений (SD-карта)", "Datos de guardado (tarjeta SD)", "存档数据（SD 卡）"),
    PfPhotos => ("Photos (camera)", "Photos (appareil photo)", "Фото (камера)", "Fotos (cámara)", "照片（相机）"),
    PfPictures => ("Pictures", "Images", "Изображения", "Imágenes", "图片"),
    PfDocuments => ("Documents", "Documents", "Документы", "Documentos", "文档"),
    PfScreenshots => ("Screenshots", "Captures d'écran", "Снимки экрана", "Capturas de pantalla", "屏幕截图"),

    // Notes des emulateurs (ecran Emulateurs)
    NoteRetroarch => ("RetroArch/saves and RetroArch/states at the root of the storage (default since RetroArch 1.9). Same cores on PC and Android: .srm files are identical; save states need the same core version on both sides. Keep the same « sort saves into folders » settings on both.", "RetroArch/saves et RetroArch/states à la racine du stockage (par défaut depuis RetroArch 1.9). Mêmes cœurs sur PC et Android : les fichiers .srm sont identiques ; les états instantanés exigent la même version du cœur des deux côtés. Garde les mêmes réglages « classer les sauvegardes par dossier » des deux côtés.", "RetroArch/saves и RetroArch/states в корне хранилища (по умолчанию с RetroArch 1.9). Одни и те же ядра на ПК и Android: файлы .srm идентичны; быстрые сохранения требуют одной версии ядра с обеих сторон. Держите одинаковые настройки «сортировать сохранения по папкам».", "RetroArch/saves y RetroArch/states en la raíz del almacenamiento (por defecto desde RetroArch 1.9). Mismos núcleos en PC y Android: los archivos .srm son idénticos; los estados guardados exigen la misma versión del núcleo en ambos lados. Mantén los mismos ajustes « ordenar guardados en carpetas » en ambos.", "存储根目录下的 RetroArch/saves 和 RetroArch/states（RetroArch 1.9 起默认）。电脑和 Android 使用相同核心：.srm 文件完全相同；即时存档要求两边核心版本一致。两边请保持相同的「按文件夹整理存档」设置。"),
    NotePpsspp => ("PSP folder at the root of the storage (memory stick). Recent PPSSPP versions default to Android/data: in PPSSPP go to Settings > System > Memory Stick folder and pick a visible folder (for example PSP at the root). Save data is the same files as on PC; save states depend on the PPSSPP version.", "Dossier PSP à la racine du stockage (memory stick). Les versions récentes de PPSSPP utilisent Android/data par défaut : dans PPSSPP, Réglages > Système > Dossier de la Memory Stick, puis choisis un dossier visible (comme PSP à la racine). Les données de sauvegarde sont les mêmes fichiers que sur PC ; les états instantanés dépendent de la version de PPSSPP.", "Папка PSP в корне хранилища (memory stick). Новые версии PPSSPP по умолчанию используют Android/data: в PPSSPP откройте Настройки > Система > Папка Memory Stick и выберите видимую папку (например PSP в корне). Данные сохранений — те же файлы, что на ПК; быстрые сохранения зависят от версии PPSSPP.", "Carpeta PSP en la raíz del almacenamiento (memory stick). Las versiones recientes de PPSSPP usan Android/data por defecto: en PPSSPP ve a Ajustes > Sistema > Carpeta del Memory Stick y elige una carpeta visible (por ejemplo PSP en la raíz). Los datos de guardado son los mismos archivos que en PC; los estados guardados dependen de la versión de PPSSPP.", "存储根目录下的 PSP 文件夹（记忆棒）。新版 PPSSPP 默认使用 Android/data：在 PPSSPP 中进入 设置 > 系统 > 记忆棒文件夹，选择一个可见的文件夹（例如根目录下的 PSP）。存档数据与电脑上的文件相同；即时存档取决于 PPSSPP 版本。"),
    NoteDuckstation => ("duckstation/memcards at the root of the storage. Memory cards (.mcd) are identical on PC and Android; save states depend on the DuckStation version.", "duckstation/memcards à la racine du stockage. Les cartes mémoire (.mcd) sont identiques sur PC et Android ; les états instantanés dépendent de la version de DuckStation.", "duckstation/memcards в корне хранилища. Карты памяти (.mcd) идентичны на ПК и Android; быстрые сохранения зависят от версии DuckStation.", "duckstation/memcards en la raíz del almacenamiento. Las tarjetas de memoria (.mcd) son idénticas en PC y Android; los estados guardados dependen de la versión de DuckStation.", "存储根目录下的 duckstation/memcards。记忆卡（.mcd）在电脑和 Android 上完全相同；即时存档取决于 DuckStation 版本。"),
    NotePcsx2 => ("AetherSX2 / NetherSX2 keep their data in Android/data by default (not readable): set System > Data directory to a visible folder named AetherSX2 or NetherSX2 at the root of the storage. 8 MB memory cards (.ps2) are the same format as PCSX2 on PC; folder memory cards and save states are not interchangeable.", "AetherSX2 / NetherSX2 rangent leurs données dans Android/data par défaut (illisible) : règle Système > Dossier de données sur un dossier visible nommé AetherSX2 ou NetherSX2 à la racine du stockage. Les cartes mémoire de 8 Mo (.ps2) ont le même format que PCSX2 sur PC ; les cartes « dossier » et les états instantanés ne sont pas échangeables.", "AetherSX2 / NetherSX2 по умолчанию хранят данные в Android/data (недоступно): в Система > Папка данных укажите видимую папку с именем AetherSX2 или NetherSX2 в корне хранилища. Карты памяти 8 МБ (.ps2) того же формата, что в PCSX2 на ПК; карты-папки и быстрые сохранения не взаимозаменяемы.", "AetherSX2 / NetherSX2 guardan sus datos en Android/data por defecto (ilegible): pon Sistema > Carpeta de datos en una carpeta visible llamada AetherSX2 o NetherSX2 en la raíz del almacenamiento. Las tarjetas de memoria de 8 MB (.ps2) tienen el mismo formato que PCSX2 en PC; las tarjetas « carpeta » y los estados guardados no son intercambiables.", "AetherSX2 / NetherSX2 默认把数据放在 Android/data（不可读）：请在 系统 > 数据目录 中选择存储根目录下名为 AetherSX2 或 NetherSX2 的可见文件夹。8 MB 记忆卡（.ps2）与电脑上的 PCSX2 格式相同；文件夹式记忆卡和即时存档不可互换。"),
    NoteDolphin => ("Official Dolphin on Android 11+ stores everything in Android/data (not readable). Only builds that use dolphin-emu at the root of the storage (older versions, MMJR forks) can be protected. GameCube memory cards (.raw) and Wii saves are the same files as on PC; save states are not portable.", "Le Dolphin officiel sur Android 11+ range tout dans Android/data (illisible). Seules les versions qui utilisent dolphin-emu à la racine du stockage (anciennes versions, forks MMJR) sont protégeables. Les cartes mémoire GameCube (.raw) et les sauvegardes Wii sont les mêmes fichiers que sur PC ; les états instantanés ne sont pas portables.", "Официальный Dolphin на Android 11+ хранит всё в Android/data (недоступно). Защитить можно только сборки, использующие dolphin-emu в корне хранилища (старые версии, форки MMJR). Карты памяти GameCube (.raw) и сохранения Wii — те же файлы, что на ПК; быстрые сохранения не переносимы.", "El Dolphin oficial en Android 11+ guarda todo en Android/data (ilegible). Solo las versiones que usan dolphin-emu en la raíz del almacenamiento (versiones antiguas, forks MMJR) se pueden proteger. Las tarjetas de memoria de GameCube (.raw) y los guardados de Wii son los mismos archivos que en PC; los estados guardados no son portables.", "Android 11+ 上的官方 Dolphin 把所有内容放在 Android/data（不可读）。只有使用存储根目录下 dolphin-emu 的版本（旧版本、MMJR 分支）可以保护。GameCube 记忆卡（.raw）和 Wii 存档与电脑上的文件相同；即时存档不可移植。"),
    NoteAzahar => ("When the Android app asks for a user folder at first launch, pick a folder named azahar (or citra-emu) at the root of the storage. The SD card tree has the same layout on PC and Android, but it contains console-specific folder names: a game saved on one side is found by the other only if both use the same console id (copy config and sdmc from one side the first time). To verify on a real device.", "Quand l'appli Android demande un dossier utilisateur au premier lancement, choisis un dossier nommé azahar (ou citra-emu) à la racine du stockage. L'arborescence de la carte SD est la même sur PC et Android, mais elle contient des noms de dossiers propres à la console : une partie sauvegardée d'un côté n'est trouvée par l'autre que si les deux utilisent le même identifiant de console (copie config et sdmc d'un côté à l'autre la première fois). À vérifier sur une vraie console.", "Когда приложение Android при первом запуске просит папку пользователя, выберите папку с именем azahar (или citra-emu) в корне хранилища. Дерево SD-карты одинаково на ПК и Android, но содержит имена папок, зависящие от консоли: игра, сохранённая с одной стороны, видна с другой только при одинаковом идентификаторе консоли (в первый раз скопируйте config и sdmc с одной стороны). Проверить на реальном устройстве.", "Cuando la app Android pide una carpeta de usuario en el primer inicio, elige una carpeta llamada azahar (o citra-emu) en la raíz del almacenamiento. El árbol de la tarjeta SD es igual en PC y Android, pero contiene nombres de carpetas propios de la consola: una partida guardada en un lado solo la encuentra el otro si ambos usan el mismo id de consola (copia config y sdmc de un lado la primera vez). Por verificar en un dispositivo real.", "当 Android 应用首次启动要求选择用户文件夹时，请选择存储根目录下名为 azahar（或 citra-emu）的文件夹。SD 卡目录结构在电脑和 Android 上相同，但包含与主机相关的文件夹名：只有两边使用相同的主机 ID，一边的存档才能被另一边找到（第一次请把 config 和 sdmc 从一边复制到另一边）。需在真机上验证。"),
}

/// Remplace `{n}` (ou un autre gabarit) dans un texte.
pub fn fill(s: &str, key: &str, value: &str) -> String {
    s.replace(&format!("{{{}}}", key), value)
}

/// Pluriel : `one` pour 1, `few` pour 2-4 (russe ; ailleurs identique a `many`), `many` sinon. 11-14 en russe = many.
pub fn count(l: Lang, n: usize, one: T, few: T, many: T) -> String {
    let k = match l {
        Lang::Ru => match (n % 10, n % 100) {
            (1, m) if !(11..=14).contains(&m) => one,
            (2..=4, m) if !(11..=14).contains(&m) => few,
            _ => many,
        },
        _ => {
            if n == 1 {
                one
            } else {
                many
            }
        }
    };
    fill(tr(l, k), "n", &n.to_string())
}

pub fn files(l: Lang, n: usize) -> String {
    count(l, n, T::FilesOne, T::FilesFew, T::Files)
}

pub fn versions(l: Lang, n: usize) -> String {
    count(l, n, T::VersionsOne, T::VersionsFew, T::Versions)
}

pub fn added(l: Lang, n: usize) -> String {
    count(l, n, T::AddedOne, T::AddedFew, T::Added)
}

/// Libelle d'un emplacement de profil (texte EXACT du profil PC -> texte de la langue ; inconnu : tel quel).
pub fn loc_label(l: Lang, label: &str) -> String {
    let k = match label {
        "Saves (SRAM, memory cards)" => T::LocSaves,
        "Save states" => T::LocStates,
        "Save data" => T::LocSaveData,
        "Memory cards" => T::LocMemcards,
        "GameCube memory cards" => T::LocGcCards,
        "Wii saves" => T::LocWii,
        "Save data (SD card)" => T::LocSdmc,
        _ => return label.to_string(),
    };
    tr(l, k).to_string()
}

/// Nom affiche d'un element (dossier perso connu -> traduit ; sinon le nom tel quel).
pub fn item_display(l: Lang, name: &str) -> String {
    let k = match name {
        "Photos (camera)" => T::PfPhotos,
        "Pictures" => T::PfPictures,
        "Documents" => T::PfDocuments,
        "Screenshots" => T::PfScreenshots,
        _ => return name.to_string(),
    };
    tr(l, k).to_string()
}

/// Note d'un emulateur de la table partagee (`agss::emulators::EMULATORS`, par identifiant de profil).
pub fn emu_note(l: Lang, id: &str) -> Option<&'static str> {
    let k = match id {
        "retroarch" => T::NoteRetroarch,
        "ppsspp" => T::NotePpsspp,
        "duckstation" => T::NoteDuckstation,
        "pcsx2" => T::NotePcsx2,
        "dolphin" => T::NoteDolphin,
        "azahar" => T::NoteAzahar,
        _ => return None,
    };
    Some(tr(l, k))
}

/// Message d'erreur du moteur (francais sans accents, premiere ligne) -> texte court de la langue. Les cas courants sont
/// reconnus par leurs mots cles ; tout le reste renvoie vers le journal, qui garde le message complet.
pub fn error_text(l: Lang, msg: &str) -> String {
    let m = msg.lines().next().unwrap_or("").to_lowercase();
    let k = if m.contains("refus") {
        T::ErrRefused
    } else if m.contains("vault unreachable") || m.contains("injoignable") {
        T::ErrUnreachable
    } else if m.contains(agss::perf::CANCELLED.to_lowercase().as_str()) {
        T::ErrCancelled
    } else if m.contains("corrompue") {
        T::ErrCorrupt
    } else if m.contains("aucun conflit") {
        T::ErrNoConflict
    } else if m.contains("vide") {
        T::ErrEmpty
    } else if m.contains("introuvable") {
        T::ErrNotFound
    } else {
        T::ErrOther
    };
    tr(l, k).to_string()
}

/// Langue par defaut d'apres le code de langue du systeme (`fr`, `ru`, `es`, `zh*`, sinon anglais).
pub fn from_system(code: &str) -> Lang {
    match code.to_lowercase().as_str() {
        "fr" => Lang::Fr,
        "ru" => Lang::Ru,
        "es" => Lang::Es,
        c if c.starts_with("zh") => Lang::Zh,
        _ => Lang::En,
    }
}

/// Duree relative dans la langue.
pub fn ago(l: Lang, now_ms: u64, then_ms: u64) -> String {
    use agss::home::Ago;
    match agss::home::ago(now_ms, then_ms) {
        Ago::JustNow => tr(l, T::AgoNow).to_string(),
        Ago::Min(n) => fill(tr(l, T::AgoMin), "n", &n.to_string()),
        Ago::Hour(n) => fill(tr(l, T::AgoHour), "n", &n.to_string()),
        Ago::Yesterday => tr(l, T::AgoYesterday).to_string(),
        Ago::Day(n) => fill(tr(l, T::AgoDay), "n", &n.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toutes_les_cles_dans_les_cinq_langues_et_vocabulaire_grand_public() {
        for t in ALL {
            for l in Lang::ALL {
                let s = tr(l, *t);
                assert!(!s.trim().is_empty(), "{:?} vide en {:?}", t, l);
            }
            // Vocabulaire : jamais « exe », « cloud » ni de chemin Windows dans les textes.
            for l in [Lang::En, Lang::Fr] {
                let s = tr(l, *t).to_lowercase();
                for banned in [" exe", "cloud", "c:\\", "%appdata%"] {
                    assert!(!s.contains(banned), "{:?} ({:?}) contient « {} »", t, l, banned);
                }
            }
        }
        assert_eq!(from_system("fr"), Lang::Fr);
        assert_eq!(from_system("zh-Hans"), Lang::Zh);
        assert_eq!(from_system("de"), Lang::En);
        assert_eq!(fill("{n} files", "n", "3"), "3 files");
        assert_eq!(ago(Lang::Fr, 100_000, 100_000), "à l'instant");
        assert_eq!(ago(Lang::En, 3_600_000 * 3, 0), "3 h ago");
    }

    #[test]
    fn pluriels_corrects() {
        assert_eq!(files(Lang::Fr, 1), "1 fichier");
        assert_eq!(files(Lang::Fr, 2), "2 fichiers");
        assert_eq!(files(Lang::En, 1), "1 file");
        assert_eq!(files(Lang::En, 0), "0 files");
        assert_eq!(files(Lang::Ru, 1), "1 файл");
        assert_eq!(files(Lang::Ru, 3), "3 файла");
        assert_eq!(files(Lang::Ru, 5), "5 файлов");
        assert_eq!(files(Lang::Ru, 11), "11 файлов");
        assert_eq!(files(Lang::Ru, 21), "21 файл");
        assert_eq!(versions(Lang::Fr, 1), "1 version dans le coffre");
        assert_eq!(versions(Lang::Es, 4), "4 versiones en la caja fuerte");
        assert_eq!(added(Lang::Fr, 1), "1 élément ajouté");
        assert_eq!(added(Lang::Fr, 2), "2 éléments ajoutés");
    }

    #[test]
    fn notes_libelles_et_erreurs_traduits() {
        // Chaque emulateur de la table partagee a sa note, et chaque libelle d'emplacement sa traduction.
        for e in agss::emulators::EMULATORS {
            assert!(emu_note(Lang::Fr, e.id).is_some(), "note manquante pour {}", e.id);
            for l in e.locations {
                assert_ne!(loc_label(Lang::Fr, l.label), l.label, "libelle non traduit : {}", l.label);
                assert_eq!(loc_label(Lang::En, l.label), l.label, "libelle anglais = celui du profil");
            }
        }
        for (name, _) in agss::emulators::PERSONAL_FOLDERS {
            assert_ne!(item_display(Lang::Ru, name), name.to_string(), "dossier perso non traduit : {}", name);
        }
        assert_eq!(loc_label(Lang::Fr, "Autre chose"), "Autre chose");
        assert_eq!(item_display(Lang::Fr, "RetroArch saves"), "RetroArch saves");
        assert_eq!(error_text(Lang::Fr, "REFUS : x est en cours d'execution"), tr(Lang::Fr, T::ErrRefused));
        assert_eq!(error_text(Lang::Fr, "dossier ou fichier de sauvegardes introuvable : /x"), tr(Lang::Fr, T::ErrNotFound));
        assert_eq!(error_text(Lang::En, "dossier de sauvegardes vide : on ne remplace jamais une sauvegarde par du vide"), tr(Lang::En, T::ErrEmpty));
        assert_eq!(error_text(Lang::Es, "sauvegarde cloud corrompue ou incomplete (x). Rien n'a ete modifie"), tr(Lang::Es, T::ErrCorrupt));
        assert_eq!(error_text(Lang::Ru, "vault unreachable"), tr(Lang::Ru, T::ErrUnreachable));
        assert_eq!(error_text(Lang::Zh, "coffre injoignable : rien n'a ete modifie"), tr(Lang::Zh, T::ErrUnreachable));
        assert_eq!(error_text(Lang::Fr, "aucun conflit a resoudre pour cet element"), tr(Lang::Fr, T::ErrNoConflict));
        assert_eq!(error_text(Lang::Fr, "quelque chose d'inattendu\ndeuxieme ligne"), tr(Lang::Fr, T::ErrOther));
    }
}
