# S10 — Study Seamless Co-op: sessions, the 6-player cap, host loss

**Status: research done in its own session (2026-10-21)**, branch `s10-sc-study` (worktree `nightreign-pvp-s10`, commit bf9fc3e, not
merged, not pushed). Result: `.claude/research/S10-seamless-coop-internals.md` on that branch. Headline: real host migration is a
**no-go**; ending the match cleanly when the host leaves, then requeuing everyone, is a **go**. Two solo runs remain open there:
R1 (hook census) and R2 (host Alt-F4 on Nucleus). The card summary is in `.claude/TASKS.md` (S10).

## Brief (as given to the session)

Hard rule 3: `nrsc.dll` is studied read-only, for interop only; never patched, copied or redistributed; no decompiled SC code in our
docs or code.

Questions, each with evidence tags:
1. How SC creates and joins a session (its `ISteamMatchmaking` proxy, `research/S9-matchmaking-decision.md` §5d/§5e,
   `re/MEMORY_MAP.md` §12), what it hooks in `nightreign.exe`, how a joiner gets from the Steam lobby into the game session.
2. Where the 6-player cap lives.
3. What happens on host loss; any path for host migration in the game or SC.
4. A sized estimate of our own session layer if SC ever blocks us.

Done = the research doc with a go/no-go on host migration and a plain-language summary for Leonardo.
