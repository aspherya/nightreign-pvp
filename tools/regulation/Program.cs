// sbwriter: builds Nightreign regulation.bin with changed param rows, written by Smithbox's own SoulsFormats writer.
//
//   sbwriter <original regulation.bin> <rows.json> <out regulation.bin>
//
// Called by scripts\build-gamedata.ps1 (UI-3), which builds this project in a temp folder against the DLLs of a Smithbox install
// (default Z:\Tools\Smithbox-2.2.6; Smithbox is third-party, its DLLs are never copied into the repo).
//
// THE TRAP (2026-11-02, game 1.03.3, TRAPS.md): only Smithbox's writer is verified to make a regulation.bin the game loads.
//   - WitchyBND 3.0.1.0 encrypts a Nightreign regulation with the ELDEN RING AES key (SoulsFormatsNEXT EncryptERNRRegulation):
//     the game closes ~10 s after start. Witchy reads with the ER key first, so re-unpacking its own output looks fine.
//   - A hand-written Python writer (NR key, zero IV, same BND4 bytes, zstd 22 with vanilla-like framing, zero padding) also made the
//     game close; cause UNKNOWN (zstd framing or padding). Smithbox's output differs in exactly those: zstd level 15 stream, PKCS7.
// So this tool does what Smithbox does when it saves an NR project (ProjectUtils.cs, ProjectType.NR):
//   SFUtil.DecryptNightreignRegulation -> change the PARAM bytes -> SFUtil.EncryptNightreignRegulation(bnd, zero IV).
//
// rows.json (gamedata\nightbossmenu-rows.json): params[] of { param, rowSize, fields: [[name, type]...], rows: [{ id, mode: "replace",
// expect: {field: vanilla value}, values: {field: new value} }] }. Only "replace" is supported: a new row id does not show in the
// expedition list (in-game test 2026-11-02). "expect" makes a game patch that moved the rows fail the build.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text.Json;
using SoulsFormats;

static class Program
{
    // regulation.bin of game 1.03.3.
    const string PinnedSha256 = "876A3CA279A4561D0C69F81FE5E510C75C59AB9A8A201694F8CF4A42C91E0268";

    static readonly Dictionary<string, int> FieldSize = new()
    {
        ["s8"] = 1, ["u8"] = 1, ["s16"] = 2, ["u16"] = 2, ["s32"] = 4, ["u32"] = 4,
    };

    static int Main(string[] args)
    {
        if (args.Length != 3)
        {
            Console.Error.WriteLine("usage: sbwriter <original regulation.bin> <rows.json> <out regulation.bin>");
            return 2;
        }
        try { Run(args[0], args[1], args[2]); return 0; }
        catch (Exception e) { Console.Error.WriteLine("sbwriter: " + e.Message); return 1; }
    }

    static void Run(string origPath, string rowsPath, string outPath)
    {
        var orig = File.ReadAllBytes(origPath);
        var hash = Convert.ToHexString(SHA256.HashData(orig));
        if (hash != PinnedSha256) throw new Exception($"regulation.bin hash {hash} is not the pinned 1.03.3 hash");

        var bnd = SFUtil.DecryptNightreignRegulation(orig);
        using var spec = JsonDocument.Parse(File.ReadAllText(rowsPath));
        var expected = new Dictionary<string, byte[]>();
        foreach (var p in spec.RootElement.GetProperty("params").EnumerateArray())
        {
            var name = p.GetProperty("param").GetString();
            var file = bnd.Files.Single(f => f.Name.EndsWith("\\" + name, StringComparison.OrdinalIgnoreCase));
            file.Bytes = PatchParam(file.Bytes.ToArray(), p, name);
            expected[name] = file.Bytes.ToArray();
        }

        var bytes = SFUtil.EncryptNightreignRegulation(bnd, new byte[16]);

        // Self-check: read the output back with the same library and compare what changed and what did not.
        var check = SFUtil.DecryptNightreignRegulation(bytes);
        var vanilla = SFUtil.DecryptNightreignRegulation(orig);
        if (check.Files.Count != vanilla.Files.Count) throw new Exception("self-check: file count changed");
        for (int i = 0; i < check.Files.Count; i++)
        {
            var a = check.Files[i];
            var v = vanilla.Files[i];
            if (a.Name != v.Name || a.ID != v.ID) throw new Exception($"self-check: entry {i} name/id changed");
            var leaf = a.Name.Substring(a.Name.LastIndexOf('\\') + 1);
            var want = expected.TryGetValue(leaf, out var w) ? w : v.Bytes.ToArray();
            if (!a.Bytes.ToArray().AsSpan().SequenceEqual(want)) throw new Exception($"self-check: {leaf} differs");
        }

        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(outPath)));
        File.WriteAllBytes(outPath, bytes);
        Console.WriteLine($"regulation.bin: {bytes.Length} bytes, sha256 {Convert.ToHexString(SHA256.HashData(bytes))}");
    }

    // PARAM with 64-bit offsets (format2D 0x85): row table at 0x40 of (i64 id, i64 dataOffset, i64 nameOffset).
    static byte[] PatchParam(byte[] param, JsonElement p, string name)
    {
        var fields = p.GetProperty("fields").EnumerateArray()
            .Select(f => (Name: f[0].GetString(), Type: f[1].GetString())).ToList();
        int size = fields.Sum(f => FieldSize.TryGetValue(f.Type, out var s) ? s : throw new Exception($"unsupported field type {f.Type}"));
        if (size != p.GetProperty("rowSize").GetInt32()) throw new Exception($"{name}: field list is {size} bytes, rowSize differs");
        if (param[0x2D] != 0x85) throw new Exception($"{name}: unexpected PARAM format 0x{param[0x2D]:X2}");
        int count = BitConverter.ToUInt16(param, 0x0A);
        var rows = new Dictionary<long, int>();
        for (int i = 0; i < count; i++)
            rows[BitConverter.ToInt64(param, 0x40 + 24 * i)] = (int)BitConverter.ToInt64(param, 0x40 + 24 * i + 8);

        foreach (var r in p.GetProperty("rows").EnumerateArray())
        {
            var mode = r.TryGetProperty("mode", out var m) ? m.GetString() : "replace";
            if (mode != "replace") throw new Exception("only mode 'replace' is supported (a new row id does not show in the expedition list)");
            long id = r.GetProperty("id").GetInt64();
            if (!rows.TryGetValue(id, out int off)) throw new Exception($"{name}: row {id} not found");
            var cur = Read(param, off, fields);
            if (r.TryGetProperty("expect", out var exp))
                foreach (var e in exp.EnumerateObject())
                {
                    if (!cur.ContainsKey(e.Name)) throw new Exception($"{name}: unknown field {e.Name}");
                    if (cur[e.Name] != e.Value.GetInt64())
                        throw new Exception($"{name} row {id}: {e.Name} is {cur[e.Name]}, expected {e.Value.GetInt64()} (game patch?)");
                }
            var changes = new List<string>();
            foreach (var v in r.GetProperty("values").EnumerateObject())
            {
                if (!cur.ContainsKey(v.Name)) throw new Exception($"{name}: unknown field {v.Name}");
                long nv = v.Value.GetInt64();
                if (cur[v.Name] != nv) changes.Add($"{v.Name} {cur[v.Name]}->{nv}");
                cur[v.Name] = nv;
            }
            Write(param, off, fields, cur);
            Console.WriteLine($"  {name} row {id}: {string.Join(", ", changes)}");
        }
        return param;
    }

    static Dictionary<string, long> Read(byte[] b, int off, List<(string Name, string Type)> fields)
    {
        var d = new Dictionary<string, long>();
        foreach (var f in fields)
        {
            d[f.Name] = f.Type switch
            {
                "s8" => (sbyte)b[off], "u8" => b[off],
                "s16" => BitConverter.ToInt16(b, off), "u16" => BitConverter.ToUInt16(b, off),
                "s32" => BitConverter.ToInt32(b, off), "u32" => BitConverter.ToUInt32(b, off),
                _ => throw new Exception(f.Type),
            };
            off += FieldSize[f.Type];
        }
        return d;
    }

    static void Write(byte[] b, int off, List<(string Name, string Type)> fields, Dictionary<string, long> d)
    {
        foreach (var f in fields)
        {
            long v = d[f.Name];
            byte[] raw = f.Type switch
            {
                "s8" => new[] { (byte)checked((sbyte)v) }, "u8" => new[] { checked((byte)v) },
                "s16" => BitConverter.GetBytes(checked((short)v)), "u16" => BitConverter.GetBytes(checked((ushort)v)),
                "s32" => BitConverter.GetBytes(checked((int)v)), "u32" => BitConverter.GetBytes(checked((uint)v)),
                _ => throw new Exception(f.Type),
            };
            Buffer.BlockCopy(raw, 0, b, off, raw.Length);
            off += raw.Length;
        }
    }
}
