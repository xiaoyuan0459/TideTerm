"""Controlled loopback SSH peer for the ignored forwarding integration test.
Requires Paramiko; never connects to a remote host or reads user credentials.
"""
import json
import socket
import struct
import threading
import paramiko

key = paramiko.RSAKey.generate(2048)
listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen(1)
print(json.dumps({"port": listener.getsockname()[1], "key": key.get_base64()}), flush=True)

class Server(paramiko.ServerInterface):
    def __init__(self):
        self.ready = threading.Event()
        self.cookie = None
        self.agent = False
    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if (username, password) == ("fixture", "fixture") else paramiko.AUTH_FAILED
    def get_allowed_auths(self, username):
        return "password"
    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_pty_request(self, channel, term, width, height, px, py, modes):
        return True
    def check_channel_x11_request(self, channel, single, protocol, cookie, screen):
        assert protocol == "MIT-MAGIC-COOKIE-1" and screen == 0
        self.cookie = bytes.fromhex(cookie)
        return True
    def check_channel_forward_agent_request(self, channel):
        self.agent = True
        return True
    def check_channel_shell_request(self, channel):
        self.ready.set()
        return True

def read_exact(channel, count):
    output = b""
    while len(output) < count:
        data = channel.recv(count - len(output))
        if not data:
            raise RuntimeError("unexpected channel EOF")
        output += data
    return output

transport = paramiko.Transport(listener.accept()[0])
transport.add_server_key(key)
server = Server()
transport.start_server(server=server)
shell = transport.accept(10)
assert shell is not None and server.ready.wait(10)
assert server.cookie is not None and server.agent
assert read_exact(shell, 5) == b"START"

# A forged cookie must be refused before touching the local X server.
bad = transport.open_x11_channel(("198.51.100.4", 1234))
bad.settimeout(10)
bad.sendall(b"l\0" + struct.pack("<5H", 11, 0, 18, 16, 0) + b"MIT-MAGIC-COOKIE-1\0\0" + bytes(16))
assert bad.recv(1) == b""
bad.close()

x11 = transport.open_x11_channel(("198.51.100.5", 2345))
x11.settimeout(10)
setup = b"l\0" + struct.pack("<5H", 11, 0, 18, 16, 0) + b"MIT-MAGIC-COOKIE-1\0\0" + server.cookie
for offset in range(0, len(setup), 3):
    x11.sendall(setup[offset:offset + 3])
x11.sendall(b"X_REQUEST")
assert read_exact(x11, 7) == b"X_REPLY"

agent = transport.open_forward_agent_channel()
agent.settimeout(10)
agent.sendall(b"\0\0\0\x01\x0b")  # SSH2_AGENTC_REQUEST_IDENTITIES
assert read_exact(agent, 9) == b"\0\0\0\x05\x0c\0\0\0\0"
shell.sendall(b"FORWARD_OK")
# Keep the transport open until the client explicitly disconnects.
shell.settimeout(10)
while shell.recv(1024):
    pass
transport.close()
listener.close()
