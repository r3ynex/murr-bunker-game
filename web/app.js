/* Mobile browser tester for the SAME Cloudflare game API as the Rust client.
   Temporary UI only. No game secrets or privileged actions are trusted here. */
"use strict";

const $ = id => document.getElementById(id);
let session = null;
let room = null;
let socket = null;
let connected = false;
let pending = false;

function setStatus(message, error = false) {
  $("status").textContent = message;
  $("status").classList.toggle("notice", error);
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
  button.addEventListener("click", action);
  parent.append(button);
  return button;
}
function addText(parent, text, tag = "p", className) {
  const node = element(tag, text, className);
  parent.append(node);
  return node;
}
function getBase() {
  const value = $("serverUrl").value.trim().replace(/\/+$/, "");
  const url = new URL(value);
  if (!["http:", "https:"].includes(url.protocol)) throw new Error("Адрес сервера должен начинаться с http:// или https://");
  if (url.pathname !== "/" || url.search || url.hash) throw new Error("Нужен адрес сервера без дополнительных путей");
  return url.origin;
}
function saveForm() {
  localStorage.setItem("murr-mobile-form", JSON.stringify({
    nickname: $("nickname").value, base: $("serverUrl").value
  }));
}
function saveSession() {
  if (session) sessionStorage.setItem("murr-mobile-session", JSON.stringify({ session, base: getBase() }));
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
    return;
  }
  const code = $("joinCode").value.trim().toUpperCase();
  if (join && !/^[A-Z2-9]{8}$/.test(code)) {
    setStatus("Введите восьмисимвольный код комнаты", true);
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
    setStatus("Не удалось создать/открыть комнату: " + error.message, true);
  } finally {
    pending = false;
    $("create").disabled = $("join").disabled = false;
  }
}
function connect() {
  if (!session) return;
  if (socket) {
    socket.onclose = null;
    socket.close();
  }
  try {
    const base = new URL(getBase());
    base.protocol = base.protocol === "https:" ? "wss:" : "ws:";
    base.pathname = "/api/rooms/" + encodeURIComponent(session.code) + "/ws";
    base.search = "?token=" + encodeURIComponent(session.token);
    // Do not log base.href: it includes the room's private token.
    socket = new WebSocket(base.href);
    setStatus("Устанавливаем WebSocket-соединение…");
    socket.onopen = () => {
      connected = true;
      updateConnection();
      setStatus("Соединение установлено");
      render();
    };
    socket.onmessage = event => {
      try {
        const message = JSON.parse(event.data);
        if (message.type === "state") {
          room = message.data;
          render();
        } else if (message.type === "error") {
          setStatus(message.message || "Ошибка комнаты", true);
        }
      } catch {
        setStatus("Получен некорректный ответ сервера", true);
      }
    };
    socket.onclose = () => {
      connected = false;
      updateConnection();
      setStatus("Соединение закрыто. Можно переподключиться.", true);
      render();
    };
    socket.onerror = () => setStatus("Ошибка WebSocket. Проверь адрес Cloudflare Worker.", true);
    $("setup").hidden = true;
    $("room").hidden = false;
  } catch (error) {
    setStatus("Ошибка подключения: " + error.message, true);
  }
}
function send(type, other = {}) {
  if (!connected || !socket || socket.readyState !== WebSocket.OPEN) {
    setStatus("Нет подключения. Нажми «Переподключиться».", true);
    return;
  }
  socket.send(JSON.stringify({ type, ...other }));
}
function updateConnection() {
  $("connection").textContent = connected ? "● Онлайн" : "○ Нет подключения";
  $("connection").classList.toggle("online", connected);
}
function render() {
  if (!room) return;
  const isHost = room.you === room.hostId;
  updateBackground(room).catch(() => {});
  $("setup").hidden = true;
  $("room").hidden = false;
  $("roomCode").textContent = room.code;
  $("roomMeta").textContent = `Раунд: ${room.round} · Мест: ${room.seats} · Пак: ${room.packName} · Подключений: ${room.connected}`;
  const mount = $("roomUI");
  const previouslyOpen = new Set(Array.from(mount.querySelectorAll("details[data-player]"))
    .filter(item => item.open).map(item => item.dataset.player));
  const hadPlayers = mount.querySelector("details[data-player]") !== null;
  mount.replaceChildren();

  if (room.stage === "lobby") {
    const panel = element("section", null, "panel");
    addText(panel, "Лобби", "h2");
    addText(panel, "Для тестовой партии достаточно двух человек.", "p", "muted");
    const list = element("div", null, "list");
    for (const player of room.players) {
      addText(list, `${player.id === room.hostId ? "♛" : "•"} ${player.name} · Аватар ${player.avatar || 1} · ${player.id === room.hostId || player.ready ? "Готов" : "Не готов"}`, "div");
    }
    panel.append(list);
    if (isHost) {
      const line = element("div", null, "actions");
      const count = element("input");
      count.type = "number";
      count.min = "1";
      count.max = "15";
      count.value = String(room.seats);
      count.setAttribute("aria-label", "Число мест в бункере");
      count.style.maxWidth = "96px";
      count.addEventListener("change", () => {
        if (count.value !== "") send("set_seats", { seats: Number(count.value) });
      });
      line.append(addText(element("span"), "Мест:"), count);
      addButton(line, "Начать игру", () => send("start"), {
        disabled: room.players.length < 2 || room.players.some(p => p.id !== room.hostId && !p.ready)
      });
      panel.append(line);
      mount.append(panel);
      const editor = element("section", null, "panel");
      const details = element("details");
      const summary = element("summary", "Свой пак (JSON / импорт Бункер Онлайн)");
      details.append(summary);
      addText(details, "JSON содержит секции roles, characteristics, items, world. ZIP — pack.json + изображения assets/. До старта ZIP можно отправить всем участникам без ручной установки.", "p", "muted tiny");
      const file = element("input");
      file.type = "file";
      file.accept = "application/json,.json,application/zip,.zip";
      const area = element("textarea");
      area.placeholder = '{"formatVersion":2,"name":"Мой пак","categories":{"profession":{"cards":[{"title":"Инженер","stage":"Опытный"}]}}}';
      area.setAttribute("aria-label", "JSON пака");
      file.addEventListener("change", async () => {
        try {
          if (file.files?.[0]) {
            const selected = file.files[0];
            if (selected.name.toLowerCase().endsWith(".zip")) {
              if (selected.size > 2000000) throw Error("ZIP не должен превышать 2 МБ");
              const response = await fetch(getBase() + `/api/rooms/${room.code}/bundle`, {
                method: "POST", headers: {"authorization": `Bearer ${session.token}`,
                  "content-type":"application/zip"}, body: selected
              });
              const answer = await response.json();
              if (!response.ok) throw Error(answer.error || "Не удалось отправить ZIP");
              setStatus("ZIP-пак загружен для всей комнаты: " + answer.packName);
              area.value = "";
            } else {
              area.value = await selected.text();
            }
          }
        } catch (err) { setStatus(err.message, true); }
      });
      details.append(file, area);
      addButton(details, "Загрузить пример v2", async () => {
        try {
          const response = await fetch("/packs/starter.json");
          if (!response.ok) throw Error("Пример пока не опубликован");
          area.value = await response.text();
          setStatus("Загружен пример структурированного пака v2");
        } catch (err) { setStatus(err.message, true); }
      });
      addButton(details, "Применить пак к комнате", () => {
        try { send("set_pack", { pack: JSON.parse(area.value) }); }
        catch (err) { setStatus("JSON: " + err.message, true); }
      });
      editor.append(details);
      mount.append(editor);
    } else {
      const me = room.players.find(p => p.id === room.you);
      addButton(panel, me?.ready ? "Не готов" : "Готов", () => send("set_ready", {ready: !me?.ready}));
      addText(panel, "Ведущий начнёт партию, когда готовы все.", "p", "muted");
      mount.append(panel);
    }
    return;
  }
  if (room.stage === "finished") {
    const panel = element("section", null, "panel");
    addText(panel, "Партия завершена", "h2");
    addText(panel, "Оставшиеся участники:");
    for (const player of room.players.filter(p => p.active)) addText(panel, "• " + player.name);
    mount.append(panel);
    return;
  }
  const scenario = element("section", null, "panel");
  addText(scenario, "Сценарий", "h2");
  addText(scenario, "Катаклизм: " + room.catastrophe, "p", "multiline");
  addText(scenario, "Бункер: " + room.bunker, "p", "multiline");
  addText(scenario, `События: WIP · шанс ${room.eventChance ?? 0}%`, "p", "muted");
  const actions = element("div", null, "actions");
  if (isHost) {
    addButton(actions, "Следующий раунд", () => send("next_round"));
    addButton(actions, room.voteOpen ? "Закрыть голосование" : "Открыть голосование",
      () => send(room.voteOpen ? "finish_vote" : "start_vote"));
    addButton(actions, "Завершить партию", () => send("finish"));
  }
  const dice = element("select");
  dice.setAttribute("aria-label", "Грани кубика");
  dice.style.maxWidth = "100px";
  for (const sides of [4, 6, 8, 10, 12, 20, 100]) {
    const opt = element("option", "d" + sides);
    opt.value = String(sides);
    if (sides === 6) opt.selected = true;
    dice.append(opt);
  }
  actions.append(dice);
  addButton(actions, "Бросить кубик", () => send("roll", { sides: Number(dice.value) }));
  scenario.append(actions);
  if (room.voteOpen) addText(scenario, "Голосование идёт! Принято голосов: " + room.ballotsCount, "p");
  for (const result of room.voteResults) addText(scenario, result.name + ": " + result.votes + " голос(ов)", "p", "note");
  mount.append(scenario);

  const participants = element("section", null, "panel");
  addText(participants, "Участники", "h2");
  for (const player of room.players) {
    const details = element("details", null, "player");
    details.dataset.player = player.id;
    details.open = previouslyOpen.has(player.id) || (!hadPlayers && player.id === room.you);
    const summary = element("summary", (player.active ? "● " : "○ ") + player.name +
      (player.id === room.hostId ? " ♛" : "") + (player.id === room.you ? " (ты)" : ""));
    details.append(summary);
    if (isHost) {
      addButton(details, player.active ? "Исключить" : "Вернуть",
        () => send("set_active", { playerId: player.id, active: !player.active }));
    }
    if (room.voteOpen && player.active && player.id !== room.you) {
      addButton(details, "Голосовать за исключение",
        () => send("vote", { target: player.id }));
    }
    const cards = element("div", null, "cards");
    if (player.cards.length === 0) addText(cards, "Пока нет открытых карт", "p", "muted");
    for (const card of player.cards) {
      const row = element("div", null, "card");
      const info = element("span");
      addText(info, room.categoryLabels?.[card.kind] || labels[card.kind] || card.kind, "small");
      addText(info, card.value, "span", "multiline");
      row.append(info);
      if (player.id === room.you && !card.revealed && player.active) {
        addButton(row, "Открыть", () => send("reveal", { kind: card.kind }));
      }
      cards.append(row);
    }
    details.append(cards);
    participants.append(details);
  }
  mount.append(participants);
  const log = element("section", null, "panel");
  addText(log, "Журнал партии", "h2");
  const list = element("ol", null, "log");
  for (const message of room.log) addText(list, message, "li");
  log.append(list);
  mount.append(log);
}
let loadedBackground = "";
let currentBackgroundUrl = null;
async function updateBackground(state) {
  const sha = state.bundle?.sha256;
  if (!sha || loadedBackground === sha || !session) return;
  loadedBackground = sha;
  try {
    const response = await fetch(getBase() + '/api/rooms/' + state.code + '/background', {
      headers: { authorization: 'Bearer ' + session.token }
    });
    if (!response.ok) return;
    if (currentBackgroundUrl) URL.revokeObjectURL(currentBackgroundUrl);
    currentBackgroundUrl = URL.createObjectURL(await response.blob());
    document.documentElement.style.setProperty('--room-background', 'url("' + currentBackgroundUrl + '")');
  } catch {
    // A missing optional background should not prevent the game from working.
  }
}
const labels = {
  gender: "Пол и возраст", body: "Телосложение", personality: "Характер",
  profession: "Профессия", health: "Здоровье", hobby: "Хобби",
  fear: "Фобия", equipment: "Инвентарь", backpack: "Рюкзак",
  fact: "Дополнительное сведение", ability: "Особая возможность"
};

$("create").addEventListener("click", () => startSession(false));
$("join").addEventListener("click", () => startSession(true));
$("reconnect").addEventListener("click", connect);
$("leave").addEventListener("click", () => {
  if (socket) { socket.onclose = null; socket.close(); }
  socket = null;
  connected = false;
  room = null;
  session = null;
  sessionStorage.removeItem("murr-mobile-session");
  $("room").hidden = true;
  $("setup").hidden = false;
  updateConnection();
  setStatus("Можно открыть другую комнату. Прежняя комната при этом не удаляется.");
});
$("copyCode").addEventListener("click", async () => {
  if (!room) return;
  try {
    await navigator.clipboard.writeText(room.code);
    setStatus("Код скопирован: " + room.code);
  } catch { setStatus("Код комнаты: " + room.code); }
});
try {
  const saved = JSON.parse(localStorage.getItem("murr-mobile-form") || "null");
  $("serverUrl").value = saved?.base || location.origin;
  if (saved?.nickname) $("nickname").value = saved.nickname;
  const current = JSON.parse(sessionStorage.getItem("murr-mobile-session") || "null");
  if (current?.session?.token && current?.session?.code && current?.base) {
    session = current.session;
    $("serverUrl").value = current.base;
    connect();
  }
} catch {
  $("serverUrl").value = location.origin;
}
