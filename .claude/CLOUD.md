# Cloud: player logs, match data, accounts later

Owner doc for everything the mod and the launcher send to our own server. Decided with Leonardo on 2026-11-11 (DECISIONS). Cards:
TASKS "Cloud (CLOUD-n)".

## 1. What and why

| Piece | Why | When |
|---|---|---|
| **Send logs** button in the launcher | Every friend session costs a round of "send me your log" (FS 2026-11-07). One click uploads the player's logs and returns a short code (`K7F2QX`) the player pastes on Discord | Before the open release (CLOUD-1, CLOUD-2) |
| **Match summaries** sent by the mod | Balance questions with numbers: BR length, Arena rounds decided by the time limit, crash rate per version | After the open release, once people play (CLOUD-3) |
| **Accounts** (optional) | A profile with past matches, Discord roles, bans | Later (CLOUD-4); never needed to play |

Not planned: rankings. The modded game runs without EasyAntiCheat and the result comes from the clients, so a ranking could be faked.

## 2. Stack (Leonardo, 2026-11-11)

- **Server:** C# on Leonardo's AWS Lightsail: ASP.NET Core Minimal API, EF Core + Npgsql, EF migrations in the repo, a
  `BackgroundService` for retention. HTTPS in front (Caddy or nginx). Code in `cloud/` of this repository (one repo, one PR flow).
- **Postgres** (Leonardo's): the facts we query. Where it runs (on the Lightsail box or a managed database): `UNKNOWN`, Leonardo to say.
- **Log files:** gzip files on the Lightsail disk (`/var/lib/nrpvp/logs/<yyyy>/<mm>/<code>.gz`); a Lightsail bucket (S3 API) if the
  disk fills. Never inside Postgres (MB-sized blobs bloat backups and vacuum; nobody queries inside a log).
- **Clients** (launcher, mod) stay Rust and only speak HTTPS + JSON / multipart. The API URL is a constant in the launcher with an
  override `NRPVP_API_URL` for tests.

## 3. Rules that keep it easy to maintain

1. **The text log is never parsed into the database.** It is stored as uploaded and read by people. Anything that must become a
   number is sent as its own JSON with a `schema` version.
2. **Typed columns only for what we filter on**, the rest in a `jsonb` column: a new field needs no migration until we filter by it.
3. **No secret in the clients** (anyone can read it out of the exe). The server protects itself: size limits (log upload ≤ 8 MB
   gzip), a rate limit per IP, schema validation, unknown fields ignored.
4. **No personal data by default.** The launcher makes a random `install_id` (UUID) once and keeps it in
   `Game\NightreignPvP\install.id`; every upload carries it, never a Steam ID or a name. Logs do contain Steam IDs and character names
   today: the launcher replaces every SteamID64 (`7656119` + 10 digits) with a short salted hash before the upload (the same id gets
   the same hash inside one upload, so the log still reads). Names stay: the player sends the log by clicking.
5. **Retention:** log files and their rows are deleted after 30 days; match rows are kept.
6. **Versioned routes** (`/v1/...`); a breaking change is `/v2`, the old route stays until no supported launcher uses it.

## 4. API v1

| Route | Body | Answer |
|---|---|---|
| `POST /v1/logs` | multipart: `meta` (JSON: `schema`, `install_id`, `mod_version`, `commit`, `game_version`, `sc_version`, `launcher_version`, `note` ≤ 500 chars) + `file` (`.zip`) | `201 {"code": "K7F2QX"}` |
| `POST /v1/matches` | JSON: `schema`, `install_id`, `mod_version`, `match_key` (hash of the queue lobby id, the same in every game of the match), `mode` (`br` / `arena`), `team_size`, `players`, `duration_s`, `ended_by`, `reporter` (`host` / `client`), `players[]` (`pid` = hash, `team`, `kills`, `deaths`, `rounds_won`, `place`, `left`), `extra` (object) | `202` |
| `GET /v1/health` | | `200` |

Each game sends its own view of a match; the server keeps all of them under `match_key` (the views can be compared, nothing merges).

## 5. Tables

- `log_upload`: `id` uuid, `code` text unique, `received_at`, `install_id` uuid, `mod_version`, `commit`, `game_version`, `sc_version`,
  `launcher_version`, `note`, `size_bytes`, `sha256`, `path`.
- `match_report`: `id` uuid, `match_key`, `received_at`, `schema`, `install_id`, `mod_version`, `mode`, `team_size`, `players`,
  `duration_s`, `ended_by`, `reporter`, `extra` jsonb.
- `match_player`: `report_id`, `pid`, `team`, `kills`, `deaths`, `rounds_won`, `place`, `left`.
- Later (CLOUD-4): `account` (`steam_id`, `discord_id`, created / deleted), `account_install` (`account_id`, `install_id`).

## 6. Accounts later (CLOUD-4, not before people play)

No passwords of our own. **Sign in with Steam** (OpenID 2.0) on the website and in the launcher gives a verified SteamID; **Discord**
(OAuth2) is a second link, for roles on the Discord server. Signing in links the `install_id`, so matches played before show up on
the profile. The game itself cannot prove a Steam ID to our server: validating a Steam session ticket needs the publisher key of the
game's app, which is Bandai's `[HYPOTHESIS]`. Needs a privacy page and a "delete my account" button. Playing never needs an account.
