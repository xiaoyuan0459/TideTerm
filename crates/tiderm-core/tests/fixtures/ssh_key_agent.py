"""An isolated Windows named-pipe SSH agent that signs with fixture keys."""
import ctypes as c
from ctypes import wintypes as w
import struct
import threading
import uuid

import paramiko


def start_agent(key):
    api = c.WinDLL("kernel32", use_last_error=True)
    api.CreateNamedPipeW.argtypes = [w.LPCWSTR, w.DWORD, w.DWORD, w.DWORD, w.DWORD, w.DWORD, w.DWORD, c.c_void_p]
    api.CreateNamedPipeW.restype = w.HANDLE
    api.ConnectNamedPipe.argtypes = [w.HANDLE, c.c_void_p]
    api.ReadFile.argtypes = [w.HANDLE, c.c_void_p, w.DWORD, c.POINTER(w.DWORD), c.c_void_p]
    api.WriteFile.argtypes = api.ReadFile.argtypes
    api.DisconnectNamedPipe.argtypes = [w.HANDLE]
    api.CloseHandle.argtypes = [w.HANDLE]
    path = r"\\.\pipe\tideterm-key-fixture-" + str(uuid.uuid4())
    pipe = api.CreateNamedPipeW(path, 3 | 0x00080000, 8, 1, 65536, 65536, 1000, None)
    if pipe == c.c_void_p(-1).value:
        raise c.WinError(c.get_last_error())

    def read_exact(length):
        result = bytearray()
        while len(result) < length:
            data = c.create_string_buffer(length - len(result))
            count = w.DWORD()
            if not api.ReadFile(pipe, data, len(data), c.byref(count), None) or not count.value:
                raise EOFError
            result.extend(data.raw[:count.value])
        return bytes(result)

    def run():
        try:
            while True:
                if not api.ConnectNamedPipe(pipe, None) and c.get_last_error() != 535:
                    raise c.WinError(c.get_last_error())
                try:
                    while True:
                        length = struct.unpack(">I", read_exact(4))[0]
                        if not 1 <= length <= 65536:
                            raise ValueError("invalid agent frame")
                        message = paramiko.Message(read_exact(length))
                        kind = message.get_byte()
                        reply = paramiko.Message()
                        if kind == b"\x0b":
                            reply.add_byte(b"\x0c")
                            reply.add_int(1)
                            reply.add_string(key.asbytes())
                            reply.add_string("fixture")
                        elif kind == b"\x0d":
                            identity, data, flags = message.get_binary(), message.get_binary(), message.get_int()
                            if identity != key.asbytes():
                                raise ValueError("unknown agent identity")
                            algorithm = "rsa-sha2-512" if flags & 4 else "rsa-sha2-256" if flags & 2 else key.get_name()
                            reply.add_byte(b"\x0e")
                            reply.add_string(key.sign_ssh_data(data, algorithm).asbytes())
                        else:
                            reply.add_byte(b"\x05")
                        payload = reply.asbytes()
                        frame = struct.pack(">I", len(payload)) + payload
                        count = w.DWORD()
                        if not api.WriteFile(pipe, frame, len(frame), c.byref(count), None) or count.value != len(frame):
                            raise EOFError
                except EOFError:
                    pass
                finally:
                    api.DisconnectNamedPipe(pipe)
        finally:
            api.CloseHandle(pipe)

    threading.Thread(target=run, daemon=True).start()
    return path
