#!/usr/bin/python3
"""Client de banc pour la socket de gpad-rec : enchaîne le scénario complet."""
import json
import socket
import sys
import time

chemin = sys.argv[1]


def cmd(c, pause=0.0):
    s = socket.socket(socket.AF_UNIX)
    for essai in range(60):          # le démon peut démarrer après nous
        try:
            s.connect(chemin)
            break
        except OSError:
            time.sleep(0.05)
    else:
        raise SystemExit("socket absente: " + chemin)
    s.sendall((json.dumps({"cmd": c}) + "\n").encode())
    d = s.recv(8192).decode().strip()
    s.close()
    print(f"{c:14s} -> {d}", flush=True)
    if pause:
        time.sleep(pause)
    return json.loads(d)


cmd("status")
cmd("replay.start", 6)
cmd("status")
rep = cmd("replay.save")
cmd("record.start", 3)
cmd("status")
cmd("record.stop")
cmd("status")
cmd("quit")
print("replay:", rep.get("file"), rep.get("t"))
