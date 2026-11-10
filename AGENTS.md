# AI agent instructions — Murr Bunker Game

This repository contains a multiplayer social deduction / bunker game prototype. Treat it as **an existing project**, not an empty Cloudflare starter. Preserve the Rust native client and the current game protocol unless a requested change needs updates in both.

## Where things live

- `client/`: portable Windows Rust / egui application.
- `server/`: Cloudflare Worker with the `Room` Durable Object, JSON packs, authoritative game logic and WebSocket protocol.
- `web/`: static mobile browser tester served by the same Worker via Static Assets.
- `.github/workflows/`: Windows build, JS checks and **manual** Cloudflare deployment.
- `DEVELOPER_GUIDE.md`: file-by-file map and game mechanics documentation.
- `CLOUDFLARE_AGENT_SETUP.md`: optional Cloudflare coding-agent setup and GitHub-only deployment.

## Cloudflare conventions

1. A Wrangler config **already exists** at `server/wrangler.toml`. Use `cd server && npm run dev` for development and `cd server && npm run deploy` for manual deployments. Do not scaffold over it.
2. `ROOMS` binds to a Durable Object class `Room`, using an SQLite migration. Preserve binding and migration history when updating code.
3. Worker handles `/api/*` and serves `../web` as Static Assets. Keep the web client and Rust client compatible with the HTTP + WebSocket protocol.
4. Each room's Durable Object is authoritative. Do **not** expose unrevealed cards to other players or let client-side UI changes bypass host authorization. Server checks must remain in `Room.webSocketMessage`.
5. Never log or commit Cloudflare API tokens, account secrets, room session tokens or `.env` contents.
6. Never deploy to a live account as part of a routine check unless the user explicitly requests deployment. GitHub deploy workflow is deliberately manual.
7. Do not install global CLIs or silently alter agent-global configuration. For instructions specific to the user's own coding agent, consult the official [Cloudflare agent setup guide](https://developers.cloudflare.com/agent-setup/prompt.md).
8. The goal is a free/low-cost prototype within Cloudflare's current limits; never assume unlimited free operations.

## Useful commands

```bash
cd server
npm install
npm run check  # JavaScript syntax and tests
npm run dev    # Wrangler local Worker + Durable Objects
npm run deploy # Only with authorized Cloudflare credentials
```

For Rust: `cd client && cargo check` and `cargo build --release`. For the browser tester: `node --check web/app.js`.

## Product priorities

- First priority: functional, testable rooms, privacy of hidden cards, reconnect flow and stable online interactions.
- The user owns the final visual design; the current egui and browser UIs are placeholders.
- Packs are user-authored **single JSON** (see `PACK_FORMAT.md`). Format v2 has sections `meta`, `roles`, `characteristics`, `items`, `world`, `customCategories` and is normalized in `server/src/content.js`. Keep legacy JSON and Bunker Online exports importable; no JS evaluation. Do not copy third-party paid content or branding assets.
- Voice communication happens outside the game, through Discord.
