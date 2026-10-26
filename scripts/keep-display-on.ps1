# Keeps the display (and the machine) awake and wakes it once: the one-PC scenarios screenshot and click the game window, which is
# black while the monitor sleeps. Runs until Ctrl-C. Needs a normal terminal (an interactive desktop).
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Awake {
  [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
}
"@
[Awake]::mouse_event(1, 1, 0, 0, [UIntPtr]::Zero)
[Awake]::mouse_event(1, -1, 0, 0, [UIntPtr]::Zero)
while ($true) {
    [void][Awake]::SetThreadExecutionState([uint32]2147483651)
    Start-Sleep -Seconds 30
}
