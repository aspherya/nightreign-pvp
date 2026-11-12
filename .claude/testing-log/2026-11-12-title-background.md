# Custom title background — 2026-11-12

Result: **PASS on one PC**, verified by a captured running title screen and user confirmation: "it worked".

- Game: pinned 1.03.3; `nightreign.exe` SHA-256 `9545E8732919BB6A63904CB73223641B3C4F9A67869BE5722612ACCA413E8B3A`.
- Seamless Co-op: pinned 1.1.3; `nrsc.dll` SHA-256 `243EEC929A97B71E1E2E3B4215778B89C37D629436B8DD5403E830593D3CE24E`.
- me3: pinned 0.11.0; `me3.exe` SHA-256 `F0282DCEE40DFC81CF46F9C18CDFF8D7EFDC4C2A6C927CC08810F80B41FFD8D3`.
- Original `menu/05_000_title.gfx`: `FC735F3E12640802DB433A631DEAAEA2BED565C8C7C85815938381C7D988963D`.
- Generated menu: `114776A53D38A21D05BD86DE918262AEC6B75B5E1E853C0BD3251A1EBD294A69`.

The 1024x1024 user image fills the 1920x1080 title screen with independent X/Y scaling. Continue, Load Game, New Game and
System remain visible above it. me3's launcher log confirms the generated GFx override was loaded. No expedition was entered.
A copy of the active save-profile directory was created under `runs/title-background-backup-*` before launch.
Screenshot artifact: `runs/title-background-evidence/title-background.png` (gitignored).

Implementation and rebuild procedure: `re/GFX_UI.md` §8.
