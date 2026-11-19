/* Web client for the same Cloudflare Worker API as the Rust desktop client.
   Authoritative rules and hidden-card privacy are enforced on the server. */
"use strict";

import { syncDiceRoll } from "./dice.js";

const $ = id => document.getElementById(id);
const AVATAR_GLYPHS = ["✦", "◆", "☼", "✿", "★", "♣", "◇", "◈", "✧", "●", "☻", "♟"];
const CARD_ORDER = [
  "gender", "body", "personality", "profession", "health",
  "hobby", "fear", "equipment", "backpack", "fact"
];
const ABILITY_KINDS = ["ability", "ability2"];
const LABELS = {
  gender: "Пол и возраст",
  body: "Телосложение",
  personality: "Характер",
  profession: "Профессия",
  health: "Здоровье",
  hobby: "Хобби",
  fear: "Фобия",
  equipment: "Инвентарь",
  backpack: "Рюкзак",
  fact: "Дополнительное сведение",
  ability: "Особая возможность",
  ability2: "Особая возможность 2"
};

let session = null;
let room = null;
let socket = null;
let connected = false;
let pending = false;
let packEditorOpen = false;
let packDraft = "";
let hostPanelOpen = true;
let copyFeedbackTimer = null;
let connectSeq = 0;
let editorState = null;

function setStatus(message, error = false) {
  const el = $("status");
  el.textContent = message;
  el.classList.toggle("notice", error);
}

function element(tag, text, className) {
  const node = document.createElement(tag);
  if (text !== undefined && text !== null) node.textContent = String(text);
  if (className) node.className = className;
  return node;
}

function addButton(parent, text, action, options = {}) {
  const button = element("button", text, options.className);
  button.type = "button";
  button.disabled = options.disabled ?? !connected;
  if (options.title) button.title = options.title;
  button.addEventListener("click", action);
  parent.append(button);
  return button;
}

function addText(parent, text, tag = "p", className) {
  const node = element(tag, text, className);
  parent.append(node);
  return node;
}

function avatarGlyph(id) {
  const numeric = Math.max(1, Number(id) || 1);
  return AVATAR_GLYPHS[(numeric - 1) % AVATAR_GLYPHS.length];
}

function createAvatarBadge(id, isYou = false) {
  const badge = element("span", avatarGlyph(id), isYou ? "avatar-badge is-you" : "avatar-badge");
  badge.title = `Аватар #${Math.max(1, Number(id) || 1)}`;
  return badge;
}

function labelFor(kind) {
  return room?.categoryLabels?.[kind] || LABELS[kind] || kind;
}

function splitCardValue(raw) {
  const text = String(raw || "").trim();
  const lines = text.split("\n").map(line => line.trim()).filter(Boolean);
  if (lines.length <= 1) return { main: text, note: "" };
  // Keep compact measurement lines (e.g. "31 год", "178 см") in main value.
  if (lines.length === 2 && (/\d+\s*(лет|год|года|см|кг)/i.test(lines[1]) || lines[1].length <= 28)) {
    return { main: lines.join(" · "), note: "" };
  }
  return { main: lines[0], note: lines.slice(1).join("\n") };
}

function parseBunkerSpecs(text) {
  const lines = String(text || "").split("\n").map(line => line.trim()).filter(Boolean);
  const specs = [];
  for (const line of lines) {
    const idx = line.indexOf(":");
    if (idx > 0 && idx < 42) {
      const key = line.slice(0, idx).trim();
      const val = line.slice(idx + 1).trim();
      if (key && val) {
        specs.push({ key, val });
        continue;
      }
    }
    if (specs.length > 0) {
      specs[specs.length - 1].val += "\n" + line;
    } else {
      specs.push({ key: "Описание", val: line });
    }
  }
  return specs;
}

function getBase() {
  const value = $("serverUrl").value.trim().replace(/\/+$/, "");
  const url = new URL(value);
  if (!["http:", "https:"].includes(url.protocol)) {
    throw new Error("Адрес сервера должен начинаться с http:// или https://");
  }
  if (url.pathname !== "/" || url.search || url.hash) {
    throw new Error("Нужен адрес сервера без дополнительных путей");
  }
  return url.origin;
}

function saveForm() {
  localStorage.setItem("murr-mobile-form", JSON.stringify({
    nickname: $("nickname").value,
    avatar: Number($("avatarChoice").value) || 1,
    base: $("serverUrl").value
  }));
}

function saveSession() {
  if (session) {
    sessionStorage.setItem("murr-mobile-session", JSON.stringify({ session, base: getBase() }));
  }
}

function renderAvatarPicker() {
  const picker = $("avatarPicker");
  if (!picker) return;
  const current = Math.max(1, Number($("avatarChoice").value) || 1);
  picker.replaceChildren();
  AVATAR_GLYPHS.forEach((glyph, index) => {
    const id = index + 1;
    const btn = element("button", glyph, id === current ? "avatar-tile selected" : "avatar-tile");
    btn.type = "button";
    btn.setAttribute("role", "radio");
    btn.setAttribute("aria-checked", String(id === current));
    btn.title = `Аватар ${id}`;
    btn.addEventListener("click", () => {
      $("avatarChoice").value = String(id);
      renderAvatarPicker();
      saveForm();
    });
    picker.append(btn);
  });
}

async function postJson(path, body) {
  const response = await fetch(getBase() + path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body)
  });
  const json = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(json.error || "Ошибка сервера HTTP " + response.status);
  return json;
}

async function startSession(join = false) {
  if (pending) return;
  const name = $("nickname").value.trim();
  if (name.length < 2 || name.length > 24) {
    setStatus("Ник должен содержать от 2 до 24 символов", true);
    $("nickname").focus();
    return;
  }
  const code = $("joinCode").value.trim().toUpperCase();
  if (join && !/^[A-Z2-9]{8}$/.test(code)) {
    setStatus("Введите 8-символьный код комнаты (латинские буквы и цифры 2–9)", true);
    $("joinCode").focus();
    return;
  }
  pending = true;
  $("create").disabled = $("join").disabled = true;
  setStatus("Подключаемся к серверу…");
  try {
    saveForm();
    const path = join ? "/api/rooms/" + code + "/join" : "/api/rooms";
    session = await postJson(path, { name, avatar: Number($("avatarChoice").value) || 1 });
    saveSession();
    connect();
  } catch (error) {
    setStatus("Не удалось открыть комнату: " + error.message, true);
  } finally {
    pending = false;
    $("create").disabled = $("join").disabled = false;
  }
}

async function requestWsTicket(baseOrigin, activeSession) {
  const response = await fetch(
    baseOrigin + "/api/rooms/" + encodeURIComponent(activeSession.code) + "/ws-ticket",
    {
      method: "POST",
      headers: { authorization: "Bearer " + activeSession.token }
    }
  );
  if (response.status === 404 || response.status === 405) {
    return null; // Backwards-compatible fallback for older Workers
  }
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    const err = new Error(data.error || "Ошибка авторизации HTTP " + response.status);
    err.status = response.status;
    throw err;
  }
  return data.ticket || null;
}

async function connect() {
  if (!session) return;
  const activeSession = session;
  const seq = ++connectSeq;
  if (socket) {
    socket.onclose = null;
    socket.close();
    socket = null;
  }
  try {
    const origin = getBase();
    setStatus("Устанавливаем WebSocket-соединение…");
    let ticket = null;
    try {
      ticket = await requestWsTicket(origin, activeSession);
    } catch (error) {
      if (seq !== connectSeq) return;
      if (error.status === 401 || error.status === 410) {
        sessionStorage.removeItem("murr-mobile-session");
      }
      setStatus("Не удалось подключиться: " + error.message, true);
      return;
    }
    if (seq !== connectSeq || session !== activeSession) return;

    const base = new URL(origin);
    base.protocol = base.protocol === "https:" ? "wss:" : "ws:";
    base.pathname = "/api/rooms/" + encodeURIComponent(activeSession.code) + "/ws";
    // Prefer short-lived single-use ticket so the session token never appears in the WS URL.
    base.search = ticket
      ? "?ticket=" + encodeURIComponent(ticket)
      : "?token=" + encodeURIComponent(activeSession.token);
    socket = new WebSocket(base.href);
    socket.onopen = () => {
      if (seq !== connectSeq) return;
      connected = true;
      updateConnection();
      setStatus("Соединение установлено");
      render();
    };
    socket.onmessage = event => {
      if (seq !== connectSeq) return;
      try {
        const message = JSON.parse(event.data);
        if (message.type === "state") {
          room = message.data;
          syncDiceRoll(room.diceRoll);
          render();
          syncCardEditor();
        } else if (message.type === "error") {
          setStatus(message.message || "Ошибка комнаты", true);
        }
      } catch {
        setStatus("Получен некорректный ответ сервера", true);
      }
    };
    socket.onclose = () => {
      if (seq !== connectSeq) return;
      connected = false;
      updateConnection();
      setStatus("Соединение закрыто. Нажми «Переподключить».", true);
      render();
    };
    socket.onerror = () => {
      if (seq === connectSeq) setStatus("Ошибка WebSocket. Проверь адрес сервера.", true);
    };
    $("setup").hidden = true;
    $("room").hidden = false;
  } catch (error) {
    setStatus("Ошибка подключения: " + error.message, true);
  }
}

function send(type, other = {}) {
  if (!connected || !socket || socket.readyState !== WebSocket.OPEN) {
    setStatus("Нет подключения. Нажми «Переподключить».", true);
    return false;
  }
  socket.send(JSON.stringify({ type, ...other }));
  return true;
}

function updateConnection() {
  const pill = $("connection");
  pill.textContent = connected ? "● Онлайн" : "○ Нет подключения";
  pill.classList.toggle("online", connected);
}

function renderSpeechTimer() {
  const timer = $("speechTimer");
  if (!timer) return;
  const active = room?.stage === "playing" && Boolean(room.speechEndsAt);
  timer.hidden = !active;
  if (!active) return;
  const seconds = Math.max(0, Math.ceil((room.speechEndsAt - Date.now()) / 1000));
  timer.textContent = seconds ? `⏱ Обсуждение · ${seconds} сек.` : "⏱ Время вышло";
  timer.classList.toggle("urgent", seconds <= 5);
}
setInterval(renderSpeechTimer, 500);

function renderLobby(mount, isHost) {
  const panel = element("section", null, "panel");
  const head = element("div", null, "panel-header");
  addText(head, "Лобби", "h2");
  addText(head, `${room.players.length} из 15 игроков`, "span", "badge accent");
  panel.append(head);

  addText(panel, "Отправь код комнаты друзьям. Для старта нужно минимум два готовых игрока.", "p", "muted");

  const grid = element("div", null, "roster-grid");
  for (const player of room.players) {
    const playerIsHost = player.id === room.hostId;
    const isYou = player.id === room.you;
    const ready = playerIsHost || player.ready;
    const card = element("div", null, `roster-card${ready ? " ready" : ""}${isYou ? " is-you" : ""}`);
    card.append(createAvatarBadge(player.avatarId ?? player.avatar ?? 1, isYou));
    const info = element("div", null, "roster-info");
    addText(info, `${playerIsHost ? "♛ " : ""}${player.name}`, "div", "roster-name");
    const subText = playerIsHost
      ? (isYou ? "Ведущий · вы" : "Ведущий")
      : (ready ? (isYou ? "Готов · вы" : "Готов") : (isYou ? "Не готов · вы" : "Ожидание"));
    addText(info, subText, "div", "roster-sub");
    card.append(info);
    grid.append(card);
  }
  panel.append(grid);

  if (isHost) {
    const controls = element("div", null, "control-row");

    const seatsGroup = element("div", null, "control-group");
    const seatsLabel = element("label", "Мест в бункере:");
    seatsLabel.htmlFor = "lobbySeatsInput";
    const seatsInput = element("input");
    seatsInput.id = "lobbySeatsInput";
    seatsInput.type = "number";
    seatsInput.min = "1";
    seatsInput.max = "15";
    seatsInput.value = String(room.seats);
    seatsInput.addEventListener("change", () => {
      if (seatsInput.value !== "") send("set_seats", { seats: Number(seatsInput.value) });
    });
    seatsGroup.append(seatsLabel, seatsInput);
    controls.append(seatsGroup);

    const chanceGroup = element("div", null, "control-group");
    const chanceValue = Number(room.eventChance ?? 0);
    const chanceLabel = element("label", `Шанс событий: ${chanceValue}%`);
    chanceLabel.htmlFor = "lobbyChanceInput";
    const chanceInput = element("input");
    chanceInput.id = "lobbyChanceInput";
    chanceInput.type = "range";
    chanceInput.min = "0";
    chanceInput.max = "100";
    chanceInput.value = String(chanceValue);
    chanceInput.style.width = "130px";
    chanceInput.addEventListener("input", () => {
      chanceLabel.textContent = `Шанс событий: ${chanceInput.value}%`;
    });
    chanceInput.addEventListener("change", () => {
      send("set_event_chance", { chance: Number(chanceInput.value) });
    });
    chanceGroup.append(chanceLabel, chanceInput);
    controls.append(chanceGroup);

    panel.append(controls);

    const actions = element("div", null, "actions");
    const canStart = connected && room.players.length >= 2 && room.players.every(p => p.id === room.hostId || p.ready);
    addButton(actions, "Начать игру", () => send("start"), {
      className: "primary",
      disabled: !canStart,
      title: canStart ? "" : "Нужны минимум два игрока, и все гости должны нажать «Готов»"
    });
    panel.append(actions);
    mount.append(panel);

    const editor = element("section", null, "panel");
    const details = element("details");
    details.dataset.packEditor = "true";
    details.open = packEditorOpen;
    details.addEventListener("toggle", () => { packEditorOpen = details.open; });
    details.append(element("summary", "Свой пак (JSON / ZIP / импорт Бункер Онлайн)"));
    addText(
      details,
      "JSON содержит секции roles, characteristics, items, world. ZIP (до 2 МБ) — pack.json + изображения assets/. До старта ZIP отправляется всем игрокам.",
      "p",
      "muted tiny"
    );

    const file = element("input");
    file.type = "file";
    file.accept = "application/json,.json,application/zip,.zip";
    const area = element("textarea");
    area.dataset.packTextarea = "true";
    area.value = packDraft;
    area.placeholder = '{"formatVersion":2,"name":"Мой пак","categories":{"profession":{"cards":[{"title":"Инженер","stage":"Опытный"}]}}}';
    area.setAttribute("aria-label", "JSON пака");
    area.addEventListener("input", () => { packDraft = area.value; });

    file.addEventListener("change", async () => {
      try {
        const selected = file.files?.[0];
        if (!selected) return;
        if (selected.name.toLowerCase().endsWith(".zip")) {
          if (selected.size > 2000000) throw new Error("ZIP не должен превышать 2 МБ");
          const response = await fetch(getBase() + `/api/rooms/${room.code}/bundle`, {
            method: "POST",
            headers: {
              authorization: `Bearer ${session.token}`,
              "content-type": "application/zip"
            },
            body: selected
          });
          const answer = await response.json();
          if (!response.ok) throw new Error(answer.error || "Не удалось отправить ZIP");
          setStatus("ZIP-пак загружен для всей комнаты: " + answer.packName);
          packDraft = "";
          area.value = "";
        } else {
          packDraft = await selected.text();
          area.value = packDraft;
          setStatus("JSON загружен в редактор. Нажми «Применить пак к комнате».");
        }
      } catch (err) {
        setStatus(err.message, true);
      }
    });

    details.append(file, area);
    const editorActions = element("div", null, "actions");
    addButton(editorActions, "Загрузить пример v2", async () => {
      try {
        const response = await fetch("/packs/starter.json");
        if (!response.ok) throw new Error("Пример пока не опубликован");
        packDraft = await response.text();
        area.value = packDraft;
        setStatus("Загружен пример структурированного пака v2");
      } catch (err) {
        setStatus(err.message, true);
      }
    }, { disabled: false });
    addButton(editorActions, "Применить пак к комнате", () => {
      try {
        if (!area.value.trim()) throw new Error("Поле JSON пустое");
        if (send("set_pack", { pack: JSON.parse(area.value) })) {
          setStatus("Пак отправлен в комнату");
        }
      } catch (err) {
        setStatus("Ошибка JSON: " + err.message, true);
      }
    }, { className: "btn-accent-ghost" });
    details.append(editorActions);
    editor.append(details);
    mount.append(editor);
  } else {
    const me = room.players.find(p => p.id === room.you);
    const actions = element("div", null, "actions");
    addButton(actions, me?.ready ? "Отменить готовность" : "Я готов", () => send("set_ready", { ready: !me?.ready }), {
      className: me?.ready ? "btn-ghost" : "ready-btn"
    });
    panel.append(actions);
    addText(panel, "Ведущий начнёт партию, как только все игроки подтвердят готовность.", "p", "muted tiny");
    mount.append(panel);
  }
}

function renderFinished(mount, isHost) {
  const panel = element("section", null, "panel");
  const survivors = room.players.filter(p => p.active);
  const eliminated = room.players.filter(p => !p.active);

  const head = element("div", null, "panel-header");
  addText(head, room.stage === "closed" ? "Комната закрыта ведущим" : "Партия завершена", "h2");
  addText(head, `Выжили: ${survivors.length} / ${room.seats}`, "span", "badge positive");
  panel.append(head);

  if (survivors.length > 0) {
    const grid = element("div", null, "roster-grid");
    for (const player of survivors) {
      const card = element("div", null, `roster-card ready${player.id === room.you ? " is-you" : ""}`);
      card.append(createAvatarBadge(player.avatarId ?? player.avatar ?? 1, player.id === room.you));
      const info = element("div", null, "roster-info");
      addText(info, player.name, "div", "roster-name");
      const prof = player.cards.find(c => c.kind === "profession" && c.revealed);
      addText(info, prof ? splitCardValue(prof.value).main : "Попал в бункер", "div", "roster-sub");
      card.append(info);
      grid.append(card);
    }
    panel.append(grid);
  } else {
    addText(panel, "Никто не остался в бункере.", "p", "muted");
  }

  if (eliminated.length > 0) {
    addText(panel, `Выбыли (${eliminated.length}): ${eliminated.map(p => p.name).join(", ")}`, "p", "muted tiny");
  }

  if (isHost && room.stage === "finished") {
    const actions = element("div", null, "actions");
    addButton(actions, "Начать заново", () => send("host_action", { action: "restart" }), { className: "primary" });
    addButton(actions, "Закрыть комнату", () => send("host_action", { action: "close" }), { className: "btn-danger-ghost" });
    panel.append(actions);
  }

  mount.append(panel);
  renderLog(mount);
}

function renderDossier(mount, me) {
  if (!me) return;
  const panel = element("section", null, "panel");
  const head = element("div", null, "panel-header");
  addText(head, `Моё досье · ${me.name}`, "h2");
  const revealedCount = me.cards.filter(c => c.revealed).length;
  addText(head, `Открыто ${revealedCount} из ${me.cards.length}`, "span", me.active ? "badge positive" : "badge danger");
  panel.append(head);

  if (!me.active) {
    addText(panel, "Вы выбыли из борьбы за бункер. Открытие новых карт заблокировано.", "p", "muted");
  }

  const ordered = [];
  for (const kind of CARD_ORDER) {
    const found = me.cards.find(c => c.kind === kind);
    if (found) ordered.push(found);
  }
  const extra = me.cards.filter(c => !CARD_ORDER.includes(c.kind) && !ABILITY_KINDS.includes(c.kind) && c.kind !== "talent");
  const abilities = ABILITY_KINDS.map(k => me.cards.find(c => c.kind === k)).filter(Boolean);

  const grid = element("div", null, "dossier-grid");
  for (const card of [...ordered, ...extra, ...abilities]) {
    const isAbility = ABILITY_KINDS.includes(card.kind);
    const box = element(
      "div",
      null,
      `dossier-card${card.revealed ? " revealed" : ""}${isAbility ? " ability" : ""}`
    );
    const body = element("div", null, "dossier-card-body");
    const cardHead = element("div", null, "dossier-card-head");
    addText(cardHead, labelFor(card.kind), "span", "dossier-card-label");
    addText(cardHead, card.revealed ? "Открыто" : "Скрыто", "span", card.revealed ? "badge positive" : "badge");
    body.append(cardHead);

    const { main, note } = splitCardValue(card.value);
    addText(body, main, "div", "dossier-card-value");
    if (note) addText(body, note, "div", "dossier-card-desc");
    box.append(body);

    if (!card.revealed && me.active) {
      addButton(box, "Открыть", () => send("reveal", { kind: card.kind }), {
        className: "btn-accent-ghost btn-sm"
      });
    }
    grid.append(box);
  }
  panel.append(grid);
  mount.append(panel);
}

function renderVoting(mount, me, isHost) {
  if (!room.voteOpen && (!room.voteResults || room.voteResults.length === 0)) return;
  const panel = element("section", null, "panel vote-panel");
  const head = element("div", null, "panel-header");
  const isRunoff = Array.isArray(room.voteCandidates) && room.voteCandidates.length > 0;
  addText(
    head,
    room.voteOpen
      ? (isRunoff ? "Переголосование открыто" : "Голосование открыто")
      : "Итоги голосования",
    "h2"
  );
  if (room.voteOpen) {
    addText(head, `Принято голосов: ${room.ballotsCount}`, "span", "badge accent");
  } else if (room.voteTied) {
    addText(head, "Ничья по голосам!", "span", "badge danger");
  }
  panel.append(head);

  if (room.voteOpen) {
    if (!me?.active) {
      addText(panel, "Выбывшие участники не могут голосовать.", "p", "muted");
    } else {
      const candidatePool = room.players.filter(
        p => p.active && p.id !== room.you && (!isRunoff || room.voteCandidates.includes(p.id))
      );
      if (candidatePool.length === 0) {
        addText(panel, "Нет доступных кандидатов для вашего голоса.", "p", "muted");
      } else {
        addText(
          panel,
          isRunoff
            ? "Переголосование среди лидеров (голос можно поменять до закрытия):"
            : "Выберите кандидата на исключение из бункера (голос можно поменять до закрытия):",
          "p",
          "muted tiny"
        );
        const options = element("div", null, "vote-options");
        for (const player of candidatePool) {
          const chosen = room.yourVote === player.id;
          addButton(
            options,
            chosen ? `✓ ${player.name}` : `Против ${player.name}`,
            () => send("vote", { target: player.id }),
            { className: chosen ? "vote-btn chosen" : "vote-btn" }
          );
        }
        panel.append(options);
      }
    }
  }

  if (room.voteResults && room.voteResults.length > 0) {
    const maxVotes = Math.max(1, ...room.voteResults.map(r => r.votes));
    const list = element("div", null, "vote-results");
    for (const result of room.voteResults) {
      const item = element("div");
      const barHead = element("div", null, "vote-bar-head");
      addText(barHead, result.name, "strong");
      addText(barHead, `${result.votes} голос(ов)`, "span", "muted");
      const track = element("div", null, "vote-bar-track");
      const fill = element("div", null, "vote-bar-fill");
      fill.style.width = `${Math.round((result.votes / maxVotes) * 100)}%`;
      track.append(fill);
      item.append(barHead, track);
      list.append(item);
    }
    panel.append(list);

    if (isHost && !room.voteOpen) {
      const actions = element("div", null, "actions");
      if (room.voteTied) {
        addButton(actions, "Переголосование среди лидеров", () => send("host_action", { action: "runoff_vote" }), {
          className: "primary btn-sm"
        });
      } else {
        const leader = room.voteResults[0];
        const leaderActive = leader?.playerId && room.players.some(p => p.id === leader.playerId && p.active);
        if (leaderActive) {
          addButton(
            actions,
            `Исключить лидера (${leader.name})`,
            () => send("host_action", { action: "exclude_vote_leader" }),
            { className: "btn-danger-ghost btn-sm" }
          );
        }
      }
      if (actions.childElementCount > 0) panel.append(actions);
    }
  }
  mount.append(panel);
}

function renderHostPanel(mount) {
  const panel = element("section", null, "panel host-box");
  const details = element("details");
  details.dataset.hostPanel = "true";
  details.open = hostPanelOpen;
  details.addEventListener("toggle", () => { hostPanelOpen = details.open; });
  details.append(element("summary", "♛ Панель ведущего"));

  addText(details, "Управление раундом", "div", "host-section-title");
  const roundActions = element("div", null, "actions");
  addButton(roundActions, "Следующий раунд", () => send("next_round"));
  addButton(
    roundActions,
    room.voteOpen ? "Закрыть голосование" : "Открыть голосование",
    () => send(room.voteOpen ? "finish_vote" : "start_vote"),
    { className: room.voteOpen ? "primary" : "btn-accent-ghost" }
  );
  addButton(roundActions, "Круг без изгнания", () => send("host_action", { action: "skip_vote" }));
  if (room.hostControls?.undo) {
    addButton(roundActions, "↩ Отменить действие", () => send("host_undo"), { className: "btn-ghost" });
  }
  addButton(roundActions, "Завершить партию", () => send("finish"), { className: "btn-danger-ghost" });
  details.append(roundActions);

  addText(details, `События раунда (шанс ${room.eventChance ?? 0}%)`, "div", "host-section-title");
  const eventActions = element("div", null, "actions");
  addButton(eventActions, "⚡ Запустить событие", () => send("host_action", { action: "trigger_event" }), {
    className: "btn-accent-ghost btn-sm"
  });
  if (room.currentEvent) {
    addButton(eventActions, "Скрыть событие", () => send("host_action", { action: "clear_event" }), {
      className: "btn-ghost btn-sm"
    });
  }
  details.append(eventActions);

  addText(details, "Таймер выступления", "div", "host-section-title");
  const timerActions = element("div", null, "actions");
  for (const seconds of [15, 30, 60]) {
    addButton(timerActions, `${seconds} сек`, () => send("host_action", { action: "timer", seconds }), {
      className: "btn-sm"
    });
  }
  addButton(timerActions, "Стоп", () => send("host_action", { action: "timer", seconds: 0 }), {
    className: "btn-ghost btn-sm"
  });
  details.append(timerActions);

  panel.append(details);
  mount.append(panel);
}

function renderLog(mount) {
  const panel = element("section", null, "panel");
  const head = element("div", null, "panel-header");
  addText(head, "Журнал партии", "h2");
  addText(head, `${room.log.length} записей`, "span", "muted tiny");
  panel.append(head);

  if (!room.log || room.log.length === 0) {
    addText(panel, "Событий пока нет.", "p", "muted");
  } else {
    const list = element("ol", null, "log");
    for (const message of [...room.log].reverse()) {
      addText(list, message, "li");
    }
    panel.append(list);
  }
  mount.append(panel);
}

function render() {
  if (!room) return;
  const isHost = room.you === room.hostId;
  const me = room.players.find(p => p.id === room.you);
  updateBackground(room).catch(() => {});

  $("setup").hidden = true;
  $("room").hidden = false;
  $("roomCode").textContent = room.code;
  const aliveCount = room.players.filter(p => p.active).length;
  const stageText = room.stage === "lobby"
    ? "Лобби"
    : room.stage === "playing"
      ? `Раунд ${room.round} · Выживших: ${aliveCount}/${room.players.length}`
      : "Финал";
  $("roomMeta").textContent = `${stageText} · Мест в бункере: ${room.seats} · Пак: ${room.packName} · В сети: ${room.connected}`;
  renderSpeechTimer();

  const mount = $("roomUI");
  const previouslyOpen = new Set(
    Array.from(mount.querySelectorAll("details[data-player]"))
      .filter(item => item.open)
      .map(item => item.dataset.player)
  );
  const hadPlayers = mount.querySelector("details[data-player]") !== null;
  const existingPackDetails = mount.querySelector("details[data-pack-editor]");
  if (existingPackDetails) packEditorOpen = existingPackDetails.open;
  const existingPackArea = mount.querySelector("textarea[data-pack-textarea]");
  if (existingPackArea) packDraft = existingPackArea.value;
  const existingHostDetails = mount.querySelector("details[data-host-panel]");
  if (existingHostDetails) hostPanelOpen = existingHostDetails.open;

  mount.replaceChildren();

  if (room.stage === "lobby") {
    renderLobby(mount, isHost);
    return;
  }

  if (["finished", "closed"].includes(room.stage)) {
    renderFinished(mount, isHost);
    return;
  }

  // Scenario & Bunker
  const scenario = element("section", null, "panel");
  const hero = element("div", null, "scenario-hero");
  addText(hero, `Катаклизм · Раунд ${room.round}`, "small", "eyebrow");
  addText(hero, room.catastrophe, "p", "multiline");
  scenario.append(hero);

  const bunkerHead = element("div", null, "panel-header");
  addText(bunkerHead, "Бункер", "h3");
  addText(bunkerHead, `Мест: ${room.seats} · Шанс событий: ${room.eventChance ?? 0}%`, "span", "badge");
  scenario.append(bunkerHead);

  const specs = parseBunkerSpecs(room.bunker);
  const bunkerGrid = element("div", null, "bunker-grid");
  for (const spec of specs) {
    const item = element("div", null, "bunker-spec");
    addText(item, spec.key, "small");
    addText(item, spec.val, "strong");
    bunkerGrid.append(item);
  }
  scenario.append(bunkerGrid);

  if (room.currentEvent || (room.eventHistory && room.eventHistory.length > 0)) {
    const eventBox = element("div", null, "event-card");
    if (room.currentEvent) {
      const evHead = element("div", null, "event-card-head");
      addText(evHead, `⚡ ${room.currentEvent.title}`, "div", "event-card-title");
      addText(evHead, `Раунд ${room.currentEvent.round}`, "span", "badge accent");
      eventBox.append(evHead);
      if (room.currentEvent.description) {
        addText(eventBox, room.currentEvent.description, "p", "multiline");
      }
    } else {
      addText(eventBox, "В текущем раунде событий нет.", "p", "muted tiny");
    }
    const pastEvents = (room.eventHistory || [])
      .slice()
      .reverse()
      .filter(ev => !room.currentEvent || ev.title !== room.currentEvent.title || ev.round !== room.currentEvent.round)
      .slice(0, 5);
    if (pastEvents.length > 0) {
      const hist = element("details", null, "event-history");
      hist.append(element("summary", `Прошедшие события (${pastEvents.length})`));
      const ul = element("ul");
      for (const ev of pastEvents) {
        addText(ul, `Раунд ${ev.round}: ${ev.title}${ev.description ? " — " + ev.description : ""}`, "li");
      }
      hist.append(ul);
      eventBox.append(hist);
    }
    scenario.append(eventBox);
  }

  const diceActions = element("div", null, "actions");
  for (const sides of [6, 20]) {
    addButton(diceActions, `🎲 Бросить D${sides}`, () => send("roll", { sides }), { className: "btn-sm" });
  }
  scenario.append(diceActions);
  mount.append(scenario);

  // Voting (prominent when open or results exist)
  renderVoting(mount, me, isHost);

  // Host controls drawer
  if (isHost) renderHostPanel(mount);

  // Player's own dossier
  renderDossier(mount, me);

  // All participants table/cards
  const participants = element("section", null, "panel");
  const partHead = element("div", null, "panel-header");
  addText(partHead, "Участники", "h2");
  addText(partHead, `Активны: ${aliveCount} из ${room.players.length}`, "span", "badge");
  participants.append(partHead);

  for (const player of room.players) {
    const details = element("details", null, `player${player.active ? "" : " eliminated"}`);
    details.dataset.player = player.id;
    details.open = previouslyOpen.has(player.id) || (!hadPlayers && player.id !== room.you && player.cards.some(c => c.revealed));

    const summary = element("summary");
    const left = element("div", null, "player-summary-left");
    left.append(createAvatarBadge(player.avatarId ?? player.avatar ?? 1, player.id === room.you));
    const displayName = `${player.name}${player.id === room.hostId ? " ♛" : ""}${player.id === room.you ? " (вы)" : ""}`;
    addText(left, displayName, "span", "player-summary-name");

    const right = element("div", null, "player-summary-right");
    const revealedCards = player.cards.filter(c => c.revealed);
    addText(right, `${revealedCards.length} откр.`, "span", "badge");
    addText(right, player.active ? "В игре" : "Выбыл", "span", player.active ? "badge positive" : "badge danger");
    summary.append(left, right);
    details.append(summary);

    const body = element("div", null, "player-body");
    if (isHost) {
      const hostRow = element("div", null, "actions");
      addButton(
        hostRow,
        player.active ? "Исключить" : "Вернуть в игру",
        () => send("set_active", { playerId: player.id, active: !player.active }),
        { className: player.active ? "btn-danger-ghost btn-sm" : "btn-accent-ghost btn-sm" }
      );
      addButton(
        hostRow,
        "✎ Редактор карт…",
        () => openCardEditor(player.id, revealedCards[0]?.kind || "profession"),
        { className: "btn-ghost btn-sm", title: "Редактировать любую карту игрока, здоровье, обмен или предметы" }
      );
      body.append(hostRow);
    }

    const cards = element("div", null, "cards");
    const visibleCards = player.id === room.you ? revealedCards : player.cards;
    if (visibleCards.length === 0) {
      addText(cards, "Пока нет открытых характеристик.", "p", "muted tiny");
    } else {
      for (const card of visibleCards) {
        const row = element("div", null, "card");
        const info = element("span");
        addText(info, labelFor(card.kind), "small");
        addText(info, card.value, "span", "multiline");
        row.append(info);
        if (isHost) {
          const cardActions = element("div", null, "card-actions");
          addButton(cardActions, "✎", () => openCardEditor(player.id, card.kind), {
            className: "btn-ghost btn-sm",
            title: "Редактировать карточку"
          });
          addButton(cardActions, "↻", () => send("host_redraw_card", { playerId: player.id, kind: card.kind }), {
            className: "btn-ghost btn-sm",
            title: "Перегенерировать характеристику"
          });
          row.append(cardActions);
        }
        cards.append(row);
      }
    }
    body.append(cards);
    details.append(body);
    participants.append(details);
  }
  mount.append(participants);

  // Newest-first party log
  renderLog(mount);
}

let loadedBackground = "";
let currentBackgroundUrl = null;
async function updateBackground(state) {
  const bundle = state.bundle;
  const key = bundle?.background ? JSON.stringify([state.code, bundle.sha256, bundle.background]) : "";
  if (loadedBackground === key || !session) return;
  loadedBackground = key;
  if (currentBackgroundUrl) {
    URL.revokeObjectURL(currentBackgroundUrl);
    currentBackgroundUrl = null;
    document.documentElement.style.removeProperty("--room-background");
    $("room").classList.remove("has-bg");
  }
  if (!key) return;
  try {
    const response = await fetch(getBase() + "/api/rooms/" + state.code + "/background", {
      headers: { authorization: "Bearer " + session.token }
    });
    if (!response.ok) return;
    const blob = await response.blob();
    if (loadedBackground !== key) return;
    currentBackgroundUrl = URL.createObjectURL(blob);
    document.documentElement.style.setProperty("--room-background", `url("${currentBackgroundUrl}")`);
    $("room").classList.add("has-bg");
  } catch {
    // A missing optional background should not prevent the game from working.
  }
}

const HEALTH_PRESETS = ["Лёгкая степень", "Средняя степень", "Тяжёлая степень", "Критическая степень"];

function allEditableKinds() {
  const seen = new Set();
  const list = [];
  for (const kind of [...CARD_ORDER, ...ABILITY_KINDS]) {
    if (!seen.has(kind)) {
      seen.add(kind);
      list.push(kind);
    }
  }
  for (const kind of Object.keys(room?.categoryLabels || {})) {
    if (kind !== "talent" && !seen.has(kind)) {
      seen.add(kind);
      list.push(kind);
    }
  }
  return list;
}

function selectedEditorItemIds() {
  return Array.from(document.querySelectorAll('#editorItemList input[type="checkbox"]:checked'))
    .map(input => input.value);
}

function closeCardEditor() {
  editorState = null;
  const dialog = $("cardEditorModal");
  if (dialog?.open) dialog.close();
}

function openCardEditor(playerId, kind) {
  if (!room || room.you !== room.hostId) return;
  const kinds = allEditableKinds();
  editorState = {
    playerId,
    kind: kinds.includes(kind) ? kind : kinds[0] || "profession",
    dirtyText: false,
    dirtyHealth: false
  };
  syncCardEditor(true);
  const dialog = $("cardEditorModal");
  if (dialog && !dialog.open) {
    if (typeof dialog.showModal === "function") {
      dialog.showModal();
    } else {
      dialog.setAttribute("open", "open");
    }
  }
}

function syncCardEditor(forcePopulate = false) {
  const dialog = $("cardEditorModal");
  if (!dialog || !editorState) return;
  if (!room || room.you !== room.hostId || room.stage !== "playing") {
    closeCardEditor();
    return;
  }
  const player = room.players.find(p => p.id === editorState.playerId);
  if (!player) {
    closeCardEditor();
    return;
  }

  $("editorModalTitle").textContent = `${player.name} · ${labelFor(editorState.kind)}`;

  const categorySelect = $("editorCategory");
  const kinds = allEditableKinds();
  categorySelect.replaceChildren();
  for (const kind of kinds) {
    const opt = element("option", labelFor(kind));
    opt.value = kind;
    opt.selected = kind === editorState.kind;
    categorySelect.append(opt);
  }

  const card = player.cards.find(c => c.kind === editorState.kind && (c.revealed || player.id === room.you)) || null;
  const isInventoryCard = Array.isArray(card?.items) || editorState.kind === "equipment" || editorState.kind === "backpack";

  // Inventory section
  const invSection = $("editorInventorySection");
  invSection.hidden = !isInventoryCard;
  if (isInventoryCard) {
    const checkedBefore = new Set(selectedEditorItemIds());
    const itemList = $("editorItemList");
    itemList.replaceChildren();
    const items = Array.isArray(card?.items) ? card.items : [];
    if (items.length === 0) {
      addText(
        itemList,
        card ? "Предметов пока нет." : "Карта скрыта или пуста. Можно добавить предмет или открыть карту.",
        "div",
        "muted tiny"
      );
    } else {
      for (const item of items) {
        const row = element("label", null, "item-check-row");
        const cb = element("input");
        cb.type = "checkbox";
        cb.value = item.id;
        cb.checked = checkedBefore.has(item.id);
        row.append(cb, element("span", `${item.id}: ${item.value}`));
        itemList.append(row);
      }
    }
  }

  // Health degree section
  const healthSection = $("editorHealthSection");
  healthSection.hidden = editorState.kind !== "health" || !room.canSetHealthDegree;
  if (!healthSection.hidden) {
    const presetsWrap = $("editorHealthPresets");
    if (presetsWrap.childElementCount === 0) {
      for (const preset of HEALTH_PRESETS) {
        addButton(presetsWrap, preset, () => {
          $("editorHealthDegree").value = preset;
          if (editorState) editorState.dirtyHealth = true;
        }, { className: "btn-ghost btn-sm", disabled: false });
      }
    }
    if (forcePopulate || !editorState.dirtyHealth) {
      $("editorHealthDegree").value = card?.healthDegree || "";
    }
  }

  // Recipient select for swap / item transfer
  const recipientSelect = $("editorRecipient");
  const prevRecipient = recipientSelect.value;
  recipientSelect.replaceChildren();
  const others = room.players.filter(p => p.id !== player.id);
  for (const other of others) {
    const opt = element("option", `${other.name}${other.active ? "" : " (выбыл)"}`);
    opt.value = other.id;
    if (other.id === prevRecipient) opt.selected = true;
    recipientSelect.append(opt);
  }
  const hasRecipient = others.length > 0;
  $("editorSwapSection").hidden = !hasRecipient;
  $("editorSwapBtn").disabled = !connected || !hasRecipient || !room.canSwapCards;
  $("editorTransferBtn").disabled = !connected || !hasRecipient;
  $("editorRemoveItemsBtn").disabled = !connected || !room.hostControls;

  // Text editor
  $("editorTextLabel").textContent = isInventoryCard ? "Заменить содержимое ячейки целиком" : "Текст характеристики";
  $("editorSaveTextBtn").textContent = isInventoryCard ? "Заменить содержимое" : "Применить текст";
  if (forcePopulate || !editorState.dirtyText) {
    $("editorText").value = !isInventoryCard && card ? card.value : "";
  }
}

$("editorCategory").addEventListener("change", () => {
  if (!editorState) return;
  editorState.kind = $("editorCategory").value;
  editorState.dirtyText = false;
  editorState.dirtyHealth = false;
  $("editorNewItem").value = "";
  syncCardEditor(true);
});

$("editorText").addEventListener("input", () => {
  if (editorState) editorState.dirtyText = true;
});

$("editorHealthDegree").addEventListener("input", () => {
  if (editorState) editorState.dirtyHealth = true;
});

$("editorCloseX").addEventListener("click", closeCardEditor);
$("editorCloseBtn").addEventListener("click", closeCardEditor);
$("cardEditorModal").addEventListener("cancel", () => { editorState = null; });
$("cardEditorModal").addEventListener("click", event => {
  if (event.target === $("cardEditorModal")) closeCardEditor();
});

$("editorAddItemBtn").addEventListener("click", () => {
  if (!editorState) return;
  const value = $("editorNewItem").value.trim();
  if (!value) return;
  if (send("host_add_item", { playerId: editorState.playerId, kind: editorState.kind, value })) {
    $("editorNewItem").value = "";
  }
});

$("editorTransferBtn").addEventListener("click", () => {
  if (!editorState) return;
  const itemIds = selectedEditorItemIds();
  const targetId = $("editorRecipient").value;
  if (itemIds.length === 0) {
    setStatus("Отметь хотя бы один предмет для передачи", true);
    return;
  }
  if (!targetId) return;
  send("host_transfer_items", {
    playerId: editorState.playerId,
    targetId,
    kind: editorState.kind,
    itemIds
  });
});

$("editorRemoveItemsBtn").addEventListener("click", () => {
  if (!editorState) return;
  const itemIds = selectedEditorItemIds();
  if (itemIds.length === 0) {
    setStatus("Отметь хотя бы один предмет для удаления", true);
    return;
  }
  send("host_remove_items", {
    playerId: editorState.playerId,
    kind: editorState.kind,
    itemIds
  });
});

$("editorSaveHealthBtn").addEventListener("click", () => {
  if (!editorState) return;
  if (send("host_set_health_degree", {
    playerId: editorState.playerId,
    degree: $("editorHealthDegree").value.trim()
  })) {
    editorState.dirtyHealth = false;
    closeCardEditor();
  }
});

$("editorSwapBtn").addEventListener("click", () => {
  if (!editorState) return;
  const targetId = $("editorRecipient").value;
  if (!targetId) return;
  if (send("host_swap_cards", {
    playerId: editorState.playerId,
    targetId,
    kind: editorState.kind
  })) {
    closeCardEditor();
  }
});

$("editorSaveTextBtn").addEventListener("click", () => {
  if (!editorState) return;
  const value = $("editorText").value.trim();
  if (!value) {
    setStatus("Текст характеристики не может быть пустым", true);
    return;
  }
  if (send("host_edit_card", {
    playerId: editorState.playerId,
    kind: editorState.kind,
    value
  })) {
    editorState.dirtyText = false;
    closeCardEditor();
  }
});

$("editorRevealBtn").addEventListener("click", () => {
  if (!editorState) return;
  if (send("host_reveal_card", { playerId: editorState.playerId, kind: editorState.kind })) {
    closeCardEditor();
  }
});

$("editorRedrawBtn").addEventListener("click", () => {
  if (!editorState) return;
  if (send("host_redraw_card", { playerId: editorState.playerId, kind: editorState.kind })) {
    closeCardEditor();
  }
});

function leaveRoom() {
  if (room?.stage === "playing" && !window.confirm("Выйти из активной партии?")) {
    return;
  }
  closeCardEditor();
  connectSeq++;
  if (connected && socket && socket.readyState === WebSocket.OPEN) {
    try {
      socket.send(JSON.stringify({ type: "leave" }));
    } catch {
      // Ignore send error while disconnecting.
    }
  }
  if (socket) {
    socket.onclose = null;
    socket.close();
  }
  socket = null;
  connected = false;
  room = null;
  session = null;
  packDraft = "";
  packEditorOpen = false;
  syncDiceRoll(null);
  sessionStorage.removeItem("murr-mobile-session");
  $("room").hidden = true;
  $("setup").hidden = false;
  updateConnection();
  setStatus("Вы вышли из комнаты. Можно создать новую или подключиться по коду.");
}

$("create").addEventListener("click", () => startSession(false));
$("join").addEventListener("click", () => startSession(true));
$("reconnect").addEventListener("click", connect);
$("leave").addEventListener("click", leaveRoom);

$("copyCode").addEventListener("click", async () => {
  if (!room) return;
  const btn = $("copyCode");
  try {
    await navigator.clipboard.writeText(room.code);
    setStatus("Код скопирован: " + room.code);
    btn.textContent = "Скопировано ✓";
    clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = setTimeout(() => { btn.textContent = "Копировать код"; }, 1800);
  } catch {
    setStatus("Код комнаты: " + room.code);
  }
});

$("joinCode").addEventListener("input", event => {
  const input = event.target;
  const cleaned = input.value.toUpperCase().replace(/[^A-Z2-9]/g, "").slice(0, 8);
  if (input.value !== cleaned) input.value = cleaned;
});

$("joinCode").addEventListener("keydown", event => {
  if (event.key === "Enter") {
    event.preventDefault();
    startSession(true);
  }
});

$("nickname").addEventListener("keydown", event => {
  if (event.key === "Enter") {
    event.preventDefault();
    const hasCode = $("joinCode").value.trim().length === 8;
    startSession(hasCode);
  }
});

$("avatarChoice").addEventListener("input", () => {
  renderAvatarPicker();
  saveForm();
});

try {
  const saved = JSON.parse(localStorage.getItem("murr-mobile-form") || "null");
  $("serverUrl").value = saved?.base || location.origin;
  if (saved?.nickname) $("nickname").value = saved.nickname;
  if (saved?.avatar) $("avatarChoice").value = String(saved.avatar);
  renderAvatarPicker();
  const current = JSON.parse(sessionStorage.getItem("murr-mobile-session") || "null");
  if (current?.session?.token && current?.session?.code && current?.base) {
    session = current.session;
    $("serverUrl").value = current.base;
    connect();
  }
} catch {
  $("serverUrl").value = location.origin;
  renderAvatarPicker();
}
