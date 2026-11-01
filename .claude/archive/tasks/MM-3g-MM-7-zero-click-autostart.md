# MM-3g + MM-7 — Zero-click lobby and auto-start

**Status: ON HOLD** (Leonardo, 2026-10-21) until S10's results are reviewed. Card summaries live in `.claude/TASKS.md` (MM-3g, MM-7);
this file is the working brief for when they start.

## Goal

After every player presses Commence once, nobody presses anything else, and no Steam window appears on any screen:
match found → the host's game opens its Seamless Co-op lobby by itself (MM-3g) → joiners join automatically (works today) → once
everyone is present and loaded, the expedition starts by itself (MM-7). The host is invisible: no host prompt in the messages.

## Why it needs RE

- **MM-3g:** the game creates the joinable SC lobby only from the Invite Members menu action. Our own `CreateLobby` is adopted by SC
  but not joinable (FS-MM2, `research/S9-matchmaking-decision.md` §5e/§5f). Call chain from the MM-3c trace (S9 §5d): menu functor
  `+0x80D743` → `+0x80E32A` → `+0x213B4F` → session code `+0x258568B` / `+0x257F5DA` (the latter obfuscated, no `.pdata`). Target: the
  highest frame with `.pdata` that means "Invite Members was chosen", and its arguments.
- **MM-7:** the Commence checks `exe+0x9EEBE0` are ordinary code (`re/MEMORY_MAP.md` §13), but they take the expedition menu object.
  FS-UI2 showed the host's start with an SC party logged no press at `exe+0x9EEBFB`, so the party start may use another path. Find
  what the host's start calls with an SC party, and whether it can run without the menu open. Alternative: an EMEVD instruction that
  starts a departure (UNKNOWN whether one exists). Also check whether an SC party has a "ready" state.

## Shared spike (Opus, mostly static + one solo trace)

1. Static: from `+0x80D743` / `+0x80E32A`, which arguments (menu object, item id), which globals; can it be called from our thread
   or must it run on the main loop (the task system at `+0xDEC332` drives it)?
2. Solo trace (Leonardo, ~5 min, `scripts\launch.ps1`): hardware breakpoints on the candidates while he opens Invite Members, and while
   he starts a solo expedition from an SC session (to see the Commence path with SC active).
3. Prototype as dev console commands: `mm_open_lobby` (MM-3g) and `expedition_start` (MM-7); solo pass = lobby created with
   `inviteParam` (SC adopts it) / departure starts, no menu touched.

## Acceptance

- Solo (dev): both console commands work from the Roundtable without touching the menu.
- FS: full match with zero clicks after Commence; nobody sees a Steam window; both load into the same expedition.

## Out of scope

Host selection (MM-8), host leaving (MM-9), Duo/Trio (MM-6).
