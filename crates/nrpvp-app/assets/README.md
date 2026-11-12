# Bundled starter save

`starter.sl2` is the original `NR0000.sl2` extracted from the user-supplied archive
`Nightreign Save rar file 904 1.0 2026-09-21T14-40Z pHiZY9l3i.rar` on 2026-11-12.
The duplicate `.bak` and Steam Cloud metadata are deliberately excluded.

- Archive SHA-256: `ADBE6B2E717CBCDA61901F50C3A67BEE6237468C19D58BAE334D8E754C771455`
- Save SHA-256: `2F3618C05E72655844780DEAC74BFFE9638BA99AFA43832173860B0A85850B9E`
- Size: 19,531,312 bytes; all fourteen decrypted entry MD5 checksums validated locally.
- First supported character: `Champion Of Zeus`.
- The binary is embedded with `include_bytes!`; players need no RAR tool or external save download.

Setup binds the template's embedded Steam owner to the selected SteamID64 and creates only
the save extension configured by Seamless Co-op (normally `NR0000.co2`). It refuses `.sl2`,
preserves an existing save, and never installs `steam_autocloud.vdf`. Name edits update the
chosen supported character slot and keep a timestamped backup before atomic replacement.
Names stored in past match history are not rewritten. Unsupported character layouts are
excluded from the selector; saves with invalid entry checksums cannot be edited.

Format references (facts, not vendored implementation):

- [Nightreign container / owner / checksum layout](https://github.com/Hapfel1/er-save-manager/blob/main/src/er_save_manager/games/nightreign_steamid.py)
- [Character inventory and name field layout](https://github.com/alfizari/Elden-Ring-Nightreign-Save-Editor/blob/main/src/inventory_handler.py)

The format tests validate the actual supplied fixture, a byte-identical decrypt/re-encrypt
roundtrip, a different Steam owner, Unicode name edits, bad names and damaged input.
Loading the resulting save in the pinned game remains a human acceptance check.
