//! The People screen, the quarantine and its thresholds.

/// The words of this area, English source → Russian text.
pub const TERMS: &[(&str, &str)] = &[
    // --- people ----------------------------------------------------------------
    ("Merge threshold:", "Порог склейки:"),
    ("face model not downloaded", "модель лиц не скачана"),
    (
        "SCRFD and ArcFace (InsightFace buffalo_l): non-commercial research use",
        "SCRFD и ArcFace (InsightFace buffalo_l): только некоммерческое исследовательское \
         использование",
    ),
    (
        "the face model download: stopping…",
        "скачивание модели лиц: останавливаю…",
    ),
    (
        "the face model download is stopped: the pass can be started again",
        "скачивание модели лиц остановлено: проход можно запустить снова",
    ),
    ("reclustering…", "пересобираю группы…"),
    ("Stop", "Стоп"),
    ("people: stopping after the current photos…", "лица: останавливаю после текущих снимков…"),
    ("Resume", "Продолжить"),
    ("Find faces", "Найти лица"),
    (
        "a long pass: detection and embeddings run locally, no network; photos already \
         scanned are skipped by sha256",
        "долгий проход: детекция и эмбеддинги локально, без сети; уже просканированные \
         снимки пропускаются по sha256",
    ),
    ("Rebuild groups", "Пересобрать группы"),
    (
        "the threshold applies to faces; manual names and splits are kept",
        "порог применяется к лицам; ручные подписи и разъединения сохраняются",
    ),
    ("Groups", "Группы"),
    ("All people", "Все лица"),
    ("no name", "без имени"),
    ("mark the group for the “all together” search", "отметить группу для поиска «все вместе»"),
    ("Show the photos (together)", "Показать лица (вместе)"),
    ("Photos with everyone together", "Фото, где все вместе"),
    (
        "show only photos that have a face from every marked group",
        "показать только снимки, где есть лица из всех отмеченных групп",
    ),
    ("No photo has faces from all the marked groups.", "Нет снимков, где есть лица из всех отмеченных групп."),
    ("photos", "снимки"),
    ("Person's name:", "Имя человека:"),
    ("e.g. Lisa", "например, Лиза"),
    ("Save the name", "Сохранить имя"),
    (
        "A name pins the group: rebuilding by threshold will not shuffle it, and the closeness in \
         “Tuning” adds faces only through the quarantine. A wrong merge is split by the \
         crossed-head button on the tile and the view stays where it was; tick several strangers \
         and “Split the picked” takes them all out at once.",
        "Подпись закрепляет группу: пересборка по порогу её не перемешает, а близость в \
         «Настройке» добавляет лица только через карантин. Ложную склейку разъединяет кнопка с \
         перечёркнутой головой на плитке, и вид остаётся на месте; отметьте нескольких чужих и \
         «Отделить выбранные» уберёт их всех разом.",
    ),
    (
        "Pick a group in the list to name it, or split the tiles.",
        "Выберите группу в списке, чтобы дать ей имя, или разъединяйте плитки.",
    ),
    ("People picked", "Выбрано лиц"),
    ("name for the picked people", "имя для выбранных"),
    ("Name the picked people", "Назначить имя выбранным"),
    (
        "the picked faces move into one group; an empty name creates an unnamed group",
        "отмеченные лица уедут в одну группу; пустое имя создаст группу без подписи",
    ),
    ("Unpick", "Снять выбор"),
    (
        "No faces yet: press “Find faces” above (the face model is required).",
        "Лиц пока нет: нажмите «Найти лица» выше (нужна модель лиц).",
    ),
    ("No faces in this group.", "В этой группе нет лиц."),
    ("no frame", "нет кадра"),
    ("split this face into a new group; this screen stays here", "отделить это лицо в новую группу; этот экран остаётся здесь"),
    ("mark this face for a shared name", "отметить это лицо для общего имени"),
    ("no faces found", "лица не найдены"),
    ("groups not built", "группы не собраны"),
    ("group name removed", "подпись группы убрана"),
    ("name not saved", "подпись не сохранена"),
    (
        "the face is in a new group; the screen stays here",
        "лицо в новой группе; экран остаётся здесь",
    ),
    (
        "the face is in a new group; the group was left empty",
        "лицо в новой группе; группа осталась пустой",
    ),
    ("Split the picked", "Отделить выбранные"),
    (
        "each picked face leaves this group into its own new one; the rest stay",
        "каждое отмеченное лицо уходит из этой группы в свою новую; остальные остаются",
    ),
    (
        "{} faces are in new groups; {} not found",
        "{} лиц в новых группах; {} не найдено",
    ),
    (
        "{} faces left the group; the screen stays here",
        "{} лиц вышло из группы; экран остаётся здесь",
    ),
    (
        "{} faces left the group and it was left empty",
        "{} лиц вышло из группы, и она осталась пустой",
    ),
    ("face not split off", "лицо не отделено"),
    ("faces not split off", "лица не отделены"),
    ("faces not assigned", "лица не назначены"),
    ("people: rebuilding groups…", "лица: пересобираю группы…"),
    (
        "downloading the face model: weights only, no photos",
        "скачивание модели лиц: только веса, без фотографий",
    ),
    // --- M26.11, the quarantine since M26.17: a proposal waits for a confirmation ------
    (
        "no named people yet: there is nothing to match to",
        "названных людей пока нет: сопоставлять некому",
    ),
    (
        "would propose {} faces to {} named people",
        "будет предложено {} лиц {} названным людям",
    ),
    (
        "proposed {} faces to {} named people — they wait in quarantine",
        "предложено {} лиц {} названным людям — они ждут в карантине",
    ),
    (
        "no new proposal yet: {} named",
        "новых предложений пока нет: названных — {}",
    ),
    (
        "{} left — the lead was too small",
        "{} осталось — перевес слишком мал",
    ),
    (
        "{} left — no named person is close enough",
        "{} осталось — ни один названный человек не достаточно близок",
    ),
    // --- M26.16: the thresholds explain themselves and count their effect --------------
    ("Tuning", "Настройка"),
    (
        "the merge threshold and the two sliders of the matching step; they decide the next pass, not the group on screen",
        "порог склейки и два ползунка сопоставления; они решают следующий проход, а не группу на экране",
    ),
    ("Merge threshold", "Порог склейки"),
    ("Closeness to a named person", "Близость к названному"),
    ("Lead over the next person", "Перевес над следующим"),
    (
        "how close a free face must be to a named person to be offered to you",
        "насколько близко неназванное лицо должно быть к названному человеку, чтобы его вам предложили",
    ),
    (
        "a face is offered only when it is at least that close and leads the next person by this much; a wrong match is turned down on its tile",
        "лицо предлагается, только когда оно настолько близко и опережает следующего человека на эту величину; ошибочное предложение отклоняется на плитке",
    ),
    (
        "These values decide the next pass — not the group on screen. “Rebuild groups” applies the merge threshold and looks for the named people; a name is checked as soon as it is saved.",
        "Эти значения решают следующий проход, а не группу на экране. «Пересобрать группы» применяет порог склейки и ищет названных людей; имя проверяется сразу после сохранения.",
    ),
    (
        "counting the free faces at {} / {}…",
        "считаю неназванные лица при {} / {}…",
    ),
    (
        "No named people yet: name a group and the free faces that resemble it will be offered here.",
        "Названных людей пока нет: назовите группу, и похожие на неё неназванные лица появятся здесь.",
    ),
    (
        "At {} / {}: 0 of {} free faces would join a named person ({} close but ambiguous, {} farther)",
        "При {} / {}: из {} неназванных лиц к названному человеку не подойдёт никто ({} близко, но неоднозначно, {} дальше)",
    ),
    (
        "At {} / {}: {} of {} free faces would join a named person ({} close but ambiguous, {} farther)",
        "При {} / {}: {} из {} неназванных лиц подойдут к названному человеку ({} близко, но неоднозначно, {} дальше)",
    ),
    ("Look for the named people", "Искать названных людей"),
    (
        "compares the free faces with the named people and puts the close ones into the quarantine; no group changes until you confirm a face",
        "сравнивает неназванные лица с названными людьми и помещает похожие в карантин; ни одна группа не меняется, пока вы не подтвердите лицо",
    ),
    // --- M26.17: the quarantine --------------------------------------------------------
    ("Quarantine", "Карантин"),
    (
        "faces that resemble a person you named; nothing has joined a group until you confirm",
        "лица, похожие на названного человека; ни одно не входит в группу, пока вы не подтвердите",
    ),
    (
        "in quarantine: {} — open the group list to confirm them",
        "в карантине: {} — откройте список групп, чтобы подтвердить",
    ),
    (
        "Each face resembles a person you named. Nothing has joined a group: the tick confirms it, the crossed head turns it down and the face stays where it is.",
        "Каждое лицо похоже на названного человека. В группу пока никто не вошёл: галочка подтверждает, перечёркнутая голова отклоняет, и лицо остаётся на месте.",
    ),
    (
        "Nothing waits for confirmation. Name a group, then open “Tuning” and press “Look for the named people”: the close faces will be offered here.",
        "Подтверждать нечего. Назовите группу, откройте «Настройку» и нажмите «Искать названных людей»: похожие лица появятся здесь.",
    ),
    ("Confirm all", "Подтвердить все"),
    (
        "every face below joins the person it resembles; a wrong one is split off on its tile afterwards",
        "каждое лицо ниже входит к человеку, на которого похоже; ошибочное потом отделяется кнопкой на плитке",
    ),
    (
        "confirm: this face joins the person it resembles",
        "подтвердить: лицо входит к человеку, на которого похоже",
    ),
    (
        "turn down: not this person; the face stays where it is",
        "отклонить: это не тот человек; лицо остаётся на месте",
    ),
    (
        "looks like {} at {} — confirm it or turn it down; nothing has changed yet",
        "похоже на {} с близостью {} — подтвердите или отклоните; пока ничего не изменилось",
    ),
    ("looking for the named people…", "ищу названных людей…"),
    ("nothing waits in quarantine", "в карантине ничего не ждёт"),
    (
        "the named people are not looked for",
        "названные люди не найдены",
    ),
    (
        "a face number is needed: faces confirm --id N",
        "нужен номер лица: faces confirm --id N",
    ),
    (
        "the free faces are not counted",
        "неназванные лица не посчитаны",
    ),
    (
        "1 face joined the person it resembles",
        "1 лицо вошло к человеку, на которого похоже",
    ),
    (
        "{} faces joined the person they resemble",
        "{} лиц вошли к человеку, на которого похожи",
    ),
    (
        "1 face stays where it is — the person is not them",
        "1 лицо остаётся на месте — это не тот человек",
    ),
    (
        "{} faces stay where they are — the person is not them",
        "{} лиц остаются на месте — это не тот человек",
    ),
    ("the faces are not confirmed", "лица не подтверждены"),
    ("the faces are not turned down", "лица не отклонены"),
];
