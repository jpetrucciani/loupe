"""Exercise the actual CLI with a controlling terminal and separate data pipes."""

import fcntl
import json
import os
import pty
import select
import signal
import struct
import sys
import termios
import time
from pathlib import Path


def attributes(descriptor: int) -> list:
    settings = termios.tcgetattr(descriptor)
    if sys.platform == "darwin":
        # Darwin sets PENDIN when returning to canonical input. It is transient
        # kernel state, not a terminal setting changed by the CLI.
        settings[3] &= ~termios.PENDIN
    return settings


def run(arguments: list[str], root: Path, action: str) -> dict[str, object]:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 110, 0, 0))
    original = attributes(slave)
    input_read, input_write = os.pipe()
    output_read, output_write = os.pipe()
    error_path = root / "pty-stderr.txt"
    error_fd = os.open(error_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    state_read, state_write = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
        os.close(state_read)
        if sys.platform == "darwin":
            # Keep the session leader alive while inspecting terminal settings:
            # Darwin revokes the slave when its session leader exits.
            command_pid = os.fork()
            if command_pid:
                for descriptor in [
                    master,
                    input_read,
                    input_write,
                    output_read,
                    output_write,
                    error_fd,
                ]:
                    os.close(descriptor)
                os.write(state_write, struct.pack("i", command_pid))
                _, command_status = os.waitpid(command_pid, 0)
                restored = attributes(slave) == original
                os.write(state_write, struct.pack("i?", command_status, restored))
                os._exit(0)
        os.close(state_write)
        os.dup2(input_read, 0)
        os.dup2(output_write, 1)
        os.dup2(error_fd, 2)
        for descriptor in [
            master,
            slave,
            input_read,
            input_write,
            output_read,
            output_write,
            error_fd,
        ]:
            if descriptor > 2:
                os.close(descriptor)
        os.chdir(root)
        os.execv(arguments[0], arguments)
    os.close(state_write)
    command_pid = (
        struct.unpack("i", os.read(state_read, struct.calcsize("i")))[0]
        if sys.platform == "darwin"
        else pid
    )
    os.close(input_read)
    os.close(output_write)
    os.close(error_fd)
    os.write(input_write, b"Checkout is down. Customers cannot place orders.\n")
    os.close(input_write)
    screen = bytearray()
    output = bytearray()
    acted = action == "NONE"
    status: int | None = None
    expires = time.monotonic() + 20
    try:
        while status is None:
            if time.monotonic() >= expires:
                raise TimeoutError("review did not finish within twenty seconds")
            readable, _, _ = select.select([master, output_read], [], [], 0.05)
            for descriptor in readable:
                try:
                    data = os.read(descriptor, 65536)
                except OSError:
                    data = b""
                if descriptor == master:
                    screen.extend(data)
                else:
                    output.extend(data)
            if not acted and b"q/Ctrl-C" in screen:
                if action == "TERM":
                    os.kill(command_pid, signal.SIGTERM)
                else:
                    os.write(master, bytes.fromhex(action))
                acted = True
            finished, result = os.waitpid(pid, os.WNOHANG)
            if finished:
                status = result
        while True:
            data = os.read(output_read, 65536)
            if not data:
                break
            output.extend(data)
        if not acted:
            raise AssertionError(f"no review screen: {error_path.read_text()}")
        if sys.platform == "darwin":
            status, restored = struct.unpack(
                "i?", os.read(state_read, struct.calcsize("i?"))
            )
        else:
            restored = attributes(slave) == original
        if not restored:
            raise AssertionError(
                f"{action}: terminal attributes were not restored; exit={os.waitstatus_to_exitcode(status)}"
            )
        if b"\x1b" in output:
            raise AssertionError("terminal display leaked to stdout")
        return {
            "exit_code": os.waitstatus_to_exitcode(status),
            "stdout": output.decode(),
            "stderr": error_path.read_text(),
            "terminal_restored": restored,
            "screen_seen": acted,
        }
    finally:
        if status is None:
            os.kill(command_pid, signal.SIGKILL)
            if command_pid != pid:
                os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        for descriptor in [master, slave, output_read, state_read]:
            os.close(descriptor)


if __name__ == "__main__":
    print(json.dumps(run(json.loads(sys.argv[1]), Path(sys.argv[2]), sys.argv[3])))
