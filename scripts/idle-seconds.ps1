# Prints how many seconds the user has had no keyboard / mouse input (GetLastInputInfo). The one-PC scenarios drive the game window with
# synthetic keys and clicks, so an agent runs them only while the PC is idle (RW tests, 2026-10-24).
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Idle { [StructLayout(LayoutKind.Sequential)] public struct LII { public uint cb; public uint t; }
[DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LII p);
public static double Secs() { LII l = new LII(); l.cb = (uint)Marshal.SizeOf(l); GetLastInputInfo(ref l); return ((uint)Environment.TickCount - l.t) / 1000.0; } }
"@
[int][Idle]::Secs()
