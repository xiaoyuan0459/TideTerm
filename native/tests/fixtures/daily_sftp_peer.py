"""Loopback SFTP fixture backed only by its explicitly supplied temporary root."""
import argparse
import errno
import json
import logging
import os
from pathlib import Path
import socket
import stat
import threading
import time
import paramiko

parser = argparse.ArgumentParser()
parser.add_argument("--directory", required=True)
parser.add_argument("--permission-fixtures", action="store_true")
args = parser.parse_args()
root = Path(args.directory).resolve()
root.mkdir(parents=True, exist_ok=True)
files = root / "files"
files.mkdir(exist_ok=True)
logging.getLogger("paramiko").setLevel(logging.CRITICAL)
links = {}
if args.permission_fixtures:
    if os.name != "posix":
        raise RuntimeError("permission fixtures require a POSIX filesystem")
    for name, mode in [("private.txt", 0o600), ("executable.sh", 0o755), ("permission-denied.txt", 0o600)]:
        path = files / name
        path.write_bytes(b"original permission fixture")
        path.chmod(mode)

def path_for(raw):
    result = (files / str(raw).lstrip("/")).resolve()
    if not result.is_relative_to(files):
        raise PermissionError("outside fixture root")
    return result

def failure(error):
    return paramiko.SFTPServer.convert_errno(getattr(error, "errno", None) or errno.EACCES)

class Files(paramiko.SFTPServerInterface):
    def lstat(self, path):
        try:
            if path in links:
                result = paramiko.SFTPAttributes()
                result.st_mode = stat.S_IFLNK | 0o777
                result.st_size = 0
                result.st_mtime = int(time.time())
                return result
            return paramiko.SFTPAttributes.from_stat(path_for(path).lstat())
        except OSError as error:
            return failure(error)

    def stat(self, path):
        return self.lstat(links.get(path, path))

    def list_folder(self, path):
        try:
            result = []
            for child in path_for(path).iterdir():
                entry = self.lstat(path.rstrip("/") + "/" + child.name)
                entry.filename = child.name
                result.append(entry)
            for link in links:
                if str(Path(link).parent).replace("\\", "/") == path.rstrip("/"):
                    entry = self.lstat(link)
                    entry.filename = Path(link).name
                    result.append(entry)
            return result
        except OSError as error:
            return failure(error)

    def open(self, path, flags, attr):
        try:
            descriptor = os.open(path_for(path), flags | getattr(os, "O_BINARY", 0), attr.st_mode if attr.st_mode is not None else 0o666)
            if args.permission_fixtures and flags & os.O_CREAT:
                with (root / "creation-modes.jsonl").open("a", encoding="utf-8") as observations:
                    observations.write(json.dumps({"path": path, "mode": stat.S_IMODE(os.fstat(descriptor).st_mode)}) + "\n")
            mode = "r+b" if flags & os.O_RDWR else "wb" if flags & os.O_WRONLY else "rb"
            stream = os.fdopen(descriptor, mode)
            handle = paramiko.SFTPHandle(flags)
            if flags & (os.O_WRONLY | os.O_RDWR):
                handle.writefile = stream
            if not flags & os.O_WRONLY:
                handle.readfile = stream
            return handle
        except OSError as error:
            return failure(error)

    def chattr(self, path, attr):
        try:
            if args.permission_fixtures and path.startswith("/permission-denied.txt.tideterm-"):
                return paramiko.SFTP_PERMISSION_DENIED
            paramiko.SFTPServer.set_file_attr(str(path_for(path)), attr)
            return paramiko.SFTP_OK
        except OSError as error:
            return failure(error)

    def mkdir(self, path, attr):
        try:
            path_for(path).mkdir()
            return paramiko.SFTP_OK
        except OSError as error:
            return failure(error)

    def remove(self, path):
        try:
            if path in links:
                del links[path]
            else:
                path_for(path).unlink()
            return paramiko.SFTP_OK
        except OSError as error:
            return failure(error)

    def rmdir(self, path):
        try:
            path_for(path).rmdir()
            return paramiko.SFTP_OK
        except OSError as error:
            return failure(error)

    def rename(self, source, target):
        try:
            if target == "/rollback.txt" and source.endswith(".part"):
                return paramiko.SFTP_FAILURE
            if path_for(target).exists() or target in links:
                return paramiko.SFTP_FAILURE
            path_for(source).rename(path_for(target))
            return paramiko.SFTP_OK
        except OSError as error:
            return failure(error)

    def posix_rename(self, source, target):
        try:
            os.replace(path_for(source), path_for(target))
            return paramiko.SFTP_OK
        except OSError as error:
            return failure(error)

    def symlink(self, target, link):
        try:
            path_for(target)
            if path_for(link).exists() or link in links:
                return paramiko.SFTP_FAILURE
            links[link] = target
            return paramiko.SFTP_OK
        except OSError as error:
            return failure(error)

class Server(paramiko.ServerInterface):
    def check_auth_password(self, user, password):
        return paramiko.AUTH_SUCCESSFUL if (user, password) == ("fixture", "fixture-password") else paramiko.AUTH_FAILED
    def get_allowed_auths(self, user):
        return "password"
    def check_channel_request(self, kind, channel):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
    def check_channel_pty_request(self, *args):
        return True
    def check_channel_shell_request(self, channel):
        return True

key = paramiko.RSAKey.generate(2048)
listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen(8)
port = listener.getsockname()[1]
known = root / "known_hosts"
known.write_text(f"[127.0.0.1]:{port} {key.get_name()} {key.get_base64()}\n", encoding="utf-8")
(root / "ready.json").write_text(json.dumps({"port": port, "known_hosts": str(known)}), encoding="utf-8")

def serve(connection):
    transport = paramiko.Transport(connection)
    transport.add_server_key(key)
    transport.set_subsystem_handler("sftp", paramiko.SFTPServer, Files)
    try:
        transport.start_server(server=Server())
        channels = []
        while transport.is_active():
            channel = transport.accept(0.1)
            if channel is not None:
                channels.append(channel)
    finally:
        transport.close()

while True:
    connection, _ = listener.accept()
    threading.Thread(target=serve, args=(connection,), daemon=True).start()
