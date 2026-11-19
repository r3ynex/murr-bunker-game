import { DurableObject } from "cloudflare:workers";
import { DEFAULT_PACK, normalizePack, drawCard } from "./content.js";
import { readBundle, sha256Hex, MAX_ZIP_BYTES } from "./bundle.js";
import { avatarFrom, publicAvatar } from "./avatars.js";
import { addInventoryItem, inventoryItems, isInventory, removeInventoryItems, replaceInventory, transferInventory } from "./inventory.js";
import { healthDegree, setHealthDegree, swapCards } from "./cards.js";
import { applyHostAction, buildUndo, bunkerSections, hostSnapshot, maybeTriggerRoundEvent, resetVote, startGame, undoHostAction } from "./host.js";
import { publicDiceRoll, rollDice } from "./dice.js";
import { unzipSync } from "fflate";

export const ROOM_TTL_MS = 10 * 60 * 60 * 1000; // 10 hours (within 8–12h cleanup window)
const WS_TICKET_TTL_MS = 30 * 1000;

const json = (data, status = 200) => new Response(JSON.stringify(data), {
  status, headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store" }
});
const codeAlphabet = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const randomCode = () => Array.from(crypto.getRandomValues(new Uint8Array(8)), b => codeAlphabet[b % codeAlphabet.length]).join("");
const randomToken = () => crypto.randomUUID() + crypto.randomUUID();
const nameFrom = body => {
  const name = body?.name;
  if (typeof name !== "string" || name.trim().length < 2 || name.trim().length > 24) throw Error("Ник: от 2 до 24 символов");
  return name.trim();
};
const bearerFrom = request => {
  const header = request.headers.get("authorization") || "";
  return header.startsWith("Bearer ") ? header.slice(7) : "";
};

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/api/health") return json({ ok: true, version: 1 });
    try {
      if (url.pathname === "/api/rooms" && request.method === "POST") {
        const body = await request.json();
        const name = nameFrom(body);
        const avatar = avatarFrom(body.avatar);
        for (let i = 0; i < 3; i++) {
          const code = randomCode();
          const room = env.ROOMS.getByName(code);
          const result = await room.fetch("https://room.internal/init", {
            method: "POST", body: JSON.stringify({ code, name, avatar })
          });
          if (result.status === 409) continue;
          return result;
        }
        return json({ error: "Не удалось создать комнату" }, 503);
      }
      const match = url.pathname.match(/^\/api\/rooms\/([A-Z2-9]{8})\/(join|ws|ws-ticket|bundle|background)$/);
      if (!match) return json({ error: "Not found" }, 404);
      const [, code, action] = match;
      const room = env.ROOMS.getByName(code);
      if (action === "join" && request.method === "POST") {
        const body = await request.json();
        const name = nameFrom(body);
        return room.fetch("https://room.internal/join", {
          method: "POST", body: JSON.stringify({ name, avatar: avatarFrom(body.avatar) })
        });
      }
      if (action === "ws-ticket" && request.method === "POST") {
        return room.fetch(new Request("https://room.internal/ws-ticket", request));
      }
      if (action === "background" && request.method === "GET") {
        return room.fetch(new Request("https://room.internal/background", request));
      }
      if (action === "bundle" && ["GET", "POST"].includes(request.method)) {
        return room.fetch(new Request("https://room.internal/bundle", request));
      }
      if (action === "ws" && request.method === "GET") {
        if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") {
          return json({ error: "WebSocket required" }, 426);
        }
        const params = new URLSearchParams();
        if (url.searchParams.has("ticket")) params.set("ticket", url.searchParams.get("ticket") || "");
        if (url.searchParams.has("token")) params.set("token", url.searchParams.get("token") || "");
        return room.fetch(new Request("https://room.internal/connect?" + params.toString(), request));
      }
      return json({ error: "Method not allowed" }, 405);
    } catch (error) {
      return json({ error: String(error.message || error) }, 400);
    }
  }
};

export class Room extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.wsTickets = new Map();
  }

  pruneTickets(now = Date.now()) {
    for (const [key, entry] of this.wsTickets.entries()) {
      if (entry.expiresAt <= now) this.wsTickets.delete(key);
    }
  }

  issueTicket(playerId, now = Date.now()) {
    this.pruneTickets(now);
    const ticket = crypto.randomUUID();
    this.wsTickets.set(ticket, { playerId, expiresAt: now + WS_TICKET_TTL_MS });
    return ticket;
  }

  consumeTicket(ticket, now = Date.now()) {
    this.pruneTickets(now);
    if (!ticket) return null;
    const entry = this.wsTickets.get(ticket);
    if (!entry) return null;
    this.wsTickets.delete(ticket);
    return entry.expiresAt > now ? entry.playerId : null;
  }

  async touchActivity(state, now = Date.now()) {
    state.lastActivityAt = now;
    if (typeof this.ctx?.storage?.setAlarm === "function") {
      await this.ctx.storage.setAlarm(now + ROOM_TTL_MS);
    }
  }

  async alarm() {
    const now = Date.now();
    const state = await this.ctx.storage.get("room");
    if (!state) {
      await this.ctx.storage.deleteAll();
      return;
    }
    const sockets = this.ctx.getWebSockets();
    const inactiveMs = now - (state.lastActivityAt || 0);
    if (sockets.length > 0 && state.stage !== "closed") {
      state.lastActivityAt = now;
      await this.ctx.storage.put("room", state);
      await this.ctx.storage.setAlarm(now + ROOM_TTL_MS);
      return;
    }
    if (inactiveMs < ROOM_TTL_MS && state.stage !== "closed") {
      await this.ctx.storage.setAlarm((state.lastActivityAt || now) + ROOM_TTL_MS);
      return;
    }
    for (const ws of sockets) {
      try { ws.close(1001, "Room expired"); } catch { /* ignore */ }
    }
    this.wsTickets.clear();
    await this.ctx.storage.deleteAll();
  }

  async fetch(request) {
    const url = new URL(request.url);
    const state = await this.ctx.storage.get("room");
    if (url.pathname === "/init" && request.method === "POST") {
      if (state) return json({ error: "Room exists" }, 409);
      const { code, name, avatar } = await request.json();
      const player = this.newPlayer(name, avatar);
      const room = {
        code, hostId: player.id, stage: "lobby", round: 0, seats: 1,
        catastrophe: "", bunker: "", pack: DEFAULT_PACK, players: [player],
        voteOpen: false, voteCandidates: null, ballots: {}, voteResults: [],
        bundle: null, eventChance: 0, currentEvent: null, eventHistory: [],
        lastActivityAt: Date.now(), log: ["Комната создана"]
      };
      await this.touchActivity(room);
      await this.ctx.storage.put("room", room);
      return json({ code, playerId: player.id, token: player.token });
    }
    if (!state) return json({ error: "Комната не существует" }, 404);
    if (url.pathname === "/join" && request.method === "POST") {
      if (state.stage !== "lobby") return json({ error: "Партия уже запущена" }, 409);
      if (state.players.length >= 15) return json({ error: "Комната заполнена" }, 409);
      const { name, avatar } = await request.json();
      const player = this.newPlayer(name, avatar);
      const needsHost = !state.players.some(p => p.id === state.hostId);
      if (needsHost) {
        state.hostId = player.id;
        player.ready = true;
      } else {
        player.ready = false;
      }
      state.players.push(player);
      if (!state.seatsCustom) {
        state.seats = Math.max(1, Math.floor(state.players.length / 2));
      }
      this.note(state, name + " присоединился");
      await this.touchActivity(state);
      await this.ctx.storage.put("room", state);
      await this.broadcast(state);
      return json({ code: state.code, playerId: player.id, token: player.token });
    }
    if (url.pathname === "/ws-ticket" && request.method === "POST") {
      const token = bearerFrom(request);
      const player = state.players.find(p => p.token === token);
      if (!player) return json({ error: "Неверный токен" }, 401);
      return json({ ticket: this.issueTicket(player.id) });
    }
    if (url.pathname === "/background" && request.method === "GET") {
      const token = bearerFrom(request);
      if (!state.players.some(p => p.token === token)) return json({ error: "Неверный токен" }, 401);
      if (!state.bundle?.background) return json({ error: "Фон не выбран" }, 404);
      const zip = await this.ctx.storage.get("bundle-data");
      if (!(zip instanceof Uint8Array)) return json({ error: "ZIP не найден" }, 404);
      const files = unzipSync(zip);
      const path = state.bundle.background;
      const data = files[path];
      if (!data) return json({ error: "Фон не найден" }, 404);
      const mime = path.toLowerCase().endsWith(".png") ? "image/png"
        : path.toLowerCase().endsWith(".webp") ? "image/webp" : "image/jpeg";
      return new Response(data, {
        headers: { "content-type": mime, "cache-control": "private, no-store" }
      });
    }
    if (url.pathname === "/bundle") {
      const token = bearerFrom(request);
      const me = state.players.find(p => p.token === token);
      if (!me) return json({ error: "Неверный токен" }, 401);
      if (request.method === "GET") {
        if (!state.bundle) return json({ error: "ZIP-пак не загружен" }, 404);
        const stored = await this.ctx.storage.get("bundle-data");
        if (!(stored instanceof Uint8Array)) return json({ error: "Архив не найден" }, 404);
        return new Response(stored, {
          headers: {
            "content-type": "application/zip",
            "cache-control": "private, no-store",
            "x-bundle-sha256": state.bundle.sha256
          }
        });
      }
      if (request.method === "POST") {
        if (state.hostId !== me.id) return json({ error: "Загрузить ZIP может только ведущий" }, 403);
        if (state.stage !== "lobby") return json({ error: "ZIP можно менять только до старта" }, 409);
        if (Number(request.headers.get("content-length") || 0) > MAX_ZIP_BYTES) {
          return json({ error: "ZIP должен быть меньше 2 МБ" }, 413);
        }
        const input = new Uint8Array(await request.arrayBuffer());
        if (input.length > MAX_ZIP_BYTES) return json({ error: "ZIP должен быть меньше 2 МБ" }, 413);
        const bundle = readBundle(input);
        const hash = await sha256Hex(input);
        // SQLite-backed DO KV supports the bounded Uint8Array directly.
        await this.ctx.storage.put("bundle-data", input);
        state.pack = bundle.pack;
        state.bundle = { sha256: hash, bytes: input.length, background: bundle.background, defaultBackground: bundle.background };
        for (const p of state.players) p.ready = p.id === state.hostId;
        this.note(state, "Выбран ZIP-пак: " + state.pack.name);
        await this.touchActivity(state);
        await this.ctx.storage.put("room", state);
        await this.broadcast(state);
        return json({ ok: true, bundle: state.bundle, packName: state.pack.name });
      }
    }
    if (url.pathname === "/connect") {
      const ticket = url.searchParams.get("ticket");
      let player = null;
      if (ticket) {
        const playerId = this.consumeTicket(ticket);
        if (playerId) player = state.players.find(p => p.id === playerId) || null;
      } else {
        const token = bearerFrom(request) || url.searchParams.get("token");
        player = state.players.find(p => p.token === token) || null;
      }
      if (!player) return json({ error: "Неверный токен" }, 401);
      await this.touchActivity(state);
      await this.ctx.storage.put("room", state);
      const pair = new WebSocketPair();
      const [client, server] = Object.values(pair);
      this.ctx.acceptWebSocket(server);
      server.serializeAttachment({ playerId: player.id });
      await this.broadcast(state);
      return new Response(null, { status: 101, webSocket: client });
    }
    return json({ error: "Not found" }, 404);
  }

  newPlayer(name, avatar = 1) {
    return { id: crypto.randomUUID(), token: randomToken(), name, avatar: avatarFrom(avatar), ready: true, active: true, cards: [] };
  }
  note(state, message) {
    state.log.push(message);
    state.log = state.log.slice(-40);
  }
  view(state, viewerId) {
    const hasSecondAbility = state.pack.categories?.ability?.length > 1
      || state.players.some(p => p.cards.some(c => c.kind === "ability2"));
    const results = Array.isArray(state.voteResults) ? state.voteResults : [];
    const voteTied = !state.voteOpen && results.length >= 2 && results[0].votes > 0
      && results[0].votes === results[1].votes;
    return {
      code: state.code, you: viewerId, hostId: state.hostId, stage: state.stage,
      round: state.round, seats: state.seats, catastrophe: state.catastrophe,
      bunker: state.bunker, packName: state.pack.name, bundle: state.bundle || null,
      eventChance: state.eventChance ?? 0,
      currentEvent: state.currentEvent || null,
      eventHistory: state.eventHistory || [],
      canSwapCards: true,
      canSetHealthDegree: true,
      speechEndsAt: state.speechEndsAt ?? 0,
      diceRoll: publicDiceRoll(state),
      hostControls: viewerId === state.hostId ? {
        undo: Boolean(state.hasHostUndo),
        catastrophes: state.pack.catastrophes?.map(entry => entry.title) ?? [],
        bunkerSections: bunkerSections(state),
      } : undefined,
      categoryLabels: {
        ...state.pack.categoryLabels,
        ...(hasSecondAbility ? { ability2: "Спец. возможность 2" } : {})
      },
      voteOpen: state.voteOpen,
      voteCandidates: state.voteCandidates || null,
      voteTied,
      yourVote: state.voteOpen ? (state.ballots[viewerId] ?? null) : null,
      ballotsCount: Object.keys(state.ballots).length, voteResults: results,
      log: state.log, connected: this.ctx.getWebSockets().length,
      players: state.players.map(p => ({
        id: p.id, name: p.name, ...publicAvatar(p.avatar),
        ready: p.ready ?? (p.id === state.hostId), active: p.active,
        cards: p.cards.filter(c => p.id === viewerId || c.revealed)
          .map(c => ({ kind: c.kind, value: c.value, revealed: c.revealed, description: c.description,
            items: isInventory(c.kind) ? inventoryItems(c) : undefined,
            healthDegree: c.kind === "health" ? healthDegree(c, state.pack.categories?.health) : undefined }))
      }))
    };
  }
  async broadcast(state) {
    if (!state) return;
    for (const ws of this.ctx.getWebSockets()) {
      try {
        const id = ws.deserializeAttachment()?.playerId;
        if (id) ws.send(JSON.stringify({ type: "state", data: this.view(state, id) }));
      } catch { /* stale socket */ }
    }
  }
  async webSocketMessage(ws, message) {
    if (typeof message !== "string" || message.length > 550000) {
      ws.send(JSON.stringify({ type: "error", message: "Слишком большое сообщение" }));
      return;
    }
    const state = await this.ctx.storage.get("room");
    const playerId = ws.deserializeAttachment()?.playerId;
    const me = state?.players.find(p => p.id === playerId);
    if (!state || !me) { ws.close(1008, "Unknown player"); return; }
    try {
      const data = JSON.parse(message);
      if (!data || typeof data !== "object") throw Error("Некорректная команда");
      const host = me.id === state.hostId;
      const hostOnly = ["start", "start_vote", "finish_vote", "next_round", "set_active", "set_seats", "set_pack", "finish", "set_event_chance", "host_edit_card", "host_redraw_card", "host_reveal_card", "host_add_item", "host_transfer_items", "host_swap_cards", "host_set_health_degree", "host_remove_items", "host_action", "host_undo"];
      if (hostOnly.includes(data.type) && !host) throw Error("Только ведущий может выполнить действие");
      if (state.stage === "closed" && data.type !== "leave") throw Error("Комната закрыта");
      const recordUndo = ["playing", "finished"].includes(state.stage) && hostOnly.includes(data.type)
        && data.type !== "host_undo" && !["close", "transfer_host", "restart"].includes(data.action);
      const before = recordUndo ? hostSnapshot(state) : null;
      if (data.type === "host_action") {
        this.note(state, applyHostAction(state, data));
      } else if (data.type === "host_undo") {
        if (!state.hasHostUndo) throw Error("Нет действия для отмены");
        undoHostAction(state, await this.ctx.storage.get("host-undo"));
        this.note(state, "Ведущий отменил предыдущее действие");
      } else if (data.type === "set_pack") {
        if (state.stage !== "lobby") throw Error("Менять пак можно только до старта");
        state.pack = normalizePack(data.pack);
        state.bundle = null;
        await this.ctx.storage.delete("bundle-data");
        for (const p of state.players) p.ready = p.id === state.hostId;
        this.note(state, "Выбран пак: " + state.pack.name);
      } else if (data.type === "set_ready") {
        if (state.stage !== "lobby") throw Error("Готовность меняется в лобби");
        me.ready = me.id === state.hostId ? true : Boolean(data.ready);
        this.note(state, me.name + (me.ready ? " готов" : " не готов"));
      } else if (data.type === "set_event_chance") {
        if (state.stage !== "lobby") throw Error("Настройки меняются до старта");
        const chance = Number(data.chance);
        if (!Number.isInteger(chance) || chance < 0 || chance > 100) throw Error("Шанс от 0 до 100");
        state.eventChance = chance;
      } else if (data.type === "set_seats") {
        if (state.stage !== "lobby") throw Error("Число мест меняется до старта");
        const seats = Number(data.seats);
        if (!Number.isInteger(seats) || seats < 1 || seats > 15) throw Error("Недопустимое число мест");
        state.seats = seats;
        state.seatsCustom = true;
      } else if (data.type === "start") {
        if (state.stage !== "lobby") throw Error("Игра уже запущена");
        if (state.players.length < 2) throw Error("Нужно минимум два игрока (тестовый режим)");
        if (state.players.some(p => p.id !== state.hostId && !p.ready)) throw Error("Все игроки должны нажать «Готов»");
        startGame(state);
        this.note(state, "Игра началась");
      } else if (data.type === "reveal") {
        if (state.stage !== "playing" || !me.active) throw Error("Сейчас нельзя открывать карты");
        const card = me.cards.find(c => c.kind === data.kind);
        if (!card) throw Error("Карта не найдена");
        if (card.revealed) throw Error("Карта уже открыта");
        card.revealed = true;
        const label = state.pack.categoryLabels?.[data.kind]
          || (data.kind === "ability2" ? "Спец. возможность 2" : data.kind);
        this.note(state, me.name + " открыл(а): " + label);
      } else if (data.type === "host_swap_cards") {
        if (state.stage !== "playing") throw Error("Обмен доступен во время игры");
        swapCards(state, data);
        this.note(state, "Ведущий обменял характеристики игроков");
      } else if (data.type === "host_transfer_items") {
        if (state.stage !== "playing") throw Error("Передача доступна во время игры");
        transferInventory(state, data);
        this.note(state, "Ведущий перенёс предметы между игроками");
      } else if (["host_edit_card", "host_redraw_card", "host_reveal_card", "host_add_item", "host_remove_items", "host_set_health_degree"].includes(data.type)) {
        if (state.stage !== "playing") throw Error("Игра ещё не началась");
        const player = state.players.find(p => p.id === data.playerId);
        const kind = data.type === "host_set_health_degree" ? "health" : data.kind;
        const card = player?.cards.find(c => c.kind === kind);
        if (!card) throw Error("Карта игрока не найдена");
        if (data.type === "host_set_health_degree") {
          setHealthDegree(card, data.degree, state.pack.categories?.health);
        } else if (data.type === "host_remove_items") {
          removeInventoryItems(card, data.itemIds);
        } else if (data.type === "host_add_item") {
          if (!isInventory(card.kind)) throw Error("Добавлять предметы можно только в инвентарь или рюкзак");
          addInventoryItem(state, card, data.value);
        } else if (data.type === "host_edit_card") {
          if (typeof data.value !== "string" || !data.value.trim() || data.value.length > 2000) {
            throw Error("Текст карты от 1 до 2000 символов");
          }
          if (isInventory(card.kind)) {
            replaceInventory(state, card, data.value.trim(), "");
          } else {
            card.value = data.value.trim();
            card.description = "";
            if (card.kind === "health" || card.kind === "profession") delete card[card.kind];
          }
        } else if (data.type === "host_reveal_card") {
          card.revealed = true;
        } else {
          const category = data.kind === "ability2" ? "ability" : data.kind;
          if (!Object.hasOwn(state.pack.categories, category)) throw Error("Категория не найдена");
          const used = new Set();
          for (const p of state.players) {
            for (const c of p.cards) {
              const cat = c.kind === "ability2" ? "ability" : c.kind;
              if (cat === category && c.value) used.add(c.value.split("\n", 1)[0]);
            }
          }
          const fresh = drawCard(state.pack, category, used);
          if (isInventory(card.kind)) {
            replaceInventory(state, card, fresh.value, fresh.description);
          } else {
            card.value = fresh.value;
            card.description = fresh.description;
            if (card.kind === "health" || card.kind === "profession") card[card.kind] = fresh[card.kind];
          }
          // Preserve visibility; never secretly hide a card that was already revealed.
        }
        this.note(state, "Ведущий изменил карточку игрока " + player.name);
      } else if (data.type === "roll") {
        if (state.stage !== "playing") throw Error("Броски доступны только во время игры");
        const sides = Number(data.sides);
        const value = rollDice(state, me, sides);
        this.note(state, me.name + " бросил(а) d" + sides + ": " + value);
      } else if (data.type === "start_vote") {
        if (state.stage !== "playing") throw Error("Игра ещё не началась");
        let candidates = null;
        if (Array.isArray(data.candidates) && data.candidates.length >= 2) {
          const unique = [...new Set(data.candidates.filter(id => state.players.some(p => p.id === id && p.active)))];
          if (unique.length >= 2) candidates = unique;
        }
        state.voteOpen = true;
        state.voteCandidates = candidates;
        state.ballots = {};
        state.voteResults = [];
        if (candidates) {
          const names = candidates.map(id => state.players.find(p => p.id === id)?.name || "?").join(", ");
          this.note(state, "Открыто переголосование: " + names);
        } else {
          this.note(state, "Голосование открыто");
        }
      } else if (data.type === "vote") {
        if (!state.voteOpen || !me.active) throw Error("Голосование недоступно");
        if (data.target === me.id) throw Error("Нельзя голосовать против себя");
        if (!state.players.some(p => p.id === data.target && p.active)) throw Error("Игрок не найден");
        if (Array.isArray(state.voteCandidates) && !state.voteCandidates.includes(data.target)) {
          throw Error("Этот игрок не участвует в переголосовании");
        }
        state.ballots[me.id] = data.target;
      } else if (data.type === "finish_vote") {
        if (!state.voteOpen) throw Error("Голосование уже закрыто");
        state.voteOpen = false;
        state.voteCandidates = null;
        const tally = {};
        for (const id of Object.values(state.ballots)) tally[id] = (tally[id] || 0) + 1;
        state.voteResults = Object.entries(tally).map(([id, count]) => ({
          playerId: id,
          name: state.players.find(p => p.id === id)?.name || "?",
          votes: count
        })).sort((a, b) => b.votes - a.votes);
        const top = state.voteResults[0];
        const tied = state.voteResults.length >= 2 && top && state.voteResults[1].votes === top.votes;
        if (!top) {
          this.note(state, "Голосование завершено без голосов");
        } else if (tied) {
          const tiedNames = state.voteResults.filter(r => r.votes === top.votes).map(r => r.name).join(", ");
          this.note(state, `Ничья в голосовании (${top.votes} голос.): ${tiedNames}`);
        } else {
          this.note(state, `Голосование завершено (лидер: ${top.name} — ${top.votes}) — исключение вручную или кнопкой ведущего`);
        }
      } else if (data.type === "set_active") {
        const player = state.players.find(p => p.id === data.playerId);
        if (!player || state.stage !== "playing") throw Error("Игрок не найден");
        player.active = Boolean(data.active);
        if (!player.active && state.voteOpen) delete state.ballots[player.id];
        this.note(state, player.name + (player.active ? " возвращён в лагерь" : " исключён из лагеря"));
      } else if (data.type === "next_round") {
        if (state.stage !== "playing") throw Error("Игра ещё не началась");
        if (state.voteOpen) throw Error("Сначала завершите голосование");
        if (state.players.filter(p => p.active).length <= state.seats || state.round >= 7) {
          state.stage = "finished"; this.note(state, "Партия завершена");
        } else {
          state.round += 1;
          state.voteResults = [];
          state.voteCandidates = null;
          const event = maybeTriggerRoundEvent(state);
          this.note(state, event ? `Раунд ${state.round} · Событие: ${event.title}` : "Раунд " + state.round);
        }
      } else if (data.type === "leave") {
        if (state.stage === "lobby") {
          state.players = state.players.filter(p => p.id !== me.id);
          if (state.hostId === me.id && state.players.length) {
            state.hostId = state.players[0].id;
            state.players[0].ready = true;
          }
          if (!state.seatsCustom) {
            state.seats = Math.max(1, Math.floor(state.players.length / 2));
          } else if (state.players.length) {
            state.seats = Math.max(1, Math.min(state.seats, 15));
          }
          this.note(state, me.name + " покинул комнату");
        } else {
          me.active = false;
          if (state.voteOpen) delete state.ballots[me.id];
          this.note(state, me.name + " покинул партию");
        }
      } else if (data.type === "finish") {
        state.stage = "finished";
        resetVote(state);
        this.note(state, "Партия завершена ведущим");
      } else {
        throw Error("Неизвестная команда");
      }
      await this.touchActivity(state);
      let undo = before && buildUndo(before, state);
      const resetUndo = data.type === "host_undo" || (data.type === "host_action"
        && ["close", "transfer_host", "restart"].includes(data.action));
      const saveUndo = Boolean(undo) || resetUndo;
      if (undo && new TextEncoder().encode(JSON.stringify(undo)).length > 1_500_000) undo = null;
      if (saveUndo) {
        state.hasHostUndo = Boolean(undo);
        await this.ctx.storage.put({ room: state, "host-undo": undo || null });
      } else {
        await this.ctx.storage.put("room", state);
      }
      await this.broadcast(state);
    } catch (error) {
      ws.send(JSON.stringify({ type: "error", message: String(error.message || error).slice(0, 200) }));
    }
  }
  async webSocketClose(ws) {
    const state = await this.ctx.storage.get("room");
    if (state) await this.broadcast(state);
  }
}
