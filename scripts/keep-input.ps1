# Keeps the screensaver (3 min on Leonardo's PC) from starting during a test: nudges the mouse 1 px and back every 45 s. Ctrl-C ends it.
# SetThreadExecutionState (scripts\keep-display-on.ps1) does not stop a screensaver, and on its desktop no game window can come to the front (TRAPS.md).
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Nudge {
  [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
}
"@
while ($true) {
    [void][Nudge]::SetThreadExecutionState([uint32]2147483651)
    [Nudge]::mouse_event(1, 1, 0, 0, [UIntPtr]::Zero)
    [Nudge]::mouse_event(1, -1, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Seconds 45
}
