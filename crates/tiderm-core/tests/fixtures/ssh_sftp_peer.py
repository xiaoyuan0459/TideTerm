"""Loopback-only SSH/SFTP fixture. No shell, exec, user files or private key on disk.

Run with --directory under a temporary directory; ready.json contains only the
port and public known_hosts path. Test login is fixture / fixture-password.
stats.json records counts, never terminal bytes or credentials.
"""
import argparse
import json
import logging
import posixpath
import socket
import stat
import threading
import time
from pathlib import Path

import paramiko

parser = argparse.ArgumentParser()
parser.add_argument("--directory", required=True)
args = parser.parse_args()
root = Path(args.directory).resolve()
root.mkdir(parents=True, exist_ok=True)
logging.getLogger("paramiko").setLevel(logging.CRITICAL)
lock = threading.Lock()
counts = {"connections": 0, "sftp_open": 0, "sftp_close": 0,
          "list": 0, "exec": 0, "terminal_bytes": 0, "file_open": 0}


def count(key, amount=1):
    with lock:
        counts[key] += amount
        (root / "stats.json").write_text(json.dumps(counts), encoding="utf-8")


class Files(paramiko.SFTPServerInterface):
    def session_started(self):
        self.active = True
        count("sftp_open")

    def session_ended(self):
        if getattr(self, "active", False):
            self.active = False
            count("sftp_close")

    def list_folder(self, path):
        count("list")
        path = posixpath.normpath(path)
        if path == "/denied":
            return paramiko.SFTP_PERMISSION_DENIED
        if path in ("/slow", "/stall"):
            time.sleep(0.25 if path == "/slow" else 6)
        if path == "/programs":
            entries = [("TIDE_TOOL", False), ("TIDE_SKIP", False),
                       ("TIDE_DIR", True), ("TIDE space's", False)]
        elif path == "/many":
            entries = [(f"p{n:04}", False) for n in range(2100)]
        elif path in ("/", "/env-a", "/env-b", "/slow", "/stall"):
            entries = [("ENV_ONLY.txt", False), ("ENV_DIR", True),
                       ("ENV unsafe.txt", False), ("中文.txt", False),
                       ("QUOTE space's.txt", False)]
        else:
            return paramiko.SFTP_NO_SUCH_FILE
        result = []
        for name, directory in entries:
            item = paramiko.SFTPAttributes()
            item.filename = name
            executable = directory or (path == "/programs" and name != "TIDE_SKIP")
            item.st_mode = (stat.S_IFDIR if directory else stat.S_IFREG) | (0o755 if executable else 0o644)
            item.st_size = 0
            item.st_mtime = 1700000000
            result.append(item)
        return result

    def open(self, path, flags, attr):
        count("file_open")
        return paramiko.SFTP_PERMISSION_DENIED


def shell(channel):
    try:
        channel.sendall(b"\x1b]9;9;/env-a\x07\r\nFIXTURE_READY> ")
        command = bytearray()
        while True:
            data = channel.recv(4096)
            if not data:
                break
            count("terminal_bytes", len(data))
            # Model only the report protocol, never execute received commands.
            # Real Bash/PowerShell generation has a separate interpreter test.
            command.extend(data)
            if len(command) > 4096:
                command.clear()
            if b"\r" in command:
                import re
                match = re.search(rb"tideterm-env;1;([0-9a-f]{32});b;", command)
                if match:
                    payload = b"/REMOTE_REPORT_ONLY:/remote space::relative".hex().encode()
                    channel.sendall(b"\x1b]777;tideterm-env;1;" + match[1] + b";b;" + payload + b"\x07")
                lookup = re.search(rb"tideterm-env;2;([0-9a-f]{32});b;%s;TIDE_REMOTE;", command)
                if lookup:
                    payload = b"/REMOTE_ONLY/TIDE_REMOTE".hex().encode()
                    channel.sendall(b"\x1b]777;tideterm-env;2;" + lookup[1] + b";b;file;TIDE_REMOTE;" + payload + b"\x07")
                git = re.search(rb"tideterm-env;3;([0-9a-f]{32});b;%s;", command)
                if git:
                    repo = b"/REMOTE_ONLY/repo.git".hex().encode()
                    refs = b"refs/heads/remote-main\nrefs/tags/remote-v1".hex().encode()
                    channel.sendall(b"\x1b]777;tideterm-env;3;" + git[1] + b";b;0;" + repo + b";" + refs + b"\x07")
                docker = re.search(rb"tideterm-env;4;([0-9a-f]{32});b;%s;fixture;", command)
                if docker:
                    rows = (b'"' + b"f" * 64 + b'"|"REMOTE_DOCKER_ONLY"').hex().encode()
                    channel.sendall(b"\x1b]777;tideterm-env;4;" + docker[1] + b";b;0;fixture;" + rows + b"\x07")
                kube = re.search(rb"tideterm-env;5;([0-9a-f]{32});b;%s;([0-9a-f]+);", command)
                if kube:
                    rows = b'"remote-uid"|"remote-pod"|"fixture"'.hex().encode()
                    channel.sendall(b"\x1b]777;tideterm-env;5;" + kube[1] + b";b;0;" + kube[2] + b";" + rows + b"\x07")
                command.clear()
            channel.sendall(b"\r\nFIXTURE_RECEIVED\r\nFIXTURE_READY> ")
    except (OSError, EOFError):
        pass


class Server(paramiko.ServerInterface):
    def check_auth_password(self, username, password):
        self.deny_sftp = username == "fixture-no-sftp"
        return (paramiko.AUTH_SUCCESSFUL if username in ("fixture", "fixture-no-sftp") and
                password == "fixture-password" else paramiko.AUTH_FAILED)

    def get_allowed_auths(self, username):
        return "password"

    def check_channel_request(self, kind, channel_id):
        return (paramiko.OPEN_SUCCEEDED if kind == "session" else
                paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED)

    def check_channel_pty_request(self, *args):
        return True

    def check_channel_shell_request(self, channel):
        threading.Thread(target=shell, args=(channel,), daemon=True).start()
        return True

    def check_channel_exec_request(self, channel, command):
        count("exec")
        return False

    def check_channel_subsystem_request(self, channel, name):
        if self.deny_sftp:
            return False
        return super().check_channel_subsystem_request(channel, name)


host_key = paramiko.RSAKey.generate(2048)
listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen(16)
port = listener.getsockname()[1]
known_hosts = root / "known_hosts"
known_hosts.write_text(f"[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n", encoding="ascii")
count("connections", 0)
(root / "ready.json").write_text(json.dumps({"port": port, "known_hosts": str(known_hosts)}), encoding="utf-8")


def serve(client):
    client.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    transport = paramiko.Transport(client)
    try:
        transport.add_server_key(host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, Files)
        transport.start_server(server=Server())
        # Retain accepted channels until the transport ends.
        channels = []
        while transport.is_active():
            channel = transport.accept(0.2)
            if channel:
                channels.append(channel)
    except (paramiko.SSHException, OSError, EOFError):
        pass
    finally:
        transport.close()


while True:
    client, _ = listener.accept()
    count("connections")
    threading.Thread(target=serve, args=(client,), daemon=True).start()
