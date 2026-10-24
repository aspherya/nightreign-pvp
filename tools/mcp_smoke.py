"""Drive nro-mcp over stdio like an MCP client would: launch, expedition, puppets, screenshot, stop. Prints a short summary."""
import json, subprocess, sys, os
exe = os.path.join(os.path.dirname(__file__), '..', 'target', 'release', 'nro-mcp.exe')
mod = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'dist')
p = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
n = 0
def rpc(method, params=None):
    global n; n += 1
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params or {}}) + "\n"); p.stdin.flush()
    return json.loads(p.stdout.readline())
def tool(tname, **args):
    r = rpc("tools/call", {"name": tname, "arguments": args})["result"]
    kinds = [c["type"] for c in r["content"]]
    text = " ".join(c.get("text", "")[:160].replace("\n", " | ") for c in r["content"] if c["type"] == "text")
    print(f"{tname}: {'ERROR ' if r.get('isError') else ''}{kinds} {text}")
    return r
rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "0"}})
print("tools:", len(rpc("tools/list")["result"]["tools"]))
tool("launch", mod_dist=mod, world=True, set={"mode": "offline"})
tool("navigate", to="expedition")
tool("console", line="god 1")
tool("puppets", n=3)
tool("puppet_front", dist=2.5)
r = tool("screenshot", name="mcp")
img = [c for c in r["content"] if c["type"] == "image"]
print("image bytes (base64):", len(img[0]["data"]) if img else 0)
tool("console", line="players")
tool("stop")
p.stdin.close(); p.wait()
