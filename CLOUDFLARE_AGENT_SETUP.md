# Подключение Cloudflare для Murr Bunker Game

Это руководство дополняет [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md). Основано на официальном файле Cloudflare [agent-setup/prompt.md](https://developers.cloudflare.com/agent-setup/prompt.md) и [инструкции Cloudflare для Codex](https://developers.cloudflare.com/agent-setup/codex/).

## Две разные задачи

**A. Публикация нашей игры.** Для запуска комнат требуется Cloudflare Worker; он доступен клиентам через `workers.dev`. GitHub Actions уже содержит отдельный ручной workflow развёртывания.

**Б. Подключение Cloudflare к AI-кодовому агенту.** MCP и Skills помогают агенту искать документацию, исследовать облачный аккаунт и работать с платформой. Это **не** то же самое, что запустить игру.

## A. Запуск комнат прямо с телефона

1. В Cloudflare Dashboard открой свой аккаунт и найди **Account ID** (не Zone ID).
2. Создай отдельный **API Token** с минимальными разрешениями для Workers и Durable Objects на нужном аккаунте. Перепроверь необходимые права, если публикация возвращает ошибку.
3. В GitHub открой [Actions secrets](https://github.com/r3ynex/murr-bunker-game/settings/secrets/actions) и создай два секрета:
   - `CLOUDFLARE_ACCOUNT_ID` — Account ID;
   - `CLOUDFLARE_API_TOKEN` — токен из Cloudflare.
4. Запусти [Deploy Cloudflare rooms and mobile tester](https://github.com/r3ynex/murr-bunker-game/actions/workflows/cloudflare-deploy.yml) вручную кнопкой **Run workflow**.
5. При успешном развёртывании найди в логе реальный адрес вида `https://murr-bunker-api.<subdomain>.workers.dev`.
6. Открой адрес на телефоне. Доступность API проверь по `/api/health`, затем создай комнату. Каждый отдельный игровой код автоматически соответствует Durable Object; вручную комнаты создавать не нужно.

**Внимание:** установка AI MCP/Skills сама по себе не создаёт секреты GitHub и не публикует игру. Токен никогда не отправляй в чат и не коммить в репозиторий. Cloudflare может потребовать отдельную настройку условий и лимитов сервиса в своём интерфейсе.

## Б. AI-агенты и Cloudflare MCP

В официальном `prompt.md` указаны команды настройки разных локальных кодовых агентов. Они **не устанавливают Cloudflare внутрь мобильного ChatGPT**. Для таких агентов выполнить соответствующий шаг нужно в той среде, где запущен агент.

### Codex CLI (на своём ПК / в рабочей среде Codex)

```bash
npx -y skills add cloudflare/skills --skill '*' --yes --global
codex mcp add cloudflare --url https://mcp.cloudflare.com/mcp
codex mcp login cloudflare
```

Команда `codex mcp login cloudflare` должна открыть OAuth-подтверждение. Авторизуй только необходимые права.

**Альтернатива для Codex app:** в разделе Plugins найди и установи Cloudflare, затем авторизуй соединение, как описано в [официальном руководстве Codex + Cloudflare](https://developers.cloudflare.com/agent-setup/codex/).

### GitHub Copilot в VS Code (необязательно)

Официальный файл предлагает сначала установить Cloudflare Skills (команда выше), затем добавить MCP-сервер в `.vscode/mcp.json` (настройка нужна лишь если ты действительно используешь VS Code Copilot):

```json
{
  "servers": {
    "cloudflare": {
      "type": "http",
      "url": "https://mcp.cloudflare.com/mcp"
    }
  }
}
```

Обрати внимание: у разных MCP-клиентов схемы конфигурации отличаются. Для VS Code следуй [документации MCP VS Code](https://code.visualstudio.com/docs/copilot/customization/mcp-servers).

## Для работы через ChatGPT

Подключение Cloudflare MCP к локальному Codex, Cursor или Copilot **не даёт** этому разговору автоматический доступ к аккаунту Cloudflare. Пока прямое Cloudflare-подключение недоступно в текущем чате, можно продолжать менять GitHub-репозиторий через подключённый GitHub, а публикацию выполнять через безопасно сохранённые GitHub Actions Secrets и workflow выше.

Если позднее появится подходящий Cloudflare-плагин/коннектор для этого ChatGPT, его нужно будет авторизовать отдельно. Не подставляй API Token в сообщения ChatGPT.

## Что уже настроено в репозитории

- `server/wrangler.toml` — имя Worker, Static Assets и Durable Object `ROOMS`;
- `.github/workflows/cloudflare-deploy.yml` — ручное развёртывание из GitHub;
- `web/` — мобильный браузерный клиент;
- `AGENTS.md` — инструкции AI-агентам для этого проекта.

**Статус:** код и инструкции подготовлены; подключение личного Cloudflare-аккаунта и публикация не подтверждены.

## Официальные источники

- [Cloudflare agent setup prompt](https://developers.cloudflare.com/agent-setup/prompt.md)
- [Cloudflare agent setup](https://developers.cloudflare.com/agent-setup/)
- [Cloudflare MCP](https://mcp.cloudflare.com/mcp)
- [Cloudflare Durable Objects](https://developers.cloudflare.com/durable-objects/)
- [Wrangler](https://developers.cloudflare.com/workers/wrangler/)
