"""Loopback SSH peer with generated keys; never reads user credentials."""
import base64
import json
from pathlib import Path
import socket
import sys
import threading

import paramiko
from cryptography.hazmat.primitives import serialization as s
from cryptography.hazmat.primitives.asymmetric import ec, ed25519, rsa

from ssh_key_agent import start_agent

root = Path(sys.argv[1])
password = b"fixture-passphrase"
cases = []
authorized = set()
agent_keys = {}
for algorithm, key in [
    ("ed25519", ed25519.Ed25519PrivateKey.generate()),
    ("rsa", rsa.generate_private_key(public_exponent=65537, key_size=2048)),
    ("ecdsa", ec.generate_private_key(ec.SECP256R1())),
]:
    public = key.public_key().public_bytes(s.Encoding.OpenSSH, s.PublicFormat.OpenSSH)
    authorized.add(base64.b64decode(public.split()[1]))
    for container, fmt in [("openssh", s.PrivateFormat.OpenSSH), ("pem", s.PrivateFormat.TraditionalOpenSSL)]:
        if algorithm == "ed25519" and container == "pem":
            continue
        for encrypted in [False, True]:
            name = f"{algorithm}-{container}-{'encrypted' if encrypted else 'plain'}"
            path = root / name
            encryption = s.BestAvailableEncryption(password) if encrypted else s.NoEncryption()
            path.write_bytes(key.private_bytes(s.Encoding.PEM, fmt, encryption))
            cases.append({"name": name, "path": str(path), "encrypted": encrypted})
            if container == "openssh" and not encrypted and algorithm in ("ed25519", "rsa"):
                agent_keys[algorithm] = paramiko.PKey.from_path(path)
unicode_path = root / "私钥-rsa.pem"
rsa_pem = next(case for case in cases if case["name"] == "rsa-pem-plain")
unicode_path.write_bytes(Path(rsa_pem["path"]).read_bytes())
cases.append({"name": "rsa-pem-unicode-path", "path": str(unicode_path), "encrypted": False})
untrusted = root / "untrusted"
untrusted.write_bytes(rsa.generate_private_key(public_exponent=65537, key_size=2048).private_bytes(
    s.Encoding.PEM, s.PrivateFormat.TraditionalOpenSSL, s.NoEncryption()))
agents = {name: start_agent(key) for name, key in agent_keys.items()}
host = paramiko.RSAKey.generate(2048)


class Server(paramiko.ServerInterface):
    def get_allowed_auths(self, username):
        return "publickey,password"

    def check_auth_publickey(self, username, key):
        return paramiko.AUTH_SUCCESSFUL if username == "fixture" and key.asbytes() in authorized else paramiko.AUTH_FAILED

    def check_auth_password(self, username, value):
        return paramiko.AUTH_SUCCESSFUL if username == "fixture" and value == "fixture-password" else paramiko.AUTH_FAILED

    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_channel_exec_request(self, channel, command):
        if command != b"fixture-round-trip":
            return False
        threading.Thread(target=self.reply, args=(channel,), daemon=True).start()
        return True

    @staticmethod
    def reply(channel):
        channel.sendall(b"KEY_AUTH_OK\n")
        channel.send_exit_status(0)
        channel.shutdown_write()


def serve(connection):
    transport = paramiko.Transport(connection)
    transport.get_security_options().kex = ['diffie-hellman-group14-sha256']
    transport.add_server_key(host)
    try:
        transport.start_server(server=Server())
        channels = []
        while transport.is_active():
            channel = transport.accept(1)
            if channel is not None:
                channels.append(channel)
    except (EOFError, OSError, paramiko.SSHException):
        pass
    finally:
        transport.close()


listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen(8)
print(json.dumps({"port": listener.getsockname()[1], "host_key": host.get_base64(),
                  "cases": cases, "agents": agents, "untrusted": str(untrusted)}), flush=True)
while True:
    connection, _ = listener.accept()
    threading.Thread(target=serve, args=(connection,), daemon=True).start()
