# S10 tools (read-only study of nrsc.dll and nightreign.exe)

Findings: `../../research/S10-seamless-coop-internals.md`. Python: `%LOCALAPPDATA%\Programs\Python\Python312\python.exe` with pefile +
capstone. None of these scripts writes to a game or to `nrsc.dll`. Their raw output about `nrsc.dll` (string lists, disassembly) stays
in `C:\Users\Leonardo\source\nrsc-study\` and is never committed (hard rule 3).

| Script | Does |
|---|---|
| `pe_overview.py <pe>` | sections + entropy, visible imports/exports |
| `strings_by_section.py <pe>` | ASCII/UTF-16 strings with section and RVA |
| `fn_strrefs.py <pe>` | per `.pdata` function, which strings it references |
| `xrefs.py <pe> <rva>…` / `lea_strings.py <pe> <a> <b>` | code references to given RVAs / strings referenced in a range |
| `disasm_at.py <pe> <rva> <len>` | disassemble a range (used on `nightreign.exe` only) |
| `hook_census.py [--pid N] [--out f]` | runtime: where the running game's code/vtables differ from the file and where they jump (runbook R1) |
